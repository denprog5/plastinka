//! Общее состояние: текущий трек (для трея), обложка и подписчики страницы OBS.
//! Поток плеера кладёт сюда новое состояние, сервер раздаёт его по SSE.
//! Тема карточки идёт отдельным событием `{"theme":"…"}`: новая страница получает её первой.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Что сейчас играет. `None` в `Hub::now` — подходящего плеера нет.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Позиция на момент отправки, мс.
    pub pos_ms: u64,
    /// Длительность, мс; 0 — плеер её не сообщает.
    pub dur_ms: u64,
    /// Версия обложки: меняется вместе с картинкой; 0 — обложки нет.
    pub cover: u64,
}

impl Track {
    /// Событие для страницы (одна строка JSON).
    pub fn to_json(t: Option<&Track>) -> String {
        match t {
            None => r#"{"on":false}"#.to_string(),
            Some(t) => serde_json::json!({
                "on": true,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "playing": t.playing,
                "pos": t.pos_ms,
                "dur": t.dur_ms,
                "cover": t.cover,
            })
            .to_string(),
        }
    }
}

pub struct Cover {
    pub ver: u64,
    pub mime: String,
    pub bytes: Arc<[u8]>,
}

#[derive(Default)]
pub struct Hub {
    clients: Mutex<Vec<Sender<Arc<str>>>>,
    /// Последнее событие — новая страница получает его сразу при подключении.
    last: Mutex<Option<Arc<str>>>,
    now: Mutex<Option<Track>>,
    cover: Mutex<Option<Cover>>,
    /// Событие с темой карточки, выбранной в трее.
    theme: Mutex<Option<Arc<str>>>,
    /// Найденная новая версия программы.
    update: Mutex<Option<String>>,
}

pub type SharedHub = Arc<Hub>;

impl Hub {
    pub fn subscribe(&self) -> Receiver<Arc<str>> {
        let (tx, rx) = mpsc::channel();
        let msg = self
            .last
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Track::to_json(None).into());
        if let Some(theme) = self.theme.lock().unwrap().clone() {
            let _ = tx.send(theme);
        }
        let _ = tx.send(msg);
        self.clients.lock().unwrap().push(tx);
        rx
    }

    pub fn publish(&self, now: Option<Track>) {
        let msg: Arc<str> = Track::to_json(now.as_ref()).into();
        *self.now.lock().unwrap() = now;
        *self.last.lock().unwrap() = Some(msg.clone());
        self.broadcast(msg);
    }

    /// Тема карточки: запоминается для новых страниц и сразу уходит открытым.
    pub fn set_theme(&self, theme: &str) {
        let msg: Arc<str> = serde_json::json!({ "theme": theme }).to_string().into();
        *self.theme.lock().unwrap() = Some(msg.clone());
        self.broadcast(msg);
    }

    fn broadcast(&self, msg: Arc<str>) {
        // Отключившиеся страницы отваливаются здесь: их приёмник уже уничтожен.
        self.clients
            .lock()
            .unwrap()
            .retain(|tx| tx.send(msg.clone()).is_ok());
    }

    pub fn now(&self) -> Option<Track> {
        self.now.lock().unwrap().clone()
    }

    pub fn set_cover(&self, cover: Option<Cover>) {
        *self.cover.lock().unwrap() = cover;
    }

    pub fn cover(&self) -> Option<(String, Arc<[u8]>)> {
        self.cover
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| (c.mime.clone(), c.bytes.clone()))
    }

    pub fn set_update(&self, v: Option<String>) {
        *self.update.lock().unwrap() = v;
    }

    pub fn update(&self) -> Option<String> {
        self.update.lock().unwrap().clone()
    }

    pub fn cover_ver(&self) -> u64 {
        self.cover.lock().unwrap().as_ref().map_or(0, |c| c.ver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_reaches_open_and_new_pages() {
        let hub = Hub::default();
        let open = hub.subscribe();
        assert_eq!(&*open.recv().unwrap(), r#"{"on":false}"#);
        hub.set_theme("imperium");
        assert_eq!(&*open.recv().unwrap(), r#"{"theme":"imperium"}"#);
        // Новая страница: сначала тема, потом трек.
        let new = hub.subscribe();
        assert_eq!(&*new.recv().unwrap(), r#"{"theme":"imperium"}"#);
        assert_eq!(&*new.recv().unwrap(), r#"{"on":false}"#);
    }
}
