use super::*;
use ratatui::backend::TestBackend;
use serde_json::json;

fn app() -> App {
    let config = Config {
        path_override: Some(std::env::temp_dir().join(format!(
            "termune-test-{}.toml",
            termune::random_id().unwrap()
        ))),
        last_category: "track".into(),
        ..Default::default()
    };
    App::with_config(config, String::new(), false)
}

fn relay_route() -> termune::Candidate {
    termune::Candidate::new(
        "https://relay.example.test".parse().unwrap(),
        true,
        "FN Connect 中继",
    )
}

#[test]
#[ignore = "exports layout previews with clearly labeled fixture data"]
fn export_ui_previews() {
    let directory = std::path::PathBuf::from(
        std::env::var("TERMUNE_UI_CAPTURE_DIR").expect("capture directory required"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = app();
    app.connected = true;
    app.server = "布局测试 NAS".into();
    app.username = "preview".into();
    app.route = Some(relay_route());
    app.config.professional = true;
    app.views[3]=View{items:(0..12).map(|i|json!({"guid":format!("fixture-{i}"),"title":(["他的睫毛","晴天","七里香","夜曲","一路向北","稻香"][i%6]),"artists":[{"name":"周杰伦"}],"album":{"name":"叶惠美"},"audioSpec":{"format":"FLAC"}})).collect(),total:440,page:1,loaded:true,table:TableState::default().with_selected(1)};
    app.show_status("布局预览 · 测试数据".into(), false);
    app.media = crate::media::Media {
        key: "fixture".into(),
        title: "晴天".into(),
        artist: "周杰伦".into(),
        album: "叶惠美".into(),
        lyrics: vec![
            "界面测试样例，非真实歌词".into(),
            "这是歌词排版示例第一行".into(),
            "每句文字保留清晰的行距".into(),
        ],
        ..Default::default()
    };
    for theme in 0..5 {
        for layout in 0..4 {
            app.config.theme = theme;
            app.config.layout = layout;
            let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let cells:Vec<_>=(0..36).map(|y|(0..120).map(|x|{let c=&buffer[(x,y)];json!({"text":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg)})}).collect::<Vec<_>>()).collect();
            std::fs::write(
                directory.join(format!("theme-{theme}-layout-{layout}.json")),
                serde_json::to_vec(&cells).unwrap(),
            )
            .unwrap();
        }
    }
}
fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                x += ratatui::text::Line::from(symbol).width().max(1) as u16;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn only_the_header_displays_the_selected_connection_route() {
    let mut app = app();
    app.connected = true;
    app.server = "Test NAS".into();
    app.username = "listener".into();
    app.config.layout = 3;
    app.route = Some(relay_route());
    let screen = render(&mut app, 120, 36);
    let metadata = screen.lines().nth(1).unwrap();
    assert!(metadata.contains("Test NAS · listener"));
    assert!(metadata.contains("FN Connect 中继"));
    assert!(!metadata.contains("relay.example.test"));
    assert_eq!(screen.matches("FN Connect 中继").count(), 1);

    let data = json!({"ipv4": ["192.168.1.42"]});
    app.route = termune::candidates(&data, false).into_iter().next();
    let narrow = render(&mut app, 36, 30);
    assert!(
        narrow
            .lines()
            .nth(2)
            .unwrap()
            .contains("局域网 · 192.168.1.42")
    );
    app.config.english = true;
    let screen = render(&mut app, 120, 36);
    assert!(
        screen
            .lines()
            .nth(1)
            .unwrap()
            .contains("LAN · 192.168.1.42")
    );
    assert_eq!(screen.matches("192.168.1.42").count(), 1);
}

#[test]
fn playback_shortcuts_select_tracks_and_cancel_pending_audio() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.connected = true;
    app.views[3].items = vec![
        json!({"guid":"first","title":"First"}),
        json!({"guid":"second","title":"Second"}),
    ];
    app.views[3].table.select(Some(0));
    key(&mut app, KeyCode::Enter);
    match commands.try_recv().unwrap() {
        Command::Play { id, guid, .. } => {
            assert_eq!(id, app.audio_generation);
            assert_eq!(guid, "first");
        }
        _ => panic!("expected play request"),
    }
    key(&mut app, KeyCode::Char('i'));
    assert!(commands.try_recv().is_err());
    key(&mut app, KeyCode::Enter);
    assert!(matches!(commands.try_recv().unwrap(), Command::Play { guid, .. } if guid == "first"));
    key(&mut app, KeyCode::Char(']'));
    match commands.try_recv().unwrap() {
        Command::Advance { context, .. } => assert_eq!(context.index, 1),
        _ => panic!("expected play request"),
    }
    let previous = app.audio_generation;
    key(&mut app, KeyCode::Char('s'));
    assert!(app.audio_generation > previous);
    assert!(app.audio_loading.is_none());
}

#[test]
fn login_render_masks_password_and_handles_small_terminals() {
    let mut app = app();
    app.fields = [
        "test-id".into(),
        "tester".into(),
        "never-show-secret".into(),
    ];
    app.focus = 2;
    let normal = render(&mut app, 100, 32);
    assert!(normal.contains("TERMUNE"));
    assert!(!normal.contains("never-show-secret"));
    assert!(normal.contains("••••"));
    let password = std::mem::take(&mut app.fields[2]);
    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let field = app
        .hits
        .iter()
        .find_map(|(rect, action)| matches!(action, Action::Field(2)).then_some(*rect))
        .unwrap();
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        (field.x + 1, field.y + 1).into()
    );
    app.fields[2] = password;
    for (w, h) in [(36, 16), (45, 20), (1, 1), (20, 8)] {
        let text = render(&mut app, w, h);
        assert!(!text.contains("never-show-secret"));
    }
}

#[test]
fn login_shortcut_letters_remain_typable_and_small_form_is_usable() {
    let mut app = app();
    for c in ['g', 't', 'm', 'l', 'd'] {
        key(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.fields[0], "gtmld");
    let screen = render(&mut app, 36, 20);
    assert!(screen.contains("[F1 帮助]"));
    assert!(screen.contains("下次自动登录"));
    assert!(screen.contains("登录飞牛音乐"));
    assert!(
        screen
            .lines()
            .any(|line| line.contains("登录") && line.contains("Enter"))
    );
    assert_eq!(
        app.hits
            .iter()
            .filter(|(_, action)| matches!(action, Action::Field(_)))
            .count(),
        3
    );
    assert!(
        app.hits
            .iter()
            .any(|(_, action)| matches!(action, Action::Connect))
    );
    assert!(
        app.hits
            .iter()
            .all(|(rect, _)| rect.right() <= 36 && rect.bottom() <= 20)
    );
    key(&mut app, KeyCode::F(1));
    assert!(app.help);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('?'));
    assert_eq!(app.fields[0], "gtmld?");
}

#[test]
fn login_arrows_edit_the_middle_of_the_address() {
    let mut app = app();
    app.fields[0] = "https://nas.example:500".into();
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(app.fields[0], "https://nas.example:5010");
}

#[test]
fn scrolled_login_address_supports_cursor_movement_and_mouse_editing() {
    let mut app = app();
    app.fields[0] = format!("https://nas.example/{}/5000", "proxy".repeat(10));
    let mut terminal = Terminal::new(TestBackend::new(36, 20)).unwrap();
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let end = terminal.get_cursor_position().unwrap();
    key(&mut app, KeyCode::Left);
    terminal
        .draw(|frame| crate::ui::draw(frame, &mut app))
        .unwrap();
    let caret = terminal.get_cursor_position().unwrap();
    assert_eq!(caret.x + 1, end.x);
    click_input(&mut app, end.x - 3, end.y);
    key(&mut app, KeyCode::Delete);
    app.event(Event::Paste("8".into()));
    assert!(app.fields[0].ends_with("/5800"));
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Backspace);
    assert!(app.fields[0].starts_with("ttps://"));
}

#[test]
fn search_and_playlist_edit_at_the_clicked_grapheme() {
    let mut app = app();
    app.connected = true;
    app.action(Action::Search);
    app.event(Event::Paste("中文e\u{301}".into()));
    render(&mut app, 100, 30);
    let second = app
        .hits
        .iter()
        .find_map(|(rect, action)| {
            matches!(action, Action::InputCaret(InputField::Search, 3)).then_some(*rect)
        })
        .unwrap();
    assert_eq!(second.width, 2);
    click_input(&mut app, second.x, second.y);
    key(&mut app, KeyCode::Delete);
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.search_input, "中");

    key(&mut app, KeyCode::Esc);
    app.new_playlist(None);
    app.event(Event::Paste("歌曲单".into()));
    render(&mut app, 100, 30);
    let second = app
        .hits
        .iter()
        .find_map(|(rect, action)| {
            matches!(action, Action::InputCaret(InputField::PlaylistName, 3)).then_some(*rect)
        })
        .unwrap();
    click_input(&mut app, second.x, second.y);
    key(&mut app, KeyCode::Right);
    app.event(Event::Paste("清".into()));
    assert!(
        matches!(app.playlist_dialog, Some(PlaylistDialog::Name { input, .. }) if input == "歌曲清单")
    );
}

fn click_input(app: &mut App, column: u16, row: u16) {
    app.event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }));
}

#[test]
fn login_button_click_only_submits_inside_the_button() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.fields = ["test-id".into(), "listener".into(), "secret".into()];
    app.focus = 2;
    render(&mut app, 86, 29);
    let button = app
        .hits
        .iter()
        .find_map(|(rect, action)| matches!(action, Action::Connect).then_some(*rect))
        .unwrap();
    assert_eq!((button.width, button.height), (14, 1));
    let click = |column, row| {
        Event::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    app.event(click(button.right(), button.y));
    assert!(commands.try_recv().is_err());
    app.event(click(button.x, button.y));
    assert!(matches!(
        commands.try_recv().unwrap(),
        Command::Login { .. }
    ));
    let screen = render(&mut app, 86, 29);
    assert!(screen.contains("正在连接…"));
    assert!(
        !app.hits
            .iter()
            .any(|(_, action)| matches!(action, Action::Connect | Action::ToggleRemember))
    );
}

#[test]
fn lan_http_setting_is_saved_and_used_for_login_and_restore() {
    let mut app = app();
    assert!(!app.allow_lan_http());
    app.action(Action::Settings);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.settings, Some(LAN_HTTP_SETTING));
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("局域网 HTTP"));
    assert!(screen.contains("HTTP 明文传输凭据"));
    key(&mut app, KeyCode::Enter);

    let path = app.config.path_override.as_ref().unwrap();
    let config: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut app = App::with_config(config, String::new(), false);
    assert!(app.allow_lan_http());
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.fields = ["test-id".into(), "listener".into(), "fixture".into()];
    app.connect();
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Login {
            allow_http: true,
            ..
        })
    ));
    app.restore();
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Restore {
            allow_http: true,
            ..
        })
    ));
}

#[test]
fn manual_http_login_waits_for_settings_and_preserves_the_form() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.fields = [
        "http://192.168.1.42:5666/Music".into(),
        "listener".into(),
        "fixture".into(),
    ];
    let screen = render(&mut app, 36, 20);
    assert!(screen.contains("局域网 HTTP 已关闭"));
    assert!(screen.contains("打开设置"));
    app.connect();
    assert!(commands.try_recv().is_err());
    assert_eq!(app.fields[2], "fixture");

    key(&mut app, KeyCode::F(2));
    assert_eq!(app.settings, Some(LAN_HTTP_SETTING));
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.fields[2], "fixture");
    let (replies, rx) = mpsc::channel();
    app.rx = rx;
    app.connect();
    let address = "http://192.168.1.42:5666/Music";
    let Ok(Command::Login {
        id,
        target,
        allow_http: true,
        ..
    }) = commands.try_recv()
    else {
        panic!("expected manual login after enabling LAN HTTP");
    };
    assert_eq!(target, address);
    let route = Target::parse(address)
        .unwrap()
        .manual_candidate(true)
        .unwrap()
        .unwrap();
    replies
        .send(Reply::Connected(
            id,
            Ok(Connected {
                server: "test NAS".into(),
                username: "listener".into(),
                route,
                remembered: true,
                warning: None,
            }),
        ))
        .unwrap();
    app.replies();
    assert!(render(&mut app, 100, 24).contains(address));

    let path = app.config.path_override.unwrap();
    let mut config: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.path_override = Some(path.clone());
    assert!(config.auto_login);
    assert_eq!(config.nas_address, address);
    assert!(config.fn_id.is_empty());
    let mut restarted = App::with_config(config, String::new(), false);
    assert_eq!(restarted.fields[0], address);
    let (tx, commands) = mpsc::channel();
    restarted.tx = tx;
    restarted.restore();
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Restore { target, allow_http: true, .. }) if target == address
    ));
    restarted.toggle_remember();
    assert!(matches!(commands.try_recv(), Ok(Command::Logout)));
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Forget { target, .. }) if target == address
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn login_hint_matches_the_entered_address_and_protocol() {
    let mut app = app();
    app.config.allow_lan_http = true;
    app.fields[0] = "https://nas.example.com/login".into();
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("HTTPS 直连"));
    assert!(screen.contains("网页登录地址请去掉 /login"));
    assert!(!screen.contains("未加密"));

    app.fields[0] = "http://192.168.1.42:5666".into();
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("局域网 HTTP 直连"));
    assert!(screen.contains("凭据未加密"));

    app.fields[0] = "test-id".into();
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("FN ID · 自动探测连接线路"));
    assert!(screen.contains("已允许未加密的局域网 HTTP 线路"));
}

#[test]
fn access_code_prompt_masks_input_retries_and_ignores_cancelled_attempts() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    let (replies, rx) = mpsc::channel();
    app.rx = rx;
    app.fields = [
        "https://nas.example".into(),
        "tester".into(),
        "music-fixture".into(),
    ];
    app.connect();
    let Ok(Command::Login { id, .. }) = commands.try_recv() else {
        panic!("expected music login");
    };
    replies
        .send(Reply::AccessCode {
            id,
            error: None,
            origin_root_available: false,
            lan_address: None,
        })
        .unwrap();
    app.replies();
    app.event(Event::Paste("access-fixture".into()));
    let screen = render(&mut app, 80, 24);
    assert!(screen.contains("飞牛访问码"));
    assert!(screen.contains("••••"));
    assert!(!screen.contains("access-fixture"));
    key(&mut app, KeyCode::Enter);
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::AccessCode { id: attempt, code, destination: AccessDestination::Preferred }) if attempt == id && code == "access-fixture"
    ));
    assert!(app.access_prompt.as_ref().unwrap().input.is_empty());
    replies
        .send(Reply::AccessCode {
            id,
            error: Some("访问码验证未通过".into()),
            origin_root_available: false,
            lan_address: None,
        })
        .unwrap();
    app.replies();
    assert!(render(&mut app, 80, 24).contains("访问码验证未通过"));
    assert!(commands.try_recv().is_err());

    key(&mut app, KeyCode::Esc);
    assert!(matches!(commands.try_recv(), Ok(Command::Logout)));
    replies
        .send(Reply::AccessCode {
            id,
            error: None,
            origin_root_available: false,
            lan_address: None,
        })
        .unwrap();
    app.replies();
    assert!(app.access_prompt.is_none());
    assert_eq!(app.fields[0], "https://nas.example/");
}

#[test]
fn root_access_code_submission_requires_a_separate_user_action() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    let (replies, rx) = mpsc::channel();
    app.rx = rx;
    let id = app.generation;
    replies
        .send(Reply::AccessCode {
            id,
            error: None,
            origin_root_available: false,
            lan_address: None,
        })
        .unwrap();
    app.replies();
    app.event(Event::Paste("fixture".into()));
    key(&mut app, KeyCode::F(3));
    assert!(commands.try_recv().is_err());
    key(&mut app, KeyCode::Enter);
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::AccessCode {
            destination: AccessDestination::Preferred,
            ..
        })
    ));

    replies
        .send(Reply::AccessCode {
            id,
            error: Some(termune::ACCESS_CODE_PATH_NOT_FOUND.into()),
            origin_root_available: true,
            lan_address: None,
        })
        .unwrap();
    app.replies();
    assert!(render(&mut app, 36, 20).contains("[F3 根路径验证]"));
    assert!(commands.try_recv().is_err());
    app.event(Event::Paste("fixture".into()));
    key(&mut app, KeyCode::F(3));
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::AccessCode { destination: AccessDestination::OriginRoot, code, .. }) if code == "fixture"
    ));
}

#[test]
fn access_code_prompt_shows_the_lan_address_and_routes_the_users_choice() {
    for (key_code, use_relay) in [(KeyCode::Enter, false), (KeyCode::F(4), true)] {
        let mut app = app();
        let (tx, commands) = mpsc::channel();
        app.tx = tx;
        let address = "http://192.168.42.1:5666/";
        app.access_prompt = Some(AccessPrompt {
            lan_address: Some(address.into()),
            ..AccessPrompt::default()
        });
        let screen = render(&mut app, 36, 20);
        assert!(screen.contains(address));
        assert!(screen.contains("[Enter 验证局域网]"));
        assert!(screen.contains("[F4 使用中继]"));
        assert!(commands.try_recv().is_err());
        app.event(Event::Paste("fixture".into()));
        key(&mut app, key_code);
        let Command::AccessCode {
            destination, code, ..
        } = commands.try_recv().unwrap()
        else {
            panic!("expected access code submission");
        };
        assert_eq!(code, "fixture");
        assert_eq!(matches!(destination, AccessDestination::Relay), use_relay);
    }
}

#[test]
fn access_code_actions_share_a_row_when_they_fit_and_stay_put_during_verification() {
    for (width, english, stacked) in [(36, false, false), (36, true, true), (80, true, false)] {
        let mut app = app();
        app.config.english = english;
        app.access_prompt = Some(AccessPrompt::default());
        render(&mut app, width, 20);
        let hit = |app: &App, predicate: fn(Action) -> bool| {
            app.hits
                .iter()
                .find_map(|(rect, action)| predicate(*action).then_some(*rect))
        };
        let verify = |action| matches!(action, Action::SubmitAccessCode);
        let switch = |action| matches!(action, Action::SwitchAddress);
        let verify_area = hit(&app, verify).unwrap();
        let switch_area = hit(&app, switch).unwrap();
        assert_eq!(switch_area.y, verify_area.y + u16::from(stacked));
        assert!(switch_area.right() <= width);
        if !stacked {
            assert!(switch_area.x > verify_area.right());
        }
        app.busy = true;
        render(&mut app, width, 20);
        assert_eq!(hit(&app, switch), Some(switch_area));
        assert!(hit(&app, verify).is_none());
    }
}

#[test]
fn settings_and_help_are_scoped_and_do_not_change_layout_on_open() {
    let mut app = app();
    app.connected = true;
    app.config.professional = false;
    app.config.layout = 2;
    key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.settings, Some(0));
    assert!(!app.config.professional);
    assert_eq!(app.config.layout, 2);
    let settings = render(&mut app, 100, 30);
    assert!(settings.contains("Mint"));
    assert!(!settings.contains("播放模式"));
    assert!(settings.contains("断开连接"));
    assert!(app.hits.iter().all(|(_, action)| matches!(
        action,
        Action::Setting(_) | Action::ChangeSetting(_, _) | Action::CloseModal | Action::Disconnect
    )));
    key(&mut app, KeyCode::Esc);
    assert!(app.settings.is_none());
    key(&mut app, KeyCode::Char('?'));
    assert!(app.help);
    assert!(render(&mut app, 100, 30).contains("快捷键"));
    key(&mut app, KeyCode::Esc);
    assert!(!app.help);
}

#[test]
fn library_render_supports_empty_unicode_pagination_and_modal() {
    let mut app = app();
    app.connected = true;
    app.server = "测试 NAS".into();
    app.username = "tester".into();
    let item = json!({"guid":"one","title":"他的睫毛 🎵","artists":[{"name":"周杰伦"}],"album":{"name":"叶惠美"},"audioSpec":{"format":"FLAC"}});
    app.views[3] = View {
        items: vec![item],
        total: 440,
        page: 1,
        loaded: true,
        table: TableState::default().with_selected(0),
    };
    let text = render(&mut app, 100, 32);
    assert!(text.contains("他的睫毛"));
    assert!(text.contains("周杰伦"));
    assert!(text.contains("440"));
    key(&mut app, KeyCode::Char('i'));
    assert!(!render(&mut app, 100, 32).contains("歌曲详情"));
    key(&mut app, KeyCode::Esc);
    app.section = 2;
    app.views[2].loaded = true;
    assert!(render(&mut app, 80, 24).contains("这里还没有音乐数据"));
    app.config.english = true;
    assert!(render(&mut app, 80, 24).contains("No music here yet"));
    for theme in 0..5 {
        app.config.theme = theme;
        render(&mut app, 36, 16);
    }
}

#[test]
fn full_layouts_are_distinct_and_minimal_mode_preserves_layout() {
    let mut app = app();
    app.connected = true;
    app.config.professional = true;
    app.views[3].items = vec![json!({"guid":"one","title":"Test song"})];
    app.views[3].table.select(Some(0));
    let markers = ["AIRWAVE", "OPERATIONS", "TRACKER", "BTOP"];
    for (layout, marker) in markers.into_iter().enumerate() {
        app.config.layout = layout;
        let screen = render(&mut app, 100, 30);
        assert!(screen.contains(marker), "missing {marker}");
        assert!(screen.contains("Test song"));
        assert!(
            app.hits
                .iter()
                .all(|(r, _)| r.right() <= 100 && r.bottom() <= 30)
        );
        render(&mut app, 36, 20);
        assert!(
            app.hits
                .iter()
                .all(|(r, _)| r.right() <= 36 && r.bottom() <= 20)
        );
    }
    app.config.professional = false;
    let minimal = render(&mut app, 100, 30);
    assert!(!markers.iter().any(|marker| minimal.contains(marker)));
    assert!(minimal.contains("Test song"));
    assert_eq!(app.config.layout, 3);
    app.config.professional = true;
    assert!(render(&mut app, 100, 30).contains("BTOP"));
}

#[test]
fn stale_response_is_discarded_and_failed_search_preserves_library() {
    let mut app = app();
    app.connected = true;
    app.views[3] = View {
        items: vec![json!({"guid":"kept","title":"library song"})],
        total: 1,
        page: 1,
        loaded: true,
        ..View::default()
    };
    let (tx, rx) = mpsc::channel();
    app.rx = rx;
    app.generation = 10;
    app.busy = true;
    let load = Load {
        id: 9,
        section: 3,
        page: 2,
        query: None,
        detail: None,
        selection: None,
    };
    tx.send(Reply::Page(
        load,
        Ok(Page {
            items: vec![json!({"guid":"old"})],
            total: 100,
        }),
    ))
    .unwrap();
    app.replies();
    assert_eq!(app.views[3].items[0]["guid"], "kept");
    assert!(app.busy);
    app.query = Some("搜索".into());
    tx.send(Reply::Page(
        Load {
            id: 10,
            section: 3,
            page: 1,
            query: Some("搜索".into()),
            detail: None,
            selection: None,
        },
        Err("请求超时".into()),
    ))
    .unwrap();
    app.replies();
    assert!(app.error);
    assert!(!app.busy);
    assert_eq!(app.views[3].items[0]["guid"], "kept");
    key(&mut app, KeyCode::Esc);
    assert!(app.query.is_none());
    assert_eq!(app.view().items.len(), 1);
    assert!(!app.error);
}

#[test]
fn failed_login_retains_id_and_username_and_clears_password() {
    let mut app = app();
    let (commands, rx) = mpsc::channel();
    app.tx = commands;
    let (replies, events) = mpsc::channel();
    app.rx = events;
    app.fields = ["test-id".into(), "tester".into(), "secret".into()];
    app.connect();
    assert!(app.fields[2].is_empty());
    let id = match rx.recv().unwrap() {
        Command::Login { id, password, .. } => {
            assert_eq!(password, "secret");
            id
        }
        _ => panic!("expected login"),
    };
    replies
        .send(Reply::Connected(id, Err("业务码 120001".into())))
        .unwrap();
    app.replies();
    assert_eq!(app.fields[0], "test-id");
    assert_eq!(app.fields[1], "tester");
    assert!(!app.connected);
    assert!(!app.busy);
}

#[test]
fn entering_album_and_back_restores_parent_selection() {
    let mut app = app();
    let (tx, rx) = mpsc::channel();
    app.tx = tx;
    app.connected = true;
    app.section = 0;
    app.views[0] = View {
        items: vec![json!({"guid":"album-id","name":"专辑"})],
        total: 1,
        page: 3,
        loaded: true,
        table: TableState::default().with_selected(0),
    };
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.detail.as_ref().unwrap().guid, "album-id");
    match rx.recv().unwrap() {
        Command::Load(load) => assert_eq!(load.detail.unwrap().kind, "album"),
        _ => panic!("expected detail load"),
    };
    key(&mut app, KeyCode::Esc);
    assert!(app.detail.is_none());
    assert_eq!(app.view().page, 3);
    assert_eq!(app.view().table.selected(), Some(0));
}

#[test]
fn remember_checkbox_is_focusable_and_forwarded_without_persisting_password() {
    let mut app = app();
    let (commands, rx) = mpsc::channel();
    app.tx = commands;
    app.fields = ["test-id".into(), "tester".into(), "secret".into()];
    app.focus = 2;
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, 3);
    key(&mut app, KeyCode::Char(' '));
    assert!(app.remember);
    assert!(!app.config.auto_login);
    assert!(render(&mut app, 100, 32).contains("[x]"));
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, 4);
    key(&mut app, KeyCode::Enter);
    match rx.recv().unwrap() {
        Command::Login {
            remember, password, ..
        } => {
            assert!(remember);
            assert_eq!(password, "secret");
        }
        _ => panic!("expected manual login"),
    };
    assert!(app.fields[2].is_empty());
}

#[test]
fn restore_has_no_password_and_falls_back_to_login_on_failure() {
    let config = Config {
        fn_id: "test-id".into(),
        username: "tester".into(),
        auto_login: true,
        ..Default::default()
    };
    let mut app = App::with_config(config, String::new(), false);
    let (commands, rx) = mpsc::channel();
    app.tx = commands;
    let (replies, events) = mpsc::channel();
    app.rx = events;
    app.restore();
    assert!(app.busy);
    assert!(app.fields[2].is_empty());
    assert!(app.restoring);
    let screen = render(&mut app, 100, 32);
    assert!(screen.contains("正在恢复登录状态"));
    assert!(!screen.contains("音乐密码"));
    let id = match rx.recv().unwrap() {
        Command::Restore {
            id,
            target,
            username,
            ..
        } => {
            assert_eq!(target, "test-id");
            assert_eq!(username, "tester");
            id
        }
        _ => panic!("expected token restoration"),
    };
    for stage in ["discovery", "probing", "initialization", "restore"] {
        replies.send(Reply::Progress(id, stage)).unwrap();
        app.replies();
        assert!(!render(&mut app, 100, 32).contains("音乐密码"));
    }
    replies
        .send(Reply::Connected(
            id,
            Err("自动登录已失效，请重新输入密码".into()),
        ))
        .unwrap();
    app.replies();
    assert!(!app.connected);
    assert!(!app.busy);
    assert_eq!(app.focus, 2);
    assert!(app.error);
    assert!(!app.restoring);
    assert!(render(&mut app, 100, 32).contains("重新输入密码"));
    assert!(render(&mut app, 100, 32).contains("音乐密码"));
}

#[test]
fn lyrics_only_appear_in_professional_mode_and_stale_session_media_is_ignored() {
    let mut app = app();
    app.connected = true;
    app.media.lyrics = vec!["可辨识的测试歌词".into()];
    for layout in 0..4 {
        app.config.layout = layout;
        let screen = render(&mut app, 120, 36);
        assert_eq!(
            screen.matches("可辨识的测试歌词").count(),
            1,
            "layout {layout}"
        );
        assert_eq!(
            app.hits
                .iter()
                .filter(|(_, action)| matches!(action, Action::LyricsScroll(0)))
                .count(),
            1,
            "layout {layout}"
        );
        render(&mut app, 36, 20);
        assert!(
            app.hits
                .iter()
                .all(|(r, _)| r.right() <= 36 && r.bottom() <= 20)
        );
    }
    app.config.professional = false;
    let minimal = render(&mut app, 100, 30);
    assert!(!minimal.contains("可辨识的测试歌词"));
    let old = app.media_epoch;
    let (tx, rx) = mpsc::channel();
    app.rx = rx;
    app.logout();
    tx.send(Reply::Media(
        old,
        "old-user".into(),
        crate::media::Download {
            lyrics: Ok(crate::media::LyricData {
                lines: vec!["private".into()],
                times: vec![None],
            }),
        },
    ))
    .unwrap_or_else(|_| panic!("closed"));
    app.replies();
    assert!(app.media.key.is_empty());
    assert!(app.media_cache.is_empty());
}

#[test]
fn opening_search_starts_with_an_empty_draft() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    key(&mut app, KeyCode::Char('/'));
    app.event(Event::Paste("previous".into()));
    key(&mut app, KeyCode::Enter);
    assert!(matches!(commands.try_recv(), Ok(Command::Load(_))));
    key(&mut app, KeyCode::Char('/'));
    assert!(app.editing);
    assert!(app.search_input.is_empty());
    assert_eq!(app.query.as_deref(), Some("previous"));
    app.event(Event::Paste("discarded".into()));
    key(&mut app, KeyCode::Esc);
    assert!(app.search_input.is_empty());
    assert_eq!(app.query.as_deref(), Some("previous"));
}

#[test]
fn footer_feedback_expires_without_input_and_reveals_pending_status() {
    let mut app = app();
    app.connected = true;
    app.action(Action::Stop);
    assert!(render(&mut app, 100, 30).contains("播放已停止"));
    app.show_notice("Credential warning".into());
    assert!(render(&mut app, 100, 30).contains("Credential warning"));
    app.fail("Request failed".into());
    assert!(render(&mut app, 100, 30).contains("Request failed"));

    app.status.as_mut().unwrap().until = Instant::now();
    let screen = render(&mut app, 100, 30);
    assert!(!screen.contains("Request failed"));
    assert!(screen.contains("Credential warning"));
    app.notice.as_mut().unwrap().until = Instant::now();
    assert!(!render(&mut app, 100, 30).contains("Credential warning"));

    app.action(Action::Stop);
    app.status.as_mut().unwrap().until = Instant::now();
    assert!(!render(&mut app, 100, 30).contains("播放已停止"));
}

#[test]
fn seek_without_duration_shows_a_temporary_message() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        for header in BufReader::new(&mut socket).lines() {
            if header.unwrap().is_empty() {
                break;
            }
        }
        let body = r#"{"code":0,"data":{"guid":"test-user","name":"test"}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let session = termune::Candidate::new(base, false, "test")
        .restore(&termune::client(3).unwrap(), "fixture-token".into())
        .unwrap();
    server.join().unwrap();

    let mut app = app();
    app.connected = true;
    app.player = Some(
        crate::player::Player::start(
            termune::stream::AudioSource {
                session,
                guid: "test-track".into(),
            },
            "Test track".into(),
            0,
            100,
        )
        .unwrap(),
    );
    app.show_status("Previous status".into(), false);
    app.event(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::CONTROL,
    )));
    let message = "尚未取得歌曲时长，暂不能拖动";
    assert!(render(&mut app, 100, 30).contains(message));
    app.seek_error.as_mut().unwrap().until = Instant::now();
    let screen = render(&mut app, 100, 30);
    assert!(!screen.contains(message), "seek message stayed visible");
    assert!(screen.contains("Previous status"));
}

#[test]
fn global_m_category_shortcuts_and_dragging_have_correct_scope() {
    let mut app = app();
    app.connected = true;
    let initial = app.config.professional;
    key(&mut app, KeyCode::Char('m'));
    assert_eq!(app.config.professional, !initial);
    key(&mut app, KeyCode::Char('M'));
    assert_eq!(app.config.professional, initial);
    app.editing = true;
    key(&mut app, KeyCode::Char('m'));
    assert_eq!(app.search_input, "m");
    assert_eq!(app.config.professional, initial);
    app.editing = false;
    for (index, character) in ('1'..='5').enumerate() {
        app.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT));
        assert_eq!(app.section, index);
    }
    app.seek_rect = Some(Rect::new(10, 10, 21, 1));
    app.hits = vec![(Rect::new(15, 10, 1, 1), Action::Seek(0.25))];
    app.event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.seek_drag, Some(0.25));
    app.event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 25,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.seek_drag, Some(0.75));
    app.event(Event::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 25,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.seek_drag.is_none());
    let _ = std::fs::remove_file(app.config.path_override.as_ref().unwrap());
}

#[test]
fn automatic_cross_page_play_and_single_repeat_keep_origin_context() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    let (replies, rx) = mpsc::channel();
    app.tx = tx;
    app.rx = rx;
    app.views[3] = View {
        items: (0..50)
            .map(|i| json!({"guid":format!("song-{i}"),"title":"test"}))
            .collect(),
        total: 51,
        page: 1,
        loaded: true,
        table: TableState::default().with_selected(49),
    };
    app.play_selected();
    assert!(matches!(commands.recv().unwrap(), Command::Play { .. }));
    app.section = 1;
    app.advance(true, true);
    match commands.recv().unwrap() {
        Command::Advance { id, context } => {
            assert_eq!(context.section, TRACKS);
            assert_eq!(context.index, 50);
            replies
                .send(Reply::Advance(
                    id,
                    context,
                    Ok(Page {
                        items: vec![json!({"guid":"song-50","title":"Next page"})],
                        total: 51,
                    }),
                ))
                .unwrap();
        }
        _ => panic!("expected cross-page queue request"),
    }
    app.replies();
    assert!(
        matches!(commands.try_recv().unwrap(), Command::Play { guid, .. } if guid == "song-50")
    );
    assert_eq!(app.section, 1);
    assert_eq!(app.views[TRACKS].page, 1);
    app.playing_context = Some(PlaybackContext {
        section: 3,
        query: None,
        detail: None,
        index: 49,
        total: 51,
        playlist: None,
    });
    app.playing_item = Some(json!({"guid":"original","title":"song"}));
    app.config.playback_mode = crate::queue::Mode::RepeatOne;
    app.advance(true, true);
    assert!(matches!(commands.recv().unwrap(),Command::Play{guid,..}if guid=="original"));
}

#[test]
fn playback_modes_focus_the_new_track_and_page_in_the_current_source() {
    use crate::queue::Mode;

    for (mode, index, total, target) in [
        (Mode::Sequence, 49, 52, 50),
        (Mode::Loop, 51, 52, 0),
        (Mode::Shuffle, 0, 2, 1),
        (Mode::RepeatOne, 1, 2, 1),
    ] {
        let items = |page: u64| {
            ((page - 1) * 50..(page * 50).min(total))
                .map(|i| json!({"guid":format!("song-{i}"),"title":format!("Song {i}")}))
                .collect()
        };
        let mut app = app();
        app.connected = true;
        app.config.playback_mode = mode;
        let (tx, commands) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        app.tx = tx;
        app.rx = rx;
        let start_page = index / 50 + 1;
        app.view_mut().set_page(
            start_page,
            Page {
                items: items(start_page),
                total,
            },
        );
        app.view_mut().table.select(Some((index % 50) as usize));
        app.play_selected();
        assert!(matches!(commands.try_recv(), Ok(Command::Play { .. })));
        app.view_mut().table.select(None);

        app.advance(true, true);
        let page = target / 50 + 1;
        if mode != Mode::RepeatOne {
            let Command::Advance { id, context } = commands.try_recv().unwrap() else {
                panic!("expected next track request for {mode:?}");
            };
            assert_eq!(context.index, target);
            replies
                .send(Reply::Advance(
                    id,
                    context,
                    Ok(Page {
                        items: items(page),
                        total,
                    }),
                ))
                .unwrap();
            app.replies();
        }
        assert!(
            matches!(commands.try_recv(), Ok(Command::Play { guid, .. }) if guid == format!("song-{target}"))
        );
        assert_eq!(app.view().page, page, "{mode:?}");
        assert_eq!(
            app.view().table.selected(),
            Some((target % 50) as usize),
            "{mode:?}"
        );
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn lyric_scrolling_resumes_sync_after_ten_seconds_without_another_scroll() {
    let mut app = app();
    let now = Instant::now();
    app.action(Action::LyricsScroll(1));
    assert!(!app.lyrics_follow);
    assert!(app.lyrics_resume_at.unwrap() >= now + Duration::from_secs(10));
    app.replies();
    assert!(!app.lyrics_follow);

    app.lyrics_resume_at = Some(Instant::now());
    app.action(Action::LyricsScroll(-1));
    app.replies();
    assert!(!app.lyrics_follow);
    app.lyrics_resume_at = Some(Instant::now());
    app.replies();
    assert!(app.lyrics_follow);
    assert!(app.lyrics_resume_at.is_none());
}

#[test]
fn favorites_require_unmodified_key_and_update_only_after_server_confirmation() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    let (events, rx) = mpsc::channel();
    app.rx = rx;
    app.views[3].items = vec![json!({"guid":"song","title":"Test","isFavorite":false})];
    app.views[3].table.select(Some(0));
    app.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert!(commands.try_recv().is_err());
    key(&mut app, KeyCode::Char('f'));
    match commands.recv().unwrap() {
        Command::Favorite { guid, enabled, .. } => {
            assert_eq!(guid, "song");
            assert!(enabled);
        }
        _ => panic!("favorite request missing"),
    };
    assert_eq!(app.views[3].items[0]["isFavorite"], false);
    events
        .send(Reply::Favorite(
            app.media_epoch,
            "song".into(),
            true,
            Ok(()),
        ))
        .unwrap();
    app.replies();
    assert_eq!(app.views[3].items[0]["isFavorite"], true);
}

#[test]
fn shuffle_previous_uses_actual_history() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.config.playback_mode = crate::queue::Mode::Shuffle;
    app.playing_context = Some(PlaybackContext {
        section: 3,
        query: None,
        detail: None,
        index: 3,
        total: 6,
        playlist: None,
    });
    app.shuffle_history.push(PlaybackContext {
        section: 3,
        query: None,
        detail: None,
        index: 1,
        total: 6,
        playlist: None,
    });
    app.advance(false, false);
    assert!(matches!(commands.recv().unwrap(),Command::Advance{context,..}if context.index==1));
    assert!(app.shuffle_history.is_empty());
}

#[test]
fn restored_session_enters_library_directly_and_escape_cancels_restore() {
    let mut app = app();
    let (commands, rx) = mpsc::channel();
    app.tx = commands;
    let (replies, events) = mpsc::channel();
    app.rx = events;
    app.restore();
    let id = match rx.recv().unwrap() {
        Command::Restore { id, .. } => id,
        _ => panic!(),
    };
    replies
        .send(Reply::Connected(
            id,
            Ok(Connected {
                server: "test NAS".into(),
                username: "tester".into(),
                route: relay_route(),
                remembered: true,
                warning: None,
            }),
        ))
        .unwrap();
    app.replies();
    assert!(app.connected);
    assert!(!app.restoring);
    assert!(matches!(rx.recv().unwrap(), Command::Load(_)));
    assert!(!render(&mut app, 100, 32).contains("音乐密码"));
    app.logout();
    while rx.try_recv().is_ok() {}
    app.restore();
    key(&mut app, KeyCode::Esc);
    assert!(!app.restoring);
    assert!(!app.busy);
    assert!(render(&mut app, 100, 32).contains("音乐密码"));
}

#[test]
fn switch_address_leaves_auto_login_and_access_code_verification() {
    for verifying in [false, true] {
        let mut app = app();
        let address = "https://nas.example/";
        app.config.auto_login = true;
        app.config.nas_address = address.into();
        app.config.username = "tester".into();
        app.fields[0] = address.into();
        app.focus = 2;
        let (tx, commands) = mpsc::channel();
        app.tx = tx;
        let (replies, rx) = mpsc::channel();
        app.rx = rx;
        app.restore();
        let Command::Restore { id, .. } = commands.try_recv().unwrap() else {
            panic!("expected automatic login");
        };
        if verifying {
            replies
                .send(Reply::AccessCode {
                    id,
                    error: None,
                    origin_root_available: false,
                    lan_address: None,
                })
                .unwrap();
            app.replies();
            app.event(Event::Paste("access-fixture".into()));
            key(&mut app, KeyCode::Enter);
            assert!(matches!(
                commands.try_recv(),
                Ok(Command::AccessCode { .. })
            ));
        }
        let screen = render(&mut app, 36, 20);
        assert!(screen.contains("[Esc 切换地址]"));
        let button = app
            .hits
            .iter()
            .find_map(|(rect, action)| matches!(action, Action::SwitchAddress).then_some(*rect))
            .unwrap();
        click_input(&mut app, button.x, button.y);
        assert!(matches!(commands.try_recv(), Ok(Command::Logout)));
        assert!(!app.busy && !app.restoring && app.access_prompt.is_none());
        assert_eq!(app.focus, 0);
        assert!(app.config.auto_login);
        assert_eq!(app.config.nas_address, address);
        assert_eq!(app.fields[0], address);
        replies
            .send(Reply::AccessCode {
                id,
                error: None,
                origin_root_available: false,
                lan_address: None,
            })
            .unwrap();
        app.replies();
        assert!(app.access_prompt.is_none());
        let screen = render(&mut app, 36, 20);
        assert!(screen.contains("FN ID 或 NAS 地址"));
        assert!(
            app.hits
                .iter()
                .any(|(_, action)| matches!(action, Action::Settings))
        );
    }
}

#[test]
fn volume_keys_work_in_settings_and_match_mouse_actions() {
    let mut app = app();
    app.connected = true;
    app.settings = Some(0);
    key(&mut app, KeyCode::Char('-'));
    assert_eq!(app.config.volume, 95);
    app.key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::SHIFT));
    assert_eq!(app.config.volume, 100);
    app.action(Action::Volume(-5));
    assert_eq!(app.config.volume, 95);
    app.settings = None;
    app.editing = true;
    key(&mut app, KeyCode::Char('-'));
    assert_eq!(app.config.volume, 95);
    assert_eq!(app.search_input, "-");
}

#[test]
fn volume_buttons_accept_clicks_next_to_the_symbols_at_narrow_widths() {
    let mut app = app();
    app.connected = true;
    app.config.volume = 90;
    for (symbol, offset, volume) in [('+', -1, 95), ('+', 1, 100), ('−', 1, 95)] {
        let screen = render(&mut app, 36, 20);
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("− ▰"))
            .unwrap();
        let column = ratatui::text::Line::from(&line[..line.find(symbol).unwrap()]).width();
        app.event(Event::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: (column as u16).saturating_add_signed(offset),
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(app.config.volume, volume);
    }
    std::fs::remove_file(app.config.path_override.as_ref().unwrap()).unwrap();
}

#[test]
fn volume_meter_updates_playback_during_drag_and_saves_on_release() {
    let mut app = app();
    app.connected = true;
    app.config.volume = 100;
    let (player, playback_volume) = crate::player::Player::volume_probe(100);
    app.player = Some(player);
    assert!(render(&mut app, 120, 36).contains("− ▰▰▰▰▰▰▰▰▰▰ 100% +"));
    let bar = app
        .hits
        .iter()
        .find_map(|&(_, action)| match action {
            Action::VolumeAt(bar, _) => Some(bar),
            _ => None,
        })
        .expect("volume meter has clickable cells");
    assert_eq!(bar.width, 10);
    let mouse = |kind, column| {
        Event::Mouse(crossterm::event::MouseEvent {
            kind,
            column,
            row: bar.y,
            modifiers: KeyModifiers::NONE,
        })
    };
    let left = bar.x;
    let right = bar.right() - 1;
    app.event(mouse(MouseEventKind::Down(MouseButton::Left), left));
    assert_eq!(playback_volume(), 0);
    assert_eq!(app.config.volume, 100);
    app.event(mouse(MouseEventKind::Up(MouseButton::Left), left));
    assert_eq!(app.config.volume, 0);

    app.event(mouse(MouseEventKind::Down(MouseButton::Left), left));
    app.event(mouse(MouseEventKind::Drag(MouseButton::Left), right));
    assert_eq!(playback_volume(), 100);
    app.event(mouse(MouseEventKind::Drag(MouseButton::Left), left + 4));
    assert_eq!(playback_volume(), 44);
    assert_eq!(app.config.volume, 0);
    let saved_volume = |app: &App| {
        let path = app.config.path_override.as_ref().unwrap();
        let saved: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        saved.volume
    };
    assert_eq!(saved_volume(&app), 0);
    assert!(render(&mut app, 120, 36).contains("− ▰▰▰▰▰▱▱▱▱▱  44% +"));
    app.event(mouse(MouseEventKind::Up(MouseButton::Left), left + 4));
    assert_eq!(app.config.volume, 44);

    app.event(mouse(MouseEventKind::Down(MouseButton::Left), right));
    assert_eq!(playback_volume(), 100);
    app.event(mouse(MouseEventKind::Drag(MouseButton::Left), 0));
    assert_eq!(playback_volume(), 0);
    key(&mut app, KeyCode::Esc);
    assert_eq!(playback_volume(), 44);
    app.event(mouse(MouseEventKind::Up(MouseButton::Left), 0));
    assert_eq!(app.config.volume, 44);

    assert_eq!(saved_volume(&app), 44);
    std::fs::remove_file(app.config.path_override.as_ref().unwrap()).unwrap();
}

#[test]
fn settings_support_hjkl_and_preserve_text_input() {
    let mut app = app();
    app.connected = true;
    key(&mut app, KeyCode::Char('p'));
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.settings, Some(1));
    key(&mut app, KeyCode::Char('l'));
    assert_eq!(app.config.layout, 1);
    key(&mut app, KeyCode::Char('h'));
    assert_eq!(app.config.layout, 0);
    key(&mut app, KeyCode::Char('k'));
    assert_eq!(app.settings, Some(0));
    key(&mut app, KeyCode::Esc);
    app.editing = true;
    key(&mut app, KeyCode::Char('p'));
    key(&mut app, KeyCode::Char('?'));
    assert_eq!(app.search_input, "p?");
    assert!(app.settings.is_none());
    assert!(!app.help);
}

#[test]
fn library_tabs_have_natural_width_and_remember_category() {
    let mut app = app();
    app.connected = true;
    app.views.iter_mut().for_each(|v| v.loaded = true);
    assert_eq!(
        KINDS,
        ["album", "artist", "genre", "track", "favorite", "playlist"]
    );
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(app.section, 1);
    let saved = std::fs::read_to_string(app.config.path_override.as_ref().unwrap()).unwrap();
    let config: Config = toml::from_str(&saved).unwrap();
    let restored = App::with_config(config, String::new(), false);
    assert_eq!(restored.section, 1);
    let text = render(&mut app, 120, 36);
    assert!(text.contains("歌单"));
    let tabs: Vec<_> = app
        .hits
        .iter()
        .filter(|(_, action)| matches!(action, Action::Section(_)))
        .collect();
    assert_eq!(tabs.len(), 6);
    assert!(tabs[0].0.width < tabs[3].0.width);
    assert_eq!(tabs[1].0.x - tabs[0].0.right(), 2);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.section, 0);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.section, PLAYLISTS);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.section, FAVORITES);
}

#[test]
fn playback_menus_select_options_directly_and_scroll_in_a_small_terminal() {
    let mut app = app();
    app.connected = true;
    let mouse = |kind, rect: Rect| {
        Event::Mouse(crossterm::event::MouseEvent {
            kind,
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })
    };
    let click = |rect| mouse(MouseEventKind::Down(MouseButton::Left), rect);
    let button = |app: &App, menu| {
        app.hits
            .iter()
            .find_map(|&(rect, action)| {
                matches!(action, Action::TogglePlaybackMenu(kind) if kind == menu).then_some(rect)
            })
            .unwrap()
    };
    let option = |app: &App, index| {
        app.hits.iter().find_map(|&(rect, action)| {
            matches!(action, Action::SelectPlaybackOption(i) if i == index).then_some(rect)
        })
    };

    render(&mut app, 100, 30);
    app.event(click(button(&app, PlaybackMenu::Mode)));
    let screen = render(&mut app, 100, 30);
    assert!(screen.contains("✓ 顺序播放"));
    assert!((0..PlaybackMenu::Mode.option_count()).all(|i| option(&app, i).is_some()));
    app.event(click(option(&app, 3).unwrap()));
    assert_eq!(app.config.playback_mode, crate::queue::Mode::Shuffle);
    assert!(app.playback_menu.is_none());

    let compact = render(&mut app, 36, 20);
    assert!(compact.contains("O ▾"));
    assert!(compact.contains("E ▾"));
    let effect_button = button(&app, PlaybackMenu::Effect);
    app.event(click(effect_button));
    let screen = render(&mut app, 36, 20);
    let last_effect = crate::effects::Effect::COUNT - 1;
    assert!(screen.contains("✓ 原声"));
    assert!(option(&app, last_effect).is_none());
    let popup = app
        .hits
        .iter()
        .find_map(|&(rect, action)| matches!(action, Action::PlaybackMenuSurface).then_some(rect))
        .unwrap();
    app.event(click(Rect::new(popup.right() - 1, popup.y + 1, 1, 1)));
    assert!(app.playback_menu.is_some());
    for _ in 0..last_effect {
        app.event(mouse(MouseEventKind::ScrollDown, effect_button));
        render(&mut app, 36, 20);
    }
    app.event(click(option(&app, last_effect).unwrap()));
    assert_eq!(app.config.audio_effect, last_effect);
    assert!(app.playback_menu.is_none());

    render(&mut app, 36, 20);
    app.event(click(button(&app, PlaybackMenu::Effect)));
    let last_label = PlaybackMenu::Effect.label(last_effect, false);
    assert!(render(&mut app, 36, 20).contains(&format!("✓ {last_label}")));
    key(&mut app, KeyCode::Esc);
    assert!(app.playback_menu.is_none());
    render(&mut app, 36, 20);
    app.event(click(button(&app, PlaybackMenu::Mode)));
    render(&mut app, 36, 20);
    app.event(click(Rect::new(0, 0, 1, 1)));
    assert!(app.playback_menu.is_none());
    assert_eq!(app.config.playback_mode, crate::queue::Mode::Shuffle);
}

#[test]
fn effect_shortcut_cycles_all_presets_and_saves_selection() {
    let mut app = app();
    app.connected = true;
    for index in 1..=crate::effects::Effect::COUNT {
        key(&mut app, KeyCode::Char('e'));
        assert_eq!(
            app.config.audio_effect,
            index % crate::effects::Effect::COUNT
        );
    }
}

#[test]
fn shift_e_cycles_effects_backwards() {
    let mut app = app();
    app.connected = true;
    app.config.audio_effect = 0;
    app.key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT));
    assert_eq!(app.config.audio_effect, crate::effects::Effect::COUNT - 1);
    app.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert_eq!(app.config.audio_effect, 0);
}

#[test]
fn ctrl_seek_keys_do_not_replace_plain_paging() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.views[TRACKS].page = 1;
    app.views[TRACKS].total = 100;
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Char('h'),
        KeyCode::Char('l'),
    ] {
        app.key(KeyEvent::new(code, KeyModifiers::CONTROL));
        assert!(commands.try_recv().is_err());
    }
    key(&mut app, KeyCode::Char('l'));
    assert!(matches!(commands.try_recv().unwrap(), Command::Load(_)));
}

#[test]
fn lyric_hit_regions_follow_word_wrapping_and_skip_plain_text() {
    let mut app = app();
    app.connected = true;
    app.config.professional = true;
    app.media.lyrics = vec![
        "a long lyric with words that wraps over several visual rows in the lyric panel".into(),
        "plain lyric without timestamp".into(),
        "last timed lyric".into(),
    ];
    app.media.times = vec![Some(0.0), None, Some(15.0)];
    render(&mut app, 120, 40);
    let timed: Vec<_> = app
        .hits
        .iter()
        .filter_map(|(rect, action)| {
            if let Action::LyricLine(index) = action {
                Some((*rect, *index))
            } else {
                None
            }
        })
        .collect();
    assert!(timed.iter().any(|(_, i)| *i == 0));
    assert!(timed.iter().all(|(_, i)| *i != 1));
    assert!(timed.iter().any(|(_, i)| *i == 2));
    assert!(timed.iter().find(|(_, i)| *i == 0).unwrap().0.height > 1);
}

#[test]
fn playlist_workflow_restores_song_combinations_and_isolates_accounts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let config = Config {
        fn_id: "test-nas".into(),
        username: "listener".into(),
        path_override: Some(path.clone()),
        last_category: "track".into(),
        ..Default::default()
    };
    let mut app = App::with_config(config, String::new(), false);
    app.connected = true;
    app.views[TRACKS] = View {
        items: vec![json!({"guid":"track-1","title":"晴天","artists":[{"name":"周杰伦"}]})],
        total: 1,
        loaded: true,
        table: TableState::default().with_selected(Some(0)),
        ..Default::default()
    };

    key(&mut app, KeyCode::Char('a'));
    assert!(render(&mut app, 100, 32).contains("新建歌单并添加歌曲"));
    key(&mut app, KeyCode::Enter);
    assert!(app.error);
    app.event(Event::Paste("通勤".into()));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.playlists[0].tracks[0]["guid"], "track-1");

    key(&mut app, KeyCode::Char('6'));
    key(&mut app, KeyCode::Char('c'));
    app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.event(Event::Paste("早上".into()));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.view().items[0]["name"], "早上");
    key(&mut app, KeyCode::Char('n'));
    app.event(Event::Paste("晚上".into()));
    key(&mut app, KeyCode::Enter);

    key(&mut app, KeyCode::Char('4'));
    key(&mut app, KeyCode::Char('a'));
    assert!(render(&mut app, 100, 32).contains("选择要加入的歌单"));
    key(&mut app, KeyCode::Enter);
    assert!(app.error);
    assert_eq!(app.playlists[0].tracks.len(), 1);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.playlists[1].tracks[0]["guid"], "track-1");

    key(&mut app, KeyCode::Char('6'));
    drop(app);
    let mut config: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.path_override = Some(path);
    let mut app = App::with_config(config, String::new(), false);
    assert_eq!(app.section, PLAYLISTS);
    let (replies, events) = mpsc::channel();
    app.rx = events;
    replies
        .send(Reply::Connected(
            0,
            Ok(Connected {
                server: "test NAS".into(),
                username: "listener".into(),
                route: relay_route(),
                remembered: true,
                warning: None,
            }),
        ))
        .unwrap();
    app.replies();
    assert_eq!(app.view().total, 2);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.view().items[0]["title"], "晴天");
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    key(&mut app, KeyCode::Char('d'));
    let modal = render(&mut app, 36, 20);
    assert!(modal.contains("从歌单移除歌曲？"));
    assert!(modal.contains("曲库里的原曲会保留。"));
    assert!(modal.contains("晴天"));
    assert_eq!(app.view().total, 1);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.view().total, 1);
    key(&mut app, KeyCode::Char('d'));
    key(&mut app, KeyCode::Char('d'));
    assert_eq!(app.view().total, 1);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.view().total, 0);
    assert_eq!(app.playlists[1].tracks[0]["guid"], "track-1");
    key(&mut app, KeyCode::Esc);

    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Delete);
    let modal = render(&mut app, 36, 20);
    assert!(modal.contains("删除歌单？"));
    assert!(modal.contains("曲库里的原曲会保留。"));
    assert!(modal.contains("晚上"));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.view().total, 2);
    key(&mut app, KeyCode::Delete);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.view().total, 1);
    assert!(commands.try_recv().is_err());

    app.config.username = "other".into();
    app.read_playlists().unwrap();
    assert!(app.playlists.is_empty());
    app.config.username = "listener".into();
    app.read_playlists().unwrap();
    assert_eq!(app.playlists.len(), 1);
    assert_eq!(app.playlists[0].name, "早上");
    assert!(app.playlists[0].tracks.is_empty());
}

#[test]
fn playlist_queue_crosses_pages_and_keeps_its_original_tracks() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.section = PLAYLISTS;
    app.playlists.push(Playlist {
        id: "folder".into(),
        name: "Mix".into(),
        tracks: (0..51)
            .map(|index| json!({"guid":format!("song-{index}"),"title":"Test"}))
            .collect(),
    });
    app.detail = Some(Detail {
        kind: "playlist".into(),
        guid: "folder".into(),
        name: "Mix".into(),
    });
    app.load_playlist_page(1, 49);
    key(&mut app, KeyCode::Enter);
    assert!(
        matches!(commands.try_recv().unwrap(), Command::Play { guid, .. } if guid == "song-49")
    );

    app.advance(true, true);
    assert!(
        matches!(commands.try_recv().unwrap(), Command::Play { guid, .. } if guid == "song-50")
    );
    assert_eq!(app.view().page, 2);
    assert_eq!(app.view().table.selected(), Some(0));
    assert_eq!(app.view().items[0]["guid"], "song-50");
    app.playlists.clear();
    app.detail = None;
    app.section = TRACKS;
    app.config.playback_mode = crate::queue::Mode::Loop;
    app.advance(true, true);
    assert!(matches!(commands.try_recv().unwrap(), Command::Play { guid, .. } if guid == "song-0"));
}

#[test]
fn playlist_edits_merge_with_changes_from_another_instance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let instance = || {
        let config = Config {
            fn_id: "test-nas".into(),
            username: "listener".into(),
            path_override: Some(path.clone()),
            last_category: "playlist".into(),
            ..Default::default()
        };
        let mut app = App::with_config(config, String::new(), false);
        app.connected = true;
        app.read_playlists().unwrap();
        app
    };
    let mut first = instance();
    let mut second = instance();
    key(&mut first, KeyCode::Char('n'));
    first.event(Event::Paste("First".into()));
    key(&mut first, KeyCode::Enter);
    key(&mut second, KeyCode::Char('n'));
    second.event(Event::Paste("Second".into()));
    key(&mut second, KeyCode::Enter);

    assert_eq!(first.playlists.len(), 1);
    assert_eq!(second.playlists.len(), 2);
    assert_eq!(second.playlists[0].name, "First");
    assert_eq!(second.playlists[1].name, "Second");
}

#[test]
fn category_sort_moves_active_tab_and_survives_restart() {
    let mut app = app();
    app.connected = true;
    app.views.iter_mut().for_each(|view| view.loaded = true);
    key(&mut app, KeyCode::Char('6'));
    for code in [KeyCode::Left, KeyCode::Char('h')] {
        app.key(KeyEvent::new(code, KeyModifiers::ALT));
    }
    assert_eq!(app.section, PLAYLISTS);
    assert_eq!(app.category_order, [0, 1, 2, PLAYLISTS, TRACKS, FAVORITES]);
    let path = app.config.path_override.as_ref().unwrap().clone();
    let mut config: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.path_override = Some(path.clone());
    let mut app = App::with_config(config, String::new(), false);
    app.connected = true;
    app.views.iter_mut().for_each(|view| view.loaded = true);
    assert_eq!(app.section, PLAYLISTS);
    render(&mut app, 120, 36);
    let rendered: Vec<_> = app
        .hits
        .iter()
        .filter_map(|(_, action)| match action {
            Action::Section(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(rendered, [0, 1, 2, PLAYLISTS, TRACKS, FAVORITES]);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.section, TRACKS);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.section, PLAYLISTS);
    for code in [KeyCode::Right, KeyCode::Char('l'), KeyCode::Right] {
        app.key(KeyEvent::new(code, KeyModifiers::ALT));
    }
    assert_eq!(app.category_order, [0, 1, 2, TRACKS, FAVORITES, PLAYLISTS]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn remembers_library_page_and_track_after_reconnecting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let config = Config {
        fn_id: "test-nas".into(),
        username: "listener".into(),
        path_override: Some(path.clone()),
        last_category: "track".into(),
        ..Default::default()
    };
    let mut app = App::with_config(config, String::new(), false);
    assert!(app.config.remember_location);
    app.connected = true;
    app.views[TRACKS] = View {
        items: (0..5)
            .map(|i| json!({"guid": format!("song-{i}")}))
            .collect(),
        page: 2,
        loaded: true,
        table: TableState::default().with_selected(Some(2)),
        ..Default::default()
    };
    app.checkpoint_location();
    drop(app);

    let mut config: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.path_override = Some(path.clone());
    let mut app = App::with_config(config, String::new(), false);
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    let (replies, events) = mpsc::channel();
    app.rx = events;
    replies
        .send(Reply::Connected(
            0,
            Ok(Connected {
                server: "test NAS".into(),
                username: "listener".into(),
                route: relay_route(),
                remembered: true,
                warning: None,
            }),
        ))
        .unwrap();
    app.replies();
    let Command::Load(load) = commands.try_recv().unwrap() else {
        panic!("expected page load")
    };
    assert_eq!((load.section, load.page), (TRACKS, 2));
    replies
        .send(Reply::Page(
            load,
            Ok(Page {
                items: vec![json!({"guid": "new"}), json!({"guid": "song-2"})],
                total: 52,
            }),
        ))
        .unwrap();
    app.replies();
    assert_eq!(app.view().page, 2);
    assert_eq!(app.view().table.selected(), Some(1));

    app.action(Action::Settings);
    let settings = render(&mut app, 100, 32);
    assert!(settings.contains("记忆位置"));
    app.action(Action::ChangeSetting(4, 1));
    assert!(!app.config.remember_location);
    let config: Config = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(!config.remember_location);
    app.restore_location();
    let Command::Load(load) = commands.try_recv().unwrap() else {
        panic!("expected default page")
    };
    assert_eq!((load.section, load.page), (0, 1));
    assert!(load.selection.is_none());
}

#[test]
fn remembers_open_playlist_and_selected_track() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let config = Config {
        fn_id: "test-nas".into(),
        username: "listener".into(),
        path_override: Some(path.clone()),
        last_category: "playlist".into(),
        ..Default::default()
    };
    let mut app = App::with_config(config, String::new(), false);
    app.connected = true;
    key(&mut app, KeyCode::Char('n'));
    app.event(Event::Paste("通勤".into()));
    key(&mut app, KeyCode::Enter);
    app.switch(TRACKS);
    app.views[TRACKS] = View {
        items: vec![
            json!({"guid": "first", "title": "第一首"}),
            json!({"guid": "second", "title": "第二首"}),
        ],
        page: 1,
        total: 2,
        loaded: true,
        table: TableState::default().with_selected(Some(0)),
    };
    app.busy = false;
    for row in 0..2 {
        app.action(Action::Row(row));
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Enter);
    }
    app.switch(PLAYLISTS);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.view().table.selected(), Some(1));
    app.checkpoint_location();
    drop(app);

    let mut config: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.path_override = Some(path);
    let mut app = App::with_config(config, String::new(), false);
    app.connected = true;
    app.read_playlists().unwrap();
    app.restore_location();
    assert_eq!(app.detail.as_ref().unwrap().name, "通勤");
    assert_eq!(app.view().table.selected(), Some(1));
    assert_eq!(app.view().items[1]["guid"], "second");

    key(&mut app, KeyCode::Char('d'));
    key(&mut app, KeyCode::Enter);
    app.restore_location();
    assert_eq!(app.view().table.selected(), Some(0));
    assert_eq!(app.view().items[0]["guid"], "first");
}
