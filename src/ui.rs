use crate::app::{
    Action, App, DISCONNECT_SETTING, FAVORITES, PLAYLISTS, PlaylistDialog, artists, bar_fraction,
    clean, name,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
    },
};

const THEMES: [&str; 5] = ["Mint", "Amber", "Violet", "Ocean", "Mono"];
const LAYOUTS: [&str; 4] = ["Airwave", "Operations", "Tracker", "Btop"];
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    bg: Color,
    fg: Color,
    dim: Color,
    accent: Color,
    secondary: Color,
    edge: Color,
    selected: Color,
    panel: Color,
}
fn palette(theme: usize) -> Palette {
    let values = match theme % 5 {
        0 => [
            (17, 24, 22),
            (215, 231, 221),
            (137, 163, 151),
            (166, 216, 145),
            (100, 207, 199),
            (57, 79, 67),
            (37, 61, 45),
            (22, 33, 28),
        ],
        1 => [
            (30, 24, 17),
            (239, 223, 196),
            (177, 151, 116),
            (237, 185, 97),
            (205, 153, 108),
            (89, 65, 39),
            (73, 51, 25),
            (39, 30, 20),
        ],
        2 => [
            (23, 21, 34),
            (228, 221, 245),
            (156, 144, 182),
            (195, 165, 246),
            (120, 190, 220),
            (67, 54, 91),
            (55, 40, 82),
            (30, 26, 45),
        ],
        3 => [
            (16, 23, 33),
            (211, 232, 243),
            (128, 158, 181),
            (99, 215, 223),
            (185, 156, 243),
            (43, 70, 94),
            (23, 57, 77),
            (20, 32, 46),
        ],
        _ => [
            (14, 14, 14),
            (224, 224, 224),
            (145, 145, 145),
            (245, 245, 245),
            (182, 182, 182),
            (65, 65, 65),
            (51, 51, 51),
            (23, 23, 23),
        ],
    };
    let [bg, fg, dim, accent, secondary, edge, selected, panel] =
        values.map(|(r, g, b)| Color::Rgb(r, g, b));
    Palette {
        bg,
        fg,
        dim,
        accent,
        secondary,
        edge,
        selected,
        panel,
    }
}
fn pane(title: String, p: Palette, active: bool) -> Block<'static> {
    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { p.accent } else { p.edge }))
        .style(Style::default().fg(p.fg).bg(p.bg))
}
fn text(f: &mut Frame, r: Rect, s: impl Into<String>, color: Color) {
    f.render_widget(
        Paragraph::new(s.into()).style(Style::default().fg(color)),
        r,
    );
}
fn center(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
fn inset(r: Rect, x: u16, y: u16) -> Rect {
    Rect::new(
        r.x + x.min(r.width),
        r.y + y.min(r.height),
        r.width.saturating_sub(x * 2),
        r.height.saturating_sub(y * 2),
    )
}
fn line(r: Rect, y: u16) -> Rect {
    Rect::new(r.x, r.y + y, r.width, if y < r.height { 1 } else { 0 })
}
fn label(a: &App, i: usize) -> &str {
    match i {
        0 => a.text("专辑", "Albums"),
        1 => a.text("歌手", "Artists"),
        2 => a.text("风格", "Genres"),
        3 => a.text("全部歌曲", "All songs"),
        FAVORITES => a.text("收藏", "Favorites"),
        _ => a.text("歌单", "Playlists"),
    }
}
fn button(f: &mut Frame, a: &mut App, r: Rect, label: &str, action: Action, p: Palette) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    text(f, r, label, p.accent);
    a.hits.push((r, action));
}
fn buttons(f: &mut Frame, a: &mut App, r: Rect, items: Vec<(String, Action)>, p: Palette) {
    let mut x = r.x;
    for (label, action) in items {
        let w = Line::from(label.as_str()).width() as u16;
        if x + w > r.right() {
            break;
        }
        button(
            f,
            a,
            Rect::new(x, r.y, w, r.height.min(1)),
            &label,
            action,
            p,
        );
        x += w + 2;
    }
}
fn status(a: &App, p: Palette) -> (String, Color) {
    let error_color = Color::Rgb(235, 126, 119);
    if a.busy {
        let stage = match a.stage {
            "restore" => a.text("验证已保存的登录状态…", "Checking saved session…"),
            "discovery" => a.text("查询 FN ID…", "Resolving FN ID…"),
            "probing" => a.text("选择连接线路…", "Selecting connection route…"),
            "initialization" => a.text("检查音乐服务…", "Checking music service…"),
            "login" => a.text("登录并验证身份…", "Signing in…"),
            _ => a.text("加载曲库…", "Loading library…"),
        };
        return (
            format!(
                "{} {stage}",
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][a.tick % 10]
            ),
            p.dim,
        );
    }
    let (status, notice, seek_error) = a.footer_messages();
    if let Some(error) = status.filter(|_| a.error).or(seek_error) {
        (clean(error), error_color)
    } else if let Some(notice) = notice {
        (clean(notice), p.accent)
    } else {
        (clean(status.unwrap_or_default()), p.dim)
    }
}
pub fn draw(f: &mut Frame, a: &mut App) {
    let p = palette(a.config.theme);
    let area = f.area();
    a.hits.clear();
    a.seek_rect = None;
    f.render_widget(
        Block::default().style(Style::default().fg(p.fg).bg(p.bg)),
        area,
    );
    if area.width < 36 || area.height < 20 {
        f.render_widget(
            Paragraph::new(a.text(
                "终端过小，请调整到至少 36 × 20\nCtrl+C 退出",
                "Resize to at least 36 × 20\nCtrl+C to quit",
            ))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(p.accent)),
            area,
        );
        return;
    }
    let body = inset(area, 1, 0);
    let parts = Layout::vertical([
        Constraint::Length(if a.connected { 3 } else { 2 }),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(body);
    if a.connected {
        header(f, a, parts[0], p);
    } else {
        login_header(f, a, parts[0], p);
    }
    let (message, message_color) = status(a, p);
    if a.connected {
        library_layout(f, a, parts[1], p);
    } else if a.restoring {
        let panel = center(parts[1], 56, 9);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(a.text("正在恢复登录状态", "Restoring your session")),
                Line::from(""),
                Line::from(a.text("验证成功后直接进入曲库", "Your library opens once verified")),
                Line::from(""),
                Line::from(message.clone()),
            ])
            .alignment(Alignment::Center)
            .style(Style::default().fg(p.accent)),
            panel,
        );
    } else {
        login(f, a, parts[1], p);
    }
    let footer = parts[2];
    text(f, line(footer, 0), message, message_color);
    let help = if a.connected {
        if area.width >= 105 {
            a.text("↑↓ 选择  Tab 分类  ←→/hl 翻页  Enter 播放/打开  / 搜索  M 极简  O 模式  ? 帮助  Q 退出","↑↓ Select  Tab Library  ←→/hl Page  Enter Play/Open  / Search  M Minimal  O Mode  ? Help  Q Quit")
        } else if area.width >= 65 {
            a.text(
                "↑↓ 选择  Tab 分类  Enter 播放/打开  / 搜索  M 极简  ? 帮助",
                "↑↓ Select  Tab Library  Enter Play/Open  / Search  M Minimal  ? Help",
            )
        } else {
            a.text(
                "↑↓选择 Tab分类 ↵打开 /搜索 ?帮助",
                "↑↓ Select Tab Library ↵ Open / Find ? Help",
            )
        }
    } else if a.restoring {
        a.text(
            "Esc 取消恢复  Ctrl+C 退出",
            "Esc Cancel restore  Ctrl+C Quit",
        )
    } else {
        a.text("Tab 切换焦点   Esc 退出", "Tab Focus   Esc Quit")
    };
    text(f, line(footer, 1), help, p.dim);
    if a.editing || a.settings.is_some() || a.help {
        a.hits.clear();
        f.render_widget(Block::default().style(Style::default().bg(p.bg)), footer);
    }
    if a.editing {
        search(f, a, area, p);
    }
    if a.settings.is_some() {
        settings(f, a, area, p);
    }
    if a.help {
        help_modal(f, a, area, p);
    }
    if a.playlist_dialog.is_some() {
        a.hits.clear();
        playlist_dialog(f, a, area, p);
    }
}
fn header(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let cols = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(12),
        Constraint::Length(4),
    ])
    .split(line(r, 0));
    text(f, cols[0], "◉ termune", p.accent);
    button(
        f,
        a,
        cols[1],
        a.text("[P 设置]", "[P Settings]"),
        Action::Settings,
        p,
    );
    text(
        f,
        cols[2],
        if a.connected {
            "●"
        } else if a.busy {
            "◌"
        } else {
            "○"
        },
        if a.connected { p.secondary } else { p.dim },
    );
    if a.connected {
        let meta = if r.width >= 85 {
            format!(
                "{} · {}   /   {} · {}",
                a.server,
                a.username,
                if a.config.professional {
                    LAYOUTS[a.config.layout % 4]
                } else {
                    a.text("极简", "Minimal")
                },
                THEMES[a.config.theme % 5]
            )
        } else {
            format!("{} · {}", a.server, a.username)
        };
        text(f, line(r, 1), meta, p.dim);
    }
}
fn login_header(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let help = a.text("[F1 帮助]", "[F1 Help]");
    let settings = a.text("[设置]", "[Settings]");
    let controls = Line::from(help).width() + Line::from(settings).width() + 2;
    let cols = Layout::horizontal([Constraint::Min(10), Constraint::Length(controls as u16)])
        .split(line(r, 0));
    let mut brand = Line::from(vec![
        Span::styled(">_  ", Style::default().fg(p.accent)),
        Span::styled(
            "TERMUNE",
            Style::default().fg(p.fg).add_modifier(Modifier::BOLD),
        ),
    ]);
    if r.width >= 55 {
        brand.spans.push(Span::styled(
            a.text("   飞牛音乐终端", "   Feiniu Music Terminal"),
            Style::default().fg(p.dim),
        ));
    }
    f.render_widget(Paragraph::new(brand), cols[0]);
    if !a.busy {
        buttons(
            f,
            a,
            cols[1],
            vec![
                (settings.into(), Action::Settings),
                (help.into(), Action::Help),
            ],
            p,
        );
    }
    f.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(p.edge)),
        line(r, 1),
    );
}

fn login(frame: &mut Frame, app: &mut App, area: Rect, p: Palette) {
    let spacious = area.height >= 20;
    let gap = u16::from(spacious);
    let form = center(area, 56, if spacious { 20 } else { 15 });
    let card = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.edge))
        .style(Style::default().fg(p.fg).bg(p.panel));
    let inner = inset(card.inner(form), if form.width >= 46 { 3 } else { 1 }, 0);
    frame.render_widget(card, form);
    let rows = Layout::vertical([
        Constraint::Length(gap),
        Constraint::Length(2),
        Constraint::Length(gap),
        Constraint::Length(6 + gap * 2),
        Constraint::Length(gap),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(app.text("登录飞牛音乐", "Sign in to Feiniu Music"))
            .style(Style::default().fg(p.fg).add_modifier(Modifier::BOLD)),
        line(rows[1], 0),
    );
    text(
        frame,
        line(rows[1], 1),
        app.text(
            "使用音乐独立账号连接曲库",
            "Sign in with your Music account",
        ),
        p.dim,
    );
    let labels = [
        "FN ID",
        app.text("音乐用户名", "Music username"),
        app.text("音乐密码", "Music password"),
    ];
    for (i, label) in labels.into_iter().enumerate() {
        let row = i as u16 * (2 + gap);
        let label_area = line(rows[3], row);
        let field = line(rows[3], row + 1);
        let active = app.focus == i && !app.busy;
        text(
            frame,
            label_area,
            label,
            if active { p.accent } else { p.fg },
        );
        let value = if i == 2 {
            "•".repeat(app.fields[i].chars().count().min(128))
        } else {
            clean(&app.fields[i])
        };
        let value = if active { format!("{value}▏") } else { value };
        let mut visible = String::new();
        let mut remaining = usize::from(field.width.saturating_sub(2));
        for c in value.chars().rev() {
            let width = Line::from(c.to_string()).width();
            if width > remaining {
                break;
            }
            remaining -= width;
            visible.insert(0, c);
        }
        let field_style = Style::default().bg(if active { p.selected } else { p.bg });
        frame.render_widget(
            Paragraph::new(format!(" {visible}")).style(field_style),
            field,
        );
        if !app.busy {
            app.hits.push((
                Rect::new(field.x, label_area.y, field.width, 2),
                Action::Field(i),
            ));
        }
    }
    let checkbox_label = app.text("下次自动登录", "Sign in automatically");
    let checkbox = format!(
        "[{}] {checkbox_label}",
        if app.remember { "x" } else { " " }
    );
    let checkbox_area = line(rows[5], 0);
    let width = (Line::from(checkbox.as_str()).width() as u16).min(checkbox_area.width);
    let checkbox_area = Rect {
        width,
        ..checkbox_area
    };
    frame.render_widget(
        Paragraph::new(checkbox).style(if app.focus == 3 && !app.busy {
            Style::default().fg(p.accent).bg(p.selected)
        } else {
            Style::default().fg(p.fg)
        }),
        checkbox_area,
    );
    if !app.busy {
        app.hits.push((checkbox_area, Action::ToggleRemember));
    }
    let hint = if app.allow_http {
        app.text("HTTP 未加密，凭据可能泄露", "HTTP may expose credentials")
    } else {
        app.text("仅在系统凭据库保存 Token", "Token stored in system vault")
    };
    text(
        frame,
        line(rows[5], 1),
        format!("    {hint}"),
        if app.allow_http { Color::Yellow } else { p.dim },
    );

    let button = Rect {
        width: 14,
        ..rows[7]
    };
    let (label, style) = if app.busy {
        (
            app.text("正在连接…", "Connecting…"),
            Style::default().fg(p.dim).bg(p.selected),
        )
    } else {
        let style = if app.focus == 4 {
            Style::default().fg(p.bg).bg(p.accent)
        } else {
            Style::default().fg(p.accent).bg(p.selected)
        };
        (
            app.text("登录", "Sign in"),
            style.add_modifier(Modifier::BOLD),
        )
    };
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style),
        button,
    );
    if !app.busy {
        app.hits.push((button, Action::Connect));
        let hint = Rect {
            x: button.right() + 2,
            width: rows[7].width.saturating_sub(button.width + 2),
            ..button
        };
        text(frame, hint, "Enter", p.dim);
    }
}

fn library_layout(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    if a.config.professional && (r.width < 72 || r.height < 18) {
        let rows = Layout::vertical([Constraint::Length(8), Constraint::Min(3)])
            .spacing(1)
            .split(r);
        player_panel(f, a, rows[0], p, false, false, None);
        library(f, a, rows[1], p, false, false);
        return;
    }
    if !a.config.professional {
        let rows = Layout::vertical([Constraint::Min(5), Constraint::Length(5)])
            .spacing(1)
            .split(r);
        library(f, a, rows[0], p, false, false);
        player_panel(f, a, rows[1], p, false, false, None);
        return;
    }
    match a.config.layout % 4 {
        0 => {
            let cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .spacing(3)
                .split(inset(r, 1, 1));
            player_panel(f, a, cols[0], p, false, true, Some("AIRWAVE"));
            library(f, a, cols[1], p, false, false);
        }
        1 => {
            let rows = Layout::vertical([Constraint::Min(5), Constraint::Length(13)])
                .spacing(1)
                .split(r);
            let cols = Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
                .spacing(1)
                .split(rows[0]);
            library(f, a, cols[0], p, true, true);
            media_sidebar(f, a, cols[1], p);
            player_panel(f, a, rows[1], p, true, false, Some("OPERATIONS"));
        }
        2 => {
            let rows = Layout::vertical([Constraint::Length(13), Constraint::Min(5)])
                .spacing(1)
                .split(r);
            player_panel(f, a, rows[0], p, false, false, Some("TRACKER"));
            library(f, a, rows[1], p, false, true);
        }
        _ => {
            let rows = Layout::vertical([Constraint::Length(14), Constraint::Min(5)])
                .spacing(1)
                .split(r);
            player_panel(f, a, rows[0], p, true, false, Some("BTOP"));
            let cols = Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)])
                .spacing(1)
                .split(rows[1]);
            library(f, a, cols[0], p, true, true);
            inspector(f, a, cols[1], p, true);
        }
    }
}
fn selected(a: &App) -> Option<&serde_json::Value> {
    a.view()
        .table
        .selected()
        .and_then(|i| a.view().items.get(i))
}
fn player_panel(
    f: &mut Frame,
    a: &mut App,
    r: Rect,
    p: Palette,
    boxed: bool,
    spacious: bool,
    layout: Option<&str>,
) {
    let title = layout.unwrap_or(a.text("播放", "Player"));
    let block = if boxed {
        pane(title.into(), p, false).border_style(Style::default().fg(p.secondary))
    } else {
        Block::default()
            .title(title.to_owned())
            .title_style(Style::default().fg(p.secondary))
    };
    let inner = block.inner(r);
    f.render_widget(block, r);
    let inner = if boxed { inset(inner, 1, 0) } else { inner };
    let (state, title) = if let Some(title) = &a.audio_loading {
        (a.text("◌ 正在连接", "◌ CONNECTING"), title.clone())
    } else if let Some(player) = &a.player {
        (
            if player.paused {
                a.text("Ⅱ 已暂停", "Ⅱ PAUSED")
            } else {
                a.text("▶ 正在播放", "▶ PLAYING")
            },
            clean(&player.title),
        )
    } else {
        (
            a.text("■ 待播放", "■ READY"),
            selected(a)
                .map(|v| name(v, if a.track_view() { "title" } else { "name" }))
                .unwrap_or_else(|| {
                    a.text("选择音乐，开始收听", "Select music to start listening")
                        .into()
                }),
        )
    };
    if a.config.professional && inner.height >= 6 {
        if spacious {
            let rows = Layout::vertical([
                Constraint::Length(4),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .spacing(1)
            .split(inner);
            text(f, line(rows[0], 0), state, p.secondary);
            text(f, line(rows[0], 1), title, p.accent);
            text(f, line(rows[0], 3), a.media.artist.clone(), p.dim);
            lyrics(f, a, rows[1], p, false);
            playback_controls(f, a, rows[2], p);
        } else {
            let cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .spacing(2)
                .split(inner);
            text(f, line(cols[0], 0), state, p.secondary);
            text(f, line(cols[0], 1), title, p.accent);
            text(f, line(cols[0], 3), a.media.artist.clone(), p.dim);
            playback_controls(
                f,
                a,
                Rect::new(
                    cols[0].x,
                    cols[0].bottom().saturating_sub(3),
                    cols[0].width,
                    3,
                ),
                p,
            );
            lyrics(f, a, cols[1], p, false);
        }
    } else {
        text(f, line(inner, 0), format!("{state} · {title}"), p.accent);
        playback_controls(
            f,
            a,
            Rect::new(
                inner.x,
                inner.y + 1,
                inner.width,
                inner.height.saturating_sub(1),
            ),
            p,
        );
    }
}

fn lyrics(f: &mut Frame, a: &mut App, r: Rect, p: Palette, boxed: bool) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    let title = if a.lyrics_follow && a.active_lyric().is_some() {
        a.text("歌词 · 同步", "Lyrics · synced")
    } else {
        a.text("歌词", "Lyrics")
    }
    .to_owned();
    let block = if boxed {
        pane(title, p, false)
    } else {
        Block::default()
            .title(title)
            .title_style(Style::default().fg(p.secondary))
    };
    let inner = block.inner(r);
    f.render_widget(block, r);
    if a.media.lyrics.is_empty() {
        let message = if a.media.loading {
            a.text("加载歌词…", "Loading lyrics…")
        } else if a.media.lyric_error {
            a.text("歌词暂不可用", "Lyrics unavailable")
        } else {
            a.text("暂无歌词", "No lyrics")
        };
        text(f, line(inner, 0), message, p.dim);
        return;
    }
    let active = a.active_lyric();
    if a.lyrics_follow
        && let Some(index) = active
    {
        a.lyric_scroll = a
            .media
            .lyrics
            .iter()
            .take(index)
            .map(|s| {
                Paragraph::new(s.as_str())
                    .wrap(Wrap { trim: false })
                    .line_count(inner.width.max(1))
                    .max(1)
            })
            .sum::<usize>()
            .saturating_sub(inner.height as usize / 2);
    }
    let count: usize = a
        .media
        .lyrics
        .iter()
        .map(|s| {
            Paragraph::new(s.as_str())
                .wrap(Wrap { trim: false })
                .line_count(inner.width.max(1))
                .max(1)
        })
        .sum();
    a.lyric_scroll = a
        .lyric_scroll
        .min(count.saturating_sub(inner.height as usize));
    let lines: Vec<_> = a
        .media
        .lyrics
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Line::from(Span::styled(
                s.clone(),
                if active == Some(i) {
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.dim)
                },
            ))
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((a.lyric_scroll.min(u16::MAX as usize) as u16, 0))
            .style(Style::default().fg(p.fg)),
        inner,
    );
    a.hits.push((inner, Action::LyricsScroll(0)));
    let mut row = 0usize;
    for (index, lyric) in a.media.lyrics.iter().enumerate() {
        let height = Paragraph::new(lyric.as_str())
            .wrap(Wrap { trim: false })
            .line_count(inner.width.max(1))
            .max(1);
        let start = row.max(a.lyric_scroll);
        let end = (row + height).min(a.lyric_scroll + inner.height as usize);
        if start < end && a.media.times.get(index).copied().flatten().is_some() {
            a.hits.push((
                Rect::new(
                    inner.x,
                    inner.y + (start - a.lyric_scroll) as u16,
                    inner.width,
                    (end - start) as u16,
                ),
                Action::LyricLine(index),
            ));
        }
        row += height;
    }
}
fn media_sidebar(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    lyrics(f, a, r, p, true);
}
fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".into();
    }
    format!("{}:{:02}", seconds as u64 / 60, seconds as u64 % 60)
}
fn playback_controls(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let toggle = if a.player.as_ref().is_some_and(|v| !v.paused) {
        "[ Ⅱ ]"
    } else {
        "[ ▶ ]"
    };
    buttons(
        f,
        a,
        line(r, 0),
        vec![
            ("[ ‹‹ ]".into(), Action::PreviousTrack),
            (toggle.into(), Action::Pause),
            ("[ ■ ]".into(), Action::Stop),
            ("[ ›› ]".into(), Action::NextTrack),
        ],
        p,
    );
    if r.height >= 2 {
        let timeline = a.player.as_ref().map(|p| p.timeline).unwrap_or_default();
        let cols = Layout::horizontal([
            Constraint::Length(6),
            Constraint::Min(1),
            Constraint::Length(7),
        ])
        .split(line(r, 1));
        let fraction = a
            .seek_drag
            .unwrap_or_else(|| {
                if timeline.duration > 0.0 {
                    timeline.position / timeline.duration
                } else {
                    0.0
                }
            })
            .clamp(0.0, 1.0);
        text(
            f,
            cols[0],
            if timeline.duration > 0.0 {
                clock(fraction * timeline.duration)
            } else {
                "--:--".into()
            },
            p.dim,
        );
        text(
            f,
            cols[2],
            if timeline.duration > 0.0 {
                format!(" {}", clock(timeline.duration))
            } else {
                " --:--".into()
            },
            p.dim,
        );
        let width = cols[1].width;
        let spans: Vec<_> = (0..width)
            .map(|x| {
                Span::styled(
                    if (x as f64) < fraction * width as f64 {
                        "━"
                    } else {
                        "─"
                    },
                    Style::default().fg(if (x as f64) < fraction * width as f64 {
                        p.accent
                    } else {
                        p.edge
                    }),
                )
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), cols[1]);
        if timeline.duration > 0.0 && width > 1 {
            a.seek_rect = Some(cols[1]);
            for x in 0..width {
                a.hits.push((
                    Rect::new(cols[1].x + x, cols[1].y, 1, 1),
                    Action::Seek(bar_fraction(cols[1], cols[1].x + x)),
                ));
            }
        }
    }
    if r.height >= 3 {
        let label = format!("[ {} ]", a.config.playback_mode.label(a.config.english));
        let controls = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(35),
            Constraint::Percentage(35),
        ])
        .split(line(r, 2));
        button(f, a, controls[0], &label, Action::CycleMode, p);
        let effect =
            crate::effects::Effect::from_index(a.config.audio_effect).label(a.config.english);
        let label = format!("[ E {} ]", effect);
        button(f, a, controls[1], &label, Action::CycleEffect, p);
        let volume = a
            .volume_drag
            .as_ref()
            .and_then(|drag| drag.preview)
            .unwrap_or(a.config.volume.min(100));
        let filled = usize::from(volume).div_ceil(20);
        let label = format!(
            "− {}{} {}% +",
            "▰".repeat(filled),
            "▱".repeat(5 - filled),
            volume
        );
        let halves = Layout::horizontal([Constraint::Percentage(50); 2]).split(controls[2]);
        text(f, controls[2], label, p.accent);
        a.hits.push((halves[0], Action::Volume(-5)));
        a.hits.push((halves[1], Action::Volume(5)));

        let bar = controls[2].intersection(Rect::new(
            controls[2].x.saturating_add(2),
            controls[2].y,
            5,
            1,
        ));
        if bar.width > 1 {
            for column in bar.x..bar.right() {
                let volume = (bar_fraction(bar, column) * 100.0).round() as u16;
                a.hits.push((
                    Rect::new(column, bar.y, 1, 1),
                    Action::VolumeAt(volume, column),
                ));
            }
        }
    }
}
fn tabs(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let order = a.category_order;
    let widths = order.map(|id| Line::from(label(a, id)).width() as u16 + 4);
    let selected = a.category_position();
    let mut start = 0;
    while start < selected
        && widths[start..=selected].iter().sum::<u16>() + (selected - start) as u16 * 2 > r.width
    {
        start += 1;
    }
    let mut x = r.x;
    for (&i, width) in order.iter().zip(widths).skip(start) {
        if x >= r.right() {
            break;
        }
        let width = width.min(r.right() - x);
        let cell = Rect::new(x, r.y, width, r.height.min(1));
        let text = format!(
            " {} {} ",
            if a.section == i { "›" } else { " " },
            label(a, i)
        );
        let style = if a.section == i {
            Style::default()
                .fg(p.accent)
                .bg(p.selected)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.dim)
        };
        f.render_widget(Paragraph::new(text).style(style), cell);
        a.hits.push((cell, Action::Section(i)));
        x = x.saturating_add(width + 2);
    }
}
fn title(a: &App) -> String {
    if let Some(detail) = &a.detail {
        format!("{} / {}", label(a, a.section), detail.name)
    } else if let Some(query) = &a.query {
        format!("{} / {}", a.text("搜索", "Search"), clean(query))
    } else {
        format!("{} · {}", label(a, a.section), a.view().total)
    }
}
fn pagination(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let cols = Layout::horizontal([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(r);
    let view = a.view();
    let page = view.page.max(1);
    let pages = view.total.div_ceil(50).max(1);
    let text = if a.config.english {
        format!("{page} / {pages} · {} total", view.total)
    } else {
        format!("第 {page} / {pages} 页 · 共 {} 条", view.total)
    };
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(p.dim)),
        cols[1],
    );
    if page > 1 {
        button(f, a, cols[0], " ‹ ", Action::Previous, p);
    }
    if page < pages {
        button(f, a, cols[2], " › ", Action::Next, p);
    }
}
fn empty(f: &mut Frame, a: &App, r: Rect, p: Palette) {
    let message = if a.busy {
        a.text("正在加载…", "Loading…")
    } else if a.error {
        a.text("加载失败", "Unable to load")
    } else if a.section == PLAYLISTS {
        if a.detail.is_some() {
            a.text(
                "从曲库选歌，按 A 加入",
                "Select a song in the library and press A",
            )
        } else {
            a.text("按 N 新建歌单", "Press N to create a playlist")
        }
    } else {
        a.text("这里还没有音乐数据", "No music here yet")
    };
    f.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(p.dim)),
        center(r, r.width, 1),
    );
}
fn library(f: &mut Frame, a: &mut App, r: Rect, p: Palette, boxed: bool, detailed: bool) {
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(r);
    tabs(f, a, parts[0], p);
    let add = (
        a.text("[A 加入歌单]", "[A Add to playlist]").into(),
        Action::PlaylistAdd,
    );
    let actions = if a.section == PLAYLISTS && a.detail.is_none() {
        let mut actions = vec![(a.text("[N 新建]", "[N New]").into(), Action::PlaylistNew)];
        if selected(a).is_some() {
            actions.extend([
                (
                    a.text("[C 改名]", "[C Rename]").into(),
                    Action::PlaylistRename,
                ),
                (
                    a.text("[D 删除]", "[D Delete]").into(),
                    Action::PlaylistDelete,
                ),
            ]);
        }
        actions
    } else if a.section == PLAYLISTS {
        let mut actions = vec![(a.text("[Esc 返回]", "[Esc Back]").into(), Action::Back)];
        if selected(a).is_some() {
            actions.extend([
                (
                    a.text("[D 移除]", "[D Remove]").into(),
                    Action::PlaylistDelete,
                ),
                (a.text("[A 加入]", "[A Add]").into(), Action::PlaylistAdd),
            ]);
        }
        actions
    } else if a.track_view() && selected(a).is_some() {
        vec![add]
    } else {
        Vec::new()
    };
    buttons(f, a, parts[1], actions, p);
    let block = if boxed {
        pane(title(a), p, true)
    } else {
        Block::default()
            .title(title(a))
            .title_style(Style::default().fg(p.secondary))
    };
    let inner = block.inner(parts[2]);
    let track = a.track_view();
    let widths = if track && detailed && r.width >= 56 {
        vec![
            Constraint::Length(4),
            Constraint::Percentage(42),
            Constraint::Percentage(28),
            Constraint::Min(8),
        ]
    } else {
        vec![
            Constraint::Length(4),
            Constraint::Percentage(64),
            Constraint::Min(8),
        ]
    };
    let mut headers = vec![
        "#".to_owned(),
        a.text(
            if track { "歌曲" } else { "名称" },
            if track { "Title" } else { "Name" },
        )
        .to_owned(),
        a.text(
            if track { "歌手" } else { "歌曲数" },
            if track { "Artist" } else { "Tracks" },
        )
        .to_owned(),
    ];
    if widths.len() == 4 {
        headers.push(a.text("专辑", "Album").into());
    }
    let base = a.view().page.max(1).saturating_sub(1) * 50;
    let rows: Vec<_> = a
        .view()
        .items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut cells = vec![
                Cell::from(format!("{:02}", base + i as u64 + 1)).style(Style::default().fg(p.dim)),
                Cell::from(
                    if track && v["isFavorite"].as_bool().unwrap_or(a.section == FAVORITES) {
                        format!("★ {}", name(v, "title"))
                    } else {
                        name(v, if track { "title" } else { "name" })
                    },
                ),
            ];
            cells.push(
                Cell::from(if track {
                    artists(v)
                } else {
                    v["trackCount"]
                        .as_u64()
                        .map(|v| v.to_string())
                        .or_else(|| v["trackCount"].as_str().map(clean))
                        .unwrap_or_else(|| "—".into())
                })
                .style(Style::default().fg(p.dim)),
            );
            if widths.len() == 4 {
                cells.push(Cell::from(name(&v["album"], "name")).style(Style::default().fg(p.dim)));
            }
            Row::new(cells)
        })
        .collect();
    let table = Table::new(rows, widths)
        .header(
            Row::new(headers)
                .style(Style::default().fg(p.dim))
                .bottom_margin(1),
        )
        .column_spacing(1)
        .block(block)
        .highlight_symbol("› ")
        .row_highlight_style(
            Style::default()
                .bg(p.selected)
                .fg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(table, parts[2], &mut a.view_mut().table);
    if a.view().items.is_empty() {
        empty(f, a, inner, p);
    }
    let offset = a.view().table.offset();
    for row in 0..inner.height.saturating_sub(2) as usize {
        if offset + row >= a.view().items.len() {
            break;
        }
        a.hits.push((
            Rect::new(inner.x, inner.y + 2 + row as u16, inner.width, 1),
            Action::Row(offset + row),
        ));
    }
    pagination(f, a, parts[3], p);
}
fn inspector(f: &mut Frame, a: &App, r: Rect, p: Palette, overview: bool) {
    let block = pane(
        a.text(
            if overview {
                "曲库概览"
            } else {
                "选中信息"
            },
            if overview {
                "Library overview"
            } else {
                "Selection"
            },
        )
        .into(),
        p,
        false,
    );
    let inner = inset(block.inner(r), 1, 0);
    f.render_widget(block, r);
    let mut lines = vec![];
    if overview {
        for i in a.category_order {
            lines.push(Line::from(vec![
                Span::styled(format!("{:<8}", label(a, i)), Style::default().fg(p.dim)),
                Span::styled(
                    if a.views[i].loaded {
                        a.views[i].total.to_string()
                    } else {
                        "—".into()
                    },
                    Style::default().fg(p.accent),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }
    if let Some(v) = selected(a) {
        lines.push(Line::from(Span::styled(
            name(v, if a.track_view() { "title" } else { "name" }),
            Style::default().fg(p.accent),
        )));
        lines.push(Line::from(""));
        if a.track_view() {
            lines.push(Line::from(artists(v)));
            lines.push(Line::from(name(&v["album"], "name")));
            lines.push(Line::from(name(&v["audioSpec"], "format")));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        a.text("连接线路", "Connection"),
        Style::default().fg(p.secondary),
    )));
    lines.push(Line::from(clean(&a.route)));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(p.dim)),
        inner,
    );
}
fn settings(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let modal = center(r, 64, 19);
    f.render_widget(Clear, modal);
    let block = pane(a.text("设置", "Settings").into(), p, true);
    let inner = inset(block.inner(modal), 2, 1);
    f.render_widget(block, modal);
    button(
        f,
        a,
        Rect::new(modal.right().saturating_sub(5), modal.y, 3, 1),
        " × ",
        Action::CloseModal,
        p,
    );
    let labels = [
        a.text("配色", "Color palette"),
        a.text("布局", "Layout"),
        a.text("显示", "Display"),
        a.text("语言", "Language"),
        a.text("记忆位置", "Remember"),
        a.text("账户", "Account"),
    ]
    .map(str::to_owned);
    let values = [
        THEMES[a.config.theme % 5].to_owned(),
        LAYOUTS[a.config.layout % 4].to_owned(),
        a.text(
            if a.config.professional {
                "专业"
            } else {
                "极简"
            },
            if a.config.professional {
                "Full"
            } else {
                "Minimal"
            },
        )
        .to_owned(),
        if a.config.english {
            "English"
        } else {
            "简体中文"
        }
        .to_owned(),
        if a.config.remember_location {
            a.text("开启", "On")
        } else {
            a.text("关闭", "Off")
        }
        .to_owned(),
        a.text("断开连接", "Disconnect").to_owned(),
    ];
    let count = DISCONNECT_SETTING + usize::from(a.connected);
    let selected = a.settings.unwrap_or(0);
    for i in 0..count {
        let row = Rect::new(inner.x, inner.y + i as u16 * 2, inner.width, 1);
        let active = i == selected;
        let cols = Layout::horizontal([
            Constraint::Length(13),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(row);
        f.render_widget(
            Paragraph::new(format!("{} {}", if active { "›" } else { " " }, labels[i]))
                .style(Style::default().fg(if active { p.accent } else { p.dim })),
            cols[0],
        );
        f.render_widget(
            Paragraph::new(values[i].clone())
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(p.fg)
                        .bg(if active { p.selected } else { p.bg }),
                ),
            cols[2],
        );
        a.hits.push((row, Action::Setting(i)));
        if i < DISCONNECT_SETTING {
            button(f, a, cols[1], " ‹ ", Action::ChangeSetting(i, -1), p);
            button(f, a, cols[3], " › ", Action::ChangeSetting(i, 1), p);
        } else {
            a.hits.push((cols[2], Action::Disconnect));
        }
    }
    text(
        f,
        line(inner, 12),
        a.text(
            "↑↓/jk 选择  ←→/hl 更改  Enter 确认  Esc 返回",
            "↑↓/jk Select  ←→/hl Change  Enter Apply  Esc Back",
        ),
        p.dim,
    );
}
fn help_modal(f: &mut Frame, a: &mut App, r: Rect, p: Palette) {
    let modal = center(r, 84, 29);
    f.render_widget(Clear, modal);
    let block = pane(a.text("快捷键", "Keyboard shortcuts").into(), p, true);
    let inner = inset(block.inner(modal), 2, 0);
    f.render_widget(block, modal);
    button(
        f,
        a,
        Rect::new(modal.right().saturating_sub(5), modal.y, 3, 1),
        " × ",
        Action::CloseModal,
        p,
    );
    let rows = [
        ("?", a.text("帮助", "Help")),
        (
            "P",
            a.text(
                "设置：配色、布局、显示、语言、断开连接",
                "Settings: palette, layout, display, language, account",
            ),
        ),
        (
            "M",
            a.text(
                "全局切换极简 / 专业（输入时保留字母）",
                "Toggle minimal/full outside text input",
            ),
        ),
        (
            "O",
            a.text(
                "顺序 / 列表循环 / 单曲循环 / 随机",
                "Sequence / loop / repeat / shuffle",
            ),
        ),
        (
            "1…6",
            a.text(
                "专辑 / 歌手 / 风格 / 全部歌曲 / 收藏 / 歌单",
                "Albums / artists / genres / all songs / favorites / playlists",
            ),
        ),
        (
            "F",
            a.text(
                "在曲库中收藏 / 取消收藏",
                "Toggle favorite in library views",
            ),
        ),
        (
            "A",
            a.text("将选中歌曲加入歌单", "Add selected track to a playlist"),
        ),
        (
            "N / C / D",
            a.text(
                "新建 / 改名 / 删除歌单或移除歌曲",
                "Create / rename / delete playlist or remove track",
            ),
        ),
        (
            "Ctrl+←→ / h l",
            a.text(
                "快退 / 快进 5 秒；进度条可点击拖动",
                "Seek 5 seconds; click/drag timeline",
            ),
        ),
        (
            "Alt+←→ / H L",
            a.text(
                "左右移动当前类目并保存",
                "Move the current category and save its position",
            ),
        ),
        (
            "Alt+Home",
            a.text("恢复歌词自动跟随", "Resume lyric following"),
        ),
        ("↑ / ↓", a.text("选择条目", "Select item")),
        (
            "Tab / Shift+Tab",
            a.text(
                "切换分类；输入时切换焦点",
                "Change library; move focus in forms",
            ),
        ),
        (
            "←→ / h l / PgUp / PgDn",
            a.text("上一页 / 下一页", "Previous / next page"),
        ),
        (
            "Enter",
            a.text(
                "播放歌曲 / 打开专辑、歌手或风格",
                "Play track / open album, artist or genre",
            ),
        ),
        (
            "Space",
            a.text(
                "暂停或继续；表单中切换复选框",
                "Pause / resume; toggle checkbox in forms",
            ),
        ),
        (
            "[ / ]",
            a.text("播放队列上一首 / 下一首", "Previous / next track in queue"),
        ),
        (
            "E / Shift+E",
            a.text(
                "音效：原声 / 低音 / 现场 / Livehouse / 律动 / 空间3D",
                "Effects: original / bass / live / Livehouse / club / 3D space",
            ),
        ),
        ("S", a.text("停止播放", "Stop playback")),
        (
            "Alt+↑ / ↓",
            a.text(
                "浏览歌词；鼠标移到歌词区可滚动",
                "Scroll lyrics; mouse wheel over lyrics",
            ),
        ),
        ("/", a.text("搜索歌曲", "Search songs")),
        ("R", a.text("刷新当前页", "Refresh page")),
        (
            "Esc",
            a.text(
                "返回或取消；登录页退出",
                "Back / cancel; quit on login screen",
            ),
        ),
        (
            "Q / Ctrl+C",
            a.text(
                "退出；文字输入时使用 Ctrl+C",
                "Quit; use Ctrl+C while typing",
            ),
        ),
    ];
    for (i, (key, description)) in rows.into_iter().enumerate() {
        let cols = Layout::horizontal([Constraint::Length(23), Constraint::Min(1)])
            .split(line(inner, i as u16));
        text(f, cols[0], key, p.accent);
        text(f, cols[1], description, p.fg);
    }
}
fn search(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let modal = center(area, 64, 7);
    frame.render_widget(Clear, modal);
    let title = app.text("搜索歌曲", "Search songs");
    let block = pane(title.into(), p, true);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" / {}▏", clean(&app.search_input)),
            Style::default().fg(p.accent),
        )),
        Line::from(""),
        Line::from(app.text(
            " Enter 搜索 · Esc 取消 · Ctrl+U 清空",
            " Enter Search · Esc Cancel · Ctrl+U Clear",
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(p.fg)),
        inner,
    );
}

fn playlist_dialog(f: &mut Frame, a: &mut App, area: Rect, p: Palette) {
    let Some(dialog) = &a.playlist_dialog else {
        return;
    };
    let (title, height) = match dialog {
        PlaylistDialog::Name { id: Some(_), .. } => (a.text("重命名歌单", "Rename playlist"), 7),
        PlaylistDialog::Name { track: Some(_), .. } => (
            a.text("新建歌单并添加歌曲", "Create playlist and add track"),
            7,
        ),
        PlaylistDialog::Name { .. } => (a.text("新建歌单", "New playlist"), 7),
        PlaylistDialog::Pick { .. } => (
            a.text("选择要加入的歌单", "Add to playlist"),
            a.playlists.len().min(9) as u16 + 5,
        ),
        PlaylistDialog::Delete {
            track_guid: Some(_),
            ..
        } => (a.text("从歌单移除歌曲？", "Remove track from playlist?"), 7),
        PlaylistDialog::Delete { .. } => (a.text("删除歌单？", "Delete playlist?"), 7),
    };
    let modal = center(area, 64, height);
    f.render_widget(Clear, modal);
    let block = pane(title.into(), p, true);
    let inner = inset(block.inner(modal), 1, 0);
    f.render_widget(block, modal);
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let mut pick_rows = None;
    match &mut a.playlist_dialog {
        Some(PlaylistDialog::Name { input, .. }) => {
            let value = format!("{}▏", clean(input));
            text(f, parts[1], value, p.accent);
        }
        Some(PlaylistDialog::Pick { track, cursor }) => {
            text(f, parts[0], name(track, "title"), p.dim);
            let list = List::new(
                a.playlists
                    .iter()
                    .map(|playlist| ListItem::new(clean(&playlist.name))),
            )
            .highlight_symbol("› ")
            .highlight_style(Style::default().bg(p.selected).fg(p.accent));
            f.render_stateful_widget(list, parts[1], cursor);
            pick_rows = Some(cursor.offset());
        }
        Some(PlaylistDialog::Delete { name, .. }) => {
            text(f, parts[0], name.clone(), p.accent);
            text(
                f,
                parts[1],
                a.text("曲库里的原曲会保留。", "Songs stay in the library."),
                p.dim,
            );
        }
        None => return,
    }
    if let Some(offset) = pick_rows {
        for row in 0..usize::from(parts[1].height).min(a.playlists.len().saturating_sub(offset)) {
            a.hits.push((
                line(parts[1], row as u16),
                Action::PlaylistPick(offset + row),
            ));
        }
    }
    let confirm = match a.playlist_dialog {
        Some(PlaylistDialog::Delete {
            track_guid: Some(_),
            ..
        }) => a.text("[Enter 移除]", "[Enter Remove]"),
        Some(PlaylistDialog::Delete { .. }) => a.text("[Enter 删除]", "[Enter Delete]"),
        Some(PlaylistDialog::Pick { .. }) => a.text("[Enter 加入]", "[Enter Add]"),
        _ => a.text("[Enter 保存]", "[Enter Save]"),
    };
    buttons(
        f,
        a,
        parts[2],
        vec![
            (confirm.into(), Action::PlaylistConfirm),
            (
                a.text("[Esc 取消]", "[Esc Cancel]").into(),
                Action::CloseModal,
            ),
        ],
        p,
    );
}
