use super::{App, Caret, Config, PLAYLISTS, View, clean, name};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::{ListState, TableState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use termune::Result;

#[derive(Deserialize, Serialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Value>,
}

pub enum PlaylistDialog {
    Name {
        id: Option<String>,
        track: Option<Value>,
        input: String,
        caret: Caret,
    },
    Pick {
        track: Value,
        cursor: ListState,
    },
    Delete {
        id: String,
        track_guid: Option<String>,
        name: String,
    },
}

fn read(path: &Path) -> Result<Vec<Playlist>> {
    match fs::File::open(path) {
        Ok(file) => serde_json::from_reader(io::BufReader::new(file))
            .map_err(|error| format!("歌单数据无法读取 / Invalid playlist data ({error})")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取歌单失败 / Could not read playlists ({error})")),
    }
}

fn track_snapshot(item: &Value) -> Option<Value> {
    let guid = item["guid"].as_str().filter(|guid| !guid.is_empty())?;
    Some(json!({
        "guid": guid,
        "title": name(item, "title"),
        "artists": item["artists"].as_array().map(|artists| {
            artists.iter().map(|artist| json!({"name": name(artist, "name")})).collect::<Vec<_>>()
        }),
        "album": {"name": name(&item["album"], "name")},
        "audioSpec": {"format": name(&item["audioSpec"], "format")},
    }))
}

impl App {
    fn playlist_path(&self) -> Result<PathBuf> {
        let config = self
            .config
            .path_override
            .clone()
            .map_or_else(Config::path, Ok)?;
        let key = termune::credentials::account_key(
            self.config.connection_target(),
            &self.config.username,
        );
        Ok(config
            .with_extension("playlists")
            .join(format!("{key}.json")))
    }

    pub(super) fn read_playlists(&mut self) -> Result<()> {
        self.playlists = read(&self.playlist_path()?)?;
        self.refresh_playlist_index();
        Ok(())
    }

    fn update_playlists(
        &mut self,
        edit: impl FnOnce(&mut Vec<Playlist>) -> Result<()>,
    ) -> Result<()> {
        let path = self.playlist_path()?;
        let directory = path.parent().expect("playlist directory");
        let storage_error =
            |error: io::Error| format!("保存歌单失败 / Could not save playlists ({error})");
        fs::create_dir_all(directory).map_err(storage_error)?;
        // Read under a separate lock so replacing the data file preserves the lock.
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(storage_error)?;
        lock.lock().map_err(storage_error)?;
        let mut playlists = read(&path)?;
        edit(&mut playlists)?;
        let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(storage_error)?;
        serde_json::to_writer(temp.as_file_mut(), &playlists)
            .map_err(|error| format!("编码歌单失败 / Could not encode playlists ({error})"))?;
        temp.as_file().sync_all().map_err(storage_error)?;
        temp.persist(path)
            .map_err(|error| storage_error(error.error))?;
        self.playlists = playlists;
        self.refresh_playlist_index();
        if self.section == PLAYLISTS && self.detail.is_some() {
            let view = &self.detail_view;
            self.load_playlist_page(view.page.max(1), view.table.selected().unwrap_or(0));
        }
        Ok(())
    }

    pub(super) fn open_playlist(&self) -> Option<&Playlist> {
        let detail = self.detail.as_ref().filter(|_| self.section == PLAYLISTS)?;
        self.playlists
            .iter()
            .find(|playlist| playlist.id == detail.guid)
    }

    fn refresh_playlist_index(&mut self) {
        let index = &self.views[PLAYLISTS];
        self.load_playlist_index(index.page.max(1), index.table.selected().unwrap_or(0));
    }

    fn load_playlist_index(&mut self, page: u64, row: usize) {
        let rows = self.playlists.iter().map(|playlist| {
            json!({"guid": playlist.id, "name": playlist.name, "trackCount": playlist.tracks.len()})
        });
        self.views[PLAYLISTS] = playlist_page(rows, self.playlists.len(), page, row);
    }

    pub(super) fn load_playlist_page(&mut self, page: u64, row: usize) {
        if let Some(playlist) = self.open_playlist() {
            let title = playlist.name.clone();
            self.detail_view = playlist_page(
                playlist.tracks.iter().cloned(),
                playlist.tracks.len(),
                page,
                row,
            );
            if let Some(detail) = &mut self.detail {
                detail.name = title;
            }
        } else {
            self.detail = None;
            self.load_playlist_index(page, row);
        }
        self.clear_status();
    }

    fn selected_playlist(&self) -> Option<(String, String)> {
        let index = &self.views[PLAYLISTS];
        let item = index.items.get(index.table.selected()?)?;
        Some((item["guid"].as_str()?.to_owned(), name(item, "name")))
    }

    pub(super) fn new_playlist(&mut self, track: Option<Value>) {
        self.clear_status();
        self.playlist_dialog = Some(PlaylistDialog::Name {
            id: None,
            track,
            input: String::new(),
            caret: Caret::default(),
        });
    }

    pub(super) fn rename_playlist(&mut self) {
        if self.section == PLAYLISTS
            && self.detail.is_none()
            && let Some((id, input)) = self.selected_playlist()
        {
            self.playlist_dialog = Some(PlaylistDialog::Name {
                id: Some(id),
                track: None,
                input,
                caret: Caret::default(),
            });
        }
    }

    pub(super) fn delete_playlist_entry(&mut self) {
        if self.section != PLAYLISTS {
            return;
        }
        let dialog = if let Some(detail) = &self.detail {
            let view = self.view();
            let Some(item) = view.table.selected().and_then(|i| view.items.get(i)) else {
                return;
            };
            let Some(guid) = item["guid"].as_str() else {
                return;
            };
            PlaylistDialog::Delete {
                id: detail.guid.clone(),
                track_guid: Some(guid.into()),
                name: name(item, "title"),
            }
        } else {
            let Some((id, name)) = self.selected_playlist() else {
                return;
            };
            PlaylistDialog::Delete {
                id,
                track_guid: None,
                name,
            }
        };
        self.clear_status();
        self.playlist_dialog = Some(dialog);
    }

    pub(super) fn add_to_playlist(&mut self) {
        if self.busy || !self.track_view() {
            return;
        }
        let Some(track) = self
            .view()
            .table
            .selected()
            .and_then(|i| self.view().items.get(i))
            .and_then(track_snapshot)
        else {
            return;
        };
        if self.playlists.is_empty() {
            self.new_playlist(Some(track));
        } else {
            self.clear_status();
            self.playlist_dialog = Some(PlaylistDialog::Pick {
                track,
                cursor: ListState::default().with_selected(Some(0)),
            });
        }
    }

    pub(super) fn pick_playlist(&mut self, index: usize) {
        if let Some(PlaylistDialog::Pick { cursor, .. }) = &mut self.playlist_dialog {
            cursor.select(Some(index));
            self.confirm_playlist();
        }
    }

    pub(super) fn confirm_playlist(&mut self) {
        let Some(dialog) = &self.playlist_dialog else {
            return;
        };
        let (result, zh, en) = match dialog {
            PlaylistDialog::Name {
                id, track, input, ..
            } => {
                let title = clean(input).trim().to_owned();
                if title.is_empty() {
                    self.fail(self.text("请输入歌单名称", "Enter a playlist name").into());
                    return;
                }
                let id = id.clone();
                let track = track.clone();
                let adding = track.is_some();
                let creating = id.is_none();
                let result = self.update_playlists(|playlists| {
                    if playlists
                        .iter()
                        .any(|playlist| playlist.name == title && Some(&playlist.id) != id.as_ref())
                    {
                        return Err("已存在同名歌单 / Playlist name already exists".into());
                    }
                    if let Some(id) = id {
                        find(playlists, &id)?.name = title;
                    } else {
                        playlists.push(Playlist {
                            id: termune::random_id()?,
                            name: title,
                            tracks: track.into_iter().collect(),
                        });
                    }
                    Ok(())
                });
                if result.is_ok() && creating {
                    let index = self.playlists.len() - 1;
                    // Keep the new folder selected on the playlist tab.
                    self.load_playlist_index((index / 50 + 1) as u64, index % 50);
                }
                let message = match (creating, adding) {
                    (_, true) => ("已创建歌单并添加歌曲", "Created playlist and added track"),
                    (true, false) => ("已创建歌单", "Created playlist"),
                    (false, false) => ("已重命名歌单", "Renamed playlist"),
                };
                (result, message.0, message.1)
            }
            PlaylistDialog::Pick { track, cursor } => {
                let Some(playlist) = cursor
                    .selected()
                    .and_then(|index| self.playlists.get(index))
                else {
                    return;
                };
                let id = playlist.id.clone();
                let track = track.clone();
                let result = self.update_playlists(|playlists| {
                    let playlist = find(playlists, &id)?;
                    if playlist
                        .tracks
                        .iter()
                        .any(|item| item["guid"] == track["guid"])
                    {
                        return Err("歌曲已在该歌单中 / Track is already in this playlist".into());
                    }
                    playlist.tracks.push(track);
                    Ok(())
                });
                (result, "已加入歌单", "Added track to playlist")
            }
            PlaylistDialog::Delete { id, track_guid, .. } => {
                let id = id.clone();
                let track_guid = track_guid.clone();
                let result = self.update_playlists(|playlists| {
                    if let Some(guid) = &track_guid {
                        let playlist = find(playlists, &id)?;
                        playlist
                            .tracks
                            .retain(|track| track["guid"].as_str() != Some(guid));
                    } else {
                        playlists.retain(|playlist| playlist.id != id);
                    }
                    Ok(())
                });
                if track_guid.is_some() {
                    (result, "已从歌单移除歌曲", "Removed track from playlist")
                } else {
                    (result, "已删除歌单", "Deleted playlist")
                }
            }
        };
        if result.is_ok() {
            self.playlist_dialog = None;
        }
        self.playlist_result(result, zh, en);
    }

    fn playlist_result(&mut self, result: Result<()>, zh: &str, en: &str) {
        match result {
            Ok(()) => {
                self.show_status(self.text(zh, en).into(), false);
            }
            Err(error) => self.fail(error),
        }
    }

    pub(super) fn move_playlist_picker(&mut self, delta: isize) {
        if let Some(PlaylistDialog::Pick { cursor, .. }) = &mut self.playlist_dialog {
            let index = cursor.selected().unwrap_or(0).saturating_add_signed(delta);
            cursor.select(Some(index.min(self.playlists.len().saturating_sub(1))));
        }
    }

    pub(super) fn playlist_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.playlist_dialog = None;
            self.clear_status();
            return;
        }
        if key.code == KeyCode::Enter {
            if key.kind == crossterm::event::KeyEventKind::Press {
                self.confirm_playlist();
            }
            return;
        }
        match &mut self.playlist_dialog {
            Some(PlaylistDialog::Name { input, caret, .. }) => caret.key(input, key),
            Some(PlaylistDialog::Pick { .. }) => match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.move_playlist_picker(-1),
                KeyCode::Down | KeyCode::Char('j') => self.move_playlist_picker(1),
                _ => {}
            },
            _ => {}
        }
    }
}

fn find<'a>(playlists: &'a mut [Playlist], id: &str) -> Result<&'a mut Playlist> {
    playlists
        .iter_mut()
        .find(|playlist| playlist.id == id)
        .ok_or_else(|| {
            "歌单已不存在，请刷新 / Playlist no longer exists. Refresh to continue.".into()
        })
}

fn playlist_page(items: impl Iterator<Item = Value>, total: usize, page: u64, row: usize) -> View {
    let page = page.min((total as u64).div_ceil(50).max(1));
    let items: Vec<_> = items.skip((page as usize - 1) * 50).take(50).collect();
    let selected = items.len().checked_sub(1).map(|last| row.min(last));
    View {
        items,
        total: total as u64,
        page,
        loaded: true,
        table: TableState::default().with_selected(selected),
    }
}
