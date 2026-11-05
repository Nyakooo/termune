use std::{
    process::{Child, Command, Stdio},
    sync::mpsc,
};
use termune::Result;
#[derive(Clone, Copy, Default, Debug)]
pub struct Timeline {
    pub position: f64,
    pub duration: f64,
}
enum Control {
    Pause,
    Seek(f64),
}
pub struct Player {
    child: Child,
    directory: std::path::PathBuf,
    _bridge: termune::stream::Bridge,
    controls: mpsc::Sender<Control>,
    updates: mpsc::Receiver<Timeline>,
    pub title: String,
    pub paused: bool,
    pub timeline: Timeline,
    spectrum: Option<crate::spectrum::Spectrum>,
    spectrum_source: termune::stream::AudioSource,
    pub spectrum_unavailable: bool,
    visualize: bool,
    spectrum_resume_at: Option<(f64, std::time::Instant)>,
}
impl Player {
    pub fn start(source: termune::stream::AudioSource, title: String) -> Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = (source, title);
            return Err("播放暂不支持此操作系统".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let bridge = source.clone().bridge()?;
            let directory = std::env::temp_dir().join(format!("termune-{}", termune::random_id()?));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .map_err(|_| "无法创建播放器控制目录")?;
            let socket = directory.join("control");
            let mut command = Command::new("vlc");
            command
                .args(["--intf", "oldrc", "--rc-fake-tty", "--rc-unix"])
                .arg(&socket)
                .args([
                    "--no-video",
                    "--no-one-instance",
                    "--play-and-exit",
                    "--quiet",
                ])
                .arg(&bridge.url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(test)]
            command.arg("--aout=dummy");
            let child = command.spawn();
            let child = match child {
                Ok(c) => c,
                Err(_) => {
                    let _ = std::fs::remove_dir_all(&directory);
                    return Err("无法启动 VLC，请检查 vlc 是否在 PATH 中".into());
                }
            };
            let (tx, rx) = mpsc::channel();
            let (updates, samples) = mpsc::channel();
            std::thread::spawn(move || control_loop(socket, rx, updates));
            Ok(Self {
                child,
                directory,
                _bridge: bridge,
                controls: tx,
                updates: samples,
                title,
                paused: false,
                timeline: Timeline::default(),
                spectrum: None,
                spectrum_source: source,
                spectrum_unavailable: false,
                visualize: false,
                spectrum_resume_at: None,
            })
        }
    }
    pub fn toggle(&mut self) -> Result<()> {
        self.controls
            .send(Control::Pause)
            .map_err(|_| "播放器控制已断开")?;
        self.paused = !self.paused;
        self.spectrum = None;
        Ok(())
    }
    pub fn seek(&mut self, fraction: f64) -> Result<()> {
        if !fraction.is_finite() || self.timeline.duration <= 0.0 {
            return Err("尚未取得歌曲时长，暂不能拖动".into());
        }
        self.spectrum = None;
        self.spectrum_resume_at = Some((
            fraction.clamp(0.0, 1.0) * self.timeline.duration,
            std::time::Instant::now(),
        ));
        self.controls
            .send(Control::Seek(
                fraction.clamp(0.0, 1.0) * self.timeline.duration,
            ))
            .map_err(|_| "播放器控制已断开".into())
    }
    pub fn visualize(&mut self, enabled: bool) {
        self.visualize = enabled;
        if !enabled {
            self.spectrum = None;
        }
    }
    pub fn spectrum_frame(&mut self) -> Option<[f32; crate::spectrum::BANDS]> {
        self.spectrum.as_mut().and_then(|s| s.frame())
    }
    pub fn poll(&mut self) -> Option<Result<()>> {
        while let Ok(sample) = self.updates.try_recv() {
            self.timeline = sample;
        }
        if self.spectrum.as_mut().is_some_and(|s| s.failed()) {
            self.spectrum = None;
            self.spectrum_unavailable = true;
        }
        if self.visualize
            && !self.paused
            && !self.spectrum_unavailable
            && self.spectrum.is_none()
            && self.timeline.duration > 0.0
        {
            self.spectrum = crate::spectrum::Spectrum::start(
                self.spectrum_source.clone(),
                self.spectrum_resume_at
                    .take()
                    .filter(|(_, time)| time.elapsed().as_secs_f32() < 1.0)
                    .map(|(position, _)| position)
                    .unwrap_or(self.timeline.position),
            );
            self.spectrum_unavailable = self.spectrum.is_none();
        }
        if let Ok(error) = self._bridge.errors.try_recv() {
            return Some(Err(error));
        }
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() => Some(Ok(())),
            Ok(Some(_)) => Some(Err("播放器退出，请检查音频设备与资源".into())),
            Err(_) => Some(Err("无法读取播放器状态".into())),
            Ok(None) => None,
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
#[cfg(unix)]
fn control_loop(path: std::path::PathBuf, rx: mpsc::Receiver<Control>, tx: mpsc::Sender<Timeline>) {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        time::Duration,
    };
    // All IPC runs here; drawing and dragging never wait on the VLC socket.
    loop {
        if !path.parent().is_some_and(|p| p.exists()) {
            return;
        }
        let socket = match UnixStream::connect(&path) {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
        let _ = socket.set_write_timeout(Some(Duration::from_millis(200)));
        let mut reader = BufReader::new(socket);
        loop {
            if !path.parent().is_some_and(|p| p.exists()) {
                return;
            }
            while let Ok(command) = rx.try_recv() {
                let text = match command {
                    Control::Pause => "pause\n".into(),
                    Control::Seek(seconds) => format!("seek {}\n", seconds.round() as u64),
                };
                if reader.get_mut().write_all(text.as_bytes()).is_err() {
                    return;
                }
            }
            if reader
                .get_mut()
                .write_all(b"get_time\nget_length\n")
                .is_err()
            {
                break;
            }
            let mut values = Vec::new();
            for _ in 0..16 {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(value) = line.trim().trim_start_matches('>').trim().parse::<f64>()
                            && value.is_finite()
                            && value >= 0.0
                        {
                            values.push(value);
                            if values.len() == 2 {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if values.len() == 2
                && tx
                    .send(Timeline {
                        position: values[0],
                        duration: values[1],
                    })
                    .is_err()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
#[cfg(not(unix))]
fn control_loop(_: std::path::PathBuf, _: mpsc::Receiver<Control>, _: mpsc::Sender<Timeline>) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires local VLC; uses synthetic WAV and fake NAS, never real credentials"]
    fn real_vlc_bridge_reports_position_pause_seek_and_eof() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::{
                Arc,
                atomic::{AtomicBool, AtomicUsize, Ordering},
            },
            time::{Duration, Instant},
        };
        let duration = 20u32;
        let size = 8000 * 2 * duration;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((size + 36).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(size.to_le_bytes());
        for i in 0..(size / 2) {
            let frequency = if i < 8000 * 10 { 125.0 } else { 1000.0 };
            let value =
                ((std::f32::consts::TAU * frequency * i as f32 / 8000.0).sin() * 16000.0) as i16;
            wav.extend(value.to_le_bytes());
        }
        let wav = Arc::new(wav);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let ranges = Arc::new(AtomicUsize::new(0));
        let seen = ranges.clone();
        let server = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        let wav = wav.clone();
                        let ranges = seen.clone();
                        std::thread::spawn(move || {
                            socket
                                .set_read_timeout(Some(Duration::from_secs(3)))
                                .unwrap();
                            let mut bytes = Vec::new();
                            let mut temp = [0; 2048];
                            loop {
                                let n = socket.read(&mut temp).unwrap();
                                if n == 0 {
                                    return;
                                }
                                bytes.extend_from_slice(&temp[..n]);
                                let text = String::from_utf8_lossy(&bytes);
                                if let Some(end) = text.find("\r\n\r\n") {
                                    let len = text[..end]
                                        .lines()
                                        .find_map(|l| {
                                            l.to_lowercase()
                                                .strip_prefix("content-length:")
                                                .map(|v| v.trim().parse::<usize>().unwrap())
                                        })
                                        .unwrap_or(0);
                                    if bytes.len() >= end + 4 + len {
                                        break;
                                    }
                                }
                            }
                            let text = String::from_utf8(bytes).unwrap();
                            let path = text.split_whitespace().nth(1).unwrap();
                            if path.contains("password-login") || path.contains("user/me") {
                                let body = if path.contains("password-login") {
                                    r#"{"code":0,"data":{"userToken":"synthetic-token"}}"#
                                } else {
                                    r#"{"code":0,"data":{"name":"test","guid":"test-user"}}"#
                                };
                                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                                return;
                            }
                            assert!(
                                text.to_ascii_lowercase()
                                    .contains("cookie: music-token=synthetic-token")
                            );
                            let range = text.lines().find_map(|l| {
                                l.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
                                    .map(|(_, v)| v.trim())
                            });
                            let mut start = 0;
                            let mut end = wav.len() - 1;
                            if let Some(range) = range {
                                ranges.fetch_add(1, Ordering::Relaxed);
                                let (a, b) = range
                                    .strip_prefix("bytes=")
                                    .unwrap()
                                    .split_once('-')
                                    .unwrap();
                                start = a.parse().unwrap();
                                if !b.is_empty() {
                                    end = b.parse::<usize>().unwrap().min(end);
                                }
                            }
                            if start >= wav.len() {
                                let _=socket.write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                return;
                            }
                            let mut h = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: audio/wav\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n",
                                if range.is_some() { 206 } else { 200 },
                                end - start + 1
                            );
                            if range.is_some() {
                                h.push_str(&format!(
                                    "Content-Range: bytes {start}-{end}/{}\r\n",
                                    wav.len()
                                ));
                            }
                            h.push_str("\r\n");
                            let _ = socket.write_all(h.as_bytes());
                            let _ = socket.write_all(&wav[start..=end]);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        let session = termune::Candidate {
            base,
            relay: false,
            label: "test",
        }
        .login(
            &termune::client(3).unwrap(),
            "test",
            "fake-password",
            "device",
        )
        .unwrap();
        let mut player = Player::start(
            termune::stream::AudioSource {
                session,
                guid: "test-track".into(),
            },
            "synthetic WAV".into(),
        )
        .unwrap();
        player.visualize(true);
        let started = Instant::now();
        while player.timeline.duration < 19.0 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "VLC did not report duration"
            );
            assert!(player.poll().is_none());
            std::thread::sleep(Duration::from_millis(80));
        }
        let ffmpeg_available = Command::new("ffmpeg").arg("-version").output().is_ok();
        if ffmpeg_available {
            let started = Instant::now();
            loop {
                assert!(player.poll().is_none());
                if let Some(frame) = player.spectrum_frame() {
                    assert!(frame.iter().copied().fold(0.0f32, f32::max) > 0.8);
                    break;
                }
                assert!(!player.spectrum_unavailable, "FFmpeg decoder failed");
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "No decoded spectrum"
                );
                std::thread::sleep(Duration::from_millis(80));
            }
        } else {
            assert!(player.spectrum_unavailable);
        }
        player.visualize(false);
        assert!(player.spectrum.is_none());
        player.visualize(true);
        player.seek(0.6).unwrap();
        let started = Instant::now();
        while !(11.0..=14.0).contains(&player.timeline.position)
            || (player.timeline.duration - 20.0).abs() > 1.0
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "VLC seek not reflected in actual position"
            );
            assert!(player.poll().is_none());
            std::thread::sleep(Duration::from_millis(80));
        }
        assert!(
            ranges.load(Ordering::Relaxed) > 0,
            "VLC seek must reach NAS as Range"
        );
        if ffmpeg_available {
            let started = Instant::now();
            loop {
                assert!(player.poll().is_none());
                if let Some(frame) = player.spectrum_frame() {
                    let peak = frame
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .unwrap()
                        .0;
                    // The first half is 125 Hz; after seeking to 12s it must be 1000 Hz.
                    let expected = ((1000.0f32 / 30.0).ln() / (8000.0f32 / 30.0).ln()
                        * crate::spectrum::BANDS as f32)
                        as usize;
                    assert_eq!(peak, expected);
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(4),
                    "No spectrum after seek"
                );
                std::thread::sleep(Duration::from_millis(80));
            }
        }

        player.toggle().unwrap();
        assert!(player.spectrum_frame().is_none());
        std::thread::sleep(Duration::from_millis(500));
        player.poll();
        let paused = player.timeline.position;
        std::thread::sleep(Duration::from_millis(600));
        player.poll();
        assert!((player.timeline.position - paused).abs() < 1.1);
        player.toggle().unwrap();
        player.seek(0.95).unwrap();
        let started = Instant::now();
        loop {
            if let Some(result) = player.poll() {
                result.unwrap();
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(7),
                "EOF was not reported"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        drop(player);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
