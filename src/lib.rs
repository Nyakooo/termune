use reqwest::{
    blocking::{Client, Response},
    redirect::Policy,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};
use url::Url;

pub mod connection;
pub mod credentials;
pub mod library;
pub mod stream;
pub mod update;

pub type Result<T> = std::result::Result<T, String>;
const API: &str = "/music/api/v1";
const MAX_JSON: u64 = 1024 * 1024;

pub fn hash_password(password: &str) -> String {
    format!("{:x}", Sha256::digest(password.as_bytes()))
}

pub fn random_id() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| "系统随机数不可用")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn validate_id(id: &str) -> Result<String> {
    let id = id.trim().to_ascii_lowercase();
    if id.is_empty()
        || id.len() > 63
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || id.starts_with('-')
        || id.ends_with('-')
    {
        return Err("请输入 FN ID 本身（字母、数字、内部连字符），无需域名或 URL".into());
    }
    Ok(id)
}

pub fn client(seconds: u64) -> Result<Client> {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(seconds))
        .user_agent("Termune/0.1 login-probe")
        .build()
        .map_err(|_| "HTTP 客户端初始化失败".into())
}

pub fn network_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "请求超时"
    } else if e.is_connect() {
        "连接失败（检查 DNS、网络、端口和 TLS 证书）"
    } else {
        "HTTP 请求失败"
    }
    .into()
}

// Never expose server messages, response bodies, signed URLs, cookies or reqwest URLs.
pub fn response_json(response: Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "HTTP {}{}",
            status.as_u16(),
            if status.is_redirection() {
                "：认证请求不转发，请重新执行地址发现"
            } else {
                ""
            }
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_JSON + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取响应失败")?;
    if bytes.len() as u64 > MAX_JSON {
        return Err("JSON 响应超过 1 MiB 限制".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| "响应不是有效 JSON（可能是远程连接页或安全码页面）".into())
}

pub fn business_data(value: Value) -> Result<Value> {
    match value.get("code").and_then(Value::as_i64) {
        Some(0) => value
            .get("data")
            .filter(|d| !d.is_null())
            .cloned()
            .ok_or("成功响应缺少 data".into()),
        Some(120001) => Err("业务码 120001：音乐账号或密码错误".into()),
        Some(401) => Err("会话失效，请重新登录（业务码 401）".into()),
        Some(_)
            if value["msg"]
                .as_str()
                .is_some_and(|s| s.to_ascii_lowercase().contains("invalid token")) =>
        {
            Err("会话失效，请重新登录（INVALID TOKEN）".into())
        }
        Some(code) => Err(format!("业务码 {code}（为避免泄露凭据，不输出原始消息）")),
        None => Err("响应缺少有效业务 code".into()),
    }
}

pub fn discovery_headers(id: &str, body: &str, nonce: &str, timestamp: &str) -> (String, String) {
    // Protocol values from the official FN Connect web client, not account secrets.
    let raw = format!(
        "NDzZTVxnRKP8Z0jXg1VAMonaG8akvh_/api/v1/fn/con_{nonce}_{timestamp}_{:x}_zIGtkc3dqZnJpd29qZXJqa2w7c",
        md5::compute(body)
    );
    (
        format!(
            "nonce={nonce}&timestamp={timestamp}&sign={:x}",
            md5::compute(raw)
        ),
        hash_password(&format!("trim_connect`{id}`{timestamp}`anna")),
    )
}

pub fn discover(client: &Client, id: &str) -> Result<Value> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "系统时钟错误")?
        .as_millis()
        .to_string();
    let random = random_id()?;
    let nonce = (100000 + u32::from_str_radix(&random[..6], 16).unwrap() % 900000).to_string();
    let body = json!({"fnId": id}).to_string();
    let (authx, sign) = discovery_headers(id, &body, &nonce, &timestamp);
    let response = client
        .post("https://fnos.net/api/v1/fn/con")
        .header("Content-Type", "application/json")
        .header("authx", authx)
        .header("fn-sign", sign)
        .body(body)
        .send()
        .map_err(network_error)?;
    let value = response_json(response)?;
    match value["code"].as_i64() {
        Some(3000006 | 3000037) => {
            Err("FN ID 未找到，请检查 ID 是否正确以及 FN Connect 是否启用".into())
        }
        Some(3000009) => Err("FN Connect 报告网络不可达，请检查 NAS 在线状态".into()),
        _ => business_data(value),
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub base: Url,
    pub relay: bool,
    pub label: &'static str,
}

fn is_lan_ip(addr: std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
        std::net::IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

fn strings<'a>(data: &'a Value, key: &str) -> Vec<&'a str> {
    data[key]
        .as_array()
        .map(|v| v.iter().filter_map(Value::as_str).take(16).collect())
        .unwrap_or_default()
}

pub fn candidates(data: &Value, allow_http: bool) -> Vec<Candidate> {
    let mut result = Vec::new();
    let https = data["port"]["httpsPort"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .unwrap_or(5667);
    let http = data["port"]["httpPort"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .unwrap_or(5666);
    for (key, label) in [
        ("ipv4", "局域网 IPv4"),
        ("ipv6", "局域网 IPv6"),
        ("publicIpv6", "公网 IPv6"),
        ("publicIpv4", "公网 IPv4"),
    ] {
        if key == "publicIpv6" && data["forbbidPublicIpv6"] == true {
            continue;
        }
        for ip in strings(data, key) {
            let Ok(addr) = ip.parse::<std::net::IpAddr>() else {
                continue;
            };
            let host = match addr {
                std::net::IpAddr::V6(_) => format!("[{addr}]"),
                _ => addr.to_string(),
            };
            for (scheme, port) in [("https", https), ("http", http)] {
                // HTTP requires an explicit opt-in and an actual local IP.
                if scheme == "http"
                    && (!allow_http || !is_lan_ip(addr) || !matches!(key, "ipv4" | "ipv6"))
                {
                    continue;
                }
                if let Ok(base) = Url::parse(&format!("{scheme}://{host}:{port}")) {
                    result.push(Candidate {
                        base,
                        relay: false,
                        label,
                    });
                }
            }
        }
    }
    for (key, label, relay) in [
        ("ddns", "DDNS HTTPS", false),
        ("fn", "FN Connect 中继", true),
    ] {
        for host in strings(data, key) {
            let Ok(base) = Url::parse(&format!("https://{host}")) else {
                continue;
            };
            if base.host_str().is_none()
                || !base.username().is_empty()
                || base.password().is_some()
                || base.path() != "/"
                || base.query().is_some()
                || base.fragment().is_some()
            {
                continue;
            }
            result.push(Candidate { base, relay, label });
        }
    }
    result
}

pub fn cookie(relay: bool, token: Option<&str>) -> Result<String> {
    let mut parts = Vec::new();
    if relay {
        parts.push("mode=relay".to_owned());
    }
    if let Some(token) = token {
        if token.is_empty()
            || !token
                .bytes()
                .all(|b| matches!(b, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e))
        {
            return Err("服务器返回无效 Token 格式".into());
        }
        parts.push(format!("music-token={token}"));
    }
    Ok(parts.join("; "))
}

impl Candidate {
    pub(crate) fn endpoint(&self, path: &str) -> Result<Url> {
        Url::parse(&format!(
            "{}{API}{path}",
            self.base.as_str().trim_end_matches('/')
        ))
        .map_err(|_| "API URL 错误".into())
    }
    pub fn get(&self, client: &Client, path: &str, token: Option<&str>) -> Result<Value> {
        let url = self.endpoint(path)?;
        let response = client
            .get(url)
            .header("Cookie", cookie(self.relay, token)?)
            .send()
            .map_err(network_error)?;
        business_data(response_json(response)?)
    }

    // Only unauthenticated discovery GETs may follow redirects. Cookie is rebuilt
    // per hop, never transferred; the chosen endpoint is fixed before login.
    pub fn probe(&self, client: &Client) -> Result<(Self, Value)> {
        let mut candidate = self.clone();
        for _ in 0..=5 {
            let url = candidate.endpoint("/sys/config")?;
            let response = client
                .get(url.clone())
                .header("Cookie", cookie(candidate.relay, None)?)
                .send()
                .map_err(network_error)?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or("跳转缺少 Location")?;
                let mut target = url.join(location).map_err(|_| "跳转 URL 错误")?;
                if target.scheme() != "https"
                    || !target.username().is_empty()
                    || target.password().is_some()
                    || target.query().is_some()
                    || target.fragment().is_some()
                {
                    return Err("拒绝不安全的跳转目标".into());
                }
                let path = target
                    .path()
                    .strip_suffix(&format!("{API}/sys/config"))
                    .ok_or("跳转未指向音乐 API（可能需要安全码）")?
                    .to_owned();
                // Reject cross-origin redirects; the caller independently probes
                // other endpoints from the official discovery response.
                if target.origin() != candidate.base.origin() {
                    return Err(if self.label == connection::MANUAL_ROUTE {
                        "地址发生跨源跳转，请填写最终的 HTTPS 地址"
                    } else {
                        "跨源跳转：继续探测官方返回的其他候选"
                    }
                    .into());
                }
                target.set_path(&path);
                candidate.base = target;
                continue;
            }
            if candidate.label == connection::MANUAL_ROUTE
                && response.status() == reqwest::StatusCode::NOT_FOUND
            {
                return Err(if connection::has_login_suffix(&candidate.base) {
                    "HTTP 404 · 请去掉地址末尾的 /login"
                } else {
                    "HTTP 404 · 请检查 NAS 地址或反向代理路径"
                }
                .into());
            }
            return Ok((candidate, business_data(response_json(response)?)?));
        }
        Err("跳转超过 5 次".into())
    }

    pub fn login(
        &self,
        client: &Client,
        username: &str,
        password: &str,
        device_id: &str,
    ) -> Result<Session> {
        let url = self.endpoint("/user/password-login")?;
        let response = client.post(url).header("Cookie", cookie(self.relay, None)?)
            .json(&json!({"username": username, "password": hash_password(password), "deviceId": device_id}))
            .send().map_err(network_error)?;
        let data = business_data(response_json(response)?)?;
        let token = data["userToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("登录响应缺少 Token")?;
        self.restore(client, token.to_owned())
    }

    pub fn restore(&self, client: &Client, token: String) -> Result<Session> {
        let me = self.get(client, "/user/me", Some(&token))?;
        let user = me.get("user").unwrap_or(&me);
        if user["guid"].as_str().is_none_or(str::is_empty)
            || user["name"].as_str().is_none_or(str::is_empty)
        {
            return Err("user/me 未返回有效 name 和 guid；需核对真实 API 结构".into());
        }
        Ok(Session {
            candidate: self.clone(),
            token,
            user: me,
        })
    }
}

// Deliberately no Debug/Serialize implementation: never print a session token.
#[derive(Clone)]
pub struct Session {
    candidate: Candidate,
    token: String,
    pub user: Value,
}

pub fn is_auth_error(error: &str) -> bool {
    error.contains("HTTP 401") || error.contains("会话失效")
}
