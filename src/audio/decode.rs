use super::Shared;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{OnceLock, atomic::Ordering},
};
use symphonia::core::{
    audio::{AudioSpec, Channels, Position},
    codecs::{audio::AudioDecoderOptions, registry::CodecRegistry},
    formats::{FormatReader, SeekMode, SeekTo, TrackType, probe::Hint, well_known::FORMAT_ID_FLAC},
    io::{MediaSource, MediaSourceStream},
    units::Time,
};
use termune::Result;

static CODECS: OnceLock<CodecRegistry> = OnceLock::new();

pub(super) fn run(source: impl MediaSource, shared: &Shared) -> Result<()> {
    let seekable = source.is_seekable();
    let stream = MediaSourceStream::new(
        Box::new(PlaybackInput { source, shared }),
        Default::default(),
    );
    let mut format = probe(stream)?;
    let native_flac = format.format_info().format == FORMAT_ID_FLAC;
    let track = format.default_track(TrackType::Audio).ok_or("找不到音轨")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or("无法识别音轨编码")?;
    let track_id = track.id;
    let time_base = track.time_base;
    let start = time_base
        .and_then(|base| base.calc_time(track.start_ts))
        .map_or(0.0, |time| time.as_secs_f64());
    let duration = time_base
        .zip(track.duration)
        .and_then(|(base, duration)| base.calc_duration(duration))
        .map(|time| time.as_secs_f64())
        .or_else(|| {
            track
                .num_frames
                .zip(params.sample_rate)
                .filter(|(_, rate)| *rate > 0)
                .map(|(frames, rate)| frames as f64 / rate as f64)
        })
        .unwrap_or(0.0);

    let codecs = CODECS.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        registry
    });
    let mut decoder = codecs
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|error| format!("不支持此音轨编码：{error}"))?;
    {
        let mut state = shared.playback.lock().unwrap_or_else(|e| e.into_inner());
        state.duration = duration;
        state.seekable = seekable;
    }

    let mut pcm = Vec::new();
    let mut output: Option<StereoOutput> = None;
    let mut trim_to = None;
    let mut decoded = false;
    let mut exhausted = false;
    loop {
        let seek = {
            let mut state = shared.playback.lock().unwrap_or_else(|e| e.into_inner());
            while state.eof && state.seek.is_none() && !shared.closing.load(Ordering::Relaxed) {
                state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if shared.closing.load(Ordering::Relaxed) {
                return Ok(());
            }
            state.seek.take()
        };
        if let Some(target) = seek {
            if target >= duration {
                finish(shared);
                continue;
            }
            if native_flac || exhausted {
                // Rebuild exhausted demuxers. FLAC also retains stale frame
                // state on some seeks in Symphonia 0.6.1.
                let mut stream = format.into_inner();
                stream
                    .rewind()
                    .map_err(|error| format!("无法拖动音频：{error}"))?;
                format = probe(stream)?;
                exhausted = false;
            }
            let time = Time::try_from_secs_f64(target + start).ok_or("无效的播放位置")?;
            format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time,
                        track_id: Some(track_id),
                    },
                )
                .map_err(|error| format!("无法拖动音频：{error}"))?;
            decoder.reset();
            output = None;
            trim_to = Some(target);
        }

        let Some(packet) = format
            .next_packet()
            .map_err(|error| format!("读取音轨失败：{error}"))?
        else {
            exhausted = true;
            if !decoded && trim_to.is_none() {
                return Err("音轨没有可播放的音频".into());
            }
            if let Some(output) = output.as_mut() {
                output.flush(shared)?;
            }
            finish(shared);
            continue;
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = decoder
            .decode(&packet)
            .map_err(|error| format!("音频解码失败：{error}"))?;
        if audio.frames() == 0 {
            continue;
        }
        let spec = audio.spec();
        let rate = spec.rate();
        audio.copy_to_vec_interleaved(&mut pcm);

        let mut skip = 0;
        if let Some(target) = trim_to {
            let position = time_base
                .and_then(|base| base.calc_time(packet.pts))
                .map_or(target, |time| time.as_secs_f64() - start);
            skip = ((target - position) * f64::from(rate)).round().max(0.0) as usize;
            if skip >= audio.frames() {
                continue;
            }
            let mut state = shared.playback.lock().unwrap_or_else(|e| e.into_inner());
            if state.seek.is_some() {
                continue;
            }
            let position = position + skip as f64 / f64::from(rate);
            state.position = (position.max(0.0) * f64::from(shared.rate)).round() as u64;
            trim_to = None;
        }

        if output.as_ref().is_none_or(|output| output.spec != *spec) {
            if let Some(output) = output.as_mut() {
                output.flush(shared)?;
            }
            output = Some(StereoOutput::new(spec.clone(), shared.rate)?);
        }
        let output = output.as_mut().unwrap();
        let frames = pcm.chunks_exact(spec.channels().count()).skip(skip);
        decoded = true;
        output.write(frames, shared)?;
    }
}

fn probe(stream: MediaSourceStream<'_>) -> Result<Box<dyn FormatReader + '_>> {
    symphonia::default::get_probe()
        .probe(&Hint::new(), stream, Default::default(), Default::default())
        .map_err(|error| format!("无法识别音频格式：{error}"))
}

fn finish(shared: &Shared) {
    let mut state = shared.playback.lock().unwrap_or_else(|e| e.into_inner());
    state.eof = state.seek.is_none();
}

// Check cancellation within demuxer operations that perform many reads or seeks.
struct PlaybackInput<'a, M> {
    source: M,
    shared: &'a Shared,
}

impl<M: MediaSource> PlaybackInput<'_, M> {
    fn check(&self) -> io::Result<()> {
        if self.shared.closing.load(Ordering::Relaxed) {
            return Err(io::Error::other("音频播放已停止"));
        }
        Ok(())
    }
}

impl<M: MediaSource> Read for PlaybackInput<'_, M> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.source.read(buffer)
    }
}

impl<M: MediaSource> Seek for PlaybackInput<'_, M> {
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.source.seek(seek)
    }
}

impl<M: MediaSource> MediaSource for PlaybackInput<'_, M> {
    fn is_seekable(&self) -> bool {
        self.source.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.source.byte_len()
    }
}

struct StereoOutput {
    spec: AudioSpec,
    gains: Vec<[f32; 2]>,
    resampler: Option<StereoResampler>,
}

impl StereoOutput {
    fn new(spec: AudioSpec, rate: u32) -> Result<Self> {
        if spec.channels().count() == 0 || spec.rate() == 0 {
            return Err("音轨采样率或声道无效".into());
        }
        let gains = stereo_gains(spec.channels());
        let resampler = (spec.rate() != rate)
            .then(|| StereoResampler::new(spec.rate(), rate))
            .transpose()?;
        Ok(Self {
            spec,
            gains,
            resampler,
        })
    }

    fn write<'a>(
        &mut self,
        frames: impl Iterator<Item = &'a [f32]>,
        shared: &Shared,
    ) -> Result<()> {
        let frames = frames.map(|frame| {
            frame
                .iter()
                .zip(&self.gains)
                .fold([0.0, 0.0], |sum, (&sample, gain)| {
                    [sum[0] + sample * gain[0], sum[1] + sample * gain[1]]
                })
        });
        if let Some(resampler) = &mut self.resampler {
            for [left, right] in frames {
                resampler.input[0].push(left);
                resampler.input[1].push(right);
                resampler.received += 1;
                if resampler.input[0].len() == resampler.converter.input_frames_next() {
                    resampler.process(shared)?;
                }
            }
        } else {
            shared.push(frames);
        }
        Ok(())
    }

    fn flush(&mut self, shared: &Shared) -> Result<()> {
        if let Some(resampler) = &mut self.resampler {
            while resampler.emitted < resampler.expected() {
                resampler.process(shared)?;
            }
        }
        Ok(())
    }
}

fn stereo_gains(channels: &Channels) -> Vec<[f32; 2]> {
    match channels.count() {
        1 => vec![[1.0, 1.0]],
        2 => vec![[1.0, 0.0], [0.0, 1.0]],
        count => {
            let mut gains: Vec<_> = if let Channels::Positioned(positions) = channels {
                let side = std::f32::consts::FRAC_1_SQRT_2;
                positions
                    .iter()
                    .map(|position| match position {
                        Position::FRONT_LEFT => [1.0, 0.0],
                        Position::FRONT_RIGHT => [0.0, 1.0],
                        Position::REAR_LEFT | Position::SIDE_LEFT | Position::FRONT_LEFT_CENTER => {
                            [side, 0.0]
                        }
                        Position::REAR_RIGHT
                        | Position::SIDE_RIGHT
                        | Position::FRONT_RIGHT_CENTER => [0.0, side],
                        Position::LFE1 | Position::LFE2 => [0.0, 0.0],
                        _ => [side, side],
                    })
                    .collect()
            } else {
                vec![[1.0, 1.0]; count]
            };
            let scale = (0..2)
                .map(|side| gains.iter().map(|gain| gain[side]).sum::<f32>())
                .fold(1.0, f32::max);
            for gain in &mut gains {
                gain[0] /= scale;
                gain[1] /= scale;
            }
            gains
        }
    }
}

struct StereoResampler {
    converter: SincFixedIn<f32>,
    input: [Vec<f32>; 2],
    output: Vec<Vec<f32>>,
    delay: usize,
    received: u64,
    emitted: u64,
    source_rate: u32,
    output_rate: u32,
}

impl StereoResampler {
    fn new(source_rate: u32, output_rate: u32) -> Result<Self> {
        let ratio = f64::from(output_rate) / f64::from(source_rate);
        let converter = SincFixedIn::new(
            ratio,
            1.0,
            SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.92,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            },
            256,
            2,
        )
        .map_err(|error| format!("无法初始化音频重采样：{error}"))?;
        let delay = converter.output_delay();
        let output = converter.output_buffer_allocate(true);
        Ok(Self {
            converter,
            input: [Vec::with_capacity(256), Vec::with_capacity(256)],
            output,
            delay,
            received: 0,
            emitted: 0,
            source_rate,
            output_rate,
        })
    }

    fn expected(&self) -> u64 {
        self.received * u64::from(self.output_rate) / u64::from(self.source_rate)
    }

    fn process(&mut self, shared: &Shared) -> Result<()> {
        for channel in &mut self.input {
            channel.resize(self.converter.input_frames_next(), 0.0);
        }
        let (_, produced) = self
            .converter
            .process_into_buffer(&self.input, &mut self.output, None)
            .map_err(|error| format!("音频重采样失败：{error}"))?;
        for channel in &mut self.input {
            channel.clear();
        }
        let start = self.delay.min(produced);
        self.delay -= start;
        let count = (produced - start).min((self.expected() - self.emitted) as usize);
        let end = start + count;
        shared.push(
            self.output[0][start..end]
                .iter()
                .zip(&self.output[1][start..end])
                .map(|(&left, &right)| [left, right]),
        );
        self.emitted += count as u64;
        Ok(())
    }
}
