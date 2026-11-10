mod app;
mod audio;
mod config;
mod effects;
mod media;
mod player;
mod queue;
mod ui;

use std::{
    io::{self, Write},
    process::ExitCode,
};
use termune::{Candidate, Probe, Result, Session, connection::Target};

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush().map_err(|_| "终端输出失败")?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(|_| "终端输入失败")?
        == 0
    {
        return Err("输入已结束".into());
    }
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err("输入不能为空".into());
    }
    Ok(value)
}

fn device_id() -> Result<String> {
    let dirs = directories::ProjectDirs::from("", "", "termune").ok_or("无法确定配置目录")?;
    std::fs::create_dir_all(dirs.config_dir()).map_err(|_| "无法创建配置目录")?;
    let path = dirs.config_dir().join("device-id");
    match std::fs::read_to_string(&path) {
        Ok(id) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => return Ok(id),
        Ok(_) => return Err("device-id 文件格式错误，请备份并移除后重试".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err("无法读取 device-id 文件".into()),
    }
    let id = termune::random_id()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "无法创建 device-id 文件")?;
    file.write_all(id.as_bytes())
        .map_err(|_| "无法保存 device-id")?;
    Ok(id)
}

fn verify_library(session: &Session, client: &reqwest::blocking::Client) -> Result<()> {
    println!("\n[曲库验证] 每类最多读取两页，每页 50 条；总数来自 NAS。");
    let (mut passed, mut failed, mut blocked) = (0, 0, 0);
    let mut track_guid = None;
    let mut cover_id = None;
    let mut default_query = None;
    for (kind, label) in [
        ("track", "歌曲"),
        ("album", "专辑"),
        ("artist", "歌手"),
        ("playlist", "歌单"),
        ("genre", "风格"),
        ("favorite", "收藏"),
    ] {
        match session.library_sample(client, kind, None) {
            Ok(sample) => {
                println!(
                    "{label}: PASS — NAS 总数 {}，读取 {} 条 / {} 页{}",
                    sample.total,
                    sample.items.len(),
                    sample.pages,
                    if sample.total == 0 {
                        "（空曲库）"
                    } else {
                        ""
                    }
                );
                passed += 1;
                for item in &sample.items {
                    if kind == "track" && track_guid.is_none() {
                        track_guid = item["guid"].as_str().map(str::to_owned);
                        default_query = item["title"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                    }
                    if cover_id.is_none() {
                        cover_id = item["coverId"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .or_else(|| {
                                item["album"]["coverId"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned)
                            });
                    }
                }
            }
            Err(error) => {
                println!("{label}: FAIL — {error}");
                failed += 1;
            }
        }
    }
    print!("搜索歌曲关键词（Enter 使用首首歌曲标题；无标题则跳过）: ");
    io::stdout().flush().map_err(|_| "终端输出失败")?;
    let mut query = String::new();
    io::stdin()
        .read_line(&mut query)
        .map_err(|_| "读取搜索关键词失败")?;
    let query = if query.trim().is_empty() {
        default_query
    } else {
        Some(query.trim().to_owned())
    };
    if let Some(query) = query {
        match session.library_sample(client, "track", Some(&query)) {
            Ok(sample) => {
                println!(
                    "搜索: PASS — 匹配总数 {}，读取 {} 条 / {} 页{}",
                    sample.total,
                    sample.items.len(),
                    sample.pages,
                    if sample.total == 0 {
                        "（空结果，需自行确认关键词是否应有匹配）"
                    } else {
                        "（仅验证接口，不自动判断内容相关性）"
                    }
                );
                passed += 1;
            }
            Err(error) => {
                println!("搜索: FAIL — {error}");
                failed += 1;
            }
        }
    } else {
        println!("搜索: BLOCKED — 无关键词");
        blocked += 1;
    }
    println!("[媒体验证] 使用已读取的首个封面与首首歌曲；不显示资源 URL。");
    if let Some(id) = cover_id {
        let cover_result = directories::ProjectDirs::from("", "", "termune")
            .ok_or_else(|| "无法确定封面缓存目录".to_owned())
            .and_then(|dirs| session.cover(client, &id, dirs.cache_dir()));
        match cover_result {
            Ok(report) => {
                println!(
                    "封面: PASS — HTTP {}，{} 字节，{} 次跳转，跨源 {}，已缓存",
                    report.status, report.bytes, report.redirects, report.cross_origin
                );
                println!("  跳转状态码: {:?}", report.redirect_statuses);
                passed += 1;
            }
            Err(error) => {
                println!("封面: FAIL — {error}");
                failed += 1;
            }
        }
    } else {
        println!("封面: BLOCKED — 已读取的项目无 coverId");
        blocked += 1;
    }
    if let Some(guid) = track_guid {
        match session.audio(client, &guid) {
            Ok(report) => {
                println!(
                    "音频访问: PASS — HTTP {}，读取 {} 字节，{} 次跳转，跨源 {}",
                    report.status, report.bytes, report.redirects, report.cross_origin
                );
                println!("  跳转状态码: {:?}", report.redirect_statuses);
                passed += 1;
                if report.range_supported {
                    println!("Range: PASS — 206 且 Content-Range/长度有效");
                    passed += 1;
                } else {
                    println!("Range: FAIL — 服务器返回 200，忽略 Range；仅读取前 4 KiB 后关闭响应");
                    failed += 1;
                }
            }
            Err(error) => {
                println!("音频/Range: FAIL — {error}");
                failed += 1;
            }
        }
    } else {
        println!("音频/Range: BLOCKED — 无可用歌曲");
        blocked += 1;
    }
    println!(
        "\n本次曲库/媒体验证：PASS {passed}，FAIL {failed}，BLOCKED {blocked}。\n结果仅代表本次线路及抽样资源，不代表全曲库可播放；未执行音频解码。"
    );
    if failed > 0 {
        Err("部分验证未通过，请反馈上述脱敏结果".into())
    } else {
        Ok(())
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] || args == ["-V"] {
        println!("termune {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "update") {
        if args.len() != 1 {
            return Err("用法: termune update（不接受其他参数）".into());
        }
        return termune::update::run();
    }
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!(
            "Termune · 飞牛音乐终端曲库\n用法: termune [--allow-lan-http] [--diagnose | --login-only]\n      termune update\n      termune --version\n默认启动 TUI。输入 FN ID 或 NAS 地址，以及音乐独立账号密码。默认仅 HTTPS。\nupdate: 校验并安装最新正式 Release，保留配置和登录凭据。\n--version: 显示程序版本。\n--allow-lan-http: 允许使用局域网 IP 的 HTTP 连接（密码摘要和 Token 可被窃听）。\n--diagnose: 使用命令行验证曲库与媒体。\n--login-only: 仅验证登录，跳过曲库及媒体。"
        );
        return Ok(());
    }
    if args.iter().any(|s| {
        !matches!(
            s.as_str(),
            "--allow-lan-http" | "--login-only" | "--diagnose"
        )
    }) {
        return Err("未知参数；使用 --help 查看用法。密码不能通过参数传入".into());
    }
    let allow_http = args.iter().any(|s| s == "--allow-lan-http");
    if !args
        .iter()
        .any(|s| matches!(s.as_str(), "--diagnose" | "--login-only"))
    {
        return app::run(allow_http);
    }
    println!("Termune · FN Connect / 飞牛音乐连接与资源验证");
    if allow_http {
        println!("警告：已允许局域网 HTTP，密码摘要和 Token 可能被窃听。仅在可信网络测试。");
    }
    let target = Target::parse(&prompt("FN ID 或 NAS 地址")?)?;
    let client = termune::client(15)?;
    let manual = target
        .manual_candidate(allow_http)
        .map_err(|_| "此地址使用 HTTP。请添加 --allow-lan-http 或填写 HTTPS 地址")?;
    let direct = manual.is_some();
    let candidates = if let Some(candidate) = manual {
        println!("[1/4] 使用手动填写的 NAS 地址");
        vec![candidate]
    } else {
        println!("[1/4] 查询官方 FN Connect 地址发现服务…");
        let data = termune::discover(&client, target.as_str())?;
        termune::candidates(&data, allow_http)
    };
    if candidates.is_empty() {
        return Err("官方未返回可用地址；检查 FN ID、设备在线状态和 FN Connect 设置".into());
    }
    if direct {
        println!("[2/4] 检测 NAS 地址…");
    } else {
        println!(
            "[2/4] 探测 {} 条候选线路（局域网 → 公网 → DDNS → 中继）…",
            candidates.len()
        );
    }
    // All requests here are unauthenticated. No music password/token leaves this
    // process until selection completes. Results retain discovery priority.
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .iter()
            .map(|candidate| {
                scope.spawn(move || {
                    let client = termune::client(if candidate.relay { 12 } else { 3 })?;
                    candidate.probe_route(&client)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("探测线程异常".into())))
            .collect::<Vec<_>>()
    });
    let relay_guid = results
        .iter()
        .find_map(|result| match result {
            Ok(Probe::Ready(candidate, config)) if candidate.relay => Some(config),
            _ => None,
        })
        .and_then(|config| config["serverGUID"].as_str())
        .map(str::to_owned);
    let mut selected: Option<(Candidate, serde_json::Value)> = None;
    let mut challenge: Option<Candidate> = None;
    for (candidate, result) in candidates.iter().zip(results) {
        match result {
            Ok(Probe::Ready(c, config)) => {
                if let Some(expected) = &relay_guid
                    && config["serverGUID"].as_str() != Some(expected.as_str())
                {
                    println!(
                        "  {} / {}: SKIP（与中继 NAS 身份不一致或缺失）",
                        c.label,
                        c.base.scheme()
                    );
                    continue;
                }
                println!("  {} / {}: PASS", c.label, c.base.scheme());
                if selected.is_none() {
                    selected = Some((c, config));
                }
            }
            Ok(Probe::AccessCode(c)) => {
                println!("  {} / {}: 需要飞牛访问码", c.label, c.base.scheme());
                challenge.get_or_insert(c);
            }
            Err(e) => println!(
                "  {} / {}: FAIL — {e}",
                candidate.label,
                candidate.base.scheme()
            ),
        }
    }
    let (candidate, config) = if let Some(selected) = selected {
        selected
    } else if let Some(challenge) = challenge {
        let code = rpassword::prompt_password("飞牛访问码（隐藏输入）: ")
            .map_err(|_| "无法安全读取访问码，请在交互终端中运行")?;
        let verified = challenge.verify_access_code(&client, &code, allow_http)?;
        drop(code);
        verified.probe(&client)?
    } else if direct {
        return Err("手动连接失败，请检查 NAS 地址、端口、TLS 证书和反向代理配置。".into());
    } else {
        return Err(
            "所有线路失败。HTTPS 直连可能因 IP 证书不匹配而失败，请检查网络和中继设置。".into(),
        );
    };
    println!(
        "选定线路：{} / {}",
        candidate.label,
        candidate.base.scheme()
    );
    if !direct && relay_guid.is_none() && !candidate.relay {
        println!(
            "提示：中继未提供身份基准，当前仅验证候选地址来自官方发现服务及 TLS（若为 HTTPS）。"
        );
    }
    // Only print tightly constrained version strings, never arbitrary server text.
    for key in ["serverVersion", "mediasrvVersion"] {
        if let Some(v) = config[key].as_str().filter(|s| {
            s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
        }) {
            println!("{key}: {v}");
        }
    }
    match candidate.get(&client, "/initialization/state", None) {
        Ok(state) => {
            if state == false
                || ["initialized", "isInitialized"]
                    .iter()
                    .any(|k| state.get(k) == Some(&serde_json::Value::Bool(false)))
            {
                return Err("音乐服务尚未初始化，请先在 NAS 网页完成设置".into());
            }
            println!("初始化接口：PASS（业务成功；具体字段以真实版本为准）");
        }
        Err(e) => println!("初始化接口：未确认 — {e}；继续独立验证登录"),
    }
    println!("[3/4] 请输入飞牛音乐独立账号（不是 fnOS 系统账号）");
    let username = prompt("音乐用户名")?;
    let password = rpassword::prompt_password("音乐密码（隐藏输入）: ")
        .map_err(|_| "无法安全读取密码，请在交互终端中运行")?;
    if password.is_empty() {
        return Err("密码不能为空".into());
    }
    let device_id = device_id()?;
    println!("[4/4] 登录并验证 /user/me…");
    let session = candidate.login(&client, &username, &password, &device_id)?;
    drop(password);
    println!(
        "PASS：登录成功，Token 非空，/user/me 返回有效用户信息。\n凭据仅在内存中使用，未保存密码或 Token。\nReal NAS Integration: 登录链路 PASS（不代表曲库或音频验证通过）。"
    );
    if args.iter().any(|s| s == "--login-only") {
        Ok(())
    } else {
        verify_library(&session, &client)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL：{error}");
            ExitCode::FAILURE
        }
    }
}
