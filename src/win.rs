//! Тонкие обёртки над WinAPI: приоритет процесса, автозапуск, буфер обмена,
//! сообщения главному потоку.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, GlobalFree};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CreateMutexW, GetCurrentProcess, GetCurrentThread,
    GetCurrentThreadId, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetPriorityClass, SetProcessInformation, SetThreadPriority,
    THREAD_PRIORITY_LOWEST,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MessageBoxW,
    PostThreadMessageW, SW_SHOWNORMAL, WM_APP,
};

pub const APP_NAME: &str = "Пластинка";

/// Сообщение-«будильник» для главного потока. Сами события лежат в `PENDING`:
/// пока открыто меню трея, модальный цикл Windows выбрасывает thread-сообщения,
/// а флаг не теряется и обработается на следующем сообщении.
const WM_WAKE: u32 = WM_APP + 1;

/// Трек или состояние плеера поменялись — обновить меню, иконку и подсказку.
pub const EV_REFRESH: u32 = 1;
/// Запущен установщик обновления — пора выходить.
pub const EV_QUIT: u32 = 2;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
static PENDING: AtomicU32 = AtomicU32::new(0);

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn register_main_thread() {
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// Отметить событие и разбудить главный поток (вызывается из любых потоков).
pub fn notify_main(ev: u32) {
    PENDING.fetch_or(ev, Ordering::AcqRel);
    let tid = MAIN_THREAD.load(Ordering::Relaxed);
    if tid != 0 {
        unsafe { PostThreadMessageW(tid, WM_WAKE, 0, 0) };
    }
}

pub fn take_pending() -> u32 {
    PENDING.swap(0, Ordering::AcqRel)
}

/// Пониженный приоритет + EcoQoS (Windows 11 отправляет процесс на энергоэффективные ядра).
pub fn make_process_humble() {
    unsafe {
        SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const _,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

pub fn lower_current_thread() {
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
}

/// `false`, если приложение уже запущено.
pub fn acquire_single_instance() -> bool {
    let name = wide("Local\\PlastinkaSingleInstance");
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        // Хэндл намеренно не закрываем: мьютекс живёт до конца процесса.
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Открыть файл, папку или ссылку программой по умолчанию; `args` — параметры для exe.
pub fn shell_open(p: impl AsRef<OsStr>, args: Option<&str>) -> bool {
    let op = wide("open");
    let file = wide(p);
    let args = args.map(wide);
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            args.as_ref().map_or(std::ptr::null(), |a| a.as_ptr()),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // По документации успех — значение больше 32.
    r as usize > 32
}

pub fn copy_text(text: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
    let w = wide(text);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        let mut ok = false;
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
                ok = !SetClipboardData(CF_UNICODETEXT as u32, h).is_null();
            }
            // При успехе памятью владеет буфер обмена.
            if !ok {
                GlobalFree(h);
            }
        }
        CloseClipboard();
        ok
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Plastinka";

/// Автозапуск: значение в HKCU\…\Run.
pub fn autostart_enabled() -> bool {
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    let mut size = 0u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    r == 0
}

pub fn set_autostart(on: bool) -> bool {
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    let r = if on {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let data = wide(format!("\"{}\"", exe.display()));
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
    };
    r == 0
}

pub fn error_box(text: &str) {
    message_box(text, MB_OK | MB_ICONERROR);
}

pub fn info_box(text: &str) {
    message_box(text, MB_OK | MB_ICONINFORMATION);
}

pub fn ask_yes_no(text: &str) -> bool {
    message_box(text, MB_ICONQUESTION | MB_YESNO) == IDYES
}

fn message_box(text: &str, flags: u32) -> i32 {
    let t = wide(text);
    let c = wide(APP_NAME);
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), flags) }
}
