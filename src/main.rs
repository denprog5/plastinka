#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Пластинка — «сейчас играет» для стрима: трек из Spotify (медиа-контролы Windows) →
//! локальная страница → источник «Браузер» в OBS. Живёт в трее.

mod config;
mod hub;
mod media;
mod server;
mod update;
mod win;

use std::sync::Arc;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetSystemMetrics, MSG, SM_CXSMICON, TranslateMessage,
};

use config::Config;
use hub::{Hub, SharedHub, Track};
use media::Media;
use update::Updater;

/// Размер источника «Браузер» в OBS, под который свёрстана страница.
const OBS_SIZE: &str = "640 × 200";

struct App {
    cfg: Config,
    hub: SharedHub,
    media: Media,
    updater: Updater,
    server_error: Option<String>,
    tray: TrayIcon,
    now: MenuItem,
    copy_url: MenuItem,
    open: MenuItem,
    any_player: CheckMenuItem,
    autostart: CheckMenuItem,
    update: MenuItem,
    quit: MenuItem,
    shown_playing: Option<bool>,
    shown_tooltip: String,
}

fn main() {
    if !win::acquire_single_instance() {
        win::info_box("Пластинка уже запущена — её иконка в трее.");
        return;
    }
    win::register_main_thread();
    win::make_process_humble();

    let first_run = !Config::path().exists();
    let (cfg, cfg_error) = Config::load();
    if let Some(e) = cfg_error {
        win::error_box(&format!("{e}\n\nИспользуются настройки по умолчанию."));
    }

    let hub: SharedHub = Arc::new(Hub::default());
    let server_error = server::spawn(cfg.port, hub.clone()).err();
    let media = Media::spawn(cfg.any_player, hub.clone());
    let updater = Updater::spawn(cfg.auto_update, hub.clone());

    // События мыши по иконке не нужны; без обработчика они копились бы в канале.
    TrayIconEvent::set_event_handler(Some(|_| {}));

    let now = MenuItem::new("Запуск…", false, None);
    let copy_url = MenuItem::new("Скопировать ссылку для OBS", true, None);
    let open = MenuItem::new("Открыть плеер в браузере", true, None);
    let any_player =
        CheckMenuItem::new("Любой плеер, не только Spotify", true, cfg.any_player, None);
    let autostart = CheckMenuItem::new(
        "Запускать вместе с Windows",
        true,
        win::autostart_enabled(),
        None,
    );
    let update = MenuItem::new(update_text(None), true, None);
    let quit = MenuItem::new("Выход", true, None);
    let menu = Menu::new();
    let sep = PredefinedMenuItem::separator;
    let _ = menu.append_items(&[
        &now,
        &sep(),
        &copy_url,
        &open,
        &sep(),
        &any_player,
        &autostart,
        &update,
        &quit,
    ]);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(win::APP_NAME)
        .with_icon(tray_icon(false))
        .build()
        .expect("tray icon");

    let mut app = App {
        cfg,
        hub,
        media,
        updater,
        server_error,
        tray,
        now,
        copy_url,
        open,
        any_player,
        autostart,
        update,
        quit,
        shown_playing: Some(false),
        shown_tooltip: String::new(),
    };
    app.update();

    if let Some(e) = &app.server_error {
        win::error_box(e);
    } else if first_run {
        app.cfg.save();
        win::copy_text(&app.cfg.url());
        win::info_box(&format!(
            "Пластинка работает — её иконка в трее.\n\n\
             Ссылка для OBS уже скопирована:\n{}\n\n\
             В OBS: «Источники» → «+» → «Браузер», вставьте ссылку в поле URL, \
             размер {OBS_SIZE}. Плеер появляется, когда в Spotify играет музыка.",
            app.cfg.url()
        ));
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        if unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } <= 0 {
            break;
        }
        if !msg.hwnd.is_null() {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let pending = win::take_pending();
        if pending & win::EV_QUIT != 0 {
            return;
        }
        let mut changed = pending & win::EV_REFRESH != 0;
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == *app.quit.id() {
                return;
            }
            app.on_menu(&ev);
            changed = true;
        }
        if changed {
            app.update();
        }
    }
}

/// Иконки — ресурсы exe (`assets/app.rc`): пластинка, серая — когда ничего не играет.
fn tray_icon(playing: bool) -> Icon {
    let n = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    Icon::from_resource(if playing { 1 } else { 2 }, Some((n, n))).expect("иконка в ресурсах exe")
}

impl App {
    fn on_menu(&mut self, ev: &MenuEvent) {
        let id = &ev.id;
        if id == self.copy_url.id() {
            if !win::copy_text(&self.cfg.url()) {
                win::error_box("Не удалось скопировать ссылку.");
            }
        } else if id == self.open.id() {
            win::shell_open(self.cfg.url(), None);
        } else if id == self.any_player.id() {
            self.cfg.any_player = !self.cfg.any_player;
            self.cfg.save();
            self.media.set_any_player(self.cfg.any_player);
        } else if id == self.autostart.id() {
            let on = !win::autostart_enabled();
            if !win::set_autostart(on) {
                win::error_box("Не удалось изменить автозапуск.");
            }
        } else if id == self.update.id() {
            if self.hub.update().is_some() {
                self.updater.install_found();
            } else {
                self.updater.check_now();
            }
        }
    }

    fn update(&mut self) {
        let track = self.hub.now();
        let playing = track.as_ref().is_some_and(|t| t.playing);
        let line = match (&self.server_error, &track) {
            (Some(e), _) => shorten(e, 60),
            (None, Some(t)) => {
                let mark = if t.playing { "▶" } else { "❚❚" };
                shorten(&format!("{mark}  {}", title_line(t)), 60)
            }
            (None, None) if self.cfg.any_player => "Ничего не играет".to_string(),
            (None, None) => "Spotify не запущен или не играет".to_string(),
        };
        self.now.set_text(line);
        self.any_player.set_checked(self.cfg.any_player);
        // Галочку автозапуска мог поменять кто-то ещё — читаем из реестра.
        self.autostart.set_checked(win::autostart_enabled());
        let found = self.hub.update();
        self.update.set_text(update_text(found.as_deref()));

        if self.shown_playing != Some(playing) {
            let _ = self.tray.set_icon(Some(tray_icon(playing)));
            self.shown_playing = Some(playing);
        }
        let mut tooltip = match &track {
            Some(t) => shorten(&format!("{} — {}", win::APP_NAME, title_line(t)), 100),
            None => win::APP_NAME.to_string(),
        };
        if let Some(v) = &found {
            tooltip.push_str(&format!(
                "
Доступна версия {v}"
            ));
        }
        if tooltip != self.shown_tooltip {
            let _ = self.tray.set_tooltip(Some(&tooltip));
            self.shown_tooltip = tooltip;
        }
    }
}

fn update_text(found: Option<&str>) -> String {
    match found {
        Some(v) => format!("Установить обновление {v}"),
        None => format!("Проверить обновления (v{})", update::VERSION),
    }
}

fn title_line(t: &Track) -> String {
    if t.artist.is_empty() {
        t.title.clone()
    } else {
        format!("{} — {}", t.artist, t.title)
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}
