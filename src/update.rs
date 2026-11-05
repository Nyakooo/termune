//! Checksum-verified updates from this repository's published releases only.
use crate::Result;
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const REPO: &str = "Nyakooo/termune";
const MAX_BINARY: u64 = 128 * 1024 * 1024;
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn platform(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => Err("当前系统或架构没有可用的自动更新资源，请查看 GitHub Releases".into()),
    }
}
fn newer(tag: &str, current: &str) -> Result<bool> {
    let latest = Version::parse(tag.strip_prefix('v').ok_or("Release 标签必须以 v 开头")?)
        .map_err(|_| "Release 版本号无效")?;
    if !latest.pre.is_empty() {
        return Err("自动更新只接受正式版本".into());
    }
    let current = Version::parse(current).map_err(|_| "当前版本号无效")?;
    Ok(latest.cmp_precedence(&current).is_gt())
}
fn checksum(text: &str, name: &str) -> Result<String> {
    let matches: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let hash = fields.next()?;
            let file = fields.next()?.trim_start_matches('*');
            (file == name && fields.next().is_none()).then_some(hash)
        })
        .collect();
    match matches.as_slice() {
        [hash] if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(hash.to_ascii_lowercase())
        }
        _ => Err("SHA256SUMS 缺少唯一有效的对应校验值，更新已取消".into()),
    }
}
fn asset<'a>(release: &'a Release, name: &str) -> Result<&'a Asset> {
    let mut matches = release.assets.iter().filter(|a| a.name == name);
    let item = matches
        .next()
        .ok_or("Release 资源尚未齐全，请稍后重试或查看发布页")?;
    let expected = format!(
        "https://github.com/{REPO}/releases/download/{}/{name}",
        release.tag_name
    );
    if matches.next().is_some() || item.browser_download_url != expected {
        return Err("Release 资源地址或名称异常，更新已取消".into());
    }
    Ok(item)
}
fn get(client: &Client, url: &str) -> Result<Response> {
    let response = client
        .get(url)
        .send()
        .map_err(|_| "无法连接 GitHub，请检查网络或 HTTPS_PROXY")?;
    if response.status().as_u16() == 404 {
        return Err("尚无可用的正式 Release 或对应资源".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub 返回 HTTP {}，更新已取消",
            response.status().as_u16()
        ));
    }
    Ok(response)
}
fn bounded(mut input: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut input)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "更新资源读取失败")?;
    if bytes.len() as u64 > limit {
        return Err("更新资源超过大小限制".into());
    }
    Ok(bytes)
}
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn valid_binary(header: &[u8], target: &str) -> bool {
    if header.len() < 20 {
        return false;
    }
    if target.ends_with("linux-gnu") {
        let machine = if target.starts_with("x86_64") {
            62u16
        } else {
            183u16
        };
        &header[..4] == b"\x7fELF"
            && header[4] == 2
            && header[5] == 1
            && u16::from_le_bytes([header[18], header[19]]) == machine
    } else {
        let cpu = if target.starts_with("x86_64") {
            0x01000007u32
        } else {
            0x0100000cu32
        };
        header[..4] == [0xcf, 0xfa, 0xed, 0xfe]
            && u32::from_le_bytes(header[4..8].try_into().unwrap()) == cpu
    }
}
/// Write beside the executable; a failed download or checksum never touches it.
fn install(
    mut input: impl Read,
    size: u64,
    hash: &str,
    destination: &Path,
    target: &str,
) -> Result<()> {
    if !(20..=MAX_BINARY).contains(&size) {
        return Err("Release 二进制大小无效".into());
    }
    let parent = destination.parent().ok_or("无法确定安装目录")?;
    let permissions = fs::metadata(destination)
        .map_err(|_| "无法读取当前程序权限")?
        .permissions();
    let staging = parent.join(format!(".termune-update-{}", crate::random_id()?));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&staging)
        .map_err(|_| "安装目录不可写，请将程序安装到用户可写目录（如 ~/.local/bin）")?;
    let _cleanup = Remove(staging.clone());
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut header = Vec::new();
    let mut buffer = [0u8; 32768];
    loop {
        let n = input
            .read(&mut buffer)
            .map_err(|_| "下载中断，原程序保留")?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > size {
            return Err("下载长度超过 Release 声明，原程序保留".into());
        }
        if header.len() < 20 {
            header.extend_from_slice(&buffer[..n.min(20 - header.len())]);
        }
        digest.update(&buffer[..n]);
        file.write_all(&buffer[..n])
            .map_err(|_| "写入更新失败，原程序保留")?;
    }
    if count != size || format!("{:x}", digest.finalize()) != hash {
        return Err("长度或 SHA256 校验失败，原程序保留".into());
    }
    if !valid_binary(&header, target) {
        return Err("二进制系统或架构不匹配，原程序保留".into());
    }
    file.set_permissions(permissions)
        .map_err(|_| "设置程序权限失败")?;
    file.sync_all().map_err(|_| "更新文件同步失败")?;
    drop(file);
    fs::rename(&staging, destination).map_err(|_| "替换程序失败，原程序保留")?;
    Ok(())
}

pub fn run() -> Result<()> {
    let target = platform(std::env::consts::OS, std::env::consts::ARCH)?;
    #[cfg(not(unix))]
    return Err("当前系统暂不支持程序替换".into());
    #[cfg(unix)]
    {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .user_agent(concat!("termune/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| "无法创建更新连接")?;
        println!("查询 Termune 最新正式版本…");
        let response = get(
            &client,
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        )?;
        let release: Release = serde_json::from_slice(&bounded(response, 2 * 1024 * 1024)?)
            .map_err(|_| "GitHub Release 信息无效")?;
        if release.draft || release.prerelease {
            return Err("自动更新只接受已发布的正式版本".into());
        }
        if !newer(&release.tag_name, env!("CARGO_PKG_VERSION"))? {
            println!(
                "当前版本 {} 已是最新或更新版本，无需更新。",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        let name = format!("termune-{target}");
        let binary = asset(&release, &name)?;
        let sums = asset(&release, "SHA256SUMS")?;
        let manifest = bounded(get(&client, &sums.browser_download_url)?, 64 * 1024)?;
        let hash = checksum(
            std::str::from_utf8(&manifest).map_err(|_| "校验文件编码错误")?,
            &name,
        )?;
        let destination = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|_| "无法确定当前程序路径")?;
        let lock = destination
            .parent()
            .ok_or("安装目录无效")?
            .join(".termune-update.lock");
        let _lock_file = OpenOptions::new().write(true).create_new(true).open(&lock)
            .map_err(|_| "安装目录不可写或另一个更新正在执行；若异常退出，请确认没有更新进程后删除 .termune-update.lock")?;
        let _unlock = Remove(lock);
        println!("下载 {}（{}）并校验 SHA256…", release.tag_name, target);
        install(
            get(&client, &binary.browser_download_url)?,
            binary.size,
            &hash,
            &destination,
            target,
        )?;
        println!(
            "已更新到 {}。重新运行 termune 即可，配置和登录凭据保留。",
            release.tag_name
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_use_numeric_precedence_and_never_downgrade_or_take_prereleases() {
        assert!(newer("v0.10.0", "0.9.9").unwrap());
        assert!(!newer("v0.1.0", "0.1.0").unwrap());
        assert!(!newer("v0.1.0", "0.2.0").unwrap());
        assert!(!newer("v0.1.0+new", "0.1.0+old").unwrap());
        assert!(newer("v0.2.0-beta.1", "0.1.0").is_err());
        assert!(newer("latest", "0.1.0").is_err());
        assert!(platform("windows", "x86_64").is_err());
        assert_eq!(
            platform("macos", "aarch64").unwrap(),
            "aarch64-apple-darwin"
        );
    }
    #[test]
    fn manifest_requires_unique_exact_name_and_valid_sha256() {
        let hash = "a".repeat(64);
        assert_eq!(
            checksum(&format!("{hash}  termune-linux\n"), "termune-linux").unwrap(),
            hash
        );
        for manifest in [
            format!("{hash}  termune-linux.zip"),
            format!("{hash} x\n{hash} x"),
            "bad x".into(),
        ] {
            assert!(checksum(&manifest, "x").is_err());
        }
        let release = Release {
            tag_name: "v0.1.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "SHA256SUMS".into(),
                browser_download_url: "https://example.com/SHA256SUMS".into(),
                size: 64,
            }],
        };
        assert!(asset(&release, "SHA256SUMS").is_err());
    }
    #[test]
    fn failed_updates_preserve_executable_and_remove_staging_files() {
        let dir = std::env::temp_dir().join(format!(
            "termune-update-test-{}",
            crate::random_id().unwrap()
        ));
        fs::create_dir(&dir).unwrap();
        let destination = dir.join("termune");
        fs::write(&destination, b"old executable").unwrap();
        let mut binary = vec![0u8; 128];
        binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
        binary[18] = 62;
        let hash = format!("{:x}", Sha256::digest(&binary));
        let target = "x86_64-unknown-linux-gnu";
        assert!(install(&binary[..], 128, &"0".repeat(64), &destination, target).is_err());
        assert!(install(&binary[..32], 128, &hash, &destination, target).is_err());
        assert!(install(&binary[..], 32, &hash, &destination, target).is_err());
        assert!(
            install(
                &binary[..],
                128,
                &hash,
                &destination,
                "aarch64-unknown-linux-gnu"
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), b"old executable");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        install(&binary[..], 128, &hash, &destination, target).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), binary);
        fs::remove_dir_all(dir).unwrap();
    }
}
