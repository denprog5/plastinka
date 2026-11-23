//! Настройки в `%APPDATA%\Plastinka\config.json`. Правится из меню трея или руками;
//! ошибка разбора не перетирает файл.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Темы карточки: имя для страницы (`data-theme`) и пункт меню трея. Первая — по умолчанию.
pub const THEMES: &[(&str, &str)] = &[
    ("glitch", "Глитч"),
    ("clean", "Глитч без полосы сверху"),
    ("imperium", "Империум"),
    ("neon", "Неон"),
    ("terminal", "Терминал"),
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    /// Порт локального сервера страницы для OBS (слушаем только 127.0.0.1).
    pub port: u16,
    /// Показывать любой плеер Windows (Яндекс Музыка, браузер…), а не только Spotify.
    pub any_player: bool,
    /// Ставить обновление, найденное сразу после запуска (найденное позже — только пункт меню).
    pub auto_update: bool,
    /// Тема карточки (см. `THEMES`). Параметр `?theme=` в адресе источника важнее.
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: 7720,
            any_player: false,
            auto_update: true,
            theme: THEMES[0].0.to_string(),
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
            Ok(mut c) => {
                c.normalize();
                (c, None)
            }
            Err(e) => (
                Config::default(),
                Some(format!("Ошибка в {}: {e}", path.display())),
            ),
        }
    }

    /// Незнакомая тема (опечатка в файле, тема из более новой версии) — тема по умолчанию.
    fn normalize(&mut self) {
        if !THEMES.iter().any(|(id, _)| *id == self.theme) {
            self.theme = THEMES[0].0.to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_theme_falls_back_to_default() {
        let mut c: Config = serde_json::from_str(r#"{"theme":"pink"}"#).unwrap();
        c.normalize();
        assert_eq!(c.theme, "glitch");
        let mut c: Config = serde_json::from_str(r#"{"theme":"neon"}"#).unwrap();
        c.normalize();
        assert_eq!(c.theme, "neon");
        // Старый config.json без поля theme.
        let c: Config = serde_json::from_str(r#"{"port":7720}"#).unwrap();
        assert_eq!(c.theme, "glitch");
    }
}
