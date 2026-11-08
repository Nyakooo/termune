//! Music DSP: RBJ shelving EQ, stereo widening, channel delays and a stereo
//! Schroeder/Moorer reverb based on Jezar's public-domain Freeverb topology.
//! See docs/AUDIO_EFFECTS.md for algorithm references and scope.
use std::{collections::VecDeque, f32::consts::PI};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Effect {
    Original,
    Bass,
    Livehouse,
    Club,
    Space3D,
    Panorama,
}
impl Effect {
    pub const COUNT: usize = Self::Panorama as usize + 1;
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Bass,
            2 => Self::Livehouse,
            3 => Self::Club,
            4 => Self::Space3D,
            5 => Self::Panorama,
            _ => Self::Original,
        }
    }
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::Original, false) => "原声",
            (Self::Original, true) => "Original",
            (Self::Bass, false) => "醇厚低音",
            (Self::Bass, true) => "Warm bass",
            (Self::Livehouse, _) => "Livehouse",
            (Self::Club, false) => "律动",
            (Self::Club, true) => "Club",
            (Self::Space3D, false) => "空间 3D",
            (Self::Space3D, true) => "3D Space",
            (Self::Panorama, false) => "全景环绕",
            (Self::Panorama, true) => "Panorama surround",
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
    fn process(&mut self, input: f32) -> f32 {
        let output = self.read();
        self.write(input);
        output
    }

    fn reset(&mut self) {
        self.data.fill(0.0);
        self.position = 0;
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
struct Profile {
    eq: Vec<Biquad>,
    side_hp: Vec<Biquad>,
    bass: Option<BassEnhancer>,
    compressor: Option<Compressor>,
    rooms: [Room; 2],
    width: f32,
    wet: f32,
    gain: f32,
}
impl Profile {
    fn new(effect: Effect, rate: f32) -> Self {
        let (eq, feedback, wet, width, gain_db, side_hp, bass, compressor) = match effect {
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
                1.0,
                -1.5,
                vec![Biquad::highpass(rate, 120.0)],
                Some(BassEnhancer::new(rate)),
                None,
            ),
            Effect::Livehouse => (
                vec![Biquad::shelf(rate, 160.0, 1.0)],
                0.80,
                1.10,
                1.20,
                -2.5,
                Vec::new(),
                None,
                None,
            ),
            // Keep bass centered and let drum attacks through before compressing the sustain.
            Effect::Club => (
                vec![
                    Biquad::highpass(rate, 30.0),
                    Biquad::shelf(rate, 85.0, 6.0),
                    Biquad::peaking(rate, 300.0, 0.8, -3.5),
                    Biquad::peaking(rate, 4000.0, 0.8, 4.0),
                    Biquad::high_shelf(rate, 11000.0, 5.0),
                ],
                0.0,
                0.0,
                1.55,
                -1.0,
                vec![Biquad::highpass(rate, 200.0); 2],
                Some(BassEnhancer::new(rate)),
                Some(Compressor::new(rate, -23.0, 2.4, 16.0, 120.0, 4.5)),
            ),
            Effect::Original | Effect::Space3D | Effect::Panorama => {
                unreachable!("effect uses a separate processing path")
            }
        };
        Self {
            eq,
            side_hp,
            bass,
            compressor,
            rooms: [
                Room::new(rate, false, feedback),
                Room::new(rate, true, feedback),
            ],
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
        let mut output = [
            signal[0] + self.rooms[0].process(mid) * self.wet,
            signal[1] + self.rooms[1].process(mid) * self.wet,
        ];
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
struct Space3D {
    eq: [Biquad; 4],
    right_delay: Option<Delay>,
    compressor: Compressor,
}

impl Space3D {
    fn new(rate: f32, stereo: bool) -> Self {
        // Fit from quiet left and right sweeps recorded at 48 kHz. The
        // reference had the same EQ on both channels and a 24 ms right delay.
        Self {
            eq: [
                Biquad::high_shelf(rate, 632.0, 3.18),
                Biquad::peaking(rate, 224.0, 1.47, -0.86),
                Biquad::peaking(rate, 64.0, 0.23, 5.0),
                Biquad::peaking(rate, 9930.0, 0.21, 3.22),
            ],
            right_delay: stereo.then(|| Delay::new((rate * 0.024).round() as usize)),
            compressor: Compressor::new(rate, -6.03, 5.92, 165.0, 423.0, 0.0),
        }
    }

    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mut signal = input.map(|sample| sample * 1.468);
        for filter in &mut self.eq {
            signal = filter.process(signal);
        }
        if let Some(delay) = &mut self.right_delay {
            signal[1] = delay.process(signal[1]);
        }
        self.compressor.process(signal)
    }

    fn reset(&mut self) {
        for filter in &mut self.eq {
            filter.z = [[0.0; 2]; 2];
        }
        if let Some(delay) = &mut self.right_delay {
            delay.reset();
        }
        self.compressor.envelope = 0.0;
    }
}

struct Panorama {
    taps: Vec<[f32; 2]>,
    history: Vec<[f32; 2]>,
    position: usize,
    right_delay: Option<Delay>,
}

impl Panorama {
    // Short left and right filters fitted independently from a 48 kHz A/B capture.
    // The right impulse response arrived 1536 samples after the left one.
    const REFERENCE_TAPS: [[f32; 2]; 33] = [
        [-0.014419, 0.000481],
        [0.028402, 0.017922],
        [-0.030550, -0.029139],
        [0.025375, 0.034660],
        [-0.009209, -0.027697],
        [-0.018912, 0.003013],
        [0.042961, 0.027064],
        [-0.052811, -0.046861],
        [0.043432, 0.059556],
        [-0.014346, -0.045364],
        [-0.030479, 0.009500],
        [0.081526, 0.045024],
        [-0.116312, -0.104598],
        [0.113881, 0.159330],
        [-0.055640, -0.168949],
        [-0.115546, 0.089169],
        [1.092287, 1.204807],
        [0.738330, 0.404801],
        [-0.358572, -0.372082],
        [-0.014670, 0.064697],
        [-0.043226, -0.110009],
        [-0.141328, -0.089757],
        [0.037990, 0.018374],
        [-0.108294, -0.103074],
        [0.004055, 0.030206],
        [0.003601, -0.028827],
        [-0.027087, 0.001352],
        [0.064904, 0.054906],
        [-0.007740, -0.012443],
        [0.043961, 0.060135],
        [0.032500, 0.010907],
        [-0.001363, 0.009002],
        [0.036452, 0.026529],
    ];

    fn new(rate: f32, stereo: bool) -> Self {
        const RADIUS: usize = 12;
        let scale = rate / 48_000.0;
        let padding = if scale.fract() == 0.0 { 0 } else { RADIUS };
        let last = ((Self::REFERENCE_TAPS.len() - 1) as f32 * scale).ceil() as usize;
        let mut taps = vec![[0.0; 2]; last + 2 * padding + 1];
        for (index, [left, right]) in Self::REFERENCE_TAPS.into_iter().enumerate() {
            let position = index as f32 * scale;
            let i = position as usize;
            let fraction = position.fract();
            if fraction == 0.0 {
                taps[i + padding][0] += left;
                taps[i + padding][1] += right;
                continue;
            }
            // Windowed sinc places fractional taps without a linear interpolator's
            // high-frequency droop. The common padding makes the filter causal.
            let weights: [f32; 2 * RADIUS + 1] = std::array::from_fn(|j| {
                let angle = PI * (j as f32 - RADIUS as f32 - fraction);
                let window = (0.5 + 0.5 * (angle / RADIUS as f32).cos())
                    * f32::from(angle.abs() < PI * RADIUS as f32);
                angle.sin() / angle * window
            });
            let sum: f32 = weights.iter().sum();
            for ([l, r], weight) in taps[i..].iter_mut().zip(weights) {
                *l += left * weight / sum;
                *r += right * weight / sum;
            }
        }
        Self {
            history: vec![[0.0; 2]; taps.len()],
            taps,
            position: 0,
            right_delay: stereo.then(|| Delay::new((rate * 0.032).round() as usize)),
        }
    }

    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        self.history[self.position] = input;
        let (recent, earlier) = self.history.split_at(self.position + 1);
        let samples = recent.iter().rev().chain(earlier.iter().rev());
        let mut left = 0.0;
        let mut right = 0.0;
        for (sample, tap) in samples.zip(&self.taps) {
            left += sample[0] * tap[0];
            right += sample[1] * tap[1];
        }
        self.position = (self.position + 1) % self.history.len();
        if let Some(delay) = &mut self.right_delay {
            right = delay.process(right);
        }
        [left, right]
    }

    fn reset(&mut self) {
        self.history.fill([0.0; 2]);
        self.position = 0;
        if let Some(delay) = &mut self.right_delay {
            delay.reset();
        }
    }
}

pub struct Processor {
    profiles: [Profile; Effect::Space3D as usize - 1],
    space_3d: Space3D,
    panorama: Panorama,
    recent: VecDeque<[f32; 2]>,
    pending_tail: usize,
    effect: Effect,
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
    #[cfg(test)]
    pub fn new(rate: u32, effect: usize, volume: u16) -> Self {
        Self::for_output(rate, effect, volume, 2)
    }
    pub fn for_output(rate: u32, effect: usize, volume: u16, channels: u16) -> Self {
        let rate = rate as f32;
        let panorama = Panorama::new(rate, channels > 1);
        let history = panorama.taps.len() - 1
            + panorama
                .right_delay
                .as_ref()
                .map_or(0, |delay| delay.data.len());
        Self {
            profiles: [Effect::Bass, Effect::Livehouse, Effect::Club]
                .map(|e| Profile::new(e, rate)),
            space_3d: Space3D::new(rate, channels > 1),
            panorama,
            recent: VecDeque::with_capacity(history),
            pending_tail: 0,
            effect: Effect::from_index(effect),
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
        self.effect = Effect::from_index(effect);
        self.volume = volume.min(100) as f32 / 100.0;
        self.target_volume = self.volume;
        self.limiter = 1.0;
        for profile in &mut self.profiles {
            profile.reset();
        }
        self.space_3d.reset();
        self.panorama.reset();
        self.recent.clear();
        self.pending_tail = 0;
    }
    pub fn set(&mut self, effect: usize, volume: u16) {
        let effect = Effect::from_index(effect);
        if effect != self.effect {
            if matches!(self.effect, Effect::Space3D | Effect::Panorama)
                || matches!(effect, Effect::Space3D | Effect::Panorama)
            {
                self.edge_mix = 0.0;
            }
            match self.effect {
                Effect::Original => {}
                Effect::Space3D => self.space_3d.reset(),
                Effect::Panorama => self.panorama.reset(),
                effect => self.profiles[effect as usize - 1].reset(),
            }
            self.effect = effect;
            match effect {
                Effect::Space3D => {
                    for &sample in &self.recent {
                        self.space_3d.process(sample);
                    }
                }
                Effect::Panorama => {
                    for &sample in &self.recent {
                        self.panorama.process(sample);
                    }
                }
                _ => {}
            }
            self.pending_tail = if self.recent.is_empty() {
                0
            } else {
                self.tail_length()
            };
        }
        self.set_volume(volume);
    }
    pub fn set_volume(&mut self, volume: u16) {
        self.target_volume = volume.min(100) as f32 / 100.0;
    }
    fn tail_length(&self) -> usize {
        match self.effect {
            Effect::Space3D => self
                .space_3d
                .right_delay
                .as_ref()
                .map_or(0, |delay| delay.data.len()),
            Effect::Panorama => {
                self.panorama.taps.len() - 1
                    + self
                        .panorama
                        .right_delay
                        .as_ref()
                        .map_or(0, |delay| delay.data.len())
            }
            _ => 0,
        }
    }
    pub fn has_tail(&self) -> bool {
        self.pending_tail != 0
    }
    pub fn drain(&mut self) -> [f32; 2] {
        let Some(remaining) = self.pending_tail.checked_sub(1) else {
            return self.render(None);
        };
        let output = self.render(Some([0.0; 2]));
        self.pending_tail = remaining;
        output
    }
    pub fn render(&mut self, sample: Option<[f32; 2]>) -> [f32; 2] {
        self.edge_last = if let Some(sample) = sample {
            self.pending_tail = self.tail_length();
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
        if self.recent.len() == self.recent.capacity() {
            self.recent.pop_front();
        }
        self.recent.push_back(input);
        let output = match self.effect {
            Effect::Original => input,
            Effect::Space3D => self.space_3d.process(input),
            Effect::Panorama => self.panorama.process(input),
            effect => self.profiles[effect as usize - 1].process(input),
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
    fn livehouse_has_audible_wet_energy() {
        let mut dsp = Processor::new(48000, Effect::Livehouse as usize, 100);
        let first = dsp.process([0.5; 2]);
        let dry = first.iter().map(|v| v * v).sum::<f32>();
        let mut tail = 0.0;
        for _ in 0..48000 {
            tail += dsp.process([0.0; 2]).iter().map(|v| v * v).sum::<f32>();
        }
        assert!(tail / dry > 0.005, "wet/dry energy {}", tail / dry);
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
        let live = Effect::Livehouse as usize;
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
    fn club_preserves_kick_attack_relative_to_sustain() {
        let mut dsp = Processor::new(48_000, Effect::Club as usize, 100);
        let mut attack = 0.0;
        let mut sustain = 0.0;
        for i in 0..2880 {
            let t = i as f32 / 48_000.0;
            let phase = 68.0 * t + 2.6 * (1.0 - (-50.0 * t).exp());
            let kick = 0.4 * (2.0 * PI * phase).sin() * (-17.0 * t).exp();
            let [left, _] = dsp.process([kick; 2]);
            if i < 720 {
                attack += left * left;
            } else if i >= 1440 {
                sustain += left * left;
            }
        }
        assert!(2.0 * attack / sustain > 4.3);
    }
    #[test]
    fn space_3d_preserves_its_measured_channel_delay_across_sample_rates() {
        let effect = Effect::Space3D as usize;
        for (rate, delay) in [(44_100, 1058), (48_000, 1152), (96_000, 2304)] {
            let mut dsp = Processor::new(rate, effect, 100);
            let [left, right] = dsp.process([0.02; 2]);
            assert!(left > 0.02);
            assert_eq!(right, 0.0);
            for _ in 1..delay {
                assert_eq!(dsp.process([0.0; 2])[1], 0.0);
            }
            assert!((dsp.process([0.0; 2])[1] - left).abs() < 1e-6);
            dsp.process([0.0, 0.02]);
            dsp.reset(effect, 100);
            for _ in 0..=delay {
                assert_eq!(dsp.process([0.0; 2]), [0.0; 2]);
            }
        }
    }
    #[test]
    fn panorama_delays_the_right_response_by_32_ms() {
        for (rate, delay) in [(44_100, 1411), (48_000, 1536), (96_000, 3072)] {
            let mut dsp = Processor::new(rate, Effect::Panorama as usize, 100);
            let frames: Vec<_> = (0..delay + 128)
                .map(|i| dsp.process(if i == 0 { [0.05; 2] } else { [0.0; 2] }))
                .collect();
            let peak = |channel: usize| {
                frames
                    .iter()
                    .map(|frame: &[f32; 2]| frame[channel])
                    .enumerate()
                    .max_by(|(_, left), (_, right)| left.total_cmp(right))
                    .unwrap()
                    .0
            };
            assert!((peak(1) - peak(0)).abs_diff(delay) <= 1);
            assert!(frames[..delay].iter().all(|frame| frame[1] == 0.0));
        }
    }

    #[test]
    fn surround_presets_keep_the_right_channel_when_switched_during_playback() {
        for effect in [Effect::Space3D, Effect::Panorama] {
            let mut dsp = Processor::new(48_000, Effect::Original as usize, 100);
            let input = Some([0.1; 2]);
            let mut previous = [0.0; 2];
            for _ in 0..2000 {
                previous = dsp.render(input);
            }
            for selected in [effect, Effect::Original] {
                dsp.set(selected as usize, 100);
                for _ in 0..2000 {
                    let output = dsp.render(input);
                    assert!(output[1] > 0.08, "{selected:?}: right-channel dropout");
                    for channel in 0..2 {
                        assert!((output[channel] - previous[channel]).abs() < 0.005);
                    }
                    previous = output;
                }
            }
        }
    }

    #[test]
    fn surround_presets_do_not_cancel_bass_when_downmixed_for_a_mono_device() {
        for (effect, frequency) in [(Effect::Space3D, 104.167), (Effect::Panorama, 109.375)] {
            let mut dsp = Processor::for_output(48_000, effect as usize, 100, 1);
            let mut power = 0.0;
            for i in 0..48_000 {
                let sample = 0.1 * (2.0 * PI * frequency * i as f32 / 48_000.0).sin();
                let [left, right] = dsp.process([sample; 2]);
                power += ((left + right) * 0.5).powi(2);
            }
            assert!((power / 48_000.0).sqrt() > 0.06, "{effect:?}");
        }
    }

    #[test]
    fn panorama_preserves_the_reference_filter_passband_at_other_rates() {
        let response =
            |rate: f32, hz: f32, channel: usize| {
                let panorama = Panorama::new(rate, false);
                let (real, imag) = panorama.taps.iter().enumerate().fold(
                    (0.0, 0.0),
                    |(real, imag), (index, tap)| {
                        let (sin, cos) = (2.0 * PI * hz * index as f32 / rate).sin_cos();
                        (real + cos * tap[channel], imag + sin * tap[channel])
                    },
                );
                real.hypot(imag)
            };
        for (rate, hz) in [
            (22_050.0, 8000.0),
            (44_100.0, 12_000.0),
            (44_100.0, 16_000.0),
        ] {
            for channel in 0..2 {
                let ratio = response(rate, hz, channel) / response(48_000.0, hz, channel);
                assert!(
                    (20.0 * ratio.log10()).abs() < 0.25,
                    "{rate} Hz, {hz} Hz, {ratio}"
                );
            }
        }
    }
}
