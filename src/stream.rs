//! Seekable NAS audio input. Authentication and redirects stay in Session.
use crate::{Result, Session};
use reqwest::blocking::{Client, Response};
use std::io::{self, Read, Seek, SeekFrom};
use symphonia::core::io::MediaSource;

#[derive(Clone)]
pub struct AudioSource {
    pub session: Session,
    pub guid: String,
}

impl AudioSource {
    pub fn open(self) -> Result<AudioStream> {
        let client = crate::client(10)?;
        let response = self.request(&client, 0)?;
        let (seekable, length, end) = if response.status().as_u16() == 206 {
            let (length, end) = content_range(&response, 0)?;
            (true, Some(length), Some(end))
        } else {
            let length = response.content_length();
            (false, length, length)
        };
        Ok(AudioStream {
            source: self,
            client,
            response,
            seekable,
            length,
            end,
            position: 0,
        })
    }

    fn request(&self, client: &Client, start: u64) -> Result<Response> {
        self.session
            .audio_response(client, &self.guid, Some(&format!("bytes={start}-")))
    }
}

pub struct AudioStream {
    source: AudioSource,
    client: Client,
    response: Response,
    seekable: bool,
    length: Option<u64>,
    end: Option<u64>,
    position: u64,
}

impl AudioStream {
    fn reopen(&mut self, start: u64) -> io::Result<()> {
        let response = self
            .source
            .request(&self.client, start)
            .map_err(io::Error::other)?;
        let (length, end) = content_range(&response, start).map_err(io::Error::other)?;
        if Some(length) != self.length {
            return Err(io::Error::other("NAS 音频长度在播放中发生变化"));
        }
        self.response = response;
        self.end = Some(end);
        self.position = start;
        Ok(())
    }
}

impl Read for AudioStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.length.is_some_and(|len| self.position >= len) {
            return Ok(0);
        }
        if self.end == Some(self.position) {
            self.reopen(self.position)?;
        }
        let count = self.end.map_or(buffer.len(), |end| {
            (buffer.len() as u64).min(end - self.position) as usize
        });
        let count = self
            .response
            .read(&mut buffer[..count])
            .map_err(|error| io::Error::new(error.kind(), "NAS 音频流读取失败"))?;
        if count == 0 && self.end.is_some_and(|end| self.position < end) {
            return Err(io::Error::other("NAS 音频流提前结束"));
        }
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for AudioStream {
    fn seek(&mut self, seek: SeekFrom) -> io::Result<u64> {
        let target = match seek {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.length.and_then(|len| len.checked_add_signed(offset)),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "无效的音频偏移"))?;
        if target != self.position {
            if !self.seekable {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "NAS 不支持音频拖动",
                ));
            }
            if self.length.is_some_and(|len| target >= len) {
                self.position = target;
            } else {
                self.reopen(target)?;
            }
        }
        Ok(self.position)
    }
}

impl MediaSource for AudioStream {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        self.length
    }
}

// Returns the full length and the exclusive end of this response.
fn content_range(response: &Response, requested: u64) -> Result<(u64, u64)> {
    let parse = || {
        if response.status().as_u16() != 206 {
            return None;
        }
        let value = response.headers().get("content-range")?.to_str().ok()?;
        let (range, length) = value.strip_prefix("bytes ")?.split_once('/')?;
        let (start, end) = range.split_once('-')?;
        let start = start.parse::<u64>().ok()?;
        let end = end.parse::<u64>().ok()?.checked_add(1)?;
        let length = length.parse::<u64>().ok()?;
        (start == requested
            && start < end
            && end <= length
            && response
                .content_length()
                .is_none_or(|size| size == end - start))
        .then_some((length, end))
    };
    parse().ok_or_else(|| "NAS 返回了无效的音频区间".into())
}
