//! LibVLC decodes into a bounded PCM queue. CPAL owns the output clock and DSP.
//! Callbacks remain alive until libvlc_media_player_stop/release returns.
use crate::effects::Processor;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::VecDeque,
    ffi::{CString, c_char, c_int, c_void},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};
use termune::Result;
type Ptr = *mut c_void;
type Setup = unsafe extern "C" fn(*mut Ptr, *mut c_char, *mut u32, *mut u32) -> c_int;
type Play = unsafe extern "C" fn(Ptr, *const c_void, u32, i64);
type Timed = unsafe extern "C" fn(Ptr, i64);
type Drain = unsafe extern "C" fn(Ptr);
struct Api {
    _library: libloading::Library,
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> Ptr,
    release: unsafe extern "C" fn(Ptr),
    media_new_location: unsafe extern "C" fn(Ptr, *const c_char) -> Ptr,
    media_release: unsafe extern "C" fn(Ptr),
    player_new_from_media: unsafe extern "C" fn(Ptr) -> Ptr,
    player_release: unsafe extern "C" fn(Ptr),
    play: unsafe extern "C" fn(Ptr) -> c_int,
    stop: unsafe extern "C" fn(Ptr),
    set_pause: unsafe extern "C" fn(Ptr, c_int),
    get_state: unsafe extern "C" fn(Ptr) -> c_int,
    get_time: unsafe extern "C" fn(Ptr) -> i64,
    get_length: unsafe extern "C" fn(Ptr) -> i64,
    set_time: unsafe extern "C" fn(Ptr, i64),
    set_callbacks: unsafe extern "C" fn(
        Ptr,
        Option<Play>,
        Option<Timed>,
        Option<Timed>,
        Option<Timed>,
        Option<Drain>,
        Ptr,
    ),
    set_format_callbacks: unsafe extern "C" fn(Ptr, Option<Setup>, Option<Drain>),
}
impl Api {
    fn load() -> Result<Arc<Self>> {
        #[cfg(target_os = "linux")]
        let paths = vec![std::path::PathBuf::from("libvlc.so.5")];
        #[cfg(target_os = "macos")]
        let paths: Vec<std::path::PathBuf> = vec![
            "/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib".into(),
            "libvlc.dylib".into(),
        ];
        #[cfg(windows)]
        let paths = {
            let mut paths = Vec::new();
            for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(base) = std::env::var_os(key) {
                    paths.push(std::path::PathBuf::from(base).join("VideoLAN/VLC/libvlc.dll"));
                }
            }
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    paths.push(dir.join("libvlc.dll"));
                }
            }
            paths
        };
        let library: libloading::Library = paths
            .into_iter()
            .find_map(|path| {
                // Windows must resolve libvlccore.dll beside libvlc.dll as well.
                #[cfg(windows)]
                {
                    unsafe {
                        libloading::os::windows::Library::load_with_flags(&path, 0x00000008)
                            .ok()
                            .map(libloading::Library::from)
                    }
                }
                #[cfg(not(windows))]
                {
                    unsafe { libloading::Library::new(path).ok() }
                }
            })
            .ok_or("无法加载 VLC 3 音频库，请安装与 Termune 架构一致的 VLC 3")?;
        let version = unsafe {
            let get_version: libloading::Symbol<unsafe extern "C" fn() -> *const c_char> = library
                .get(b"libvlc_get_version\0")
                .map_err(|_| "无法读取 VLC 版本")?;
            std::ffi::CStr::from_ptr(get_version())
                .to_string_lossy()
                .into_owned()
        };
        if !version.starts_with("3.") {
            return Err(format!("当前音频引擎需要 VLC 3，检测到 {version}"));
        }
        macro_rules! symbol {
            ($name:literal) => {
                unsafe {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|_| concat!("VLC 3 缺少接口：", $name))?
                }
            };
        }
        Ok(Arc::new(Self {
            new: symbol!("libvlc_new"),
            release: symbol!("libvlc_release"),
            media_new_location: symbol!("libvlc_media_new_location"),
            media_release: symbol!("libvlc_media_release"),
            player_new_from_media: symbol!("libvlc_media_player_new_from_media"),
            player_release: symbol!("libvlc_media_player_release"),
            play: symbol!("libvlc_media_player_play"),
            stop: symbol!("libvlc_media_player_stop"),
            set_pause: symbol!("libvlc_media_player_set_pause"),
            get_state: symbol!("libvlc_media_player_get_state"),
            get_time: symbol!("libvlc_media_player_get_time"),
            get_length: symbol!("libvlc_media_player_get_length"),
            set_time: symbol!("libvlc_media_player_set_time"),
            set_callbacks: symbol!("libvlc_audio_set_callbacks"),
            set_format_callbacks: symbol!("libvlc_audio_set_format_callbacks"),
            _library: library,
        }))
    }
}
struct Shared {
    samples: Mutex<VecDeque<[f32; 2]>>,
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
    fn take(&self, output: &mut [[f32; 2]], dsp: &mut Processor) {
        let effect = self.effect.load(Ordering::Relaxed);
        let volume = self.volume.load(Ordering::Relaxed);
        if self.reset.swap(false, Ordering::Relaxed) {
            dsp.reset(effect, volume);
        }
        dsp.set(effect, volume);
        let mut queue = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        for frame in output {
            *frame = if self.paused.load(Ordering::Relaxed) {
                dsp.render(None)
            } else {
                match queue.pop_front() {
                    Some(sample) => {
                        #[cfg(test)]
                        self.rendered.fetch_add(1, Ordering::Relaxed);
                        dsp.render(Some(sample))
                    }
                    None => dsp.render(None),
                }
            };
        }
        drop(queue);
        self.wake.notify_all();
    }
}
unsafe extern "C" fn audio_setup(
    data: *mut Ptr,
    format: *mut c_char,
    rate: *mut u32,
    channels: *mut u32,
) -> c_int {
    unsafe {
        let shared = &*(*data as *const Shared);
        std::ptr::copy_nonoverlapping(b"S16N".as_ptr(), format.cast::<u8>(), 4);
        *rate = shared.rate;
        *channels = 2;
    }
    0
}
unsafe extern "C" fn audio_play(data: Ptr, samples: *const c_void, count: u32, _pts: i64) {
    // VLC 3 amem supports S16N. The setup callback fixes stereo and device rate.
    let shared = unsafe { &*(data as *const Shared) };
    let samples = unsafe { std::slice::from_raw_parts(samples as *const i16, count as usize * 2) };
    let mut queue = shared.samples.lock().unwrap_or_else(|e| e.into_inner());
    for frame in samples.as_chunks::<2>().0 {
        while queue.len() >= shared.rate as usize / 3 && !shared.closing.load(Ordering::Relaxed) {
            let result = shared
                .wake
                .wait_timeout(queue, Duration::from_millis(20))
                .unwrap_or_else(|e| e.into_inner());
            queue = result.0;
        }
        if shared.closing.load(Ordering::Relaxed) {
            return;
        }
        queue.push_back([frame[0] as f32 / 32768.0, frame[1] as f32 / 32768.0]);
    }
}
unsafe extern "C" fn audio_pause(data: Ptr, _pts: i64) {
    unsafe { &*(data as *const Shared) }
        .paused
        .store(true, Ordering::Relaxed);
}
unsafe extern "C" fn audio_resume(data: Ptr, _pts: i64) {
    unsafe { &*(data as *const Shared) }
        .paused
        .store(false, Ordering::Relaxed);
}
unsafe extern "C" fn audio_flush(data: Ptr, _pts: i64) {
    let shared = unsafe { &*(data as *const Shared) };
    shared
        .samples
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    shared.reset.store(true, Ordering::Relaxed);
    shared.wake.notify_all();
}
unsafe extern "C" fn audio_drain(data: Ptr) {
    let shared = unsafe { &*(data as *const Shared) };
    let mut queue = shared.samples.lock().unwrap_or_else(|e| e.into_inner());
    while !queue.is_empty() && !shared.closing.load(Ordering::Relaxed) {
        let result = shared
            .wake
            .wait_timeout(queue, Duration::from_millis(20))
            .unwrap_or_else(|e| e.into_inner());
        queue = result.0;
    }
}
pub struct Decoder {
    api: Arc<Api>,
    instance: Ptr,
    player: Ptr,
    shared: Arc<Shared>,
    _stream: Option<cpal::Stream>,
    #[cfg(test)]
    sink: Option<std::thread::JoinHandle<()>>,
}
impl Decoder {
    pub fn start(url: &str, effect: usize, volume: u16) -> Result<Self> {
        Self::start_mode(url, effect, volume, cfg!(test))
    }
    fn start_mode(url: &str, effect: usize, volume: u16, headless: bool) -> Result<Self> {
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
        let shared = Arc::new(Shared {
            samples: Mutex::new(VecDeque::with_capacity(rate as usize / 3)),
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
        });
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
        let api = Api::load()?;
        // Ignore VLC's saved filters: all DSP and volume now belong to Termune.
        let args = [
            c"--no-video",
            c"--no-osd",
            c"--quiet",
            c"--ignore-config",
            c"--no-audio-time-stretch",
        ];
        let args: Vec<_> = args.iter().map(|s| s.as_ptr()).collect();
        let instance = unsafe { (api.new)(args.len() as c_int, args.as_ptr()) };
        if instance.is_null() {
            return Err("无法初始化 VLC 3 解码器".into());
        }
        let url = CString::new(url).map_err(|_| "无效的音频地址")?;
        let media = unsafe { (api.media_new_location)(instance, url.as_ptr()) };
        if media.is_null() {
            unsafe { (api.release)(instance) };
            return Err("无法打开音频资源".into());
        }
        let player = unsafe { (api.player_new_from_media)(media) };
        unsafe { (api.media_release)(media) };
        if player.is_null() {
            unsafe { (api.release)(instance) };
            return Err("无法创建音频解码器".into());
        }
        unsafe {
            (api.set_callbacks)(
                player,
                Some(audio_play),
                Some(audio_pause),
                Some(audio_resume),
                Some(audio_flush),
                Some(audio_drain),
                Arc::as_ptr(&shared) as Ptr,
            );
            (api.set_format_callbacks)(player, Some(audio_setup), None);
        }
        #[cfg(test)]
        let sink = if headless {
            let shared = shared.clone();
            Some(std::thread::spawn(move || {
                let mut dsp = Processor::new(rate, effect, volume);
                let mut frames = vec![[0.0; 2]; rate as usize / 100];
                while !shared.closing.load(Ordering::Relaxed) {
                    shared.take(&mut frames, &mut dsp);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }))
        } else {
            None
        };
        let result = Self {
            api,
            instance,
            player,
            shared,
            _stream: stream,
            #[cfg(test)]
            sink,
        };
        if unsafe { (result.api.play)(player) } != 0 {
            return Err("VLC 无法开始解码".into());
        }
        Ok(result)
    }
    #[cfg(all(test, unix))]
    pub fn player_id_for_test(&self) -> usize {
        self.player as usize
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
        unsafe { (self.api.set_pause)(self.player, paused as c_int) };
    }
    pub fn seek(&self, seconds: f64) {
        unsafe { (self.api.set_time)(self.player, (seconds * 1000.0).round() as i64) };
    }
    pub fn time(&self) -> (f64, f64) {
        unsafe {
            (
                (self.api.get_time)(self.player).max(0) as f64 / 1000.0,
                (self.api.get_length)(self.player).max(0) as f64 / 1000.0,
            )
        }
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
        match unsafe { (self.api.get_state)(self.player) } {
            6 => Some(Ok(())),
            7 => Some(Err("音频解码失败，请检查资源与 VLC".into())),
            _ => None,
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        self.shared.closing.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        unsafe {
            (self.api.stop)(self.player);
            (self.api.player_release)(self.player);
            (self.api.release)(self.instance);
        }
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
                errors.closing.store(true, Ordering::Relaxed);
                errors.wake.notify_all();
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
    #[test]
    fn live_switch_changes_pcm_beyond_simple_volume_gain() {
        let shared = Shared {
            samples: Mutex::new(VecDeque::with_capacity(480)),
            wake: Condvar::new(),
            closing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            effect: AtomicUsize::new(0),
            volume: AtomicU16::new(100),
            reset: AtomicBool::new(false),
            error: Mutex::new(None),
            rate: 48000,
            rendered: AtomicUsize::new(0),
        };
        let mut dsp = Processor::new(48000, 0, 100);
        for effect in 1..5 {
            shared.effect.store(effect, Ordering::Relaxed);
            let mut source_energy = 0.0f64;
            let mut output_energy = 0.0f64;
            let mut dot = 0.0f64;
            for block in 0..80 {
                let samples: Vec<i16> = (0..480)
                    .flat_map(|i| {
                        let t = (block * 480 + i) as f32 / 48000.0;
                        let l = (t * std::f32::consts::TAU * 90.0).sin() * 0.2
                            + (t * std::f32::consts::TAU * 1700.0).sin() * 0.1;
                        let r = (t * std::f32::consts::TAU * 330.0).sin() * 0.2;
                        [(l * 32768.0) as i16, (r * 32768.0) as i16]
                    })
                    .collect();
                unsafe {
                    audio_play(
                        &shared as *const Shared as Ptr,
                        samples.as_ptr().cast(),
                        480,
                        0,
                    );
                }
                let mut output = [[0.0; 2]; 480];
                shared.take(&mut output, &mut dsp);
                if block > 40 {
                    for (input, output) in samples.as_chunks::<2>().0.iter().zip(output) {
                        for c in 0..2 {
                            let x = input[c] as f64 / 32768.0;
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
    fn callback_negotiates_s16_and_preserves_each_channel() {
        let shared = Shared {
            samples: Mutex::new(VecDeque::with_capacity(4)),
            wake: Condvar::new(),
            closing: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            effect: AtomicUsize::new(0),
            volume: AtomicU16::new(100),
            reset: AtomicBool::new(false),
            error: Mutex::new(None),
            rate: 48000,
            rendered: AtomicUsize::new(0),
        };
        let mut opaque = &shared as *const Shared as Ptr;
        let mut format = [0u8; 4];
        let mut rate = 0;
        let mut channels = 0;
        unsafe {
            assert_eq!(
                audio_setup(
                    &mut opaque,
                    format.as_mut_ptr().cast(),
                    &mut rate,
                    &mut channels
                ),
                0
            );
        }
        assert_eq!(format, *b"S16N");
        assert_eq!((rate, channels), (48000, 2));
        let samples = [8192i16, -16384, -4096, 12288];
        unsafe {
            audio_play(opaque, samples.as_ptr().cast(), 2, 0);
        }
        let mut output = [[0.0; 2]; 2];
        shared.take(&mut output, &mut Processor::new(48000, 0, 100));
        assert_eq!(output, [[0.25, -0.5], [-0.125, 0.375]]);
    }
    #[test]
    #[ignore = "requires local VLC 3 and an output device; plays only silence"]
    fn real_output_device_plays_silence_to_eof() {
        let path = std::env::temp_dir().join(format!(
            "termune-silence-{}.wav",
            termune::random_id().unwrap()
        ));
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
        std::fs::write(&path, wav).unwrap();
        let url = url::Url::from_file_path(&path).unwrap();
        let decoder = Decoder::start_mode(url.as_str(), 0, 100, false).unwrap();
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
        std::fs::remove_file(path).unwrap();
    }
}
