use crate::config::Config;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect, widgets::TableState};
use serde_json::Value;
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use termune::{Result, Session, library::Page};

pub const KINDS: [&str; 6] = ["album", "artist", "genre", "track", "favorite", "playlist"];
pub const TRACKS: usize = 3;
pub const FAVORITES: usize = 4;
pub const PLAYLISTS: usize = 5;
pub const DISCONNECT_SETTING: usize = 5;

mod location;
pub use location::BrowseLocation;
use location::BrowseSelection;

mod playlists;
use playlists::Playlist;
pub use playlists::PlaylistDialog;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[derive(Default)]
pub struct View {
    pub items: Vec<Value>,
    pub total: u64,
    pub page: u64,
    pub loaded: bool,
    pub table: TableState,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Detail {
    pub kind: String,
    pub guid: String,
    pub name: String,
}
#[derive(Clone)]
struct Load {
    id: u64,
    section: usize,
    page: u64,
    query: Option<String>,
    detail: Option<Detail>,
    selection: Option<BrowseSelection>,
}
enum Command {
    Login {
        id: u64,
        fn_id: String,
        username: String,
        password: String,
        allow_http: bool,
        remember: bool,
    },
    Restore {
        id: u64,
        fn_id: String,
        username: String,
        allow_http: bool,
    },
    Forget {
        fn_id: String,
        username: String,
    },
    Media {
        epoch: u64,
        key: String,
        track_guid: Option<String>,
    },
    Load(Load),
    Play {
        id: u64,
        guid: String,
        title: String,
    },
    Advance {
        id: u64,
        context: PlaybackContext,
    },
    Favorite {
        guid: String,
        enabled: bool,
        epoch: u64,
    },
    Logout,
}
enum Reply {
    Progress(u64, &'static str),
    Connected(u64, Result<Connected>),
    Forgotten(Result<()>),
    Media(u64, String, crate::media::Download),
    Page(Load, Result<Page>),
    Audio(u64, String, Result<termune::stream::AudioSource>),
    Advance(u64, PlaybackContext, Result<Value>),
    Favorite(u64, String, bool, Result<()>),
    Fatal(String),
}
struct Connected {
    server: String,
    username: String,
    route: String,
    remembered: bool,
    warning: Option<String>,
}

enum Authentication {
    Password(String, bool),
    Restore,
}

#[derive(Clone)]
struct PlaybackContext {
    section: usize,
    query: Option<String>,
    detail: Option<Detail>,
    index: u64,
    total: u64,
    playlist: Option<Arc<[Value]>>,
}
#[derive(Clone, Copy)]
pub enum Action {
    Field(usize),
    Connect,
    ToggleRemember,
    Section(usize),
    Back,
    Row(usize),
    Previous,
    Next,
    Select,
    Settings,
    Help,
    Setting(usize),
    ChangeSetting(usize, i8),
    CloseModal,
    Pause,
    Stop,
    PreviousTrack,
    NextTrack,
    Disconnect,
    LyricsScroll(isize),
    LyricLine(usize),
    Seek(f64),
    CycleMode,
    CycleEffect,
    CycleEffectBack,
    Volume(i16),
    VolumeAt(u16, u16),
    Favorite,
    PlaylistNew,
    PlaylistRename,
    PlaylistDelete,
    PlaylistAdd,
    PlaylistPick(usize),
    PlaylistConfirm,
    ToggleMode,
    FollowLyrics,
    Search,
    Refresh,
}

pub struct VolumeDrag {
    column: u16,
    initial: u16,
    click: u16,
    pub preview: Option<u16>,
}

struct TimedMessage {
    text: String,
    until: Instant,
}

impl TimedMessage {
    fn new(text: String, seconds: u64) -> Self {
        Self {
            text,
            until: Instant::now() + Duration::from_secs(seconds),
        }
    }

    fn active(&self, now: Instant) -> Option<&str> {
        (now < self.until).then_some(self.text.as_str())
    }
}

pub struct App {
    pub config: Config,
    pub fields: [String; 3],
    pub focus: usize,
    pub remember: bool,
    notice: Option<TimedMessage>,
    pub connected: bool,
    pub restoring: bool,
    pub server: String,
    pub username: String,
    pub route: String,
    pub section: usize,
    pub category_order: [usize; KINDS.len()],
    pub views: [View; KINDS.len()],
    pub playlists: Vec<Playlist>,
    pub playlist_dialog: Option<PlaylistDialog>,
    pub search_view: View,
    pub detail_view: View,
    pub query: Option<String>,
    pub detail: Option<Detail>,
    pub editing: bool,
    pub settings: Option<usize>,
    pub help: bool,
    pub search_input: String,
    pub busy: bool,
    pub stage: &'static str,
    status: Option<TimedMessage>,
    pub error: bool,
    seek_error: Option<TimedMessage>,
    pub hits: Vec<(Rect, Action)>,
    last_click: Option<(u16, u16, Instant)>,
    pub tick: usize,
    pub allow_http: bool,
    pub player: Option<crate::player::Player>,
    pub audio_loading: Option<String>,
    pub media: crate::media::Media,
    pub lyric_scroll: usize,
    pub lyrics_follow: bool,
    pub seek_drag: Option<f64>,
    pub seek_rect: Option<Rect>,
    pub volume_drag: Option<VolumeDrag>,
    playing_context: Option<PlaybackContext>,
    shuffle_history: Vec<PlaybackContext>,
    favorite_busy: bool,
    media_epoch: u64,
    media_busy: bool,
    media_due: Instant,
    media_cache: std::collections::VecDeque<crate::media::Media>,
    playing_item: Option<Value>,
    audio_generation: u64,
    generation: u64,
    latest: Arc<AtomicU64>,
    tx: mpsc::Sender<Command>,
    rx: mpsc::Receiver<Reply>,
    quit: bool,
}

fn worker(rx: mpsc::Receiver<Command>, tx: mpsc::Sender<Reply>, latest: Arc<AtomicU64>) {
    let client = match termune::client(15) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(Reply::Fatal(e));
            return;
        }
    };
    let mut session: Option<Session> = None;
    for command in rx {
        match command {
            Command::Login {
                id,
                fn_id,
                username,
                password,
                allow_http,
                remember,
            } => {
                if latest.load(Ordering::Relaxed) != id {
                    continue;
                }
                session = None;
                let result = authenticate(
                    &client,
                    &fn_id,
                    &username,
                    allow_http,
                    Authentication::Password(password, remember),
                    (&latest, id),
                    |stage| {
                        let _ = tx.send(Reply::Progress(id, stage));
                    },
                );
                let result = result.map(|(authenticated, connected)| {
                    session = Some(authenticated);
                    connected
                });
                let _ = tx.send(Reply::Connected(id, result));
            }
            Command::Restore {
                id,
                fn_id,
                username,
                allow_http,
            } => {
                if latest.load(Ordering::Relaxed) != id {
                    continue;
                }
                session = None;
                let result = authenticate(
                    &client,
                    &fn_id,
                    &username,
                    allow_http,
                    Authentication::Restore,
                    (&latest, id),
                    |stage| {
                        let _ = tx.send(Reply::Progress(id, stage));
                    },
                );
                let result = result.map(|(authenticated, connected)| {
                    session = Some(authenticated);
                    connected
                });
                let _ = tx.send(Reply::Connected(id, result));
            }
            Command::Forget { fn_id, username } => {
                use termune::credentials::TokenStore;
                let result = termune::credentials::KeyringStore::new(&fn_id, &username)
                    .and_then(|store| store.clear());
                let _ = tx.send(Reply::Forgotten(result));
            }
            Command::Load(load) => {
                if latest.load(Ordering::Relaxed) != load.id {
                    continue;
                }
                let result = match &session {
                    Some(s) => {
                        if let Some(detail) = &load.detail {
                            s.detail_page(&client, &detail.kind, &detail.guid, load.page)
                        } else {
                            s.page(
                                &client,
                                KINDS[load.section],
                                load.page,
                                load.query.as_deref(),
                            )
                        }
                    }
                    None => Err("会话不可用，请重新连接".into()),
                };
                let _ = tx.send(Reply::Page(load, result));
            }
            Command::Play { id, guid, title } => {
                let session = session.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result = session
                        .ok_or_else(|| "会话不可用，请重新连接".to_owned())
                        .map(|session| termune::stream::AudioSource { session, guid });
                    let _ = tx.send(Reply::Audio(id, title, result));
                });
            }
            Command::Media {
                epoch,
                key,
                track_guid,
                ..
            } => {
                let session = session.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let lyrics = (|| {
                        let Some(guid) = track_guid else {
                            return Ok(crate::media::LyricData::default());
                        };
                        let session = session.ok_or("未连接")?;
                        Ok(session
                            .lyric_text(&termune::client(8)?, &guid)?
                            .map(|text| crate::media::parse_lyrics(&text))
                            .unwrap_or_default())
                    })();
                    let _ = tx.send(Reply::Media(epoch, key, crate::media::Download { lyrics }));
                });
            }
            Command::Advance { id, context } => {
                let session = session.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result = (|| {
                        let session = session.ok_or("未连接")?;
                        let client = termune::client(15)?;
                        let page = context.index / 50 + 1;
                        let data = if let Some(detail) = &context.detail {
                            session.detail_page(&client, &detail.kind, &detail.guid, page)?
                        } else {
                            session.page(
                                &client,
                                KINDS[context.section],
                                page,
                                context.query.as_deref(),
                            )?
                        };
                        if data.total != context.total {
                            return Err("播放队列已变化，请重新选择歌曲".into());
                        }
                        data.items
                            .get((context.index % 50) as usize)
                            .cloned()
                            .ok_or_else(|| "播放队列已变化，请重新选择歌曲".into())
                    })();
                    let _ = tx.send(Reply::Advance(id, context, result));
                });
            }
            Command::Favorite {
                guid,
                enabled,
                epoch,
            } => {
                let result = session
                    .as_ref()
                    .ok_or_else(|| "未连接".to_owned())
                    .and_then(|s| s.set_favorite(&client, &guid, enabled));
                let _ = tx.send(Reply::Favorite(epoch, guid, enabled, result));
            }
            Command::Logout => session = None,
        }
    }
}

fn authenticate(
    client: &reqwest::blocking::Client,
    fn_id: &str,
    username: &str,
    allow_http: bool,
    method: Authentication,
    attempt: (&AtomicU64, u64),
    mut progress: impl FnMut(&'static str),
) -> Result<(Session, Connected)> {
    use termune::credentials::{KeyringStore, TokenStore};
    let (latest, id) = attempt;
    let saved = if matches!(method, Authentication::Restore) {
        progress("restore");
        Some(
            KeyringStore::new(fn_id, username)?
                .load()?
                .ok_or("没有已保存的登录状态，请输入密码登录")?,
        )
    } else {
        None
    };
    let connection =
        termune::connection::discover_connection(client, fn_id, allow_http, &mut progress)?;
    if latest.load(Ordering::Relaxed) != id {
        return Err("连接已取消".into());
    }
    let server_guid = connection.config["serverGUID"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let (authenticated, remembered, warning) = match method {
        Authentication::Password(password, remember) => {
            progress("login");
            let authenticated =
                connection
                    .candidate
                    .login(client, username, &password, &crate::device_id()?)?;
            if latest.load(Ordering::Relaxed) != id {
                return Err("连接已取消".into());
            }
            let warning = if remember {
                KeyringStore::new(fn_id, username)
                    .and_then(|store| {
                        termune::credentials::persist(
                            &store,
                            &authenticated,
                            server_guid.clone(),
                            true,
                        )
                    })
                    .err()
            } else {
                None
            };
            let remembered = remember && warning.is_none();
            (authenticated, remembered, warning)
        }
        Authentication::Restore => {
            progress("restore");
            let saved = saved.ok_or("没有保存的登录状态")?;
            let store = KeyringStore::new(fn_id, username)?;
            let authenticated = termune::credentials::restore_saved(
                &store,
                &connection.candidate,
                client,
                saved,
                server_guid.as_deref(),
            )?;
            (authenticated, true, None)
        }
    };
    let user = authenticated
        .user
        .get("user")
        .unwrap_or(&authenticated.user);
    let connected = Connected {
        server: clean(
            connection.config["serverName"]
                .as_str()
                .unwrap_or("Feiniu NAS"),
        ),
        username: clean(user["name"].as_str().unwrap_or("Music user")),
        route: format!(
            "{} · {}",
            connection.candidate.base.scheme().to_uppercase(),
            if connection.candidate.relay {
                "FN Connect"
            } else {
                "Direct"
            }
        ),
        remembered,
        warning,
    };
    Ok((authenticated, connected))
}

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(160)
        .collect()
}
pub fn name(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .map(clean)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into())
}
pub fn artists(item: &Value) -> String {
    item["artists"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|a| name(a, "name"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into())
}

impl App {
    fn restore(&mut self) {
        let id = self.next_id();
        self.busy = true;
        self.stage = "restore";
        self.restoring = true;
        self.clear_status();
        if self
            .tx
            .send(Command::Restore {
                id,
                fn_id: self.config.fn_id.clone(),
                username: self.config.username.clone(),
                allow_http: self.allow_http,
            })
            .is_err()
        {
            self.fail("后台任务已停止".into());
        }
    }

    fn toggle_remember(&mut self) {
        self.remember = !self.remember;
        if !self.remember {
            if self.busy {
                self.cancel();
                let _ = self.tx.send(Command::Logout);
            }
            self.config.auto_login = false;
            self.save();
            if !self.config.fn_id.is_empty() && !self.config.username.is_empty() {
                let _ = self.tx.send(Command::Forget {
                    fn_id: self.config.fn_id.clone(),
                    username: self.config.username.clone(),
                });
            }
        }
    }
    fn new(allow_http: bool) -> Self {
        let (config, status) = match Config::load() {
            Ok(c) => (c, String::new()),
            Err(e) => (Config::default(), e),
        };
        let mut app = Self::with_config(config, status, allow_http);
        if app.config.auto_login && !app.config.fn_id.is_empty() && !app.config.username.is_empty()
        {
            app.restore();
        }
        app
    }
    fn with_config(config: Config, status: String, allow_http: bool) -> Self {
        let (tx, commands) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let worker_latest = latest.clone();
        std::thread::spawn(move || worker(commands, replies, worker_latest));
        let section = if config.remember_location {
            KINDS
                .iter()
                .position(|kind| *kind == config.last_category)
                .unwrap_or(0)
        } else {
            0
        };
        let mut category_order = std::array::from_fn(|i| i);
        for kind in config.category_order.iter().rev() {
            if let Some(position) = category_order.iter().position(|&id| KINDS[id] == kind) {
                category_order[..=position].rotate_right(1);
            }
        }
        Self {
            fields: [config.fn_id.clone(), config.username.clone(), String::new()],
            remember: config.auto_login,
            focus: if !config.fn_id.is_empty() && !config.username.is_empty() {
                2
            } else {
                0
            },
            notice: None,
            config,
            connected: false,
            restoring: false,
            server: String::new(),
            username: String::new(),
            route: String::new(),
            section,
            category_order,
            views: std::array::from_fn(|_| View::default()),
            playlists: Vec::new(),
            playlist_dialog: None,
            search_view: View::default(),
            detail_view: View::default(),
            query: None,
            detail: None,
            editing: false,
            settings: None,
            help: false,
            search_input: String::new(),
            busy: false,
            stage: "",
            error: !status.is_empty(),
            status: (!status.is_empty()).then(|| TimedMessage::new(status, 8)),
            seek_error: None,
            hits: Vec::new(),
            last_click: None,
            tick: 0,
            allow_http,
            player: None,
            audio_loading: None,
            media: crate::media::Media::default(),
            lyric_scroll: 0,
            lyrics_follow: true,
            seek_drag: None,
            seek_rect: None,
            volume_drag: None,
            playing_context: None,
            shuffle_history: Vec::new(),
            favorite_busy: false,
            media_epoch: 0,
            media_busy: false,
            media_due: Instant::now(),
            media_cache: std::collections::VecDeque::new(),
            playing_item: None,
            audio_generation: 0,
            generation: 0,
            latest,
            tx,
            rx,
            quit: false,
        }
    }
    pub fn text<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        self.config.text(zh, en)
    }
    pub fn view(&self) -> &View {
        if self.detail.is_some() {
            &self.detail_view
        } else if self.query.is_some() {
            &self.search_view
        } else {
            &self.views[self.section]
        }
    }
    pub fn view_mut(&mut self) -> &mut View {
        if self.detail.is_some() {
            &mut self.detail_view
        } else if self.query.is_some() {
            &mut self.search_view
        } else {
            &mut self.views[self.section]
        }
    }
    fn next_id(&mut self) -> u64 {
        self.generation += 1;
        self.latest.store(self.generation, Ordering::Relaxed);
        self.generation
    }
    fn fail(&mut self, error: String) {
        self.restoring = false;
        self.show_status(error, true);
        self.busy = false;
    }
    fn show_status(&mut self, message: String, error: bool) {
        self.status = Some(TimedMessage::new(message, if error { 8 } else { 3 }));
        self.error = error;
    }
    fn clear_status(&mut self) {
        self.status = None;
        self.error = false;
    }
    fn show_notice(&mut self, warning: String) {
        self.notice = Some(TimedMessage::new(warning, 8));
    }
    fn show_seek_error(&mut self, error: String) {
        self.seek_error = Some(TimedMessage::new(error, 3));
    }
    pub fn footer_messages(&self) -> (Option<&str>, Option<&str>, Option<&str>) {
        let now = Instant::now();
        (
            self.status.as_ref().and_then(|message| message.active(now)),
            self.notice.as_ref().and_then(|message| message.active(now)),
            self.seek_error
                .as_ref()
                .and_then(|message| message.active(now)),
        )
    }
    fn save(&mut self) {
        if let Err(error) = self.config.save() {
            self.show_status(error, true);
        }
    }
    fn load(&mut self, page: u64) {
        self.load_with_selection(page, None);
    }
    fn load_with_selection(&mut self, page: u64, selection: Option<BrowseSelection>) {
        if self.section == PLAYLISTS {
            self.cancel();
            self.load_playlist_page(page, 0);
            if let Some(selection) = selection {
                selection.apply(self.view_mut());
            }
            return;
        }
        let id = self.next_id();
        self.busy = true;
        self.clear_status();
        self.stage = "loading";
        let load = Load {
            id,
            section: self.section,
            page,
            query: self.query.clone(),
            detail: self.detail.clone(),
            selection,
        };
        if self.tx.send(Command::Load(load)).is_err() {
            self.fail("后台任务已停止".into());
        }
    }
    fn connect(&mut self) {
        if self.busy {
            return;
        }
        let fn_id = match termune::validate_id(&self.fields[0]) {
            Ok(id) => id,
            Err(e) => {
                self.fail(e);
                return;
            }
        };
        let username = self.fields[1].trim().to_owned();
        if username.is_empty() || self.fields[2].is_empty() {
            self.fail(
                self.text(
                    "请输入音乐用户名和密码",
                    "Enter Music username and password",
                )
                .into(),
            );
            return;
        }
        let id = self.next_id();
        self.busy = true;
        self.clear_status();
        self.stage = "discovery";
        let password = std::mem::take(&mut self.fields[2]);
        if self
            .tx
            .send(Command::Login {
                id,
                fn_id,
                username,
                password,
                allow_http: self.allow_http,
                remember: self.remember,
            })
            .is_err()
        {
            self.fail("后台任务已停止".into());
        }
    }
    fn cancel(&mut self) {
        self.restoring = false;
        self.next_id();
        self.busy = false;
    }
    fn logout(&mut self) {
        self.checkpoint_location();
        self.settings = None;
        self.help = false;
        self.media_epoch = self.media_epoch.wrapping_add(1);
        self.media_busy = false;
        self.favorite_busy = false;
        self.shuffle_history.clear();
        self.media = crate::media::Media::default();
        self.media_cache.clear();
        self.stop_audio();
        self.cancel();
        let _ = self.tx.send(Command::Logout);
        self.connected = false;
        self.views = std::array::from_fn(|_| View::default());
        self.playlists.clear();
        self.playlist_dialog = None;
        self.query = None;
        self.detail = None;
        self.close_search();
        self.search_view = View::default();
        self.detail_view = View::default();
        self.notice = None;
        self.show_status(
            self.text("已断开，请重新连接", "Disconnected. Connect again.")
                .into(),
            false,
        );
        self.fields[2].clear();
        self.focus = 2;
    }
    fn replies(&mut self) {
        if let Some(result) = self.player.as_mut().and_then(|p| p.poll()) {
            self.player = None;
            self.seek_error = None;
            match result {
                Ok(()) => {
                    self.show_status(self.text("播放结束", "Playback finished").into(), false);
                    self.advance(true, true);
                }
                Err(error) => {
                    if termune::is_auth_error(&error) {
                        let _ = self.tx.send(Command::Forget {
                            fn_id: self.config.fn_id.clone(),
                            username: self.config.username.clone(),
                        });
                        self.logout();
                    }
                    self.fail(error);
                }
            }
        }
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Progress(id, stage) if id == self.generation => self.stage = stage,
                Reply::Connected(id, result) if id == self.generation => match result {
                    Ok(connected) => {
                        self.connected = true;
                        self.restoring = false;
                        self.server = connected.server;
                        self.username = connected.username;
                        self.route = connected.route;
                        self.remember = connected.remembered;
                        self.config.auto_login = connected.remembered;
                        self.notice = connected
                            .warning
                            .map(|warning| TimedMessage::new(warning, 8));
                        self.busy = false;
                        self.config.fn_id = self.fields[0].trim().to_ascii_lowercase();
                        self.config.username = self.fields[1].trim().to_owned();
                        if let Err(error) = self.config.save() {
                            self.show_notice(error);
                        }
                        if let Err(error) = self.read_playlists() {
                            self.show_notice(error);
                        }
                        self.restore_location();
                    }
                    Err(e) => self.fail(e),
                },
                Reply::Page(load, result) if load.id == self.generation => {
                    self.busy = false;
                    match result {
                        Ok(page) => {
                            let max_page = page.total.div_ceil(50).max(1);
                            if load.page > max_page {
                                self.load_with_selection(max_page, load.selection);
                                continue;
                            }
                            let loaded = page.items.len();
                            let view = self.view_mut();
                            view.items = page.items;
                            view.total = page.total;
                            view.page = load.page;
                            view.loaded = true;
                            view.table = TableState::default();
                            if let Some(selection) = load.selection {
                                selection.apply(view);
                            } else if loaded > 0 {
                                view.table.select(Some(0));
                            }
                            let message = if self.config.english {
                                format!("Loaded {loaded} items")
                            } else {
                                format!("已加载 {loaded} 条")
                            };
                            self.show_status(message, false);
                        }
                        Err(e) => {
                            if termune::is_auth_error(&e) {
                                let _ = self.tx.send(Command::Forget {
                                    fn_id: self.config.fn_id.clone(),
                                    username: self.config.username.clone(),
                                });
                                self.logout();
                            }
                            self.fail(e);
                        }
                    }
                }
                Reply::Media(epoch, key, download) if epoch == self.media_epoch => {
                    self.media_busy = false;
                    let mut media = if self.media.key == key {
                        self.media.clone()
                    } else {
                        crate::media::Media {
                            key: key.clone(),
                            ..Default::default()
                        }
                    };
                    media.loading = false;
                    media.lyric_error = download.lyrics.is_err();
                    let lyrics = download.lyrics.unwrap_or_default();
                    media.lyrics = lyrics.lines;
                    media.times = lyrics.times;
                    if self.media.key == key {
                        self.media = media.clone();
                    }
                    if self.media_cache.len() >= 16 {
                        self.media_cache.pop_front();
                    }
                    self.media_cache.push_back(media);
                }
                Reply::Advance(id, context, result) if id == self.audio_generation => {
                    match result {
                        Ok(item) => self.start_item(item, context),
                        Err(e) => {
                            self.audio_loading = None;
                            self.fail(e);
                        }
                    }
                }
                Reply::Favorite(epoch, guid, enabled, result) if epoch == self.media_epoch => {
                    self.favorite_busy = false;
                    match result {
                        Ok(()) => {
                            for view in self
                                .views
                                .iter_mut()
                                .chain([&mut self.search_view, &mut self.detail_view])
                            {
                                for item in &mut view.items {
                                    if item["guid"].as_str() == Some(guid.as_str()) {
                                        item["isFavorite"] = Value::Bool(enabled);
                                    }
                                }
                            }
                            if let Some(item) = &mut self.playing_item
                                && item["guid"].as_str() == Some(guid.as_str())
                            {
                                item["isFavorite"] = Value::Bool(enabled);
                            }
                            self.views[FAVORITES].loaded = false;
                            if self.section == FAVORITES
                                && self.detail.is_none()
                                && self.query.is_none()
                            {
                                self.load(self.view().page.max(1));
                            } else {
                                let message = self.text(
                                    if enabled {
                                        "已收藏"
                                    } else {
                                        "已取消收藏"
                                    },
                                    if enabled {
                                        "Added to favorites"
                                    } else {
                                        "Removed from favorites"
                                    },
                                );
                                self.show_status(message.into(), false);
                            }
                        }
                        Err(e) => self.fail(e),
                    }
                }
                Reply::Forgotten(Err(error)) => self.show_notice(error),
                Reply::Audio(id, title, result) if id == self.audio_generation => {
                    self.audio_loading = None;
                    match result.and_then(|response| {
                        crate::player::Player::start(
                            response,
                            title,
                            self.config.audio_effect,
                            self.config.volume.min(100),
                        )
                    }) {
                        Ok(player) => {
                            self.player = Some(player);
                            self.clear_status();
                        }
                        Err(error) => self.fail(error),
                    }
                }
                Reply::Forgotten(Ok(())) => {}
                Reply::Fatal(e) => self.fail(e),
                _ => {}
            }
        }
    }
    fn move_selection(&mut self, delta: isize) {
        let view = self.view_mut();
        if view.items.is_empty() {
            return;
        }
        let selected = view
            .table
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(view.items.len() - 1);
        view.table.select(Some(selected));
    }
    fn stop_audio(&mut self) {
        self.audio_generation = self.audio_generation.wrapping_add(1);
        self.audio_loading = None;
        self.player = None;
        self.seek_error = None;
        self.playing_item = None;
        self.playing_context = None;
        self.seek_drag = None;
    }
    pub fn track_view(&self) -> bool {
        matches!(self.section, TRACKS | FAVORITES) || self.query.is_some() || self.detail.is_some()
    }
    fn play_selected(&mut self) {
        if !self.track_view() {
            return;
        }
        let item = self
            .view()
            .table
            .selected()
            .and_then(|i| self.view().items.get(i))
            .cloned();
        let Some(item) = item else {
            return;
        };
        let view = self.view();
        let context = PlaybackContext {
            section: self.section,
            query: self.query.clone(),
            detail: self.detail.clone(),
            index: (view.page.max(1) - 1) * 50 + view.table.selected().unwrap_or(0) as u64,
            total: view.total.max(view.items.len() as u64),
            playlist: self
                .open_playlist()
                .map(|playlist| playlist.tracks.clone().into()),
        };
        self.shuffle_history.clear();
        self.start_item(item, context);
    }
    fn start_item(&mut self, item: Value, context: PlaybackContext) {
        let Some(guid) = item["guid"].as_str().map(str::to_owned) else {
            return;
        };
        let title = name(&item, "title");
        self.stop_audio();
        self.playing_context = Some(context);
        self.playing_item = Some(item);
        self.audio_loading = Some(title.clone());
        self.lyrics_follow = true;
        self.seek_drag = None;
        self.clear_status();
        if self
            .tx
            .send(Command::Play {
                id: self.audio_generation,
                guid,
                title,
            })
            .is_err()
        {
            self.audio_loading = None;
            self.fail("后台任务已停止".into());
        }
    }
    fn advance(&mut self, forward: bool, automatic: bool) {
        let Some(mut context) = self.playing_context.clone() else {
            if !automatic {
                self.move_selection(if forward { 1 } else { -1 });
                self.play_selected();
            }
            return;
        };
        if automatic
            && self.config.playback_mode == crate::queue::Mode::RepeatOne
            && let Some(item) = self.playing_item.clone()
        {
            self.start_item(item, context);
            return;
        }
        if self.config.playback_mode == crate::queue::Mode::Shuffle
            && !forward
            && let Some(previous) = self.shuffle_history.pop()
        {
            self.advance_to(previous, false);
            return;
        }
        let random = termune::random_id()
            .ok()
            .and_then(|s| u64::from_str_radix(&s[..16], 16).ok())
            .unwrap_or(0);
        let Some(index) = self.config.playback_mode.target(
            context.index,
            context.total,
            forward,
            automatic,
            random,
        ) else {
            return;
        };
        if self.config.playback_mode == crate::queue::Mode::Shuffle && forward {
            if self.shuffle_history.len() >= 100 {
                self.shuffle_history.remove(0);
            }
            self.shuffle_history.push(context.clone());
        }
        context.index = index;
        self.advance_to(context, forward);
    }
    fn advance_to(&mut self, context: PlaybackContext, forward: bool) {
        if let Some(tracks) = &context.playlist {
            if let Some(item) = tracks.get(context.index as usize) {
                self.start_item(item.clone(), context);
            }
            return;
        }
        self.stop_audio();
        self.playing_context = Some(context.clone());
        self.audio_loading = Some(
            if forward {
                self.text("准备下一首…", "Loading next track…")
            } else {
                self.text("准备上一首…", "Loading previous track…")
            }
            .into(),
        );
        if self
            .tx
            .send(Command::Advance {
                id: self.audio_generation,
                context,
            })
            .is_err()
        {
            self.audio_loading = None;
            self.fail("后台任务已停止".into());
        }
    }
    fn favorite(&mut self) {
        if self.favorite_busy || !self.track_view() || self.section == PLAYLISTS {
            return;
        }
        let Some(item) = self
            .view()
            .table
            .selected()
            .and_then(|i| self.view().items.get(i))
        else {
            return;
        };
        let Some(guid) = item["guid"].as_str() else {
            return;
        };
        let enabled = !item["isFavorite"]
            .as_bool()
            .unwrap_or(self.section == FAVORITES);
        let guid = guid.to_owned();
        self.favorite_busy = true;
        let _ = self.tx.send(Command::Favorite {
            guid,
            enabled,
            epoch: self.media_epoch,
        });
    }
    fn toggle_audio(&mut self) {
        if let Some(player) = &mut self.player {
            if let Err(error) = player.toggle() {
                self.fail(error);
            }
        } else if self.audio_loading.is_none() {
            self.play_selected();
        }
    }
    pub fn category_position(&self) -> usize {
        self.category_order
            .iter()
            .position(|&id| id == self.section)
            .expect("all library categories are in the tab order")
    }
    fn reorder_category(&mut self, delta: isize) {
        let position = self.category_position();
        let Some(target) = position
            .checked_add_signed(delta)
            .filter(|&i| i < KINDS.len())
        else {
            return;
        };
        let mut order = self.category_order;
        order.swap(position, target);
        let previous = std::mem::replace(
            &mut self.config.category_order,
            order.iter().map(|&id| KINDS[id].to_owned()).collect(),
        );
        if let Err(error) = self.config.save() {
            self.config.category_order = previous;
            self.show_status(error, true);
            return;
        }
        self.category_order = order;
        self.show_status(
            self.text("已保存类目顺序", "Category order saved").into(),
            false,
        );
    }
    fn switch(&mut self, section: usize) {
        if section >= KINDS.len() {
            return;
        }
        self.cancel();
        self.section = section;
        if self.config.remember_location {
            self.config.last_category = KINDS[section].into();
            self.save();
        }
        self.query = None;
        self.detail = None;
        self.close_search();
        if !self.view().loaded {
            self.load(1);
        } else {
            self.clear_status();
        }
    }
    fn select(&mut self) {
        if self.busy {
            return;
        }
        if self.track_view() {
            self.play_selected();
            return;
        }
        let Some(item) = self
            .view()
            .table
            .selected()
            .and_then(|i| self.view().items.get(i))
            .cloned()
        else {
            return;
        };
        let Some(guid) = item["guid"].as_str() else {
            return;
        };
        self.detail = Some(Detail {
            kind: KINDS[self.section].into(),
            guid: guid.into(),
            name: name(&item, "name"),
        });
        self.detail_view = View::default();
        self.load(1);
    }
    fn page(&mut self, next: bool) {
        if self.busy {
            return;
        }
        let view = self.view();
        let page = view.page.max(1);
        if next {
            if page.saturating_mul(50) < view.total {
                self.load(page + 1);
            }
        } else if page > 1 {
            self.load(page - 1);
        }
    }
    pub fn active_lyric(&self) -> Option<usize> {
        let player = self.player.as_ref()?;
        let guid = self.playing_item.as_ref()?["guid"].as_str()?;
        if self.media.key != format!("track:{guid}") || player.timeline.duration <= 0.0 {
            return None;
        }
        crate::media::active_line(&self.media.times, player.timeline.position)
    }
    fn refresh_media(&mut self) {
        if !self.connected || !self.config.professional {
            return;
        }
        let playing = self.player.is_some() || self.audio_loading.is_some();
        let item = if playing {
            self.playing_item.clone()
        } else {
            self.view()
                .table
                .selected()
                .and_then(|i| self.view().items.get(i))
                .cloned()
        };
        let Some(item) = item else {
            self.media = crate::media::Media::default();
            return;
        };
        let Some(guid) = item["guid"].as_str() else {
            return;
        };
        let track = playing || self.track_view();
        if self.section == PLAYLISTS && !track {
            self.media = crate::media::Media::default();
            return;
        }
        let key = format!(
            "{}:{guid}",
            if track { "track" } else { KINDS[self.section] }
        );
        if self.media.key != key {
            self.lyric_scroll = 0;
            self.lyrics_follow = true;
            self.media_due = Instant::now();
            let title = name(&item, if track { "title" } else { "name" });
            let artist = artists(&item);
            let album = name(&item["album"], "name");
            self.media = self
                .media_cache
                .iter()
                .find(|m| m.key == key)
                .cloned()
                .unwrap_or_else(|| crate::media::Media {
                    key: key.clone(),
                    loading: true,
                    ..Default::default()
                });
            self.media.title = title;
            self.media.artist = artist;
            self.media.album = album;
        }
        if self.media_busy
            || !self.media.loading
            || self.media_due.elapsed() < Duration::from_millis(250)
        {
            return;
        }
        self.media_busy = true;
        if self
            .tx
            .send(Command::Media {
                epoch: self.media_epoch,
                key,
                track_guid: if track { Some(guid.to_owned()) } else { None },
            })
            .is_err()
        {
            self.media_busy = false;
            self.media.loading = false;
        }
    }
    fn seek_relative(&mut self, seconds: f64) {
        self.seek_drag = None;
        if let Some(player) = self.player.as_mut() {
            let duration = player.timeline.duration;
            if let Err(error) = player.seek((player.timeline.position + seconds) / duration) {
                self.show_seek_error(error);
            }
        }
    }
    fn adjacent_track(&mut self, delta: isize) {
        self.advance(delta > 0, false);
    }
    fn adjust_setting(&mut self, delta: i8) {
        let Some(row) = self.settings else {
            return;
        };
        match row {
            0 => self.config.theme = (self.config.theme as i8 + delta).rem_euclid(5) as usize,
            1 => {
                self.config.layout = (self.config.layout as i8 + delta).rem_euclid(4) as usize;
                self.config.professional = true;
            }
            2 => self.config.professional = !self.config.professional,
            3 => self.config.english = !self.config.english,
            4 => self.config.remember_location = !self.config.remember_location,
            _ => return,
        }
        self.save();
    }
    fn set_volume(&mut self, volume: u16) {
        if let Some(player) = self.player.as_mut() {
            player.set_volume(volume);
        }
        self.config.volume = volume;
        self.show_status(format!("音量 / Volume: {volume}%"), false);
        self.save();
    }
    fn cycle_effect(&mut self, direction: isize) {
        let count = crate::effects::Effect::COUNT as isize;
        let effect = (self.config.audio_effect as isize + count + direction) % count;
        let effect = effect as usize;
        if let Some(player) = self.player.as_mut() {
            player.set_effect(effect);
        }
        self.config.audio_effect = effect;
        self.save();
    }
    fn action(&mut self, action: Action) {
        match action {
            Action::Seek(fraction) => {
                self.volume_drag = None;
                self.seek_drag = Some(fraction.clamp(0.0, 1.0));
            }
            Action::Volume(delta) => {
                let volume = (self.config.volume.min(100) as i16 + delta).clamp(0, 100) as u16;
                self.volume_drag = None;
                self.set_volume(volume);
            }
            Action::VolumeAt(volume, column) => {
                self.seek_drag = None;
                self.volume_drag = Some(VolumeDrag {
                    column,
                    initial: self.config.volume.min(100),
                    click: volume,
                    preview: None,
                });
            }
            Action::CycleEffect => self.cycle_effect(1),
            Action::CycleEffectBack => self.cycle_effect(-1),
            Action::CycleMode => {
                self.config.playback_mode = self.config.playback_mode.next();
                self.save();
            }
            Action::Favorite => self.favorite(),
            Action::PlaylistNew => self.new_playlist(None),
            Action::PlaylistRename => self.rename_playlist(),
            Action::PlaylistDelete => self.delete_playlist_entry(),
            Action::PlaylistAdd => self.add_to_playlist(),
            Action::PlaylistPick(index) => self.pick_playlist(index),
            Action::PlaylistConfirm => self.confirm_playlist(),
            Action::ToggleMode => {
                self.seek_drag = None;
                self.config.professional = !self.config.professional;
                self.save();
            }
            Action::FollowLyrics => self.lyrics_follow = true,
            Action::Field(i) => self.focus = i,
            Action::Connect => self.connect(),
            Action::ToggleRemember => self.toggle_remember(),
            Action::Section(i) => self.switch(i),
            Action::Back => self.back(),
            Action::Row(i) => {
                if i < self.view().items.len() {
                    self.view_mut().table.select(Some(i));
                }
            }
            Action::Previous => self.page(false),
            Action::Next => self.page(true),
            Action::Select => self.select(),
            Action::LyricLine(index) => {
                let matching = self
                    .playing_item
                    .as_ref()
                    .and_then(|item| item["guid"].as_str())
                    .is_some_and(|guid| self.media.key == format!("track:{guid}"));
                if matching
                    && let Some(seconds) = self.media.times.get(index).copied().flatten()
                    && let Some(player) = self.player.as_mut()
                {
                    let duration = player.timeline.duration;
                    if let Err(error) = player.seek(seconds / duration) {
                        self.show_seek_error(error);
                    } else {
                        self.lyrics_follow = true;
                        self.seek_drag = None;
                    }
                }
            }
            Action::LyricsScroll(delta) => {
                if delta != 0 {
                    self.lyrics_follow = false;
                }
                self.lyric_scroll = self
                    .lyric_scroll
                    .saturating_add_signed(delta)
                    .min(self.media.lyrics.len().saturating_mul(8))
            }
            Action::Settings => {
                self.seek_drag = None;
                self.settings = Some(0);
                self.help = false;
                self.close_search();
            }
            Action::Help => {
                self.seek_drag = None;
                self.help = true;
                self.settings = None;
                self.close_search();
            }
            Action::Setting(row) => self.settings = Some(row),
            Action::ChangeSetting(row, delta) => {
                self.settings = Some(row);
                self.adjust_setting(delta);
            }
            Action::CloseModal => {
                self.playlist_dialog = None;
                self.settings = None;
                self.help = false;
                self.close_search();
            }
            Action::Pause => self.toggle_audio(),
            Action::Stop => {
                self.stop_audio();
                self.show_status(self.text("播放已停止", "Playback stopped").into(), false);
            }
            Action::PreviousTrack => self.adjacent_track(-1),
            Action::NextTrack => self.adjacent_track(1),
            Action::Disconnect => {
                self.settings = None;
                self.help = false;
                self.logout();
            }
            Action::Search => {
                self.editing = true;
                self.search_input.clear();
            }
            Action::Refresh => {
                if self.section == PLAYLISTS
                    && let Err(error) = self.read_playlists()
                {
                    self.fail(error);
                    return;
                }
                self.media_cache.clear();
                self.media.key.clear();
                self.load(self.view().page.max(1));
            }
        }
    }
    fn close_search(&mut self) {
        self.editing = false;
        self.search_input.clear();
    }
    fn back(&mut self) {
        if self.seek_drag.take().is_some() || self.volume_drag.take().is_some() {
            return;
        }

        if self.editing {
            self.close_search();
            return;
        }
        if self.detail.take().is_some() || self.query.take().is_some() {
            self.cancel();
            self.clear_status();
            if !self.view().loaded {
                self.load(1);
            }
        }
    }
    fn key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.playlist_dialog.is_some() {
            self.playlist_key(key);
            return;
        }
        if self.connected
            && !self.editing
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char('m' | 'M'))
        {
            self.action(Action::ToggleMode);
            return;
        }
        if self.connected
            && !self.editing
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char('+' | '=' | '-'))
        {
            self.action(Action::Volume(if key.code == KeyCode::Char('-') {
                -5
            } else {
                5
            }));
            return;
        }
        if (key.code == KeyCode::Char('?') && (self.connected || self.help)
            || key.code == KeyCode::F(1)
                && !self.connected
                && !self.busy
                && self.settings.is_none())
            && !self.editing
        {
            if self.help {
                self.help = false;
            } else {
                self.action(Action::Help);
            }
            return;
        }
        if matches!(key.code, KeyCode::Char('p' | 'P')) && !self.editing && self.connected {
            if self.settings.is_some() {
                self.settings = None;
            } else {
                self.action(Action::Settings);
            }
            return;
        }
        if self.help {
            if key.code == KeyCode::Esc {
                self.help = false;
            }
            return;
        }
        if let Some(row) = self.settings {
            let count = DISCONNECT_SETTING + usize::from(self.connected);
            match key.code {
                KeyCode::Esc => self.settings = None,
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    self.settings = Some((row + count - 1) % count)
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    self.settings = Some((row + 1) % count)
                }
                KeyCode::Left | KeyCode::Char('h') if row < DISCONNECT_SETTING => {
                    self.adjust_setting(-1)
                }
                KeyCode::Right | KeyCode::Char('l') if row < DISCONNECT_SETTING => {
                    self.adjust_setting(1)
                }
                KeyCode::Enter if row == DISCONNECT_SETTING && self.connected => {
                    self.action(Action::Disconnect)
                }
                KeyCode::Enter => self.adjust_setting(1),
                _ => {}
            }
            return;
        }
        if self.connected
            && self.config.professional
            && !self.editing
            && key.modifiers.contains(KeyModifiers::ALT)
        {
            if key.code == KeyCode::Up {
                self.action(Action::LyricsScroll(-1));
                return;
            }
            if key.code == KeyCode::Down {
                self.action(Action::LyricsScroll(1));
                return;
            }
        }
        if self.connected && !self.editing && key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c @ '1'..='6') = key.code {
                self.switch(c as usize - '1' as usize);
                return;
            }
            if key.code == KeyCode::Home {
                self.action(Action::FollowLyrics);
                return;
            }
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                let delta = match key.code {
                    KeyCode::Left | KeyCode::Char('h' | 'H') => -1,
                    KeyCode::Right | KeyCode::Char('l' | 'L') => 1,
                    _ => 0,
                };
                if delta != 0 {
                    self.reorder_category(delta);
                    return;
                }
            }
        }

        if !self.connected {
            if key.code == KeyCode::Esc {
                if self.busy {
                    self.cancel();
                    let _ = self.tx.send(Command::Logout);
                } else {
                    self.quit = true;
                }
                return;
            }
            if self.busy {
                return;
            }
            match key.code {
                KeyCode::Tab => self.focus = (self.focus + 1) % 5,
                KeyCode::BackTab => self.focus = (self.focus + 4) % 5,
                KeyCode::Enter => {
                    if self.focus < 2 {
                        self.focus += 1;
                    } else if self.focus == 3 {
                        self.toggle_remember();
                    } else {
                        self.connect();
                    }
                }
                KeyCode::Char(' ') if self.focus == 3 => self.toggle_remember(),
                KeyCode::Backspace if self.focus < 3 => {
                    self.fields[self.focus].pop();
                }
                KeyCode::Char('u')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && self.focus < 3 =>
                {
                    self.fields[self.focus].clear()
                }
                KeyCode::Char(c)
                    if self.focus < 3
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !c.is_control() =>
                {
                    self.fields[self.focus].push(c)
                }
                _ => {}
            }
            return;
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.close_search(),
                KeyCode::Backspace => {
                    self.search_input.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.search_input.clear()
                }
                KeyCode::Enter => {
                    let query = self.search_input.trim().to_owned();
                    self.close_search();
                    if query.is_empty() {
                        return;
                    }
                    self.cancel();
                    self.section = TRACKS;
                    self.detail = None;
                    self.query = Some(query);
                    self.search_view = View::default();
                    self.load(1);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !c.is_control() =>
                {
                    self.search_input.push(c)
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            )
        {
            self.seek_relative(if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
                5.0
            } else {
                -5.0
            });
            return;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        match key.code {
            KeyCode::Char('q' | 'Q') => self.quit = true,
            KeyCode::Esc => self.back(),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Home => {
                if !self.view().items.is_empty() {
                    self.view_mut().table.select(Some(0));
                }
            }
            KeyCode::End => {
                let len = self.view().items.len();
                if len > 0 {
                    self.view_mut().table.select(Some(len - 1));
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::PageUp => self.page(false),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::PageDown => self.page(true),
            KeyCode::Tab => {
                self.switch(self.category_order[(self.category_position() + 1) % KINDS.len()])
            }
            KeyCode::BackTab => self.switch(
                self.category_order[(self.category_position() + KINDS.len() - 1) % KINDS.len()],
            ),
            KeyCode::Char(c @ '1'..='6') => self.switch(c as usize - '1' as usize),
            KeyCode::Char('r' | 'R') => self.action(Action::Refresh),
            KeyCode::Char('/') => self.action(Action::Search),
            KeyCode::Char('e' | 'E') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.action(Action::CycleEffectBack);
            }
            KeyCode::Char('e' | 'E') => self.action(Action::CycleEffect),
            KeyCode::Char('o' | 'O') => self.action(Action::CycleMode),
            KeyCode::Char('f' | 'F') => self.action(Action::Favorite),
            KeyCode::Char('a' | 'A') => self.action(Action::PlaylistAdd),
            KeyCode::Char('n' | 'N') if self.section == PLAYLISTS && self.detail.is_none() => {
                self.action(Action::PlaylistNew);
            }
            KeyCode::Char('c' | 'C') => self.action(Action::PlaylistRename),
            KeyCode::Delete | KeyCode::Char('d' | 'D') if key.kind == KeyEventKind::Press => {
                self.action(Action::PlaylistDelete);
            }
            KeyCode::Char(' ') => self.toggle_audio(),
            KeyCode::Char('s' | 'S') => self.action(Action::Stop),
            KeyCode::Char('[') => self.adjacent_track(-1),
            KeyCode::Char(']') => self.adjacent_track(1),
            KeyCode::Enter => self.action(Action::Select),
            _ => {}
        }
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.key(key),
            Event::Paste(text) if self.settings.is_none() && !self.help => {
                let value: String = text
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(4096)
                    .collect();
                if !self.connected && !self.busy && self.focus < 3 {
                    self.fields[self.focus].push_str(&value);
                } else if let Some(PlaylistDialog::Name { input, .. }) = &mut self.playlist_dialog {
                    input.push_str(&value);
                } else if self.editing {
                    self.search_input.push_str(&value);
                }
            }
            Event::Mouse(mouse)
                if (self.seek_drag.is_some() || self.volume_drag.is_some())
                    && matches!(
                        mouse.kind,
                        MouseEventKind::Drag(MouseButton::Left)
                            | MouseEventKind::Up(MouseButton::Left)
                    ) =>
            {
                let released = mouse.kind == MouseEventKind::Up(MouseButton::Left);
                if let Some(drag) = self.volume_drag.as_mut() {
                    let offset = i32::from(mouse.column) - i32::from(drag.column);
                    if offset != 0 || drag.preview.is_some() {
                        // Allow fine adjustments beyond the five visible cells.
                        drag.preview =
                            Some((i32::from(drag.initial) + offset * 5).clamp(0, 100) as u16);
                    }
                    if released {
                        let volume = drag.preview.unwrap_or(drag.click);
                        self.volume_drag = None;
                        self.set_volume(volume);
                    }
                } else if let Some(rect) = self.seek_rect {
                    let fraction = bar_fraction(rect, mouse.column);
                    self.seek_drag = Some(fraction);
                    if released {
                        self.seek_drag = None;
                        if let Some(player) = &mut self.player
                            && let Err(e) = player.seek(fraction)
                        {
                            self.show_seek_error(e);
                        }
                    }
                }
            }
            Event::Mouse(mouse) if !self.editing => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let action = self
                        .hits
                        .iter()
                        .rev()
                        .find(|(r, _)| r.contains((mouse.column, mouse.row).into()))
                        .map(|(_, a)| *a);
                    let now = Instant::now();
                    let double = self.last_click.take().is_some_and(|(x, y, time)| {
                        x == mouse.column
                            && y == mouse.row
                            && now.duration_since(time) <= Duration::from_millis(400)
                    });
                    if !double {
                        self.last_click = Some((mouse.column, mouse.row, now));
                    }
                    if let Some(action) = action {
                        if !matches!(action, Action::LyricLine(_)) || double {
                            self.action(action);
                        }
                        if double && matches!(action, Action::Row(_)) {
                            self.action(Action::Select);
                        }
                    }
                }
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                    if self.playlist_dialog.is_some() =>
                {
                    self.move_playlist_picker(if mouse.kind == MouseEventKind::ScrollDown {
                        1
                    } else {
                        -1
                    });
                }
                MouseEventKind::ScrollDown
                    if self.connected && self.settings.is_none() && !self.help =>
                {
                    if self.hits.iter().any(|(r, a)| {
                        matches!(a, Action::LyricsScroll(_) | Action::LyricLine(_))
                            && r.contains((mouse.column, mouse.row).into())
                    }) {
                        self.action(Action::LyricsScroll(3));
                    } else {
                        self.move_selection(3);
                    }
                }
                MouseEventKind::ScrollUp
                    if self.connected && self.settings.is_none() && !self.help =>
                {
                    if self.hits.iter().any(|(r, a)| {
                        matches!(a, Action::LyricsScroll(_) | Action::LyricLine(_))
                            && r.contains((mouse.column, mouse.row).into())
                    }) {
                        self.action(Action::LyricsScroll(-3));
                    } else {
                        self.move_selection(-3);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn bar_fraction(rect: Rect, column: u16) -> f64 {
    let span = rect.width.saturating_sub(1).max(1);
    f64::from(column.saturating_sub(rect.x).min(span)) / f64::from(span)
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

pub fn run(allow_http: bool) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("TUI 需要交互终端；命令行诊断请使用 --diagnose".into());
    }
    let mut app = App::new(allow_http);
    enable_raw_mode().map_err(|_| "无法启用终端原始模式")?;
    let _guard = TerminalGuard;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
    .map_err(|_| "无法初始化终端")?;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::Show
        );
        previous(info);
    }));
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|_| "终端初始化失败")?;
    let mut last_tick = Instant::now();
    while !app.quit {
        app.replies();
        app.checkpoint_location();
        app.refresh_media();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .map_err(|_| "终端绘制失败")?;
        if event::poll(Duration::from_millis(80)).map_err(|_| "终端事件读取失败")? {
            app.event(event::read().map_err(|_| "终端事件读取失败")?);
        }
        if last_tick.elapsed() >= Duration::from_millis(120) {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
        }
    }
    Ok(())
}
