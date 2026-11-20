//! Настройки в `%APPDATA%\Plastinka\config.json`. Правится из меню трея или руками;
//! ошибка разбора не перетирает файл.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    /// Порт локального сервера страницы для OBS (слушаем только 127.0.0.1).
    pub port: u16,
    /// Показывать любой плеер Windows (Яндекс Музыка, браузер…), а не только Spotify.
    pub any_player: bool,
    /// Ставить обновление, найденное сразу после запуска (найденное позже — только пункт меню).
    pub auto_update: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: 7720,
            any_player: false,
            auto_update: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        app_dir().join("config.json")
    }

    /// `(config, ошибка)`; при ошибке — настройки по умолчанию.
    pub fn load() -> (Config, Option<String>) {
        let path = Self::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Config::default(), None);
        };
        match serde_json::from_str::<Config>(&text) {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!("Ошибка в {}: {e}", path.display())),
            ),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        let _ = std::fs::create_dir_all(app_dir());
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default()
}

pub fn app_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Plastinka")
}
