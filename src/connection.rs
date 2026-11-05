use crate::{Candidate, Result};
use reqwest::blocking::Client;
use serde_json::Value;

pub struct Connection {
    pub candidate: Candidate,
    pub config: Value,
}

/// Same FN Connect discovery and identity checks as the verified CLI probe.
/// Progress carries only fixed stage names, never URLs or credentials.
pub fn discover_connection(
    client: &Client,
    id: &str,
    allow_http: bool,
    mut progress: impl FnMut(&'static str),
) -> Result<Connection> {
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
                    candidate.probe(&crate::client(if candidate.relay { 12 } else { 3 })?)
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap_or_else(|_| Err("探测线程异常".into())))
            .collect::<Vec<_>>()
    });
    let guid = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .find(|(c, _)| c.relay)
        .and_then(|(_, v)| v["serverGUID"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let (candidate, config) = results
        .into_iter()
        .filter_map(std::result::Result::ok)
        .find(|(_, config)| {
            guid.as_ref()
                .is_none_or(|expected| config["serverGUID"].as_str() == Some(expected))
        })
        .ok_or("所有连接线路失败：请检查网络、TLS 证书、中继安全码和 NAS 在线状态")?;
    progress("initialization");
    if let Ok(state) = candidate.get(client, "/initialization/state", None)
        && (state == false
            || ["initialized", "isInitialized"]
                .iter()
                .any(|key| state.get(key) == Some(&Value::Bool(false))))
    {
        return Err("音乐服务尚未初始化，请先在 NAS 网页完成设置".into());
    }
    Ok(Connection { candidate, config })
}
