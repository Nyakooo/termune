//! Music DSP: RBJ shelving EQ, delayed low-pass crossfeed and a stereo
//! Schroeder/Moorer reverb based on Jezar's public-domain Freeverb topology.
//! See docs/AUDIO_EFFECTS_RESEARCH.md for algorithm references and scope.
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Effect {
    Original,
    Spatial,
    Bass,
    Live,
    Livehouse,
}
impl Effect {
    pub const COUNT: usize = 5;
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Spatial,
            2 => Self::Bass,
            3 => Self::Live,
            4 => Self::Livehouse,
            _ => Self::Original,
        }
    }
    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (Self::Original, false) => "原声",
            (Self::Original, true) => "Original",
            (Self::Spatial, false) => "耳机空间",
            (Self::Spatial, true) => "Headphone",
            (Self::Bass, false) => "醇厚低音",
            (Self::Bass, true) => "Warm bass",
            (Self::Live, false) => "现场",
            (Self::Live, true) => "Live stage",
            (Self::Livehouse, _) => "Livehouse",
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
    // RBJ low shelf, slope S=1. Negative gain also supported.
    fn shelf(rate: f32, frequency: f32, db: f32) -> Self {
        let a = 10.0f32.powf(db / 40.0);
        let w = 2.0 * PI * frequency / rate;
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
struct Profile {
    eq: Biquad,
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
        let (bass, feedback, wet, crossfeed, width, gain_db) = match effect {
            Effect::Spatial => (0.0, 0.62, 0.45, 0.30, 1.25, -1.0),
            Effect::Bass => (7.0, 0.0, 0.0, 0.0, 1.0, -2.0),
            Effect::Live => (1.0, 0.80, 1.10, 0.0, 1.20, -2.5),
            Effect::Livehouse => (3.5, 0.57, 0.80, 0.0, 1.08, -2.0),
            _ => (0.0, 0.0, 0.0, 0.0, 1.0, 0.0),
        };
        Self {
            eq: Biquad::shelf(rate, 160.0, bass),
            rooms: [
                Room::new(rate, false, feedback),
                Room::new(rate, true, feedback),
            ],
            cross_delay: [
                Delay::new((rate * 0.0003) as usize),
                Delay::new((rate * 0.0003) as usize),
            ],
            cross_lp: [0.0; 2],
            lp_coeff: 1.0 - (-2.0 * PI * 700.0 / rate).exp(),
            crossfeed,
            width,
            wet,
            gain: 10.0f32.powf(gain_db / 20.0),
        }
    }
    fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mut signal = self.eq.process(input);
        let mid = (signal[0] + signal[1]) * 0.5;
        let side = (signal[0] - signal[1]) * 0.5 * self.width;
        signal = [mid + side, mid - side];
        for (c, value) in signal.iter().enumerate() {
            self.cross_lp[c] += self.lp_coeff * (*value - self.cross_lp[c]);
        }
        let cross = [self.cross_delay[1].read(), self.cross_delay[0].read()];
        for c in 0..2 {
            self.cross_delay[c].write(self.cross_lp[c]);
        }
        let room_input = (signal[0] + signal[1]) * 0.5;
        std::array::from_fn(|c| {
            // Dry stereo channels are preserved independently (unlike VLC 3 spatializer).
            let direct = (signal[c] + cross[c] * self.crossfeed) / (1.0 + self.crossfeed);
            (direct + self.rooms[c].process(room_input) * self.wet) * self.gain
        })
    }
}
pub struct Processor {
    profiles: [Profile; 4],
    weights: [f32; 5],
    target: usize,
    fade: f32,
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
        let effect = Effect::from_index(effect) as usize;
        let mut weights = [0.0; 5];
        weights[effect] = 1.0;
        Self {
            profiles: [
                Effect::Spatial,
                Effect::Bass,
                Effect::Live,
                Effect::Livehouse,
            ]
            .map(|e| Profile::new(e, rate)),
            weights,
            target: effect,
            fade: 1.0 - (-1.0 / (rate * 0.035)).exp(),
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
        self.target = Effect::from_index(effect) as usize;
        self.weights = [0.0; 5];
        self.weights[self.target] = 1.0;
        self.volume = volume.min(100) as f32 / 100.0;
        self.target_volume = self.volume;
        self.limiter = 1.0;
        for profile in &mut self.profiles {
            profile.eq.z = [[0.0; 2]; 2];
            profile.cross_lp = [0.0; 2];
            for delay in &mut profile.cross_delay {
                delay.data.fill(0.0);
                delay.position = 0;
            }
            for room in &mut profile.rooms {
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
    pub fn set(&mut self, effect: usize, volume: u16) {
        self.target = Effect::from_index(effect) as usize;
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
        for i in 0..5 {
            self.weights[i] +=
                self.fade * ((if i == self.target { 1.0 } else { 0.0 }) - self.weights[i]);
        }
        let mut output = input.map(|v| v * self.weights[0]);
        for (i, profile) in self.profiles.iter_mut().enumerate() {
            let sample = profile.process(input);
            for c in 0..2 {
                output[c] += sample[c] * self.weights[i + 1];
            }
        }
        self.volume += self.volume_fade * (self.target_volume - self.volume);
        output = output.map(|v| v * self.volume);
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
        for effect in 0..5 {
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
    fn switching_is_continuous_and_original_returns_to_dry() {
        let mut dsp = Processor::new(48000, 0, 100);
        let mut previous = [0.0; 2];
        for i in 0..144000 {
            if i % 24000 == 0 {
                dsp.set((i / 24000) % 5, 100);
            }
            let input = [0.2, 0.2];
            let y = dsp.process(input);
            if i > 0 {
                assert!((y[0] - previous[0]).abs() < 0.005);
            }
            previous = y;
        }
        dsp.set(0, 100);
        for _ in 0..48000 {
            previous = dsp.process([0.2, -0.2]);
        }
        assert!((previous[0] - 0.2).abs() < 1e-4 && (previous[1] + 0.2).abs() < 1e-4);
    }
    #[test]
    fn presets_are_bounded_at_device_sample_rates_and_seek_clears_tails() {
        for rate in [44100, 48000, 96000] {
            for effect in 0..5 {
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
        assert!(tail(3) > tail(4) * 10.0);
        assert!(tail(4) > 0.0);
    }
}
