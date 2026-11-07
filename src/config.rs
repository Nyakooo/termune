use crate::app::BrowseLocation;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write};
use termune::Result;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub fn_id: String,
    pub username: String,
    pub english: bool,
    pub theme: usize,
    pub layout: usize,
    #[serde(default)]
    pub layout_version: u8,
    pub professional: bool,
    pub auto_login: bool,
    pub volume: u16,
    pub audio_effect: usize,
    #[serde(default)]
    pub effect_version: Option<u8>,
    pub last_category: String,
    pub remember_location: bool,
    pub browse_locations: BTreeMap<String, BrowseLocation>,
    pub category_order: Vec<String>,
    pub playback_mode: crate::queue::Mode,
    #[serde(skip)]
    pub path_override: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fn_id: String::new(),
            username: String::new(),
            english: false,
            theme: 0,
            layout: 0,
            layout_version: 1,
            professional: true,
            auto_login: false,
            volume: 100,
            audio_effect: 0,
            effect_version: Some(1),
            last_category: "album".into(),
            remember_location: true,
            browse_locations: BTreeMap::new(),
            category_order: Vec::new(),
            playback_mode: crate::queue::Mode::default(),
            path_override: None,
        }
    }
}

impl Config {
    pub(crate) fn path() -> Result<std::path::PathBuf> {
        Ok(directories::ProjectDirs::from("", "", "termune")
            .ok_or("无法确定配置目录")?
            .config_dir()
            .join("config.toml"))
    }
    fn migrate_effects(&mut self) {
        if self.effect_version.is_none() {
            // v1 removed the old "headphone" preset at index 1.
            self.audio_effect = match self.audio_effect {
                1 | 6 => 5,
                index @ 2..=5 => index - 1,
                _ => 0,
            };
            self.effect_version = Some(1);
        }
    }
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err("读取 config.toml 失败".into()),
        };
        let mut config: Self = toml::from_str(&text).map_err(|_| "config.toml 格式错误")?;
        config.migrate_effects();
        config.audio_effect = crate::effects::Effect::from_index(config.audio_effect) as usize;
        config.theme %= 5;
        if config.layout_version == 0 {
            config.layout = match config.layout {
                0 | 1 => 0,
                n => n.saturating_sub(1).min(3),
            };
        }
        config.layout %= 4;
        config.layout_version = 1;
        Ok(config)
    }
    pub fn save(&self) -> Result<()> {
        let path = self
            .path_override
            .clone()
            .map(Ok)
            .unwrap_or_else(Self::path)?;
        let directory = path.parent().unwrap();
        std::fs::create_dir_all(directory).map_err(|_| "创建配置目录失败")?;
        let text = toml::to_string_pretty(self).map_err(|_| "配置编码失败")?;
        let mut staging =
            tempfile::NamedTempFile::new_in(directory).map_err(|_| "保存 config.toml 失败")?;
        staging
            .write_all(text.as_bytes())
            .map_err(|_| "保存 config.toml 失败")?;
        staging.persist(path).map_err(|_| "保存 config.toml 失败")?;
        Ok(())
    }
    pub fn text<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        if self.english { en } else { zh }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    #[test]
    fn legacy_effect_indices_migrate_once() {
        let mut config: Config = toml::from_str("audio_effect = 6").unwrap();
        assert!(config.effect_version.is_none());
        config.migrate_effects();
        assert_eq!(config.audio_effect, 5);
        assert_eq!(config.effect_version, Some(1));
        config.audio_effect = 4;
        config.migrate_effects();
        assert_eq!(config.audio_effect, 4);
    }
}
