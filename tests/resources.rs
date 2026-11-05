use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use termune::{
    Candidate, Session, client,
    library::{
        parse_content_range, parse_page, redirect_target, resource_headers, valid_audio,
        valid_image,
    },
};

struct Reply {
    status: &'static str,
    headers: String,
    body: Vec<u8>,
}

#[test]
fn playback_stream_uses_authenticated_media_route() {
    let (session, handle) = logged_in(vec![media(
        "200 OK",
        "Content-Type: audio/wav\r\n",
        b"RIFFtestWAVE",
    )]);
    let mut stream = session
        .audio_stream(&client(3).unwrap(), "song & one")
        .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"RIFFtestWAVE");
    let requests = handle.join().unwrap();
    assert!(requests[2].starts_with("GET /music/api/v1/track/stream?guid=song+%26+one "));
    assert!(
        requests[2]
            .to_ascii_lowercase()
            .contains("cookie: mode=relay; music-token=test-token")
    );
}

#[test]
fn playback_rejects_error_pages() {
    let (session, handle) = logged_in(vec![media(
        "200 OK",
        "Content-Type: text/html\r\n",
        b"error",
    )]);
    assert!(session.audio_stream(&client(3).unwrap(), "song").is_err());
    handle.join().unwrap();
}

#[test]
fn mock_album_artist_playlist_details_use_guid_and_pagination() {
    let replies = (0..3)
        .map(|_| json_reply(json!({"list":[{"guid":"track-one","title":null}],"total":1})))
        .collect();
    let (session, handle) = logged_in(replies);
    let client = client(3).unwrap();
    for kind in ["album", "artist", "playlist"] {
        let page = session.detail_page(&client, kind, "id & 中文", 2).unwrap();
        assert_eq!(page.items.len(), 1);
    }
    let requests = handle.join().unwrap();
    for (i, kind) in ["album", "artist", "playlist"].into_iter().enumerate() {
        let path = requests[i + 2].split_whitespace().nth(1).unwrap();
        let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
        assert_eq!(
            url.path(),
            format!("/music/api/v1/track/{kind}-detail/list")
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == &format!("{kind}GUID"))
                .unwrap()
                .1,
            "id & 中文"
        );
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "page").unwrap().1,
            "2"
        );
    }
}

#[test]
fn lyrics_select_preferred_and_handle_empty_without_raw_payloads() {
    let (session, handle) = logged_in(vec![
        json_reply(
            json!({"preferred":"translated","list":[{"guid":"original","content":"[00:01]original"},{"guid":"translated","content":"[00:01]中文歌词"}]}),
        ),
        json_reply(json!({"list":[]})),
    ]);
    let client = client(3).unwrap();
    assert_eq!(
        session
            .lyric_text(&client, "track & 中文")
            .unwrap()
            .unwrap(),
        "[00:01]中文歌词"
    );
    assert!(session.lyric_text(&client, "empty").unwrap().is_none());
    let requests = handle.join().unwrap();
    let path = requests[2].split_whitespace().nth(1).unwrap();
    let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
    assert_eq!(url.path(), "/music/api/v1/lyric/list");
    assert_eq!(
        url.query_pairs().find(|(k, _)| k == "trackGUID").unwrap().1,
        "track & 中文"
    );
}

#[test]
fn genre_favorites_and_writes_use_authenticated_real_routes() {
    let replies = vec![
        json_reply(json!({"list":[{"guid":"genre-id","name":"Jazz"}],"total":1})),
        json_reply(json!({"list":[{"guid":"song-id","title":"song"}],"total":1})),
        json_reply(json!({"list":[{"guid":"song-id"}],"total":1})),
        media(
            "200 OK",
            "Content-Type: application/json\r\n",
            b"{\"code\":0}",
        ),
        media(
            "200 OK",
            "Content-Type: application/json\r\n",
            b"{\"code\":0,\"data\":null}",
        ),
    ];
    let (session, handle) = logged_in(replies);
    let client = client(3).unwrap();
    assert_eq!(session.page(&client, "genre", 1, None).unwrap().total, 1);
    assert_eq!(session.page(&client, "favorite", 1, None).unwrap().total, 1);
    session
        .detail_page(&client, "genre", "genre-id", 1)
        .unwrap();
    session.set_favorite(&client, "song-id", true).unwrap();
    session.set_favorite(&client, "song-id", false).unwrap();
    let requests = handle.join().unwrap();
    for (request, path) in requests[2..].iter().zip([
        "/genre/list",
        "/favorite-track/list",
        "/track/genre-detail/list",
        "/favorite-track/create",
        "/favorite-track/delete",
    ]) {
        assert!(request.contains(path));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("cookie: mode=relay; music-token=test-token")
        );
    }
    assert!(requests[5].contains("\"trackGUID\":\"song-id\""));
}
fn json_reply(data: Value) -> Reply {
    Reply {
        status: "200 OK",
        headers: "Content-Type: application/json\r\n".into(),
        body: json!({"code":0,"data":data}).to_string().into_bytes(),
    }
}
fn media(status: &'static str, headers: &str, body: &[u8]) -> Reply {
    Reply {
        status,
        headers: headers.into(),
        body: body.to_vec(),
    }
}

fn logged_in(replies: Vec<Reply>) -> (Session, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let mut all = vec![
        json_reply(json!({"userToken":"test-token"})),
        json_reply(json!({"name":"tester","guid":"user-guid"})),
    ];
    all.extend(replies);
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in all {
            // Nonblocking accept with a deadline prevents a failed assertion in
            // the client from leaving a mock server blocked indefinitely.
            listener.set_nonblocking(true).unwrap();
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(5));
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            // Accepted Windows sockets may inherit the listener's nonblocking mode.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut data = Vec::new();
            loop {
                let mut buf = [0; 1024];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(data).unwrap());
            write!(
                stream,
                "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                reply.status,
                reply.headers,
                reply.body.len()
            )
            .unwrap();
            // A 200 response intentionally ignored by a bounded client may close early.
            let _ = stream.write_all(&reply.body);
        }
        requests
    });
    let session = Candidate {
        base,
        relay: true,
        label: "Mock",
    }
    .login(&client(3).unwrap(), "tester", "secret", "device")
    .unwrap();
    (session, handle)
}

#[test]
fn page_parser_accepts_numeric_string_and_rejects_broken_structure() {
    let parsed =
        parse_page(json!({"list":[{"guid":"one","title":null,"duration":"123"}],"total":"1"}))
            .unwrap();
    assert_eq!(parsed.total, 1);
    assert_eq!(parse_page(json!({"list":[],"total":0})).unwrap().total, 0);
    for invalid in [
        json!({"total":0}),
        json!({"list":[],"total":null}),
        json!({"list":[{}],"total":1}),
        json!({"list":[{"guid":"one"}],"total":0}),
        json!({"list":[{"guid":"one"},{"guid":"one"}],"total":2}),
    ] {
        assert!(parse_page(invalid).is_err());
    }
}

#[test]
fn media_redirects_strip_all_cookies_across_scheme_host_and_port() {
    let origin = "https://nas.example:8443".parse().unwrap();
    for address in [
        "https://nas.example:8443/cover",
        "https://nas.example:8443/stream?signature=secret",
    ] {
        let target = address.parse().unwrap();
        assert_eq!(
            resource_headers(&origin, &target, true, "test-token").unwrap()["cookie"],
            "mode=relay; music-token=test-token"
        );
    }
    for address in [
        "https://cdn.example/stream",
        "https://nas.example:443/stream",
        "http://nas.example:8443/stream",
    ] {
        assert!(
            !resource_headers(&origin, &address.parse().unwrap(), true, "test-token")
                .unwrap()
                .contains_key("cookie")
        );
    }
    assert_eq!(
        redirect_target(&origin, "/new/path").unwrap().path(),
        "/new/path"
    );
    assert!(redirect_target(&origin, "https://cdn.example/audio?secret=hidden").is_ok());
    for bad in [
        "http://nas.example/stream",
        "file:///etc/passwd",
        "https://name:pass@cdn.example/stream",
    ] {
        let error = redirect_target(&origin, bad).err().unwrap();
        assert!(!error.contains("pass@"));
    }
}

#[test]
fn range_and_media_validation() {
    assert_eq!(
        parse_content_range("bytes 0-4095/9000", 4096).unwrap(),
        4096
    );
    assert_eq!(parse_content_range("bytes 0-1/*", 4096).unwrap(), 2);
    for bad in [
        "bytes 2-3/100",
        "bytes 0-4096/9000",
        "bytes 0-3/3",
        "bytes */100",
        "not a range",
    ] {
        assert!(parse_content_range(bad, 4096).is_err());
    }
    assert!(valid_image("image/png", b"\x89PNG\r\n\x1a\n"));
    assert!(!valid_image("image/png", b"<html>"));
    assert!(!valid_image("text/html", b"\x89PNG\r\n\x1a\n"));
    assert!(valid_audio("application/octet-stream", b"fLaCsample"));
    assert!(!valid_audio("application/octet-stream", b"unknown"));
    assert!(!valid_audio("audio/mpeg", b"  {\"code\":401}"));
}

#[test]
fn mock_lists_search_chinese_and_second_page() {
    let first: Vec<_> = (0..50)
        .map(|i| json!({"guid":format!("track-{i}"),"title":null}))
        .collect();
    let mut replies = vec![
        json_reply(json!({"list":first,"total":51})),
        json_reply(json!({"list":[{"guid":"last"}],"total":"51"})),
    ];
    for _ in 0..4 {
        replies.push(json_reply(json!({"list":[],"total":0})));
    }
    let (session, handle) = logged_in(replies);
    let client = client(3).unwrap();
    let sample = session.library_sample(&client, "track", None).unwrap();
    assert_eq!(
        (sample.total, sample.items.len(), sample.pages),
        (51, 51, 2)
    );
    for kind in ["album", "artist", "playlist"] {
        assert_eq!(
            session.library_sample(&client, kind, None).unwrap().total,
            0
        );
    }
    assert_eq!(
        session
            .library_sample(&client, "track", Some("中文 & 歌曲?"))
            .unwrap()
            .total,
        0
    );
    let requests = handle.join().unwrap();
    assert!(requests[2].starts_with("GET /music/api/v1/track/list?page=1&size=50 "));
    assert!(requests[3].contains("page=2&size=50"));
    let request = requests.last().unwrap();
    let path = request.split_whitespace().nth(1).unwrap();
    let url = url::Url::parse(&format!("http://localhost{path}")).unwrap();
    assert_eq!(
        url.query_pairs().find(|(k, _)| k == "q").unwrap().1,
        "中文 & 歌曲?"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("cookie: mode=relay; music-token=test-token")
    );
}

#[test]
fn mock_second_page_overlap_and_business_error_fail() {
    let first: Vec<_> = (0..50)
        .map(|i| json!({"guid":format!("track-{i}")}))
        .collect();
    let replies = vec![
        json_reply(json!({"list":first,"total":51})),
        json_reply(json!({"list":[{"guid":"track-0"}],"total":51})),
        media(
            "200 OK",
            "Content-Type: application/json\r\n",
            b"{\"code\":999,\"msg\":\"INVALID TOKEN secret\"}",
        ),
    ];
    let (session, handle) = logged_in(replies);
    let client = client(3).unwrap();
    assert!(
        session
            .library_sample(&client, "track", None)
            .err()
            .unwrap()
            .contains("重复")
    );
    let error = session.page(&client, "album", 1, None).err().unwrap();
    assert!(error.contains("会话失效"));
    assert!(!error.contains("secret"));
    handle.join().unwrap();
}

#[test]
fn mock_cover_binary_and_same_origin_audio_redirect() {
    let png = b"\x89PNG\r\n\x1a\nmock";
    let replies = vec![
        media("200 OK", "Content-Type: image/png\r\n", png),
        media(
            "302 Found",
            "Location: /cdn/audio?signature=hidden\r\n",
            b"",
        ),
        media(
            "206 Partial Content",
            "Content-Type: application/octet-stream\r\nContent-Range: bytes 0-7/10000\r\n",
            b"fLaCmock",
        ),
    ];
    let (session, handle) = logged_in(replies);
    let client = client(3).unwrap();
    let cache =
        std::env::temp_dir().join(format!("termune-test-{}", termune::random_id().unwrap()));
    let cover = session.cover(&client, "cover & 中文", &cache).unwrap();
    assert_eq!(cover.bytes, png.len());
    let files: Vec<_> = std::fs::read_dir(cache.join("covers")).unwrap().collect();
    assert_eq!(files.len(), 1);
    assert_eq!(
        std::fs::read(files[0].as_ref().unwrap().path()).unwrap(),
        png
    );
    std::fs::remove_dir_all(cache).unwrap();
    let audio = session.audio(&client, "track & id").unwrap();
    assert_eq!(audio.redirects, 1);
    assert!(audio.range_supported);
    assert!(!audio.cross_origin);
    let requests = handle.join().unwrap();
    for request in &requests[3..] {
        assert!(request.to_ascii_lowercase().contains("range: bytes=0-4095"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("cookie: mode=relay; music-token=test-token")
        );
    }
}

#[test]
fn mock_ignored_range_is_bounded_and_not_reported_supported() {
    let mut body = vec![0; 128 * 1024];
    body[..4].copy_from_slice(b"fLaC");
    let (session, handle) = logged_in(vec![media("200 OK", "Content-Type: audio/flac\r\n", &body)]);
    let report = session.audio(&client(3).unwrap(), "track").unwrap();
    assert_eq!(report.bytes, 4096);
    assert!(!report.range_supported);
    handle.join().unwrap();
}

#[test]
fn mock_invalid_media_and_missing_range_fail_without_leaking_payload() {
    let replies = vec![
        media(
            "206 Partial Content",
            "Content-Type: audio/mpeg\r\n",
            b"ID3mock",
        ),
        media(
            "200 OK",
            "Content-Type: audio/mpeg\r\n",
            b"<html>SECRET</html>",
        ),
        media(
            "206 Partial Content",
            "Content-Type: audio/flac\r\nContent-Range: bytes 0-7/100\r\n",
            b"fLaC",
        ),
        media(
            "302 Found",
            "Location: /music/api/v1/track/stream?guid=loop\r\n",
            b"",
        ),
    ];
    let (session, handle) = logged_in(replies);
    for guid in ["missing-range", "html", "short", "loop"] {
        let error = session.audio(&client(3).unwrap(), guid).err().unwrap();
        assert!(!error.contains("SECRET"));
    }
    handle.join().unwrap();
}
