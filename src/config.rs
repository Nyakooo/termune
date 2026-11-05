use serde::{Deserialize, Serialize};
use termune::Result;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub fn_id: String,
    pub username: String,
    pub english: bool,
    pub theme: usize,
    pub layout: usize,
    pub professional: bool,
    pub auto_login: bool,
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
            professional: true,
            auto_login: false,
            playback_mode: crate::queue::Mode::default(),
            path_override: None,
        }
    }
}

impl Config {
    fn path() -> Result<std::path::PathBuf> {
        Ok(directories::ProjectDirs::from("", "", "termune")
            .ok_or("无法确定配置目录")?
            .config_dir()
            .join("config.toml"))
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
        config.theme %= 5;
        config.layout %= 5;
        Ok(config)
    }
    pub fn save(&self) -> Result<()> {
        let path = self
            .path_override
            .clone()
            .map(Ok)
            .unwrap_or_else(Self::path)?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "创建配置目录失败")?;
        let text = toml::to_string_pretty(self).map_err(|_| "配置编码失败")?;
        std::fs::write(path, text).map_err(|_| "保存 config.toml 失败".into())
    }
    pub fn text<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        if self.english { en } else { zh }
    }
}
