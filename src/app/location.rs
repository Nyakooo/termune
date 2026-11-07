use super::{App, Detail, KINDS, PLAYLISTS, View};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct BrowseLocation {
    category: String,
    page: u64,
    detail: Option<Detail>,
    query: Option<String>,
    selection: Option<BrowseSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct BrowseSelection {
    row: usize,
    guid: Option<String>,
}

impl BrowseSelection {
    pub(super) fn apply(&self, view: &mut View) {
        let row = self
            .guid
            .as_deref()
            .and_then(|guid| {
                view.items
                    .iter()
                    .position(|item| item["guid"].as_str() == Some(guid))
            })
            .or_else(|| {
                view.items
                    .len()
                    .checked_sub(1)
                    .map(|last| self.row.min(last))
            });
        view.table.select(row);
    }
}

impl App {
    pub(super) fn checkpoint_location(&mut self) {
        if !self.connected || !self.config.remember_location || self.busy || !self.view().loaded {
            return;
        }
        let view = self.view();
        let location = BrowseLocation {
            category: KINDS[self.section].into(),
            page: view.page.max(1),
            detail: self.detail.clone(),
            query: self.query.clone(),
            selection: view.table.selected().and_then(|row| {
                let item = view.items.get(row)?;
                Some(BrowseSelection {
                    row,
                    guid: item["guid"].as_str().map(str::to_owned),
                })
            }),
        };
        let key = termune::credentials::account_key(&self.config.fn_id, &self.config.username);
        if self.config.browse_locations.get(&key) != Some(&location) {
            self.config.browse_locations.insert(key, location);
            if let Err(error) = self.config.save() {
                self.show_notice(error);
            }
        }
    }

    pub(super) fn restore_location(&mut self) {
        if !self.config.remember_location {
            self.section = 0;
            self.load(1);
            return;
        }
        let key = termune::credentials::account_key(&self.config.fn_id, &self.config.username);
        let Some(location) = self.config.browse_locations.get(&key).cloned() else {
            self.load(1);
            return;
        };
        let Some(section) = KINDS.iter().position(|kind| *kind == location.category) else {
            self.load(1);
            return;
        };
        self.section = section;
        self.detail = location.detail;
        self.query = location.query;
        let mut page = location.page.max(1);
        if section == PLAYLISTS {
            if self.detail.is_some() && self.open_playlist().is_none() {
                self.detail = None;
                self.load(1);
                return;
            }
            // A local song or folder can be found even if edits moved it onto another page.
            if let Some(guid) = location
                .selection
                .as_ref()
                .and_then(|selection| selection.guid.as_deref())
            {
                let index = if let Some(playlist) = self.open_playlist() {
                    playlist
                        .tracks
                        .iter()
                        .position(|track| track["guid"].as_str() == Some(guid))
                } else {
                    self.playlists
                        .iter()
                        .position(|playlist| playlist.id == guid)
                };
                if let Some(index) = index {
                    page = (index / 50 + 1) as u64;
                }
            }
        }
        self.load_with_selection(page, location.selection);
    }
}
