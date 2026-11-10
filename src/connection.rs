use crate::{Candidate, Probe, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::net::Ipv6Addr;
use url::Url;

pub const MANUAL_ROUTE: &str = "手动地址";
pub const ACCESS_CODE_USE_DIRECT_ADDRESS: &str =
    "FN ID 的直连线路要求访问码，请手动填写确认过的 NAS 地址";

pub fn has_login_suffix(address: &Url) -> bool {
    address.path().trim_end_matches('/').ends_with("/login")
}

pub enum Target {
    FnId(String),
    Manual {
        base: Url,
        explicit_default_port_address: Option<String>,
    },
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
        // URL normalization removes :80/:443. Keep the input in that case so
        // saving or resubmitting the address still honors the explicit port.
        let explicit_default_port_address = (url.port().is_none()
            && url.path() == "/"
            && crate::is_lan_ip_host(&url)
            && authority
                .rsplit_once(':')
                .is_some_and(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit())))
        .then_some(address);
        Ok(Self::Manual {
            base: url,
            explicit_default_port_address,
        })
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::FnId(id) => id,
            Self::Manual { base, .. } => base.as_str(),
        }
    }

    pub fn input_address(&self) -> &str {
        match self {
            Self::Manual {
                explicit_default_port_address: Some(address),
                ..
            } => address,
            _ => self.as_str(),
        }
    }

    pub fn requires_lan_http(&self) -> bool {
        matches!(self, Self::Manual { base, .. } if base.scheme() == "http")
    }

    pub fn manual_candidate(&self, allow_http: bool) -> Result<Option<Candidate>> {
        let Self::Manual { base, .. } = self else {
            return Ok(None);
        };
        if self.requires_lan_http() && !allow_http {
            return Err("此地址使用 HTTP。请在设置中开启「局域网 HTTP」，或填写 HTTPS 地址".into());
        }
        Ok(Some(Candidate::new(base.clone(), false, MANUAL_ROUTE)))
    }

    pub fn manual_candidates(&self, allow_http: bool) -> Result<Vec<Candidate>> {
        let Some(candidate) = self.manual_candidate(allow_http)? else {
            return Ok(Vec::new());
        };
        let base = &candidate.base;
        let mut routes = Vec::new();
        if matches!(
            self,
            Self::Manual {
                explicit_default_port_address: None,
                ..
            }
        ) && base.port().is_none()
            && base.path() == "/"
            && crate::is_lan_ip_host(base)
        {
            let ports = if base.scheme() == "http" {
                [5666, 8000]
            } else {
                [5667, 8001]
            };
            for port in ports {
                let mut address = base.clone();
                address.set_port(Some(port)).map_err(|_| "NAS 端口无效")?;
                routes.push(Candidate::new(address, false, MANUAL_ROUTE));
            }
        }
        routes.insert(0, candidate);
        Ok(routes)
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
        let mut candidates = self.manual_candidates(allow_http)?.into_iter();
        let Some(primary) = candidates.next() else {
            return discover_connection(client, self.as_str(), allow_http, progress);
        };
        progress("manual_probe");
        let result = primary.probe_route(client).or_else(|error| {
            if candidates.as_slice().is_empty() {
                return Err(error);
            }
            let probe_client = crate::client(3)?;
            candidates
                .find_map(|candidate| candidate.probe_route(&probe_client).ok())
                .ok_or_else(|| format!("{error}；飞牛默认端口也未连通，请核对 NAS 端口"))
        })?;
        complete_probe(client, result, progress)
    }
}

pub struct Connection {
    pub candidate: Candidate,
    pub config: Value,
}

pub enum ConnectionAttempt {
    Ready(Connection),
    AccessCode(AccessChallenge),
}

pub struct AccessChallenge {
    pub candidate: Candidate,
    lan: Vec<(Candidate, String)>,
    protected_lan: Option<Candidate>,
}

impl AccessChallenge {
    pub fn from_probes(candidate: Candidate, results: &[Result<Probe>]) -> Self {
        let lan = if candidate.relay {
            results
                .iter()
                .filter_map(|result| match result {
                    Ok(Probe::Ready(route, config))
                        if !route.relay && matches!(route.label, "局域网 IPv4" | "局域网 IPv6") =>
                    {
                        server_guid(config).map(|guid| (route.clone(), guid.to_owned()))
                    }
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        let protected_lan = if candidate.relay {
            results.iter().find_map(|result| match result {
                Ok(Probe::AccessCode(route))
                    if !route.relay
                        && matches!(route.label, "局域网 IPv4" | "局域网 IPv6")
                        && crate::is_lan_ip_host(&route.base) =>
                {
                    Some(route.clone())
                }
                _ => None,
            })
        } else {
            None
        };
        Self {
            candidate,
            lan,
            protected_lan,
        }
    }

    pub fn lan_challenge(&self) -> Option<&Candidate> {
        self.protected_lan.as_ref()
    }

    pub fn resume(
        &self,
        client: &Client,
        verified: &Candidate,
        prefer_lan: bool,
        mut progress: impl FnMut(&'static str),
    ) -> Result<Connection> {
        let Probe::Ready(relay, config) = verified.probe_route(client)? else {
            return Err("飞牛未接受访问码会话，请检查 NAS 地址或反向代理".into());
        };
        if prefer_lan
            && relay.relay
            && let Some(guid) = server_guid(&config)
        {
            for (route, _) in self
                .lan
                .iter()
                .filter(|(_, previous_guid)| previous_guid == guid)
            {
                progress("probing");
                if let Ok(Probe::Ready(candidate, config)) = route.probe_route(client)
                    && server_guid(&config) == Some(guid)
                {
                    return Ok(Connection { candidate, config });
                }
            }
        }
        Ok(Connection {
            candidate: relay,
            config,
        })
    }
}

fn server_guid(config: &Value) -> Option<&str> {
    config["serverGUID"]
        .as_str()
        .filter(|guid| !guid.is_empty())
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
        Probe::AccessCode(candidate) => Ok(ConnectionAttempt::AccessCode(
            AccessChallenge::from_probes(candidate, &[]),
        )),
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
    match select_probe(&results)? {
        ConnectionAttempt::Ready(connection) => {
            check_initialization(client, &connection.candidate, progress)?;
            Ok(ConnectionAttempt::Ready(connection))
        }
        challenge => Ok(challenge),
    }
}

fn select_probe(results: &[Result<Probe>]) -> Result<ConnectionAttempt> {
    let guid = results.iter().find_map(|result| match result {
        Ok(Probe::Ready(candidate, config)) if candidate.relay => server_guid(config),
        _ => None,
    });
    let mut direct_challenge = false;
    let challenge = results.iter().find_map(|result| match result {
        Ok(Probe::AccessCode(candidate)) if candidate.relay => Some(candidate.clone()),
        Ok(Probe::AccessCode(_)) => {
            direct_challenge = true;
            None
        }
        _ => None,
    });
    if guid.is_none()
        && let Some(candidate) = challenge
    {
        return Ok(ConnectionAttempt::AccessCode(AccessChallenge::from_probes(
            candidate, results,
        )));
    }
    results
        .iter()
        .find_map(|result| match result {
            Ok(Probe::Ready(candidate, config))
                if guid.is_none_or(|expected| server_guid(config) == Some(expected)) =>
            {
                Some(ConnectionAttempt::Ready(Connection {
                    candidate: candidate.clone(),
                    config: config.clone(),
                }))
            }
            _ => None,
        })
        .or_else(|| {
            challenge.map(|candidate| {
                ConnectionAttempt::AccessCode(AccessChallenge::from_probes(candidate, results))
            })
        })
        .ok_or_else(|| {
            if direct_challenge {
                ACCESS_CODE_USE_DIRECT_ADDRESS
            } else {
                "所有连接线路失败：请检查网络、TLS 证书、中继安全码和 NAS 在线状态"
            }
            .to_owned()
        })
}

pub fn check_initialization(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fn_id_access_code_challenge_requires_relay_or_explicit_address() {
        let lan = Candidate::new("http://192.168.1.42:5666".parse().unwrap(), false, "LAN");
        let relay = Candidate::new("https://relay.example".parse().unwrap(), true, "relay");
        assert_eq!(
            select_probe(&[Ok(Probe::AccessCode(lan.clone()))])
                .err()
                .unwrap(),
            ACCESS_CODE_USE_DIRECT_ADDRESS
        );
        assert!(matches!(
            select_probe(&[Ok(Probe::AccessCode(lan.clone())), Ok(Probe::AccessCode(relay.clone()))]),
            Ok(ConnectionAttempt::AccessCode(challenge)) if challenge.candidate.base == relay.base
        ));
        let available_lan = || {
            Ok(Probe::Ready(
                lan.clone(),
                serde_json::json!({"serverGUID":"nas"}),
            ))
        };
        assert!(matches!(
            select_probe(&[available_lan(), Ok(Probe::AccessCode(relay.clone()))]),
            Ok(ConnectionAttempt::AccessCode(challenge)) if challenge.candidate.base == relay.base
        ));
        assert!(matches!(
            select_probe(&[available_lan(), Ok(Probe::Ready(relay, serde_json::json!({"serverGUID":"nas"})))]),
            Ok(ConnectionAttempt::Ready(connection)) if connection.candidate.base == lan.base
        ));
        assert!(matches!(
            select_probe(&[available_lan()]),
            Ok(ConnectionAttempt::Ready(connection)) if connection.candidate.base == lan.base
        ));
    }
}
