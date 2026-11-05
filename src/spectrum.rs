use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Instant,
};

pub const BANDS: usize = 32;
const SAMPLES: usize = 1024;
const RATE: f32 = 16_000.0;

/// Optional, independent decoder. Only the private loopback URL reaches FFmpeg.
/// Bounded frames keep memory constant even when the UI stops polling.
pub struct Spectrum {
    child: Child,
    _bridge: termune::stream::Bridge,
    frames: Receiver<([f32; BANDS], Instant)>,
    latest: Option<([f32; BANDS], Instant)>,
}
impl Spectrum {
    pub fn start(source: termune::stream::AudioSource, position: f64) -> Option<Self> {
        let bridge = source.bridge().ok()?;
        let mut command = Command::new("ffmpeg");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .args(["-nostdin", "-loglevel", "error", "-re", "-ss"])
            .arg(format!("{position:.3}"))
            .args([
                "-i",
                &bridge.url,
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-f",
                "f32le",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut output = child.stdout.take()?;
        let (tx, frames) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = [0u8; SAMPLES * 4];
            let mut smooth = [0.0f32; BANDS];
            while output.read_exact(&mut bytes).is_ok() {
                let samples = std::array::from_fn(|i| {
                    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let bands = analyze(&samples);
                for i in 0..BANDS {
                    smooth[i] = bands[i].max(smooth[i] * 0.78);
                }
                if let Err(mpsc::TrySendError::Disconnected(_)) =
                    tx.try_send((smooth, Instant::now()))
                {
                    break;
                }
            }
        });
        Some(Self {
            child,
            _bridge: bridge,
            frames,
            latest: None,
        })
    }
    pub fn frame(&mut self) -> Option<[f32; BANDS]> {
        while let Ok(frame) = self.frames.try_recv() {
            self.latest = Some(frame);
        }
        self.latest
            .filter(|(_, t)| t.elapsed().as_secs_f32() < 1.0)
            .map(|(frame, _)| frame)
    }
    pub fn failed(&mut self) -> bool {
        self._bridge.errors.try_recv().is_ok()
            || matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}
impl Drop for Spectrum {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Hann-windowed radix-2 FFT, logarithmic frequency bands, fixed -70..0 dB scale.
/// Fixed normalization preserves silence and changes in actual audio energy.
fn analyze(samples: &[f32; SAMPLES]) -> [f32; BANDS] {
    use std::f32::consts::PI;
    let mut re = [0.0f32; SAMPLES];
    let mut im = [0.0f32; SAMPLES];
    for (i, &sample) in samples.iter().enumerate() {
        let j = i.reverse_bits() >> (usize::BITS - SAMPLES.ilog2());
        re[j] = if sample.is_finite() { sample } else { 0.0 }
            * (0.5 - 0.5 * (2.0 * PI * i as f32 / (SAMPLES - 1) as f32).cos());
    }
    let mut len = 2;
    while len <= SAMPLES {
        for start in (0..SAMPLES).step_by(len) {
            for j in 0..len / 2 {
                let angle = -2.0 * PI * j as f32 / len as f32;
                let (sin, cos) = angle.sin_cos();
                let a = start + j;
                let b = a + len / 2;
                let tr = re[b] * cos - im[b] * sin;
                let ti = re[b] * sin + im[b] * cos;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len *= 2;
    }
    let mut bands = [0.0f32; BANDS];
    for k in 2..SAMPLES / 2 {
        let freq = k as f32 * RATE / SAMPLES as f32;
        let band = ((freq / 30.0).ln() / (8000.0f32 / 30.0).ln() * BANDS as f32) as usize;
        if band < BANDS {
            let amplitude = (re[k].hypot(im[k]) * 4.0 / SAMPLES as f32).max(1e-8);
            let level = ((20.0 * amplitude.log10() + 70.0) / 70.0).clamp(0.0, 1.0);
            bands[band] = bands[band].max(level);
        }
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_and_nonfinite_input_have_no_energy() {
        assert_eq!(analyze(&[0.0; SAMPLES]), [0.0; BANDS]);
        assert_eq!(analyze(&[f32::NAN; SAMPLES]), [0.0; BANDS]);
    }
    #[test]
    fn tones_land_in_the_correct_frequency_band_and_preserve_level() {
        for frequency in [125.0f32, 1000.0, 4000.0] {
            let signal = std::array::from_fn(|i| {
                (std::f32::consts::TAU * frequency * i as f32 / RATE).sin() * 0.5
            });
            let bands = analyze(&signal);
            let peak = bands
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0;
            let expected =
                ((frequency / 30.0).ln() / (8000.0f32 / 30.0).ln() * BANDS as f32) as usize;
            assert_eq!(peak, expected);
            let quiet = analyze(&signal.map(|x| x * 0.01));
            assert!(bands[peak] - quiet[peak] > 0.5);
        }
    }
}
