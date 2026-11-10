use termune::Result;
#[derive(Clone, Copy, Default, Debug)]
pub struct Timeline {
    pub position: f64,
    pub duration: f64,
}
pub struct Player {
    decoder: crate::audio::Decoder,
    pub title: String,
    pub paused: bool,
    pub timeline: Timeline,
}
impl Player {
    #[cfg(test)]
    pub(crate) fn volume_probe(volume: u16) -> (Self, impl Fn() -> u16) {
        let (decoder, probe) = crate::audio::Decoder::volume_probe(volume);
        (
            Self {
                decoder,
                title: String::new(),
                paused: false,
                timeline: Timeline::default(),
            },
            probe,
        )
    }

    pub fn start(
        source: termune::stream::AudioSource,
        title: String,
        effect: usize,
        volume: u16,
    ) -> Result<Self> {
        let decoder = crate::audio::Decoder::start(source, effect, volume)?;
        Ok(Self {
            decoder,
            title,
            paused: false,
            timeline: Timeline::default(),
        })
    }
    pub fn set_effect(&mut self, effect: usize) {
        self.decoder.set_effect(effect);
    }
    pub fn set_volume(&mut self, volume: u16) {
        self.decoder.volume(volume.min(100));
    }
    pub fn toggle(&mut self) -> Result<()> {
        self.paused = !self.paused;
        self.decoder.pause(self.paused);
        Ok(())
    }
    pub fn seek(&mut self, fraction: f64) -> Result<()> {
        if !fraction.is_finite() || self.timeline.duration <= 0.0 {
            return Err("尚未取得歌曲时长，暂不能拖动".into());
        }
        self.decoder
            .seek(fraction.clamp(0.0, 1.0) * self.timeline.duration)
    }
    pub fn poll(&mut self) -> Option<Result<()>> {
        let (position, duration) = self.decoder.time();
        self.timeline = Timeline { position, duration };
        self.decoder.finished()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nas_stream_reports_position_pause_seek_and_eof() {
        exercise(true);
    }

    #[test]
    fn nas_stream_without_ranges_still_plays() {
        exercise(false);
    }

    fn exercise(supports_ranges: bool) {
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
                        socket.set_nonblocking(false).unwrap();
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
                            let range = text.lines().filter(|_| supports_ranges).find_map(|l| {
                                l.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
                                    .map(|(_, v)| v.trim())
                            });
                            let mut start = 0;
                            let mut end = wav.len() - 1;
                            if let Some(range) = range {
                                let (a, b) = range
                                    .strip_prefix("bytes=")
                                    .unwrap()
                                    .split_once('-')
                                    .unwrap();
                                start = a.parse().unwrap();
                                if start > 0 && start < wav.len() {
                                    ranges.fetch_add(1, Ordering::Relaxed);
                                }
                                if !b.is_empty() {
                                    end = b.parse::<usize>().unwrap().min(end);
                                }
                            }
                            if start >= wav.len() {
                                let _=socket.write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                                return;
                            }
                            let mut h = format!(
                                "HTTP/1.1 {} OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n",
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
        let session = termune::Candidate::new(base, false, "test")
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
            0,
            100,
        )
        .unwrap();
        let started = Instant::now();
        while player.timeline.duration < 19.0 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "Decoder did not report duration"
            );
            assert!(player.poll().is_none());
            std::thread::sleep(Duration::from_millis(80));
        }
        let before = player.timeline.position;
        let rendered = player.decoder.rendered_for_test();
        for effect in 0..crate::effects::Effect::COUNT {
            player.set_effect(effect);
            assert!(player.timeline.duration > 19.0);
            std::thread::sleep(Duration::from_millis(80));
            assert!(player.poll().is_none());
            assert!(player.timeline.position >= before);
        }
        assert!(player.decoder.rendered_for_test() > rendered);
        if !supports_ranges {
            let before = player.timeline.position;
            assert!(player.seek(0.6).is_err());
            std::thread::sleep(Duration::from_millis(80));
            assert!(player.poll().is_none());
            assert!(player.timeline.position > before);
            drop(player);
            done.store(true, Ordering::Relaxed);
            server.join().unwrap();
            return;
        }
        let before_seek = ranges.load(Ordering::Relaxed);
        let before_seek_frames = player.decoder.rendered_for_test();
        player.seek(0.6).unwrap();
        let started = Instant::now();
        while !(11.0..=14.0).contains(&player.timeline.position)
            || (player.timeline.duration - 20.0).abs() > 1.0
            || ranges.load(Ordering::Relaxed) == before_seek
            || player.decoder.rendered_for_test() <= before_seek_frames
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "Seek not reflected in actual position"
            );
            assert!(player.poll().is_none());
            std::thread::sleep(Duration::from_millis(80));
        }
        assert!(
            ranges.load(Ordering::Relaxed) > before_seek,
            "Seek must reach NAS as Range"
        );
        player.toggle().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        player.poll();
        let paused = player.timeline.position;
        std::thread::sleep(Duration::from_millis(600));
        player.poll();
        assert_eq!(player.timeline.position, paused);
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
        assert!((player.timeline.position - 20.0).abs() < 0.02);
        drop(player);
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
