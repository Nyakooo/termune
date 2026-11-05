use super::*;
use ratatui::backend::TestBackend;
use serde_json::json;

fn app() -> App {
    let config = Config {
        path_override: Some(std::env::temp_dir().join(format!(
            "termune-test-{}.toml",
            termune::random_id().unwrap()
        ))),
        ..Default::default()
    };
    App::with_config(config, String::new(), false)
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
    app.route = "HTTPS · FN Connect".into();
    app.config.professional = true;
    app.views[0]=View{items:(0..12).map(|i|json!({"guid":format!("fixture-{i}"),"title":(["他的睫毛","晴天","七里香","夜曲","一路向北","稻香"][i%6]),"artists":[{"name":"周杰伦"}],"album":{"name":"叶惠美"},"audioSpec":{"format":"FLAC"}})).collect(),total:440,page:1,loaded:true,table:TableState::default().with_selected(1)};
    app.status = "布局预览 · 测试数据".into();
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
        for layout in 0..5 {
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
fn playback_shortcuts_select_tracks_and_cancel_pending_audio() {
    let mut app = app();
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.connected = true;
    app.views[0].items = vec![
        json!({"guid":"first","title":"First"}),
        json!({"guid":"second","title":"Second"}),
    ];
    app.views[0].table.select(Some(0));
    key(&mut app, KeyCode::Enter);
    match commands.try_recv().unwrap() {
        Command::Play { id, guid, .. } => {
            assert_eq!(id, app.audio_generation);
            assert_eq!(guid, "first");
        }
        _ => panic!("expected play request"),
    }
    key(&mut app, KeyCode::Char('i'));
    assert_eq!(app.info.as_ref().unwrap()["guid"], "first");
    key(&mut app, KeyCode::Enter);
    assert!(app.info.is_none());
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
    assert!(normal.contains("termune"));
    assert!(!normal.contains("never-show-secret"));
    assert!(normal.contains("••••"));
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
    assert!(screen.contains("F1"));
    assert!(screen.contains("下次自动登录"));
    assert!(screen.contains("连接飞牛音乐"));
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
}

#[test]
fn settings_and_help_are_scoped_and_do_not_change_layout_on_open() {
    let mut app = app();
    app.connected = true;
    app.config.professional = false;
    app.config.layout = 2;
    key(&mut app, KeyCode::F(2));
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
    key(&mut app, KeyCode::F(1));
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
    app.views[0] = View {
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
    assert!(render(&mut app, 100, 32).contains("FLAC"));
    key(&mut app, KeyCode::Esc);
    assert!(app.info.is_none());
    app.section = 3;
    app.views[3].loaded = true;
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
    app.views[0].items = vec![json!({"guid":"one","title":"Test song"})];
    app.views[0].table.select(Some(0));
    let markers = ["AIRWAVE", "NIGHT RECEIVER", "OPERATIONS", "TRACKER", "BTOP"];
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
    assert_eq!(app.config.layout, 4);
    app.config.professional = true;
    assert!(render(&mut app, 100, 30).contains("BTOP"));
}

#[test]
fn stale_response_is_discarded_and_failed_search_preserves_library() {
    let mut app = app();
    app.connected = true;
    app.views[0] = View {
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
        section: 0,
        page: 2,
        query: None,
        detail: None,
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
    assert_eq!(app.views[0].items[0]["guid"], "kept");
    assert!(app.busy);
    app.query = Some("搜索".into());
    tx.send(Reply::Page(
        Load {
            id: 10,
            section: 0,
            page: 1,
            query: Some("搜索".into()),
            detail: None,
        },
        Err("请求超时".into()),
    ))
    .unwrap();
    app.replies();
    assert!(app.error);
    assert!(!app.busy);
    assert_eq!(app.views[0].items[0]["guid"], "kept");
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
    app.section = 1;
    app.views[1] = View {
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
            fn_id,
            username,
            ..
        } => {
            assert_eq!(fn_id, "test-id");
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
    app.media.lyrics = vec!["可辨识的测试歌词".into(); 12];
    for layout in 0..5 {
        app.config.layout = layout;
        assert!(render(&mut app, 120, 36).contains("可辨识的测试歌词"));
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
fn global_m_category_shortcuts_and_dragging_have_correct_scope() {
    let mut app = app();
    app.connected = true;
    let initial = app.config.professional;
    key(&mut app, KeyCode::Char('m'));
    assert_eq!(app.config.professional, !initial);
    app.info = Some(json!({"title":"test"}));
    key(&mut app, KeyCode::Char('M'));
    assert_eq!(app.config.professional, initial);
    app.info = None;
    app.editing = true;
    key(&mut app, KeyCode::Char('m'));
    assert_eq!(app.search_input, "m");
    assert_eq!(app.config.professional, initial);
    app.editing = false;
    for (index, character) in ('1'..='6').enumerate() {
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
    app.tx = tx;
    app.views[0] = View {
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
    app.section = 2;
    app.advance(true, true);
    match commands.recv().unwrap() {
        Command::Advance { context, .. } => {
            assert_eq!(context.section, 0);
            assert_eq!(context.index, 50);
        }
        _ => panic!("expected cross-page queue request"),
    }
    app.playing_context = Some(PlaybackContext {
        section: 0,
        query: None,
        detail: None,
        index: 49,
        total: 51,
    });
    app.playing_item = Some(json!({"guid":"original","title":"song"}));
    app.config.playback_mode = crate::queue::Mode::RepeatOne;
    app.advance(true, true);
    assert!(matches!(commands.recv().unwrap(),Command::Play{guid,..}if guid=="original"));
}

#[test]
fn favorites_require_unmodified_key_and_update_only_after_server_confirmation() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    let (events, rx) = mpsc::channel();
    app.rx = rx;
    app.views[0].items = vec![json!({"guid":"song","title":"Test","isFavorite":false})];
    app.views[0].table.select(Some(0));
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
    assert_eq!(app.views[0].items[0]["isFavorite"], false);
    events
        .send(Reply::Favorite(
            app.media_epoch,
            "song".into(),
            true,
            Ok(()),
        ))
        .unwrap();
    app.replies();
    assert_eq!(app.views[0].items[0]["isFavorite"], true);
}

#[test]
fn shuffle_previous_uses_actual_history() {
    let mut app = app();
    app.connected = true;
    let (tx, commands) = mpsc::channel();
    app.tx = tx;
    app.config.playback_mode = crate::queue::Mode::Shuffle;
    app.playing_context = Some(PlaybackContext {
        section: 0,
        query: None,
        detail: None,
        index: 3,
        total: 6,
    });
    app.shuffle_history.push(PlaybackContext {
        section: 0,
        query: None,
        detail: None,
        index: 1,
        total: 6,
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
                route: "HTTPS".into(),
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
