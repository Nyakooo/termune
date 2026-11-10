use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use termune::{
    Candidate, business_data, candidates, client, cookie, hash_password, random_id, validate_id,
};

#[test]
fn official_discovery_signature_vector() {
    let (authx, sign) = termune::discovery_headers(
        "example",
        "{\"fnId\":\"example\"}",
        "123456",
        "1793836800000",
    );
    assert_eq!(
        authx,
        "nonce=123456&timestamp=1793836800000&sign=0e1a783884fa3e02af2de69c54d84121"
    );
    assert_eq!(
        sign,
        "c7cb65909d94f8aaa040bf34486a6252d10091c35503387f1119ac70869c6f85"
    );
}

#[test]
fn password_and_device_id() {
    assert_eq!(
        hash_password("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let id = random_id().unwrap();
    assert_eq!(id.len(), 32);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_ne!(id, random_id().unwrap());
}

#[test]
fn ids_and_cookie_injection() {
    assert_eq!(validate_id(" My-Nas ").unwrap(), "my-nas");
    for invalid in [
        "",
        "https://nas.fnos.net",
        "nas.fnos.net",
        "-nas",
        "nas/path",
        "a\r\nb",
    ] {
        assert!(validate_id(invalid).is_err());
    }
    assert_eq!(
        cookie(true, Some("test-token")).unwrap(),
        "mode=relay; music-token=test-token"
    );
    for bad in ["", "x; evil=yes", "x\r\nheader: yes", "x\"y"] {
        assert!(cookie(false, Some(bad)).is_err());
    }
}

#[test]
fn manual_target_parses_addresses_and_keeps_http_limited_to_lan_ips() {
    use termune::connection::Target;

    for (input, normalized) in [
        (" My-Nas ", "my-nas"),
        (
            "NAS.example.test:443/Music/",
            "https://nas.example.test/Music",
        ),
        ("192.168.1.42:5667", "https://192.168.1.42:5667/"),
        ("fd00::42", "https://[fd00::42]/"),
        ("[::1]", "https://[::1]/"),
        ("http://[fd00::42]:5666", "http://[fd00::42]:5666/"),
    ] {
        assert_eq!(Target::parse(input).unwrap().as_str(), normalized);
    }
    for invalid in [
        "http://nas.example.test",
        "http://8.8.8.8",
        "http://[2001:db8::1]",
        "https://user:secret@nas.example.test",
        "https://@nas.example.test",
        "https://nas.example.test/?token=secret",
        "https://nas.example.test/#fragment",
        "https:///nas.example.test",
        "ftp://nas.example.test",
    ] {
        assert!(Target::parse(invalid).is_err(), "{invalid}");
    }
    let https = Target::parse("https://nas.example.test").unwrap();
    assert!(https.manual_candidate(false).unwrap().is_some());
}

#[test]
fn manual_lan_port_fallback_respects_explicit_ports_and_paths() {
    use termune::connection::Target;

    for (input, ports) in [
        ("http://192.168.1.42", &[80, 5666, 8000][..]),
        ("http://192.168.1.42/", &[80, 5666, 8000][..]),
        ("https://[fd00::42]", &[443, 5667, 8001][..]),
        ("http://192.168.1.42:80", &[80][..]),
        ("https://[fd00::42]:443", &[443][..]),
        ("http://192.168.1.42:1234", &[1234][..]),
        ("http://192.168.1.42/proxy", &[80][..]),
        ("https://nas.example.test", &[443][..]),
        ("https://8.8.8.8", &[443][..]),
        ("my-nas", &[][..]),
    ] {
        let target = Target::parse(input).unwrap();
        // The TUI submits the normalized input and parses it again on restore.
        let target = Target::parse(target.input_address()).unwrap();
        let actual: Vec<_> = target
            .manual_candidates(true)
            .unwrap()
            .iter()
            .map(|candidate| candidate.base.port_or_known_default().unwrap())
            .collect();
        assert_eq!(actual, ports, "{input}");
    }
    assert!(
        Target::parse("http://192.168.1.42")
            .unwrap()
            .manual_candidates(false)
            .is_err()
    );
    assert_eq!(
        termune::credentials::account_key("http://192.168.1.42:80", "tester"),
        termune::credentials::account_key("http://192.168.1.42/", "tester")
    );
}

#[test]
fn manual_account_keys_preserve_proxy_path_case_and_old_fn_id_keys() {
    use termune::credentials::account_key;

    let legacy = hash_password(r#"["my-nas","tester"]"#);
    assert_eq!(account_key(" My-Nas ", "tester"), legacy);
    assert_ne!(
        account_key("https://nas.example.test/Proxy", "tester"),
        legacy
    );
    assert_ne!(
        account_key("https://nas.example.test/Proxy", "tester"),
        account_key("https://nas.example.test/proxy", "tester"),
    );
}

#[test]
fn discovery_candidates_prioritize_and_validate() {
    let data = json!({"ipv4":["192.168.1.2", "bad/path"], "publicIpv6":["2001:db8::1"],
        "fn":["nas.fnos.net:443", "evil@host", "host/path"], "port":{"httpsPort":1234,"httpPort":4321}});
    let secure = candidates(&data, false);
    assert_eq!(secure.len(), 3);
    assert_eq!(secure[0].base.as_str(), "https://192.168.1.2:1234/");
    assert_eq!(secure[1].base.as_str(), "https://[2001:db8::1]:1234/");
    assert!(secure[2].relay);
    let insecure = candidates(&data, true);
    assert_eq!(insecure.len(), 4);
    assert_eq!(insecure[1].base.scheme(), "http");
    assert!(
        insecure
            .iter()
            .filter(|c| c.label == "公网 IPv6")
            .all(|c| c.base.scheme() == "https")
    );
}

#[test]
fn business_failure_is_not_empty_success_or_secret_echo() {
    assert!(business_data(json!({"code":0})).is_err());
    assert!(business_data(json!({"data":{}})).is_err());
    let error = business_data(json!({"code":120001,"msg":"SECRET"}))
        .err()
        .unwrap();
    assert!(error.contains("120001"));
    assert!(!error.contains("SECRET"));
    assert!(business_data(json!({"code":401,"msg":"SECRET"})).is_err());
}

fn server(responses: Vec<(&'static str, String)>) -> (Candidate, thread::JoinHandle<Vec<String>>) {
    server_with_content_type(responses, "application/json")
}

fn server_with_content_type(
    responses: Vec<(&'static str, String)>,
    content_type: &'static str,
) -> (Candidate, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (headers, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 1024];
            loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length: usize = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            write!(stream, "HTTP/1.1 {headers}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (Candidate::new(base, true, "Mock"), handle)
}

#[test]
fn mock_login_hashes_password_and_authenticates_me() {
    let (candidate, handle) = server(vec![
        (
            "200 OK",
            json!({"code":0,"data":{"userToken":"mock-token"}}).to_string(),
        ),
        (
            "200 OK",
            json!({"code":0,"data":{"name":"tester","guid":"mock-guid"}}).to_string(),
        ),
    ]);
    candidate
        .login(
            &client(3).unwrap(),
            "tester",
            "plain-secret",
            "0123456789abcdef0123456789abcdef",
        )
        .unwrap();
    let requests = handle.join().unwrap();
    assert!(requests[0].starts_with("POST /music/api/v1/user/password-login "));
    assert!(
        requests[0]
            .to_lowercase()
            .contains("cookie: mode=relay\r\n")
    );
    assert!(!requests[0].contains("plain-secret"));
    assert!(requests[0].contains(&hash_password("plain-secret")));
    assert!(requests[1].starts_with("GET /music/api/v1/user/me "));
    assert!(
        requests[1]
            .to_lowercase()
            .contains("cookie: mode=relay; music-token=mock-token\r\n")
    );
}

#[test]
fn mock_wrong_password_and_invalid_me_fail() {
    for (responses, expected) in [
        (
            vec![("200 OK", json!({"code":120001,"msg":"secret"}).to_string())],
            "120001",
        ),
        (
            vec![
                (
                    "200 OK",
                    json!({"code":0,"data":{"userToken":"mock-token"}}).to_string(),
                ),
                ("401 Unauthorized", "secret".into()),
            ],
            "401",
        ),
        (
            vec![
                (
                    "200 OK",
                    json!({"code":0,"data":{"userToken":"mock-token"}}).to_string(),
                ),
                ("200 OK", json!({"code":0,"data":{}}).to_string()),
            ],
            "user/me",
        ),
    ] {
        let (candidate, handle) = server(responses);
        let error = candidate
            .login(&client(3).unwrap(), "tester", "password", "device")
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("secret"));
        handle.join().unwrap();
    }
}

#[test]
fn mock_auth_redirect_never_reposts_credentials() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let header: &'static str = Box::leak(
        format!(
            "307 Temporary Redirect\r\nLocation: http://{}/stolen",
            destination.local_addr().unwrap()
        )
        .into_boxed_str(),
    );
    let (candidate, handle) = server(vec![(header, String::new())]);
    let error = candidate
        .login(&client(3).unwrap(), "tester", "secret", "device")
        .err()
        .unwrap();
    assert!(error.contains("307"));
    handle.join().unwrap();
    assert_eq!(
        destination.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn mock_probe_rejects_html_and_keeps_proxy_prefix() {
    let (mut candidate, handle) = server(vec![("200 OK", "<html>FN Connect</html>".into())]);
    candidate.base.set_path("/proxy/");
    assert!(
        candidate
            .probe(&client(3).unwrap())
            .err()
            .unwrap()
            .contains("JSON")
    );
    assert!(handle.join().unwrap()[0].starts_with("GET /proxy/music/api/v1/sys/config "));
}

#[test]
fn manual_probe_explains_404_without_rewriting_the_entered_path() {
    for (path, label, expected) in [
        (
            "/nas/login",
            termune::connection::MANUAL_ROUTE,
            "去掉地址末尾的 /login",
        ),
        (
            "/nas",
            termune::connection::MANUAL_ROUTE,
            "检查 NAS 地址或反向代理路径",
        ),
        ("/nas", "FN Connect 中继", "HTTP 404"),
    ] {
        let (mut candidate, handle) = server(vec![("404 Not Found", "SECRET".into())]);
        candidate.base.set_path(path);
        candidate.label = label;
        let error = candidate.probe(&client(3).unwrap()).err().unwrap();
        assert!(error.contains(expected));
        assert!(!error.contains("SECRET"));
        if label != termune::connection::MANUAL_ROUTE {
            assert_eq!(error, expected);
        }
        assert!(
            handle.join().unwrap()[0].starts_with(&format!("GET {path}/music/api/v1/sys/config "))
        );
    }
}

#[test]
fn probe_distinguishes_the_access_code_page_from_other_html() {
    let challenge = r#"<script>fetch("/access_code_verify", { headers: { "x-access-code": "example", "x-access-source": "web" } })</script>"#;
    for (body, required) in [(challenge, true), ("<html>Login page</html>", false)] {
        let (candidate, handle) =
            server_with_content_type(vec![("200 OK", body.into())], "text/html; charset=utf-8");
        let result = candidate.probe_route(&client(3).unwrap());
        assert_eq!(
            matches!(&result, Ok(termune::Probe::AccessCode(_))),
            required
        );
        if !required {
            assert!(result.is_err());
        }
        handle.join().unwrap();
    }
}
