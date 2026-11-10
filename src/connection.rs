use crate::{Candidate, Probe, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::net::Ipv6Addr;
use url::Url;

pub const MANUAL_ROUTE: &str = "手动地址";

pub fn has_login_suffix(address: &Url) -> bool {
    address.path().trim_end_matches('/').ends_with("/login")
}

pub enum Target {
    FnId(String),
    Manual(Url),
}

impl Target {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if let Ok(id) = crate::validate_id(input) {
            return Ok(Self::FnId(id));
        }
        let invalid = "请输入 FN ID 或有效的 NAS 地址（例如 https://nas.example.com）";
        if input.is_empty()
            || input.contains('\\')
            || input.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(invalid.into());
        }
        let address = if input.contains("://") {
            input.to_owned()
        } else if let Ok(ip) = input.parse::<Ipv6Addr>() {
            format!("https://[{ip}]")
        } else if input
            .split('/')
            .next()
            .is_some_and(|host| host.contains(['.', ':']))
        {
            format!("https://{input}")
        } else {
            return Err(invalid.into());
        };
        let authority = address
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
            .filter(|host| !host.is_empty() && !host.contains('@'))
            .ok_or(invalid)?;
        let mut url = Url::parse(&address).map_err(|_| invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
            || authority.ends_with(':')
        {
            return Err(invalid.into());
        }
        if url.scheme() == "http" && !crate::is_lan_ip_host(&url) {
            return Err("HTTP 仅支持局域网 IP，请填写 HTTPS 地址".into());
        }
        let path = url.path().trim_end_matches('/').to_owned();
        if path
            .strip_prefix("/c/")
            .is_some_and(|code| !code.is_empty())
        {
            return Err("请填写不含访问码的 NAS 基础地址，访问码将在连接时单独输入".into());
        }
        url.set_path(&path);
        Ok(Self::Manual(url))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::FnId(id) => id,
            Self::Manual(url) => url.as_str(),
        }
    }

    pub fn requires_lan_http(&self) -> bool {
        matches!(self, Self::Manual(url) if url.scheme() == "http")
    }

    pub fn manual_candidate(&self, allow_http: bool) -> Result<Option<Candidate>> {
        let Self::Manual(base) = self else {
            return Ok(None);
        };
        if self.requires_lan_http() && !allow_http {
            return Err("此地址使用 HTTP。请在设置中开启「局域网 HTTP」，或填写 HTTPS 地址".into());
        }
        Ok(Some(Candidate::new(base.clone(), false, MANUAL_ROUTE)))
    }

    pub fn connect(
        &self,
        client: &Client,
        allow_http: bool,
        progress: impl FnMut(&'static str),
    ) -> Result<Connection> {
        match self.connect_with_challenge(client, allow_http, progress)? {
            ConnectionAttempt::Ready(connection) => Ok(connection),
            ConnectionAttempt::AccessCode(_) => Err("飞牛要求验证访问码".into()),
        }
    }

    pub fn connect_with_challenge(
        &self,
        client: &Client,
        allow_http: bool,
        mut progress: impl FnMut(&'static str),
    ) -> Result<ConnectionAttempt> {
        match self.manual_candidate(allow_http)? {
            Some(candidate) => {
                progress("manual_probe");
                complete_probe(client, candidate.probe_route(client)?, progress)
            }
            None => discover_connection(client, self.as_str(), allow_http, progress),
        }
    }
}

pub struct Connection {
    pub candidate: Candidate,
    pub config: Value,
}

pub enum ConnectionAttempt {
    Ready(Connection),
    AccessCode(Candidate),
}

pub fn complete_probe(
    client: &Client,
    probe: Probe,
    progress: impl FnMut(&'static str),
) -> Result<ConnectionAttempt> {
    match probe {
        Probe::Ready(candidate, config) => {
            check_initialization(client, &candidate, progress)?;
            Ok(ConnectionAttempt::Ready(Connection { candidate, config }))
        }
        Probe::AccessCode(candidate) => Ok(ConnectionAttempt::AccessCode(candidate)),
    }
}

/// Same FN Connect discovery and identity checks as the verified CLI probe.
/// Progress carries only fixed stage names, never URLs or credentials.
fn discover_connection(
    client: &Client,
    id: &str,
    allow_http: bool,
    mut progress: impl FnMut(&'static str),
) -> Result<ConnectionAttempt> {
    let id = crate::validate_id(id)?;
    progress("discovery");
    let data = crate::discover(client, &id)?;
    let candidates = crate::candidates(&data, allow_http);
    if candidates.is_empty() {
        return Err("官方未返回可用地址，请检查 FN Connect 和 NAS 在线状态".into());
    }
    progress("probing");
    let results = std::thread::scope(|scope| {
        let jobs: Vec<_> = candidates
            .iter()
            .map(|candidate| {
                scope.spawn(move || {
                    candidate.probe_route(&crate::client(if candidate.relay { 12 } else { 3 })?)
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap_or_else(|_| Err("探测线程异常".into())))
            .collect::<Vec<_>>()
    });
    let guid = results
        .iter()
        .find_map(|result| match result {
            Ok(Probe::Ready(candidate, config)) if candidate.relay => Some(config),
            _ => None,
        })
        .and_then(|config| config["serverGUID"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let challenge = results.iter().find_map(|result| match result {
        Ok(Probe::AccessCode(candidate)) => Some(candidate.clone()),
        _ => None,
    });
    let probe = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .find(|probe| match probe {
            Probe::Ready(_, config) => guid
                .as_ref()
                .is_none_or(|expected| config["serverGUID"].as_str() == Some(expected)),
            Probe::AccessCode(_) => false,
        })
        .or_else(|| challenge.map(Probe::AccessCode))
        .ok_or("所有连接线路失败：请检查网络、TLS 证书、中继安全码和 NAS 在线状态")?;
    complete_probe(client, probe, progress)
}

fn check_initialization(
    client: &Client,
    candidate: &Candidate,
    mut progress: impl FnMut(&'static str),
) -> Result<()> {
    progress("initialization");
    if let Ok(state) = candidate.get(client, "/initialization/state", None)
        && (state == false
            || ["initialized", "isInitialized"]
                .iter()
                .any(|key| state.get(key) == Some(&Value::Bool(false))))
    {
        return Err("音乐服务尚未初始化，请先在 NAS 网页完成设置".into());
    }
    Ok(())
}
