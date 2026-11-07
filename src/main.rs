// des-k: per-monitor virtual desktops for Windows.
// Win+Ctrl+Left/Right switches the workspace of the monitor under the mouse.
// Win+Ctrl+Shift/Alt+Left/Right sends the focused window over, with or without you.
// Win+Ctrl(+Shift/Alt)+1..9 does the same straight to workspace N.
// Win+F opens the window finder: search every window on every screen and workspace.
// Win+Tab opens the workspace overview on every screen: every workspace and its windows.
// Three and four finger touchpad gestures run what settings.toml maps them to, read by des-k itself (spec 0011).
// Win+Ctrl+U pins the focused window to every workspace of its screen (spec 0014).
// The Copilot key acts as Right Ctrl. `des-k --quit` (or Ctrl+C in a console) shows
// every hidden window again and exits; `des-k --start` (the Start menu entry) starts the
// installed des-k through its logon task, so it runs as admin. Release builds have no
// console and log to %LOCALAPPDATA%\des-k\des-k.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cloak;
mod finder;
mod gestures;
mod indicator;
mod keys;
mod overview;
mod restore;
mod settings;
mod settings_page;
mod setup;
mod touchpad;
mod tray;
mod ui;
mod uia;
mod update;
mod wallpaper;
mod welcome;
mod ws;

use std::os::windows::io::IntoRawHandle;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM},
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx},
        Console::{CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, GetConsoleWindow, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler, SetStdHandle},
        Threading::{GetCurrentProcess, GetCurrentThreadId, OpenProcessToken},
    },
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook},
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        WindowsAndMessaging::{
            CHILDID_SELF, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, FindWindowW, GetMessageW, KillTimer, MSG, OBJID_WINDOW, PostMessageW,
            PostThreadMessageW, SetTimer, TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_CLOSE, WM_TIMER,
        },
    },
};
use windows::core::BOOL;

/// Thread message, wParam = the window that came to the front.
const WM_FOLLOW: u32 = WM_APP + 2;

/// Out of context, so this runs on the main thread inside GetMessageW. It only posts: the
/// follow then runs after whatever switch is in progress has set its final focus (spec 0002).
unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, hwnd: HWND, obj: i32, child: i32, _: u32, _: u32) {
    if !hwnd.is_invalid() && obj == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 {
        unsafe { let _ = PostThreadMessageW(GetCurrentThreadId(), WM_FOLLOW, WPARAM(hwnd.0 as usize), LPARAM(0)); }
    }
}

unsafe extern "system" fn on_console_close(event: u32) -> BOOL {
    // Runs on a fresh thread: needs its own COM.
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    ws::restore_all();
    // des-k starts again at sign in, so a sign out or shutdown must not give Windows its gestures back.
    if !matches!(event, CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        setup::write_back();
    }
    std::process::exit(0);
}

/// No console (release build started at login): send println!/eprintln! to a log file.
/// Rust's stdio looks the std handle up on every write, so swapping it here is enough.
/// `append`: add to the log (a `--quit` with nothing running) instead of starting a new one.
fn log_to_file_without_console(append: bool) {
    if !unsafe { GetConsoleWindow() }.is_invalid() {
        return;
    }
    let log = ws::data_dir().join("des-k.log");
    let file = if append {
        ws::append_to(&log)
    } else {
        // The last run's log survives one restart: after a crash it still says why.
        let _ = std::fs::rename(&log, ws::data_dir().join("des-k.old.log"));
        ws::create_fresh(&log)
    };
    if let Ok(f) = file {
        let h = HANDLE(f.into_raw_handle());
        unsafe {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, h);
            let _ = SetStdHandle(STD_ERROR_HANDLE, h);
        }
    }
}

/// Whether des-k's own token is elevated. Without it, Windows (UIPI) keeps every key away
/// from the hook while an admin window has focus (spec 0008).
fn elevated() -> windows::core::Result<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0;
        let r = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as _), size_of_val(&e) as u32, &mut len);
        let _ = CloseHandle(token);
        r.map(|()| e.TokenIsElevated != 0)
    }
}

/// `des-k --start`: runs the logon task, which starts the installed des-k as admin. Started
/// straight from the Start menu it would run without admin, and then the task's copy could
/// not start (spec 0008). Logs to the console only: no des-k log is open yet.
fn start_task() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let schtasks = std::env::var("SystemRoot").map_or("schtasks.exe".into(), |r| format!(r"{r}\System32\schtasks.exe"));
    match std::process::Command::new(schtasks).args(["/run", "/tn", "des-k"]).creation_flags(CREATE_NO_WINDOW).status() {
        Ok(s) if s.success() => println!("des-k task started"),
        Ok(s) => eprintln!("des-k task did not start ({s}): install des-k first, with its setup or install.ps1"),
        Err(e) => eprintln!("schtasks did not run: {e}"),
    }
}

fn main() -> windows::core::Result<()> {
    let running = unsafe { FindWindowW(indicator::CLASS, None) }.ok();
    if std::env::args().nth(1).as_deref() == Some("--start") {
        match running {
            Some(_) => println!("des-k is already running"),
            None => start_task(),
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--quit") {
        match running {
            Some(h) => unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0))? },
            None => {
                // Stopped by a crash or a kill: still show what it hid and give Windows its
                // gestures back, as a quit would (the uninstaller relies on this).
                log_to_file_without_console(true);
                println!("des-k --quit: des-k is not running");
                unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
                ws::rescue();
                setup::write_back();
            }
        }
        return Ok(());
    }
    if running.is_some() {
        // Two keyboard hooks would switch twice per key press.
        println!("des-k is already running");
        return Ok(());
    }
    log_to_file_without_console(false); // only now: a second copy must not truncate the running one's log
    match elevated() {
        Ok(true) => println!("des-k runs as admin: its keys work whatever window has focus"),
        Ok(false) => println!("des-k runs without admin: its keys stop while an admin window (Task Manager, an admin terminal) has focus"),
        Err(e) => eprintln!("cannot tell whether des-k runs as admin: {e}"),
    }

    // Real pixels on scaled monitors, so the indicator is sharp and correctly placed.
    unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    indicator::init();
    settings::load(); // before anything that reads it: ws, keys, the indicator
    let restoring = ws::load();
    // Always on: the finder's recency needs it even with follow_activation off (spec 0007).
    // Skipping our own process keeps des-k's own focus changes from triggering a follow.
    // Never unhooked: the hook dies with the process.
    let hook = unsafe {
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
    };
    if hook.is_invalid() {
        eprintln!("foreground hook not installed: no follow, and the finder only counts its own picks as recent");
    }

    let default_panic = std::panic::take_hook();
    // Any thread: a des-k missing its keyboard hook or touchpad reader must not run on, so it
    // always ends here, after showing every window.
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        ws::restore_after_panic();
        setup::write_back();
        std::process::exit(101);
    }));
    unsafe { let _ = SetConsoleCtrlHandler(Some(on_console_close), true); }

    keys::spawn(unsafe { GetCurrentThreadId() });
    let first_start = setup::start(); // before the touchpad thread reads Windows' gestures
    touchpad::spawn(unsafe { GetCurrentThreadId() });
    tray::init();
    if first_start {
        welcome::show(true);
    }
    println!("des-k running: the shortcuts switch the monitor under the mouse ({} workspaces each).", settings::get().workspaces);

    // Only now, with every exit path in place, may restore hide anything. The first tick
    // runs at once, so windows already open are matched like later ones (spec 0003).
    let mut restore_timer = 0;
    if restoring {
        restore_timer = unsafe { SetTimer(None, 0, 500, None) };
        if !ws::restore_tick() && restore_timer != 0 {
            unsafe { let _ = KillTimer(None, restore_timer); }
            restore_timer = 0;
        }
    }
    // Hand edits to settings.toml apply live (spec 0012 AC-5). Never killed: dies with the process.
    let watch_timer = unsafe { SetTimer(None, 0, 1000, None) };
    if watch_timer == 0 {
        eprintln!("settings: no file watch, hand edits apply at the next start");
    }

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        // Any switch, jump, send, pin or screen change closes the finder and the overview first,
        // focus back (specs 0007, 0010).
        if matches!(msg.message, WM_APP | indicator::WM_DISPLAY_CHANGED | keys::WM_SEND | keys::WM_JUMP | keys::WM_SEND_TO | keys::WM_PIN) {
            finder::close(true);
            overview::close(true);
        }
        match msg.message {
            WM_APP => ws::switch(msg.wParam.0 as isize),
            indicator::WM_DISPLAY_CHANGED => {
                ws::check_screens(false);
                settings_page::tick();
            }
            WM_FOLLOW => {
                let h = HWND(msg.wParam.0 as *mut _);
                finder::used(h);
                if settings::get().follow_activation {
                    ws::follow(h);
                }
            }
            keys::WM_SEND => ws::send(ws::To::Step(msg.wParam.0 as isize), msg.lParam.0 != 0),
            keys::WM_JUMP => ws::jump(msg.wParam.0),
            keys::WM_SEND_TO => ws::send(ws::To::Index(msg.wParam.0), msg.lParam.0 != 0),
            keys::WM_PIN => ws::pin(),
            keys::WM_FINDER => {
                overview::close(true);
                finder::toggle();
            }
            keys::WM_RECORDED => settings_page::recorded(msg.wParam.0 as u16, msg.lParam.0 as u8),
            keys::WM_OVERVIEW => {
                finder::close(true);
                overview::toggle();
            }
            touchpad::WM_GESTURE => gestures::run(msg.wParam.0),
            touchpad::WM_TOUCHPADS_CHANGED => welcome::refresh(),
            // Indicator timers carry its window and go to DispatchMessageW below.
            WM_TIMER if msg.hwnd.is_invalid() && restore_timer != 0 && msg.wParam.0 == restore_timer => {
                if !ws::restore_tick() {
                    unsafe { let _ = KillTimer(None, restore_timer); }
                    restore_timer = 0;
                }
            }
            WM_TIMER if msg.hwnd.is_invalid() && watch_timer != 0 && msg.wParam.0 == watch_timer => {
                // The theme broadcast may never reach an admin des-k (spec 0017 AC-2).
                if ui::refresh() {
                    ui::repaint_all();
                }
                settings::watch();
                ws::check_screens(true);
                ws::resync();
                tray::warn_if_broken();
                settings_page::tick();
            }
            // Posted by `des-k --quit` (to the indicator) and the tray's Quit. The finder and
            // the overview post WM_CLOSE to any window they close, des-k's own settings page
            // and welcome screen too: those go to their window, never end des-k.
            WM_CLOSE if matches!(ws::class_name(msg.hwnd).as_str(), "des-k-indicator" | "des-k-tray") => break,
            // Indicator, tray, finder, overview, welcome screen and settings page messages.
            // TranslateMessage makes the finder's typed characters and IME input; the finder and the
            // overview take their own keys before that, and the welcome screen's go through
            // IsDialogMessageW (Tab, Enter, Esc). The settings page takes its keys in its wndproc.
            _ if welcome::key(&msg) || finder::key(&msg) || overview::key(&msg) => {}
            _ => unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            },
        }
    }
    ws::restore_all();
    setup::write_back(); // Quit and --quit
    tray::remove();
    println!("des-k stopped, all windows shown");
    Ok(())
}
