use crate::{Candidate, Result, Session, business_data, network_error, response_json};
use reqwest::{
    blocking::{Client, Response},
    header::{CONTENT_RANGE, CONTENT_TYPE, COOKIE, HeaderMap, LOCATION, RANGE},
};
use serde_json::Value;
use std::{collections::HashSet, io::Read, path::Path};
use url::Url;

const PAGE_SIZE: u64 = 50;
const MAX_COVER: u64 = 4 * 1024 * 1024;
const AUDIO_SAMPLE: u64 = 4096;

pub struct Page {
    pub items: Vec<Value>,
    pub total: u64,
}

pub fn parse_page(data: Value) -> Result<Page> {
    let items = data["list"]
        .as_array()
        .ok_or("分页响应缺少 list 数组")?
        .clone();
    let total = data["total"]
        .as_u64()
        .or_else(|| data["total"].as_str()?.parse().ok())
        .ok_or("分页响应缺少有效 total")?;
    if total < items.len() as u64 {
        return Err("total 小于当前页数量".into());
    }
    let mut ids = HashSet::new();
    for item in &items {
        let guid = item["guid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("列表项目缺少有效 guid")?;
        if !ids.insert(guid) {
            return Err("当前页出现重复 guid".into());
        }
    }
    Ok(Page { items, total })
}

pub struct LibrarySample {
    pub items: Vec<Value>,
    pub total: u64,
    pub pages: usize,
}

pub struct MediaReport {
    pub status: u16,
    pub bytes: usize,
    pub redirects: usize,
    pub redirect_statuses: Vec<u16>,
    pub cross_origin: bool,
    pub range_supported: bool,
}

// The Cookie is recreated on every hop against the original authenticated
// origin, never copied from a response or sent to a CDN, including mode=relay.
pub fn resource_headers(candidate: &Candidate, target: &Url, token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if candidate.sends_media_credentials_to(target) {
        headers.insert(COOKIE, candidate.cookie_header(Some(token))?);
    }
    Ok(headers)
}

pub fn redirect_target(current: &Url, location: &str) -> Result<Url> {
    let target = current.join(location).map_err(|_| "媒体跳转 URL 错误")?;
    if !target.username().is_empty()
        || target.password().is_some()
        || target.host_str().is_none()
        || (target.scheme() != "https"
            && !(target.scheme() == "http" && target.origin() == current.origin()))
    {
        return Err("拒绝媒体跳转：不安全的协议、降级或内嵌凭据".into());
    }
    Ok(target)
}

impl Session {
    pub fn set_favorite(&self, client: &Client, guid: &str, enabled: bool) -> Result<()> {
        let path = if enabled {
            "/favorite-track/create"
        } else {
            "/favorite-track/delete"
        };
        let response = client
            .post(self.candidate.endpoint(path)?)
            .header(COOKIE, self.candidate.cookie_header(Some(&self.token))?)
            .json(&serde_json::json!({"trackGUID":guid}))
            .send()
            .map_err(network_error)?;
        let value = response_json(response)?;
        if value["code"].as_i64() == Some(0) {
            Ok(())
        } else {
            business_data(value).map(|_| ())
        }
    }
    fn cover_cache_path(&self, cover_id: &str, cache_dir: &Path) -> std::path::PathBuf {
        let user = self.user.get("user").unwrap_or(&self.user);
        let key =
            serde_json::json!([self.candidate.base.as_str(), user["guid"], cover_id]).to_string();
        cache_dir.join("covers").join(crate::hash_password(&key))
    }
    pub fn cached_cover_bytes(
        &self,
        client: &Client,
        cover_id: &str,
        cache_dir: &Path,
    ) -> Result<Vec<u8>> {
        let path = self.cover_cache_path(cover_id, cache_dir);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_COVER)
            && let Ok(bytes) = std::fs::read(&path)
        {
            return Ok(bytes);
        }
        self.cover(client, cover_id, cache_dir)?;
        std::fs::read(path).map_err(|_| "读取封面缓存失败".into())
    }
    pub fn lyric_text(&self, client: &Client, guid: &str) -> Result<Option<String>> {
        let mut url = self.candidate.endpoint("/lyric/list")?;
        url.query_pairs_mut().append_pair("trackGUID", guid);
        let response = client
            .get(url)
            .header(COOKIE, self.candidate.cookie_header(Some(&self.token))?)
            .send()
            .map_err(network_error)?;
        let data = business_data(response_json(response)?)?;
        let list = data["list"].as_array().ok_or("歌词响应缺少 list 数组")?;
        let preferred = data["preferred"].as_str();
        let item = preferred
            .and_then(|id| {
                list.iter().find(|v| {
                    v["guid"].as_str() == Some(id)
                        && v["content"].as_str().is_some_and(|s| !s.trim().is_empty())
                })
            })
            .or_else(|| {
                list.iter()
                    .find(|v| v["content"].as_str().is_some_and(|s| !s.trim().is_empty()))
            });
        Ok(item
            .and_then(|v| v["content"].as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned))
    }
    pub fn audio_stream(&self, client: &Client, guid: &str) -> Result<Response> {
        self.audio_response(client, guid, None)
    }
    pub fn audio_response(
        &self,
        client: &Client,
        guid: &str,
        range: Option<&str>,
    ) -> Result<Response> {
        let mut url = self.candidate.endpoint("/track/stream")?;
        url.query_pairs_mut().append_pair("guid", guid);
        let (response, _, _) = self.resource(client, url, range)?;
        if !matches!(response.status().as_u16(), 200 | 206) {
            return Err(format!("音频 HTTP {}", response.status().as_u16()));
        }
        let mime = media_type(&response);
        if !mime.starts_with("audio/")
            && !matches!(
                mime.as_str(),
                "application/octet-stream" | "application/ogg"
            )
        {
            return Err("音频响应类型无效".into());
        }
        Ok(response)
    }
    pub fn detail_page(&self, client: &Client, kind: &str, guid: &str, page: u64) -> Result<Page> {
        let (path, key) = match kind {
            "album" => ("/track/album-detail/list", "albumGUID"),
            "artist" => ("/track/artist-detail/list", "artistGUID"),
            "playlist" => ("/track/playlist-detail/list", "playlistGUID"),
            "genre" => ("/track/genre-detail/list", "genreGUID"),
            _ => return Err("无效详情类型".into()),
        };
        if page == 0 {
            return Err("页码必须大于 0".into());
        }
        let mut url = self.candidate.endpoint(path)?;
        url.query_pairs_mut()
            .append_pair(key, guid)
            .append_pair("page", &page.to_string())
            .append_pair("size", &PAGE_SIZE.to_string());
        let response = client
            .get(url)
            .header(COOKIE, self.candidate.cookie_header(Some(&self.token))?)
            .send()
            .map_err(network_error)?;
        let parsed = parse_page(business_data(response_json(response)?)?)?;
        if parsed.items.len() as u64 > PAGE_SIZE {
            return Err("服务器返回数量超过请求页大小".into());
        }
        Ok(parsed)
    }

    pub fn page(
        &self,
        client: &Client,
        kind: &str,
        page: u64,
        query: Option<&str>,
    ) -> Result<Page> {
        let path = match (kind, query) {
            ("track", Some(_)) => "/search/track".to_owned(),
            ("track" | "album" | "artist" | "playlist" | "genre", None) => format!("/{kind}/list"),
            ("favorite", None) => "/favorite-track/list".into(),
            _ => return Err("无效曲库类型".into()),
        };
        if page == 0 {
            return Err("页码必须大于 0".into());
        }
        let mut url = self.candidate.endpoint(&path)?;
        url.query_pairs_mut()
            .append_pair("page", &page.to_string())
            .append_pair("size", &PAGE_SIZE.to_string());
        if let Some(query) = query {
            url.query_pairs_mut().append_pair("q", query);
        }
        let response = client
            .get(url)
            .header(COOKIE, self.candidate.cookie_header(Some(&self.token))?)
            .send()
            .map_err(network_error)?;
        let parsed = parse_page(business_data(response_json(response)?)?)?;
        if parsed.items.len() as u64 > PAGE_SIZE {
            return Err("服务器返回数量超过请求页大小".into());
        }
        Ok(parsed)
    }

    /// Probe at most two pages; counts are reported by the NAS, not an assertion
    /// that the entire library has been downloaded.
    pub fn library_sample(
        &self,
        client: &Client,
        kind: &str,
        query: Option<&str>,
    ) -> Result<LibrarySample> {
        let first = self.page(client, kind, 1, query)?;
        let mut result = LibrarySample {
            items: first.items,
            total: first.total,
            pages: 1,
        };
        if result.items.is_empty() && result.total > 0 {
            return Err("total 非零但首页为空".into());
        }
        if result.total > PAGE_SIZE {
            if result.items.len() as u64 != PAGE_SIZE {
                return Err("首页未满但 total 超出页大小，需核对分页协议".into());
            }
            let next = self.page(client, kind, 2, query)?;
            if next.total != result.total {
                return Err("两页 total 变化，请在曲库稳定后重试".into());
            }
            if next.items.is_empty() {
                return Err("分页预期有数据但第二页为空".into());
            }
            let ids: HashSet<_> = result
                .items
                .iter()
                .filter_map(|v| v["guid"].as_str())
                .collect();
            if next
                .items
                .iter()
                .any(|v| v["guid"].as_str().is_some_and(|id| ids.contains(id)))
            {
                return Err("第二页与首页 guid 重复，需核对分页协议或曲库变更".into());
            }
            result.items.extend(next.items);
            result.pages = 2;
        }
        if result.items.len() as u64 > result.total {
            return Err("两页数量超过 total".into());
        }
        Ok(result)
    }

    fn resource(
        &self,
        client: &Client,
        url: Url,
        range: Option<&str>,
    ) -> Result<(Response, Vec<u16>, bool)> {
        let origin = &self.candidate.base;
        let mut current = url;
        let mut seen = HashSet::new();
        let mut cross_origin = false;
        let mut statuses = Vec::new();
        for hops in 0..=5 {
            if !seen.insert(current.clone()) {
                return Err("媒体跳转循环".into());
            }
            let mut request = client.get(current.clone()).headers(resource_headers(
                &self.candidate,
                &current,
                &self.token,
            )?);
            if let Some(range) = range {
                request = request.header(RANGE, range);
            }
            let response = request.send().map_err(network_error)?;
            if response.status().is_redirection() {
                if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                    return Err("不支持的媒体重定向状态".into());
                }
                if hops == 5 {
                    return Err("媒体跳转超过 5 次".into());
                }
                statuses.push(response.status().as_u16());
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|h| h.to_str().ok())
                    .ok_or("媒体跳转缺少 Location")?;
                current = redirect_target(&current, location)?;
                cross_origin |= current.origin() != origin.origin();
                continue;
            }
            return Ok((response, statuses, cross_origin));
        }
        unreachable!()
    }

    pub fn cover(&self, client: &Client, cover_id: &str, cache_dir: &Path) -> Result<MediaReport> {
        let mut url = self.candidate.endpoint("/static/cover")?;
        url.query_pairs_mut()
            .append_pair("coverId", cover_id)
            .append_pair("size", "800");
        let (response, redirects, cross_origin) = self.resource(client, url, None)?;
        if response.status().as_u16() != 200 {
            return Err(format!("封面 HTTP {}", response.status().as_u16()));
        }
        let mime = media_type(&response);
        let bytes = read_limited(response, MAX_COVER, true)?;
        if !valid_image(&mime, &bytes) {
            return Err("封面 Content-Type 或文件签名不是支持的 PNG/JPEG/GIF/WebP".into());
        }
        // Fresh request on every run proves the network endpoint works. Cache is
        // scoped to origin, user GUID and cover ID; no signed URL/token is stored.
        let path = self.cover_cache_path(cover_id, cache_dir);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "创建封面缓存失败")?;
        std::fs::write(path, &bytes).map_err(|_| "保存封面缓存失败")?;
        Ok(MediaReport {
            status: 200,
            bytes: bytes.len(),
            redirects: redirects.len(),
            redirect_statuses: redirects,
            cross_origin,
            range_supported: false,
        })
    }

    pub fn audio(&self, client: &Client, guid: &str) -> Result<MediaReport> {
        let mut url = self.candidate.endpoint("/track/stream")?;
        url.query_pairs_mut().append_pair("guid", guid);
        // A small prefix (4 KiB) also permits signature checks for octet-stream.
        let (response, redirects, cross_origin) =
            self.resource(client, url, Some("bytes=0-4095"))?;
        let status = response.status().as_u16();
        if !matches!(status, 200 | 206) {
            return Err(format!("音频 HTTP {status}"));
        }
        let expected = if status == 206 {
            Some(parse_content_range(
                response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|h| h.to_str().ok())
                    .ok_or("206 缺少 Content-Range")?,
                AUDIO_SAMPLE,
            )?)
        } else {
            None
        };
        let mime = media_type(&response);
        let bytes = read_limited(
            response,
            expected.unwrap_or(AUDIO_SAMPLE),
            expected.is_some(),
        )?;
        if bytes.is_empty() {
            return Err("音频响应为空".into());
        }
        if expected.is_some_and(|size| size != bytes.len() as u64) {
            return Err("音频长度与 Content-Range 不一致".into());
        }
        if !valid_audio(&mime, &bytes) {
            return Err("响应不是可识别的音频（可能是 JSON/HTML 错误页或未知格式）".into());
        }
        Ok(MediaReport {
            status,
            bytes: bytes.len(),
            redirects: redirects.len(),
            redirect_statuses: redirects,
            cross_origin,
            range_supported: status == 206,
        })
    }
}

fn media_type(response: &Response) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn read_limited(response: Response, limit: u64, reject_oversize: bool) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .take(limit + u64::from(reject_oversize))
        .read_to_end(&mut bytes)
        .map_err(|_| "读取媒体响应失败或超时")?;
    if bytes.len() as u64 > limit {
        return Err("媒体响应超过大小限制".into());
    }
    Ok(bytes)
}

pub fn parse_content_range(value: &str, limit: u64) -> Result<u64> {
    let (range, total) = value
        .strip_prefix("bytes ")
        .and_then(|s| s.split_once('/'))
        .ok_or("无效 Content-Range")?;
    let (start, end) = range.split_once('-').ok_or("无效 Content-Range")?;
    let start: u64 = start.parse().map_err(|_| "无效 Range 起点")?;
    let end: u64 = end.parse().map_err(|_| "无效 Range 终点")?;
    if start != 0 || end >= limit {
        return Err("服务器未遵守所请求的 Range 区间".into());
    }
    if total != "*" {
        let total: u64 = total.parse().map_err(|_| "无效 Range 总长度")?;
        if total <= end {
            return Err("无效 Range 总长度".into());
        }
    }
    Ok(end + 1)
}

pub fn valid_image(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    }
}

pub fn valid_audio(mime: &str, bytes: &[u8]) -> bool {
    let trimmed = bytes
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take(16)
        .collect::<Vec<_>>();
    if trimmed.starts_with(b"<") || trimmed.starts_with(b"{") || trimmed.starts_with(b"[") {
        return false;
    }
    if mime.starts_with("audio/") {
        return true;
    }
    if !matches!(mime, "application/octet-stream" | "application/ogg") {
        return false;
    }
    bytes.starts_with(b"fLaC")
        || bytes.starts_with(b"ID3")
        || bytes.starts_with(b"OggS")
        || bytes.starts_with(b"DSD ")
        || bytes.starts_with(b"FRM8")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"))
        || bytes.get(4..8) == Some(b"ftyp")
        || (bytes.first() == Some(&0xff) && bytes.get(1).is_some_and(|b| b & 0xe0 == 0xe0))
}
