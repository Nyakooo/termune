//! Decode on a worker and feed the existing PCM queue, DSP, and CPAL output.
mod decode;

use crate::effects::Processor;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    },
};
use symphonia::core::io::MediaSource;
use termune::Result;

#[derive(Default)]
struct Playback {
    samples: VecDeque<[f32; 2]>,
    position: u64,
    duration: f64,
    seekable: bool,
    seek: Option<f64>,
    eof: bool,
}
struct Shared {
    playback: Mutex<Playback>,
    wake: Condvar,
    closing: AtomicBool,
    paused: AtomicBool,
    effect: AtomicUsize,
    volume: AtomicU16,
    reset: AtomicBool,
    error: Mutex<Option<String>>,
    rate: u32,
    #[cfg(test)]
    rendered: AtomicUsize,
}
impl Shared {
    fn new(rate: u32, effect: usize, volume: u16) -> Self {
        Self {
            playback: Mutex::new(Playback {
                samples: VecDeque::with_capacity(rate as usize / 3),
                ..Playback::default()
            }),
            wake: Condvar::new(),
            closing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            effect: AtomicUsize::new(effect),
            volume: AtomicU16::new(volume),
            reset: AtomicBool::new(false),
            error: Mutex::new(None),
            rate,
            #[cfg(test)]
            rendered: AtomicUsize::new(0),
        }
    }

    fn push(&self, frames: impl Iterator<Item = [f32; 2]>) {
        let mut state = self.playback.lock().unwrap_or_else(|e| e.into_inner());
        for frame in frames {
            while state.samples.len() >= self.rate as usize / 3
                && state.seek.is_none()
                && !self.closing.load(Ordering::Relaxed)
            {
                state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state.seek.is_some() || self.closing.load(Ordering::Relaxed) {
                break;
            }
            state.samples.push_back(frame);
        }
    }

    fn take(&self, output: &mut [[f32; 2]], dsp: &mut Processor) {
        let effect = self.effect.load(Ordering::Relaxed);
        let volume = self.volume.load(Ordering::Relaxed);
        let mut state = self.playback.lock().unwrap_or_else(|e| e.into_inner());
        if self.reset.swap(false, Ordering::Relaxed) {
            dsp.reset(effect, volume);
        }
        dsp.set(effect, volume);
        let paused = self.paused.load(Ordering::Relaxed);
        for frame in output {
            *frame = if paused {
                dsp.render(None)
            } else {
                match state.samples.pop_front() {
                    Some(sample) => {
                        state.position += 1;
                        #[cfg(test)]
                        self.rendered.fetch_add(1, Ordering::Relaxed);
                        dsp.render(Some(sample))
                    }
                    None => dsp.render(None),
                }
            };
        }
        drop(state);
        self.wake.notify_all();
    }

    fn stop(&self) {
        // Hold the queue lock so the worker cannot miss the shutdown wake.
        let _state = self.playback.lock().unwrap_or_else(|e| e.into_inner());
        self.closing.store(true, Ordering::Relaxed);
        self.wake.notify_all();
    }
}
pub struct Decoder {
    shared: Arc<Shared>,
    _stream: Option<cpal::Stream>,
    #[cfg(test)]
    sink: Option<std::thread::JoinHandle<()>>,
}
impl Decoder {
    pub fn start(source: termune::stream::AudioSource, effect: usize, volume: u16) -> Result<Self> {
        Self::start_mode(move || source.open(), effect, volume, cfg!(test))
    }

    fn start_mode<M: MediaSource + 'static>(
        open: impl FnOnce() -> Result<M> + Send + 'static,
        effect: usize,
        volume: u16,
        headless: bool,
    ) -> Result<Self> {
        let device = if headless {
            None
        } else {
            cpal::default_host().default_output_device()
        };
        let config = if headless {
            cpal::SupportedStreamConfig::new(
                2,
                cpal::SampleRate(48000),
                cpal::SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            )
        } else {
            device
                .as_ref()
                .ok_or("未找到音频输出设备")?
                .default_output_config()
                .map_err(|e| format!("无法读取音频设备：{e}"))?
        };
        let rate = config.sample_rate().0;
        let mut output_config: cpal::StreamConfig = config.clone().into();
        if let cpal::SupportedBufferSize::Range { min, max } = config.buffer_size() {
            output_config.buffer_size = cpal::BufferSize::Fixed(512u32.clamp(*min, *max));
        }
        let shared = Arc::new(Shared::new(rate, effect, volume));
        let stream = if headless {
            None
        } else {
            Some(match config.sample_format() {
                cpal::SampleFormat::F32 => {
                    output::<f32>(device.as_ref().unwrap(), &output_config, shared.clone())?
                }
                cpal::SampleFormat::I16 => {
                    output::<i16>(device.as_ref().unwrap(), &output_config, shared.clone())?
                }
                cpal::SampleFormat::U16 => {
                    output::<u16>(device.as_ref().unwrap(), &output_config, shared.clone())?
                }
                format => return Err(format!("暂不支持音频设备格式 {format:?}")),
            })
        };
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("termune-decode".into())
            .spawn(move || {
                if let Err(error) = open().and_then(|source| decode::run(source, &worker)) {
                    *worker.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                }
            })
            .map_err(|error| format!("无法启动音频解码：{error}"))?;
        #[cfg(test)]
        let sink = if headless {
            let shared = shared.clone();
            Some(std::thread::spawn(move || {
                let mut dsp = Processor::new(rate, effect, volume);
                let mut frames = vec![[0.0; 2]; rate as usize / 100];
                while !shared.closing.load(Ordering::Relaxed) {
                    shared.take(&mut frames, &mut dsp);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }))
        } else {
            None
        };
        Ok(Self {
            shared,
            _stream: stream,
            #[cfg(test)]
            sink,
        })
    }
    #[cfg(test)]
    pub fn rendered_for_test(&self) -> usize {
        self.shared.rendered.load(Ordering::Relaxed)
    }
    pub fn set_effect(&self, effect: usize) {
        self.shared.effect.store(effect, Ordering::Relaxed);
    }
    pub fn volume(&self, volume: u16) {
        self.shared.volume.store(volume, Ordering::Relaxed);
    }
    pub fn pause(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }
    pub fn seek(&self, seconds: f64) -> Result<()> {
        let mut state = self
            .shared
            .playback
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !state.seekable {
            return Err("当前音频流不支持拖动".into());
        }
        state.samples.clear();
        state.position = (seconds * f64::from(self.shared.rate)).round() as u64;
        state.seek = Some(seconds);
        state.eof = false;
        self.shared.reset.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        Ok(())
    }
    pub fn time(&self) -> (f64, f64) {
        let state = self
            .shared
            .playback
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let position = state.position as f64 / f64::from(self.shared.rate);
        (position, state.duration)
    }
    pub fn finished(&self) -> Option<Result<()>> {
        if let Some(e) = self
            .shared
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Some(Err(e));
        }
        let state = self
            .shared
            .playback
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (state.eof && state.samples.is_empty()).then_some(Ok(()))
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        self.shared.stop();
        #[cfg(test)]
        if let Some(sink) = self.sink.take() {
            let _ = sink.join();
        }
    }
}
fn output<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    match output_with_config::<T>(device, config, shared.clone()) {
        Ok(stream) => Ok(stream),
        Err(_) if matches!(config.buffer_size, cpal::BufferSize::Fixed(_)) => {
            let mut fallback = config.clone();
            fallback.buffer_size = cpal::BufferSize::Default;
            output_with_config::<T>(device, &fallback, shared)
        }
        Err(error) => Err(error),
    }
}
fn output_with_config<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let errors = shared.clone();
    let mut dsp = Processor::new(
        shared.rate,
        shared.effect.load(Ordering::Relaxed),
        shared.volume.load(Ordering::Relaxed),
    );
    // Fixed scratch storage; no allocation in the audio callback.
    let mut scratch = [[0.0; 2]; 1024];
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for chunk in data.chunks_mut(channels * 1024) {
                    let frames = chunk.len() / channels;
                    shared.take(&mut scratch[..frames], &mut dsp);
                    for (frame, sample) in chunk.chunks_mut(channels).zip(&scratch[..frames]) {
                        for (c, value) in frame.iter_mut().enumerate() {
                            *value = T::from_sample(if channels == 1 {
                                (sample[0] + sample[1]) * 0.5
                            } else if c < 2 {
                                sample[c]
                            } else {
                                0.0
                            });
                        }
                    }
                }
            },
            move |error| {
                *errors.error.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(format!("音频输出中断：{error}"));
                errors.stop();
            },
            None,
        )
        .map_err(|e| format!("无法打开音频输出：{e}"))?;
    stream
        .play()
        .map_err(|e| format!("无法开始音频输出：{e}"))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_effect_replaces_pcm_processing_in_the_first_output_block() {
        let rate = 48000;
        let effect = crate::effects::Effect::Bass as usize;
        let switched = Shared::new(rate, 0, 100);
        let mut switched_dsp = Processor::new(rate, 0, 100);
        let mut target_dsp = Processor::new(rate, effect, 100);
        let mut output = [[0.0; 2]; 480];
        let frames = (0..960).map(|i| {
            let t = i as f32 / rate as f32;
            let sample = (std::f32::consts::TAU * 90.0 * t).sin() * 0.2;
            [sample; 2]
        });
        switched.push(frames.clone());
        switched.take(&mut output, &mut switched_dsp);

        switched.effect.store(effect, Ordering::Relaxed);
        switched.take(&mut output, &mut switched_dsp);
        for (frame, input) in output.into_iter().zip(frames.skip(480)) {
            assert_eq!(frame, target_dsp.render(Some(input)));
        }
    }

    #[test]
    fn live_switch_changes_pcm_beyond_simple_volume_gain() {
        let shared = Shared::new(48000, 0, 100);
        let mut dsp = Processor::new(48000, 0, 100);
        for effect in 1..crate::effects::Effect::COUNT {
            shared.effect.store(effect, Ordering::Relaxed);
            let mut source_energy = 0.0f64;
            let mut output_energy = 0.0f64;
            let mut dot = 0.0f64;
            for block in 0..80 {
                let samples: Vec<_> = (0..480)
                    .map(|i| {
                        let t = (block * 480 + i) as f32 / 48000.0;
                        let l = (t * std::f32::consts::TAU * 90.0).sin() * 0.2
                            + (t * std::f32::consts::TAU * 1700.0).sin() * 0.1;
                        let r = (t * std::f32::consts::TAU * 330.0).sin() * 0.2;
                        [l, r]
                    })
                    .collect();
                shared.push(samples.iter().copied());
                let mut output = [[0.0; 2]; 480];
                shared.take(&mut output, &mut dsp);
                if block > 40 {
                    for (input, output) in samples.iter().zip(output) {
                        for c in 0..2 {
                            let x = input[c] as f64;
                            let y = output[c] as f64;
                            source_energy += x * x;
                            output_energy += y * y;
                            dot += x * y;
                        }
                    }
                }
            }
            let residual = (output_energy - dot * dot / source_energy) / output_energy;
            assert!(
                residual > 0.0025,
                "effect {effect}: only gain changed, residual {residual}"
            );
        }
    }
    #[test]
    fn pcm_queue_preserves_each_channel() {
        let shared = Shared::new(48000, 0, 100);
        shared.push([[0.25, -0.5], [-0.125, 0.375]].into_iter());
        let mut output = [[0.0; 2]; 2];
        shared.take(&mut output, &mut Processor::new(48000, 0, 100));
        assert_eq!(output, [[0.25, -0.5], [-0.125, 0.375]]);
    }

    #[test]
    fn bundled_codecs_render_music_without_external_decoders() {
        // Fixtures are 120 ms synthetic 440 Hz tones, encoded offline.
        let fixtures: &[(&str, &[u8])] = &[
            ("MP3", include_bytes!("../tests/fixtures/audio/tone.mp3")),
            ("FLAC", include_bytes!("../tests/fixtures/audio/tone.flac")),
            ("AAC-LC", include_bytes!("../tests/fixtures/audio/aac.m4a")),
            ("ALAC", include_bytes!("../tests/fixtures/audio/alac.m4a")),
            ("Vorbis", include_bytes!("../tests/fixtures/audio/tone.ogg")),
            ("Opus", include_bytes!("../tests/fixtures/audio/tone.opus")),
        ];
        for &(name, data) in fixtures {
            let decoder =
                Decoder::start_mode(move || Ok(std::io::Cursor::new(data)), 0, 100, true).unwrap();
            let wait_for_end = || {
                let start = std::time::Instant::now();
                loop {
                    if let Some(result) = decoder.finished() {
                        assert!(result.is_ok(), "{name}: {result:?}");
                        return;
                    }
                    assert!(start.elapsed() < Duration::from_secs(5), "{name}: timeout");
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            wait_for_end();
            let rendered = decoder.rendered_for_test();
            assert!(
                (4800..=8000).contains(&rendered),
                "{name}: {rendered} frames"
            );
            let (end, duration) = decoder.time();
            assert!(duration > 0.05, "{name}: {duration}s duration");
            decoder.seek(0.05).unwrap();
            wait_for_end();
            assert!(
                decoder.rendered_for_test() - rendered > 2000,
                "{name}: too few frames after seek"
            );
            assert!(
                (decoder.time().0 - end).abs() < 0.035,
                "{name}: end after seek"
            );
        }
    }

    #[test]
    #[ignore = "requires an output device; plays only silence"]
    fn real_output_device_plays_silence_to_eof() {
        let size = 48000u32;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((size + 36).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes());
        wav.extend(192000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(size.to_le_bytes());
        wav.resize(size as usize + 44, 0);
        let decoder =
            Decoder::start_mode(move || Ok(std::io::Cursor::new(wav)), 0, 100, false).unwrap();
        let start = std::time::Instant::now();
        loop {
            if let Some(result) = decoder.finished() {
                result.unwrap();
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(decoder.rendered_for_test() > 1000);
        drop(decoder);
    }
}
