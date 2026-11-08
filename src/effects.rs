//! Music DSP: RBJ shelving EQ, delayed low-pass crossfeed and a stereo
//! Schroeder/Moorer reverb based on Jezar's public-domain Freeverb topology.
//! See docs/AUDIO_EFFECTS.md for algorithm references and scope.
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Effect {
    Original,
    Bass,
    Live,
    Livehouse,
    Club,
    Space3D,
}
impl Effect {
    pub const COUNT: usize = 6;
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Bass,
            2 => Self::Live,
            3 => Self::Livehouse,
            4 => Self::Club,
            5 => Self::Space3D,
            _ => Self::Original,
        }
    }
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::Original, false) => "原声",
            (Self::Original, true) => "Original",
            (Self::Bass, false) => "醇厚低音",
            (Self::Bass, true) => "Warm bass",
            (Self::Live, false) => "现场",
            (Self::Live, true) => "Live stage",
            (Self::Livehouse, _) => "Livehouse",
            (Self::Club, false) => "律动",
            (Self::Club, true) => "Club",
            (Self::Space3D, false) => "空间3D",
            (Self::Space3D, true) => "3D space",
        }
    }
}
#[derive(Clone)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}
impl Biquad {
    fn omega(rate: f32, frequency: f32) -> f32 {
        // Keep filter poles away from Nyquist even on low-rate output devices.
        2.0 * PI * frequency.min(rate * 0.45) / rate
    }

    // RBJ low shelf, slope S=1. Negative gain also supported.
    fn shelf(rate: f32, frequency: f32, db: f32) -> Self {
        let a = 10.0f32.powf(db / 40.0);
        let w = Self::omega(rate, frequency);
        let c = w.cos();
        let alpha = w.sin() / 2.0 * 2.0f32.sqrt();
        let t = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) + (a - 1.0) * c + t;
        Self {
            b: [
                a * ((a + 1.0) - (a - 1.0) * c + t) / a0,
                2.0 * a * ((a - 1.0) - (a + 1.0) * c) / a0,
                a * ((a + 1.0) - (a - 1.0) * c - t) / a0,
            ],
            a: [
                -2.0 * ((a - 1.0) + (a + 1.0) * c) / a0,
                ((a + 1.0) + (a - 1.0) * c - t) / a0,
            ],
            z: [[0.0; 2]; 2],
        }
    }
    // RBJ peaking EQ.
    fn peaking(rate: f32, frequency: f32, q: f32, db: f32) -> Self {
        let a = 10.0f32.powf(db / 40.0);
        let w = Self::omega(rate, frequency);
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b: [
                (1.0 + alpha * a) / a0,
                -2.0 * w.cos() / a0,
                (1.0 - alpha * a) / a0,
            ],
            a: [-2.0 * w.cos() / a0, (1.0 - alpha / a) / a0],
            z: [[0.0; 2]; 2],
        }
    }
    // RBJ high shelf, slope S=1.
    fn high_shelf(rate: f32, frequency: f32, db: f32) -> Self {
        let a = 10.0f32.powf(db / 40.0);
        let w = Self::omega(rate, frequency);
        let c = w.cos();
        let alpha = w.sin() / 2.0 * 2.0f32.sqrt();
        let t = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) - (a - 1.0) * c + t;
        Self {
            b: [
                a * ((a + 1.0) + (a - 1.0) * c + t) / a0,
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c) / a0,
                a * ((a + 1.0) + (a - 1.0) * c - t) / a0,
            ],
            a: [
                2.0 * ((a - 1.0) - (a + 1.0) * c) / a0,
                ((a + 1.0) - (a - 1.0) * c - t) / a0,
            ],
            z: [[0.0; 2]; 2],
        }
    }
    // RBJ high-pass with Q = 1/sqrt(2).
    fn highpass(rate: f32, frequency: f32) -> Self {
        let w = Self::omega(rate, frequency);
        let c = w.cos();
        let alpha = w.sin() / 2.0f32.sqrt();
        let a0 = 1.0 + alpha;
        Self {
            b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            z: [[0.0; 2]; 2],
        }
    }
    fn lowpass(rate: f32, frequency: f32) -> Self {
        let w = Self::omega(rate, frequency);
        let c = w.cos();
        let alpha = w.sin() / 2.0f32.sqrt();
        let a0 = 1.0 + alpha;
        Self {
            b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            z: [[0.0; 2]; 2],
        }
    }
    fn process_mono(&mut self, input: f32) -> f32 {
        let y = self.b[0] * input + self.z[0][0];
        self.z[0][0] = self.b[1] * input - self.a[0] * y + self.z[0][1];
        self.z[0][1] = self.b[2] * input - self.a[1] * y;
        y
    }
    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|c| {
            let y = self.b[0] * input[c] + self.z[c][0];
            self.z[c][0] = self.b[1] * input[c] - self.a[0] * y + self.z[c][1];
            self.z[c][1] = self.b[2] * input[c] - self.a[1] * y;
            y
        })
    }
}
struct Delay {
    data: Vec<f32>,
    position: usize,
}
impl Delay {
    fn new(length: usize) -> Self {
        Self {
            data: vec![0.0; length.max(1)],
            position: 0,
        }
    }
    fn read(&self) -> f32 {
        self.data[self.position]
    }
    fn write(&mut self, v: f32) {
        self.data[self.position] = v;
        self.position = (self.position + 1) % self.data.len();
    }
}
struct Comb {
    delay: Delay,
    lowpass: f32,
}
struct Room {
    combs: Vec<Comb>,
    allpasses: Vec<Delay>,
    feedback: f32,
    damp: f32,
}
impl Room {
    fn new(rate: f32, right: bool, feedback: f32) -> Self {
        let size =
            |n: usize| ((n + if right { 23 } else { 0 }) as f32 * rate / 44100.0).round() as usize;
        Self {
            combs: [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]
                .into_iter()
                .map(|n| Comb {
                    delay: Delay::new(size(n)),
                    lowpass: 0.0,
                })
                .collect(),
            allpasses: [556, 441, 341, 225]
                .into_iter()
                .map(|n| Delay::new(size(n)))
                .collect(),
            feedback,
            damp: 0.35,
        }
    }
    fn process(&mut self, input: f32) -> f32 {
        let mut output = 0.0;
        for comb in &mut self.combs {
            let delayed = comb.delay.read();
            comb.lowpass = delayed * (1.0 - self.damp) + comb.lowpass * self.damp;
            comb.delay
                .write(input * 0.015 + comb.lowpass * self.feedback);
            output += delayed;
        }
        for ap in &mut self.allpasses {
            let delayed = ap.read();
            ap.write(output + delayed * 0.5);
            output = delayed - output;
        }
        output
    }
}
struct Compressor {
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    envelope: f32,
}
impl Compressor {
    fn new(
        rate: f32,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
    ) -> Self {
        Self {
            threshold: 10.0f32.powf(threshold_db / 20.0),
            ratio,
            attack: 1.0 - (-1.0 / (rate * attack_ms / 1000.0)).exp(),
            release: 1.0 - (-1.0 / (rate * release_ms / 1000.0)).exp(),
            makeup: 10.0f32.powf(makeup_db / 20.0),
            envelope: 0.0,
        }
    }
    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let peak = input[0].abs().max(input[1].abs());
        let coeff = if peak > self.envelope {
            self.attack
        } else {
            self.release
        };
        self.envelope += coeff * (peak - self.envelope);
        let env = self.envelope.max(1e-6);
        let gain = if env > self.threshold {
            (self.threshold / env).powf(1.0 - 1.0 / self.ratio)
        } else {
            1.0
        };
        input.map(|v| v * gain * self.makeup)
    }
}
// Psychoacoustic bass enhancement: rectify the sub band and fold its harmonics
// back in so small speakers can reproduce the low end.
struct BassEnhancer {
    low: Biquad,
    high: Biquad,
    smooth: Biquad,
    amount: f32,
}
impl BassEnhancer {
    fn new(rate: f32) -> Self {
        Self {
            low: Biquad::lowpass(rate, 90.0),
            high: Biquad::highpass(rate, 100.0),
            smooth: Biquad::lowpass(rate, 380.0),
            amount: 0.12,
        }
    }
    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mono = (input[0] + input[1]) * 0.5;
        let low = self.low.process_mono(mono);
        let harmonics = self.smooth.process_mono(self.high.process_mono(low.abs()));
        [
            input[0] + harmonics * self.amount,
            input[1] + harmonics * self.amount,
        ]
    }
    fn reset(&mut self) {
        self.low.z = [[0.0; 2]; 2];
        self.high.z = [[0.0; 2]; 2];
        self.smooth.z = [[0.0; 2]; 2];
    }
}
// Early-reflection taps plus a slow rotation of the wet field. Reflections are
// fed mostly from the side signal so the dry vocal/centre image stays stable.
struct Space {
    taps: [Delay; 6],
    tap_gains: [[f32; 2]; 6],
    phase: f32,
    phase_step: f32,
    depth: f32,
}
impl Space {
    fn new(rate: f32) -> Self {
        let gains = [0.60, 0.50, 0.40, 0.32, 0.24, 0.18];
        let pans: [f32; 6] = [-0.85, 0.85, -0.60, 0.60, -0.35, 0.35];
        Self {
            taps: [0.0073, 0.0119, 0.0173, 0.0241, 0.0313, 0.0419]
                .map(|seconds| Delay::new((rate * seconds) as usize)),
            tap_gains: std::array::from_fn(|i| {
                [
                    ((1.0 - pans[i]) * 0.5).sqrt() * gains[i],
                    ((1.0 + pans[i]) * 0.5).sqrt() * gains[i],
                ]
            }),
            phase: 0.0,
            phase_step: 0.10 / rate,
            depth: 0.55,
        }
    }
    fn process(&mut self, mid: f32, side: f32) -> [f32; 2] {
        let feed = mid * 0.22 + side * 1.15;
        let mut output = [0.0; 2];
        for (tap, [left, right]) in self.taps.iter_mut().zip(self.tap_gains) {
            let delayed = tap.read();
            tap.write(feed);
            output[0] += delayed * left;
            output[1] += delayed * right;
        }
        self.phase = (self.phase + self.phase_step) % 1.0;
        output
    }
    fn angle(&self) -> f32 {
        (self.phase * 2.0 * PI).sin() * self.depth
    }
}
fn rotate([left, right]: [f32; 2], angle: f32) -> [f32; 2] {
    let (sin, cos) = angle.sin_cos();
    [left * cos + right * sin, right * cos - left * sin]
}
struct Profile {
    eq: Vec<Biquad>,
    side_hp: Vec<Biquad>,
    bass: Option<BassEnhancer>,
    compressor: Option<Compressor>,
    space: Option<Space>,
    rooms: [Room; 2],
    cross_delay: [Delay; 2],
    cross_lp: [f32; 2],
    lp_coeff: f32,
    crossfeed: f32,
    width: f32,
    wet: f32,
    gain: f32,
}
impl Profile {
    fn new(effect: Effect, rate: f32) -> Self {
        let (eq, feedback, wet, crossfeed, width, gain_db, side_hp, bass, compressor, space) =
            match effect {
                // Sub weight plus harmonic bass so small speakers still feel the low end.
                Effect::Bass => (
                    vec![
                        Biquad::highpass(rate, 30.0),
                        Biquad::peaking(rate, 65.0, 0.7, 8.0),
                        Biquad::peaking(rate, 140.0, 1.0, 4.0),
                        Biquad::peaking(rate, 300.0, 0.8, -2.5),
                    ],
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    -1.5,
                    vec![Biquad::highpass(rate, 120.0)],
                    Some(BassEnhancer::new(rate)),
                    None,
                    None,
                ),
                Effect::Live => (
                    vec![Biquad::shelf(rate, 160.0, 1.0)],
                    0.80,
                    1.10,
                    0.0,
                    1.20,
                    -2.5,
                    Vec::new(),
                    None,
                    None,
                    None,
                ),
                // Small, tight room: shorter tail, more presence and compression.
                Effect::Livehouse => (
                    vec![
                        Biquad::highpass(rate, 35.0),
                        Biquad::shelf(rate, 120.0, 4.0),
                        Biquad::peaking(rate, 380.0, 0.8, -2.0),
                        Biquad::peaking(rate, 2800.0, 0.8, 2.0),
                        Biquad::high_shelf(rate, 9000.0, 2.0),
                    ],
                    0.42,
                    0.55,
                    0.0,
                    1.12,
                    -1.5,
                    vec![Biquad::highpass(rate, 130.0)],
                    None,
                    Some(Compressor::new(rate, -18.0, 2.0, 20.0, 180.0, 3.0)),
                    None,
                ),
                // Tight sub, de-mud, vocal presence, air, narrower bass and peak compression.
                Effect::Club => (
                    vec![
                        Biquad::highpass(rate, 30.0),
                        Biquad::shelf(rate, 70.0, 8.0),
                        Biquad::peaking(rate, 300.0, 0.8, -3.5),
                        Biquad::peaking(rate, 4000.0, 0.8, 4.0),
                        Biquad::high_shelf(rate, 11000.0, 5.0),
                    ],
                    0.0,
                    0.0,
                    0.0,
                    1.45,
                    -1.0,
                    vec![Biquad::highpass(rate, 200.0); 2],
                    Some(BassEnhancer::new(rate)),
                    Some(Compressor::new(rate, -20.0, 2.8, 8.0, 120.0, 5.0)),
                    None,
                ),
                // Strong externalisation: early reflections, head-shadow crossfeed,
                // side-driven reverb and a slow rotation of the wet field.
                Effect::Space3D => (
                    vec![Biquad::highpass(rate, 30.0)],
                    0.78,
                    0.72,
                    0.20,
                    1.90,
                    -1.0,
                    vec![Biquad::highpass(rate, 140.0)],
                    None,
                    None,
                    Some(Space::new(rate)),
                ),
                Effect::Original => unreachable!("original uses the dry path"),
            };
        let (cross_seconds, cross_hz) = if matches!(effect, Effect::Space3D) {
            (0.00045, 4500.0)
        } else {
            (0.0003, 700.0)
        };
        Self {
            eq,
            side_hp,
            bass,
            compressor,
            space,
            rooms: [
                Room::new(rate, false, feedback),
                Room::new(rate, true, feedback),
            ],
            cross_delay: [
                Delay::new((rate * cross_seconds) as usize),
                Delay::new((rate * cross_seconds) as usize),
            ],
            cross_lp: [0.0; 2],
            lp_coeff: 1.0 - (-2.0 * PI * cross_hz / rate).exp(),
            crossfeed,
            width,
            wet,
            gain: 10.0f32.powf(gain_db / 20.0),
        }
    }
    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mut signal = input;
        for eq in &mut self.eq {
            signal = eq.process(signal);
        }
        if let Some(bass) = &mut self.bass {
            signal = bass.process(signal);
        }
        let mid = (signal[0] + signal[1]) * 0.5;
        let mut side = (signal[0] - signal[1]) * 0.5;
        for hp in &mut self.side_hp {
            side = hp.process_mono(side);
        }
        side *= self.width;
        signal = [mid + side, mid - side];
        for (c, value) in signal.iter().enumerate() {
            self.cross_lp[c] += self.lp_coeff * (*value - self.cross_lp[c]);
        }
        let cross = [self.cross_delay[1].read(), self.cross_delay[0].read()];
        for c in 0..2 {
            self.cross_delay[c].write(self.cross_lp[c]);
        }
        let room_input = if self.space.is_some() {
            mid * 0.35 + side * 0.90
        } else {
            mid
        };
        let mut wet = [
            self.rooms[0].process(room_input) * self.wet,
            self.rooms[1].process(room_input) * self.wet,
        ];
        if let Some(space) = &mut self.space {
            let early = space.process(mid, side);
            wet[0] += early[0];
            wet[1] += early[1];
            wet = rotate(wet, space.angle());
        }
        let mut output = [0.0; 2];
        for c in 0..2 {
            // Dry stereo channels are preserved independently (unlike VLC 3 spatializer).
            let direct = (signal[c] + cross[c] * self.crossfeed) / (1.0 + self.crossfeed);
            output[c] = direct + wet[c];
        }
        if let Some(compressor) = &mut self.compressor {
            output = compressor.process(output);
        }
        output.map(|v| v * self.gain)
    }
    fn reset(&mut self) {
        for eq in &mut self.eq {
            eq.z = [[0.0; 2]; 2];
        }
        for hp in &mut self.side_hp {
            hp.z = [[0.0; 2]; 2];
        }
        if let Some(bass) = &mut self.bass {
            bass.reset();
        }
        if let Some(compressor) = &mut self.compressor {
            compressor.envelope = 0.0;
        }
        if let Some(space) = &mut self.space {
            for tap in &mut space.taps {
                tap.data.fill(0.0);
                tap.position = 0;
            }
            space.phase = 0.0;
        }
        self.cross_lp = [0.0; 2];
        for delay in &mut self.cross_delay {
            delay.data.fill(0.0);
            delay.position = 0;
        }
        for room in &mut self.rooms {
            for comb in &mut room.combs {
                comb.delay.data.fill(0.0);
                comb.delay.position = 0;
                comb.lowpass = 0.0;
            }
            for ap in &mut room.allpasses {
                ap.data.fill(0.0);
                ap.position = 0;
            }
        }
    }
}
pub struct Processor {
    profiles: [Profile; Effect::COUNT - 1],
    effect: usize,
    volume: f32,
    target_volume: f32,
    volume_fade: f32,
    limiter: f32,
    release: f32,
    edge_last: [f32; 2],
    edge_from: [f32; 2],
    edge_mix: f32,
    edge_step: f32,
}
impl Processor {
    pub fn new(rate: u32, effect: usize, volume: u16) -> Self {
        let rate = rate as f32;
        Self {
            profiles: [
                Effect::Bass,
                Effect::Live,
                Effect::Livehouse,
                Effect::Club,
                Effect::Space3D,
            ]
            .map(|e| Profile::new(e, rate)),
            effect: Effect::from_index(effect) as usize,
            volume: volume.min(100) as f32 / 100.0,
            target_volume: volume.min(100) as f32 / 100.0,
            volume_fade: 1.0 - (-1.0 / (rate * 0.01)).exp(),
            limiter: 1.0,
            release: 1.0 - (-1.0 / (rate * 0.06)).exp(),
            edge_last: [0.0; 2],
            edge_from: [0.0; 2],
            edge_mix: 1.0,
            edge_step: 1.0 / (rate * 0.005),
        }
    }
    pub fn reset(&mut self, effect: usize, volume: u16) {
        self.edge_mix = 0.0;
        self.effect = Effect::from_index(effect) as usize;
        self.volume = volume.min(100) as f32 / 100.0;
        self.target_volume = self.volume;
        self.limiter = 1.0;
        for profile in &mut self.profiles {
            profile.reset();
        }
    }
    pub fn set(&mut self, effect: usize, volume: u16) {
        let effect = Effect::from_index(effect) as usize;
        if effect != self.effect {
            if self.effect != 0 {
                self.profiles[self.effect - 1].reset();
            }
            self.effect = effect;
        }
        self.target_volume = volume.min(100) as f32 / 100.0;
    }
    pub fn render(&mut self, sample: Option<[f32; 2]>) -> [f32; 2] {
        self.edge_last = if let Some(sample) = sample {
            if self.edge_mix == 0.0 {
                self.edge_from = self.edge_last;
            }
            let output = self.process(sample);
            self.edge_mix = (self.edge_mix + self.edge_step).min(1.0);
            std::array::from_fn(|c| {
                output[c] * self.edge_mix + self.edge_from[c] * (1.0 - self.edge_mix)
            })
        } else {
            self.edge_mix = 0.0;
            self.edge_last.map(|v| v * (1.0 - self.edge_step))
        };
        self.edge_last
    }
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let input = input.map(|v| if v.is_finite() { v } else { 0.0 });
        let output = if self.effect == 0 {
            input
        } else {
            self.profiles[self.effect - 1].process(input)
        };
        self.volume += self.volume_fade * (self.target_volume - self.volume);
        let output = output.map(|v| v * self.volume);
        let peak = output[0].abs().max(output[1].abs());
        let gain = if peak > 0.98 { 0.98 / peak } else { 1.0 };
        if gain < self.limiter {
            self.limiter = gain;
        } else {
            self.limiter += self.release * (gain - self.limiter);
        }
        output.map(|v| v * self.limiter)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn underrun_pause_and_seek_edges_are_smoothed() {
        let mut dsp = Processor::new(48000, 0, 100);
        let mut last = dsp.render(Some([0.8; 2]));
        for sample in
            std::iter::repeat_n(None, 2400).chain(std::iter::repeat_n(Some([-0.8; 2]), 2400))
        {
            let output = dsp.render(sample);
            assert!((output[0] - last[0]).abs() < 0.02);
            last = output;
        }
        dsp.reset(0, 100);
        assert!((dsp.render(Some([0.8; 2]))[0] - last[0]).abs() < 0.02);
    }
    #[test]
    fn venue_presets_have_audible_wet_energy() {
        for effect in [Effect::Live, Effect::Livehouse] {
            let mut dsp = Processor::new(48000, effect as usize, 100);
            let first = dsp.process([0.5; 2]);
            let dry = first.iter().map(|v| v * v).sum::<f32>();
            let mut tail = 0.0;
            for _ in 0..48000 {
                tail += dsp.process([0.0; 2]).iter().map(|v| v * v).sum::<f32>();
            }
            assert!(
                tail / dry > 0.005,
                "{effect:?}: wet/dry energy {}",
                tail / dry
            );
        }
    }
    #[test]
    fn all_presets_preserve_left_dry_channel_and_stay_finite() {
        for effect in 0..Effect::COUNT {
            let mut dsp = Processor::new(48000, effect, 100);
            let mut energy = 0.0;
            for i in 0..48000 {
                let x = (i as f32 * 2.0 * PI * 440.0 / 48000.0).sin() * 0.8;
                let y = dsp.process([x, 0.0]);
                energy += y[0] * y[0];
                assert!(y.into_iter().all(|v| v.is_finite() && v.abs() <= 0.981));
            }
            assert!(energy > 1000.0, "left dry channel lost: {effect}");
        }
    }
    #[test]
    fn switching_to_original_immediately_drops_the_previous_tail() {
        let live = Effect::Live as usize;
        let mut dsp = Processor::new(48000, live, 100);
        for _ in 0..4800 {
            dsp.process([0.1, 0.1]);
        }
        dsp.set(0, 100);
        assert_eq!(dsp.process([0.1, -0.1]), [0.1, -0.1]);
        dsp.set(live, 100);
        assert_eq!(dsp.process([0.0; 2]), [0.0; 2]);
    }
    #[test]
    fn presets_are_bounded_at_device_sample_rates_and_seek_clears_tails() {
        for rate in [8000, 16000, 20000, 22050, 44100, 48000, 96000] {
            for effect in 0..Effect::COUNT {
                let mut dsp = Processor::new(rate, effect, 100);
                for i in 0..6000 {
                    let y = dsp.process([
                        if i % 2 == 0 { 0.99 } else { -0.99 },
                        (i as f32 * 0.09).sin() * 0.99,
                    ]);
                    assert!(y.into_iter().all(|v| v.is_finite() && v.abs() <= 0.981));
                }
                dsp.reset(effect, 100);
                for _ in 0..4000 {
                    assert_eq!(dsp.process([0.0; 2]), [0.0; 2]);
                }
            }
        }
    }
    #[test]
    fn club_attenuates_bass_side_and_opens_high_side() {
        let side_energy = |hz: f32| {
            let mut dsp = Processor::new(48000, Effect::Club as usize, 100);
            let (mut input, mut output) = (0.0, 0.0);
            for i in 0..48000 {
                let x = (i as f32 * 2.0 * PI * hz / 48000.0).sin() * 0.3;
                let y = dsp.process([x, -x]);
                if i > 24000 {
                    input += x * x;
                    output += ((y[0] - y[1]) * 0.5).powi(2);
                }
            }
            output / input
        };
        for hz in [60.0, 100.0, 140.0, 150.0] {
            assert!(
                side_energy(hz) < 0.25,
                "expected at least 6 dB attenuation at {hz} Hz"
            );
        }
        assert!(side_energy(4000.0) > 1.0);
    }
    #[test]
    fn club_compresses_loud_material_more_than_quiet() {
        let gain_at = |amplitude: f32| {
            let mut dsp = Processor::new(48000, Effect::Club as usize, 100);
            let signal = |i: usize| (i as f32 * 2.0 * PI * 1000.0 / 48000.0).sin() * amplitude;
            for i in 0..24000 {
                dsp.process([signal(i), signal(i)]);
            }
            let (mut input, mut output) = (0.0, 0.0);
            for i in 0..24000 {
                let x = signal(i);
                let y = dsp.process([x, x]);
                input += x * x;
                output += y[0] * y[0];
            }
            output / input
        };
        assert!(gain_at(0.6) < gain_at(0.05) * 0.8);
    }
    #[test]
    fn space3d_has_strong_side_energy() {
        let side_energy = |effect: Effect| {
            let mut dsp = Processor::new(48000, effect as usize, 100);
            let (mut input, mut output) = (0.0, 0.0);
            for i in 0..48000 {
                let x = (i as f32 * 2.0 * PI * 1000.0 / 48000.0).sin() * 0.3;
                let y = dsp.process([x, 0.0]);
                if i > 24000 {
                    input += x * x;
                    output += ((y[0] - y[1]) * 0.5).powi(2);
                }
            }
            output / input
        };
        assert!(
            side_energy(Effect::Space3D) > side_energy(Effect::Original) * 1.5,
            "3D space should be clearly wider"
        );
    }
    #[test]
    fn live_and_livehouse_have_distinct_reverb_tails() {
        let tail = |effect| {
            let mut dsp = Processor::new(48000, effect, 100);
            let mut energy = 0.0;
            for i in 0..96000 {
                let y = dsp.process(if i == 0 { [0.8; 2] } else { [0.0; 2] });
                if i > 24000 {
                    energy += y[0] * y[0] + y[1] * y[1];
                }
            }
            energy
        };
        assert_eq!(tail(0), 0.0);
        let live = Effect::Live as usize;
        let livehouse = Effect::Livehouse as usize;
        assert!(tail(live) > tail(livehouse) * 10.0);
        assert!(tail(livehouse) > 0.0);
    }
}
