//! Поток плеера: читает текущий трек из системных медиа-контролов Windows (GSMTC) —
//! тех же, что показывают трек в оверлее громкости. Spotify публикует туда название,
//! исполнителя, обложку, статус и позицию. Никакого опроса: поток спит в `park()`,
//! его будят события Windows (смена трека, пауза, перемотка, запуск/закрытие плеера).

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::core::HSTRING;

use crate::hub::{Cover, SharedHub, Track};
use crate::win;

/// Пачку событий (Spotify при смене трека шлёт несколько подряд) собираем в одно обновление.
const SETTLE: Duration = Duration::from_millis(200);
/// Позиция, расходящаяся с ожидаемой больше чем на столько, — это перемотка: шлём заново.
const SEEK_TOLERANCE_MS: i64 = 1500;

pub struct Media {
    thread: Thread,
    any_player: Arc<AtomicBool>,
}

impl Media {
    pub fn spawn(any_player: bool, hub: SharedHub) -> Media {
        let any = Arc::new(AtomicBool::new(any_player));
        let any2 = any.clone();
        let h = thread::Builder::new()
            .name("media".into())
            .stack_size(512 * 1024)
            .spawn(move || run(any2, hub))
            .expect("media thread");
        Media {
            thread: h.thread().clone(),
            any_player: any,
        }
    }

    pub fn set_any_player(&self, on: bool) {
        self.any_player.store(on, Ordering::Release);
        self.thread.unpark();
    }
}

/// Подписки на события выбранного плеера; снимаются при смене плеера.
struct Watched {
    session: Session,
    aumid: String,
    tokens: [i64; 3],
}

impl Drop for Watched {
    fn drop(&mut self) {
        let s = &self.session;
        let _ = s.RemoveMediaPropertiesChanged(self.tokens[0]);
        let _ = s.RemovePlaybackInfoChanged(self.tokens[1]);
        let _ = s.RemoveTimelinePropertiesChanged(self.tokens[2]);
    }
}

fn run(any_player: Arc<AtomicBool>, hub: SharedHub) {
    unsafe {
        use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
    }
    let me = thread::current();
    // Медиа-контролы есть с Windows 10 1809; если менеджер не получен — пробуем изредка.
    let mgr = loop {
        match Manager::RequestAsync().and_then(|op| op.join()) {
            Ok(m) => break m,
            Err(_) => thread::park_timeout(Duration::from_secs(60)),
        }
    };
    let wake = {
        let me = me.clone();
        move || me.unpark()
    };
    {
        let w = wake.clone();
        let _ = mgr.SessionsChanged(&TypedEventHandler::new(move |_, _| {
            w();
            Ok(())
        }));
        let w = wake.clone();
        let _ = mgr.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            w();
            Ok(())
        }));
    }
    // Свойства (название, обложка) перечитываем только по их событию — остальное дёшево.
    let props_dirty = Arc::new(AtomicBool::new(true));

    let mut watched: Option<Watched> = None;
    let mut props = Props::default();
    let mut sent: Option<(Track, Instant)> = None;
    let mut published_once = false;

    loop {
        let any = any_player.load(Ordering::Acquire);
        let pick = choose(&mgr, any);
        let pick_id = pick.as_ref().map(|(_, id)| id.clone());
        if watched.as_ref().map(|w| &w.aumid) != pick_id.as_ref() {
            watched = pick.and_then(|(s, id)| watch(s, id, &wake, &props_dirty));
            props_dirty.store(true, Ordering::Release);
        }

        let track = watched.as_ref().and_then(|w| {
            if props_dirty.swap(false, Ordering::AcqRel) {
                props = read_props(&w.session, &hub, &props);
            }
            snapshot(&w.session, &props)
        });

        if !published_once || differs(sent.as_ref(), track.as_ref()) {
            published_once = true;
            sent = track.clone().map(|t| (t, Instant::now()));
            hub.publish(track);
            win::notify_main(win::EV_REFRESH);
        }

        thread::park();
        thread::sleep(SETTLE);
    }
}

/// Spotify — по имени приложения; в режиме «любой плеер» — текущий плеер Windows,
/// а если он стоит на паузе, то тот, что играет.
fn choose(mgr: &Manager, any: bool) -> Option<(Session, String)> {
    let sessions: Vec<(Session, String)> = mgr
        .GetSessions()
        .ok()?
        .into_iter()
        .filter_map(|s| {
            let id = s.SourceAppUserModelId().ok()?.to_string();
            Some((s, id))
        })
        .collect();
    if !any {
        return sessions
            .into_iter()
            .find(|(_, id)| id.to_lowercase().contains("spotify"));
    }
    let playing = |s: &Session| {
        s.GetPlaybackInfo()
            .and_then(|p| p.PlaybackStatus())
            .is_ok_and(|st| st == Status::Playing)
    };
    let current = mgr
        .GetCurrentSession()
        .ok()
        .and_then(|s| s.SourceAppUserModelId().ok().map(|id| (s, id.to_string())));
    match current {
        Some(c) if playing(&c.0) => Some(c),
        current => sessions.into_iter().find(|(s, _)| playing(s)).or(current),
    }
}

fn watch(
    s: Session,
    aumid: String,
    wake: &(impl Fn() + Clone + Send + Sync + 'static),
    props_dirty: &Arc<AtomicBool>,
) -> Option<Watched> {
    let (w, d) = (wake.clone(), props_dirty.clone());
    let t0 = s
        .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            d.store(true, Ordering::Release);
            w();
            Ok(())
        }))
        .ok()?;
    let w = wake.clone();
    let t1 = s
        .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            w();
            Ok(())
        }))
        .unwrap_or_default();
    let w = wake.clone();
    let t2 = s
        .TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
            w();
            Ok(())
        }))
        .unwrap_or_default();
    Some(Watched {
        session: s,
        aumid,
        tokens: [t0, t1, t2],
    })
}

#[derive(Default, Clone)]
struct Props {
    title: String,
    artist: String,
    album: String,
    cover_hash: u64,
    cover_ver: u64,
}

fn read_props(s: &Session, hub: &SharedHub, prev: &Props) -> Props {
    let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()) else {
        return prev.clone();
    };
    let text = |r: windows::core::Result<HSTRING>| r.map(|h| h.to_string()).unwrap_or_default();
    let mut out = Props {
        title: text(p.Title()),
        artist: text(p.Artist()),
        album: text(p.AlbumTitle()),
        cover_hash: prev.cover_hash,
        cover_ver: prev.cover_ver,
    };
    match p.Thumbnail().ok().and_then(|t| read_thumbnail(&t)) {
        Some((mime, bytes)) => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut h);
            let hash = h.finish();
            if hash != prev.cover_hash {
                out.cover_hash = hash;
                out.cover_ver = hub.cover_ver().max(prev.cover_ver) + 1;
                hub.set_cover(Some(Cover {
                    ver: out.cover_ver,
                    mime,
                    bytes: bytes.into(),
                }));
            }
        }
        None => {
            out.cover_hash = 0;
            out.cover_ver = 0;
        }
    }
    out
}

fn read_thumbnail(
    t: &windows::Storage::Streams::IRandomAccessStreamReference,
) -> Option<(String, Vec<u8>)> {
    let stream = t.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 << 20 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let n = reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; n as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = stream
        .ContentType()
        .map(|h| h.to_string())
        .ok()
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| sniff_mime(&buf).to_string());
    Some((mime, buf))
}

fn sniff_mime(b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

fn snapshot(s: &Session, p: &Props) -> Option<Track> {
    if p.title.is_empty() {
        return None;
    }
    let playing = s
        .GetPlaybackInfo()
        .and_then(|i| i.PlaybackStatus())
        .is_ok_and(|st| st == Status::Playing);
    let (mut pos_ms, mut dur_ms) = (0u64, 0u64);
    if let Ok(tl) = s.GetTimelineProperties() {
        let ticks = |r: windows::core::Result<windows::Foundation::TimeSpan>| {
            r.map(|t| t.Duration).unwrap_or(0)
        };
        let start = ticks(tl.StartTime());
        let end = ticks(tl.EndTime());
        let mut pos = ticks(tl.Position()) - start;
        // Позиция — на момент LastUpdatedTime; пока играет, она с тех пор ушла вперёд.
        if playing && let Ok(upd) = tl.LastUpdatedTime() {
            let since = now_filetime() - upd.UniversalTime;
            if since > 0 {
                pos += since;
            }
        }
        if end > start {
            dur_ms = ((end - start) / 10_000) as u64;
            pos_ms = (pos.max(0) / 10_000) as u64;
            pos_ms = pos_ms.min(dur_ms);
        }
    }
    Some(Track {
        title: p.title.clone(),
        artist: p.artist.clone(),
        album: p.album.clone(),
        playing,
        pos_ms,
        dur_ms,
        cover: p.cover_ver,
    })
}

/// Текущее время в единицах `DateTime` WinRT: 100 нс с 1601-01-01 UTC.
fn now_filetime() -> i64 {
    use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
    let mut ft = unsafe { std::mem::zeroed() };
    unsafe { GetSystemTimePreciseAsFileTime(&mut ft) };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

/// Нужно ли слать новое состояние: поменялось что-то кроме позиции, или позиция
/// разошлась с той, что страница насчитала сама (перемотка).
fn differs(sent: Option<&(Track, Instant)>, now: Option<&Track>) -> bool {
    match (sent, now) {
        (None, None) => false,
        (Some((a, at)), Some(b)) => {
            let same = Track {
                pos_ms: b.pos_ms,
                ..a.clone()
            } == *b;
            if !same {
                return true;
            }
            let expected = if a.playing {
                a.pos_ms + at.elapsed().as_millis() as u64
            } else {
                a.pos_ms
            };
            (b.pos_ms as i64 - expected as i64).abs() > SEEK_TOLERANCE_MS
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(pos: u64, playing: bool) -> Track {
        Track {
            title: "a".into(),
            playing,
            pos_ms: pos,
            dur_ms: 200_000,
            ..Default::default()
        }
    }

    #[test]
    fn position_drift_within_tolerance_is_not_resent() {
        let sent = (t(10_000, true), Instant::now());
        assert!(!differs(Some(&sent), Some(&t(10_500, true))));
        assert!(differs(Some(&sent), Some(&t(60_000, true))));
        assert!(differs(Some(&sent), Some(&t(10_000, false))));
        assert!(differs(Some(&sent), None));
        assert!(!differs(None, None));
    }
}
