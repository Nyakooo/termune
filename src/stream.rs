//! Loopback-only seekable media bridge. NAS credentials never reach VLC.
use crate::{Result, Session};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone)]
pub struct AudioSource {
    pub session: Session,
    pub guid: String,
}
pub struct Bridge {
    pub url: String,
    stop: Arc<AtomicBool>,
    pub errors: mpsc::Receiver<String>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl AudioSource {
    pub fn bridge(self) -> Result<Bridge> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "无法创建本机音频通道")?;
        let address = listener.local_addr().map_err(|_| "读取本机音频端口失败")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "无法配置音频通道")?;
        let path = format!("/{}", crate::random_id()?);
        let url = format!("http://{address}{path}");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let source = Arc::new(self);
        let active = Arc::new(AtomicUsize::new(0));
        let (tx, errors) = mpsc::channel();
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        if active.fetch_add(1, Ordering::Relaxed) >= 6 {
                            active.fetch_sub(1, Ordering::Relaxed);
                            drop(socket);
                            continue;
                        }
                        let source = source.clone();
                        let path = path.clone();
                        let flag = flag.clone();
                        let active = active.clone();
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            if let Err(error) = serve(socket, &path, &source, &flag)
                                && !flag.load(Ordering::Relaxed)
                            {
                                let _ = tx.send(error);
                            }
                            active.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(15))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Bridge { url, stop, errors })
    }
}
fn serve(mut socket: TcpStream, path: &str, source: &AudioSource, stop: &AtomicBool) -> Result<()> {
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| "音频请求超时设置失败")?;
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "音频通道设置失败")?;
    let mut header = Vec::new();
    let mut byte = [0];
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 8192 {
            return Err("本机音频请求头过长".into());
        }
        if socket.read(&mut byte).map_err(|_| "本机音频请求失败")? == 0 {
            return Ok(());
        }
        header.push(byte[0]);
    }
    let text = std::str::from_utf8(&header).map_err(|_| "本机音频请求编码错误")?;
    let mut lines = text.lines();
    let request = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();
    if request.len() != 3 || request[1] != path || !matches!(request[0], "GET" | "HEAD") {
        let _ = socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Ok(());
    }
    let range = lines.find_map(|line| {
        line.split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
            .map(|(_, v)| v.trim())
    });
    if range.is_some_and(|s| {
        !s.starts_with("bytes=")
            || s.len() > 80
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || b"bytes=-".contains(&b))
    }) {
        return Err("本机音频 Range 格式无效".into());
    }
    let response = match source
        .session
        .audio_response(&crate::client(3600)?, &source.guid, range)
    {
        Ok(r) => r,
        Err(e) => {
            let _ = socket.write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return Err(e);
        }
    };
    if range.is_some_and(|s| !s.starts_with("bytes=0-")) && response.status().as_u16() != 206 {
        return Err("NAS 不支持所请求的拖动区间".into());
    }
    if let Some(range) = range
        && response.status().as_u16() == 206
    {
        let start = range
            .trim_start_matches("bytes=")
            .split('-')
            .next()
            .unwrap_or("0");
        let prefix = format!("bytes {start}-");
        if !response
            .headers()
            .get("content-range")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.starts_with(&prefix))
        {
            return Err("NAS 返回的音频区间与拖动位置不一致".into());
        }
    }
    let status = response.status().as_u16();
    let mut headers = format!("HTTP/1.1 {status} OK\r\nConnection: close\r\n");
    for name in [
        "content-type",
        "content-length",
        "content-range",
        "accept-ranges",
    ] {
        if let Some(value) = response.headers().get(name).and_then(|h| h.to_str().ok()) {
            headers.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    headers.push_str("\r\n");
    if socket.write_all(headers.as_bytes()).is_err() {
        return Ok(());
    }
    if request[0] == "HEAD" {
        return Ok(());
    }
    let mut reader = response;
    let mut buffer = [0; 32768];
    while !stop.load(Ordering::Relaxed) {
        let n = reader.read(&mut buffer).map_err(|_| "NAS 音频流读取失败")?;
        if n == 0 {
            break;
        }
        if socket.write_all(&buffer[..n]).is_err() {
            break;
        }
    }
    Ok(())
}
