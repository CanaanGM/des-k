// des-k: per-monitor virtual desktops for Windows.
// Win+Ctrl+Left/Right switches the workspace of the monitor under the mouse.
// Win+Ctrl+Shift/Alt+Left/Right sends the focused window over, with or without you.
// Win+Ctrl(+Shift/Alt)+1..9 does the same straight to workspace N.
// Win+F opens the window finder: search every window on every screen and workspace.
// Win+Tab opens the workspace overview on every screen: every workspace and its windows.
// Three and four finger touchpad gestures run what settings.toml maps them to, read by des-k itself (spec 0011).
// Win+Ctrl+U pins the focused window to every workspace of its screen (spec 0014).
// The tray icon opens des-k's menu; its Shortcut hints (or a key you set) lists every shortcut (spec 0017).
// The Copilot key acts as Right Ctrl. `des-k --quit` (or Ctrl+C in a console) shows
// every hidden window again and exits; `des-k --start` (the Start menu entry) starts the
// installed des-k through its logon task, so it runs as admin. Release builds have no
// console and log to %LOCALAPPDATA%\des-k\des-k.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod log; // first: its info!, warn! and error! macros reach only the modules after it
mod apps;
mod backdrop;
mod cloak;
mod controls;
#[cfg(feature = "drill")]
mod drill;
mod finder;
mod gestures;
mod hints;
mod indicator;
mod install;
mod keys;
mod overview;
mod restore;
mod settings;
mod settings_page;
mod setup;
#[cfg(test)]
mod snapshot;
mod touchpad;
mod tray;
mod tray_menu;
mod ui;
mod uia;
mod update;
mod wallpaper;
mod welcome;
mod whats_new;
mod ws;

use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM},
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx},
        Console::{CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler},
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
    ws::restore::restore_all();
    // des-k starts again at sign in, so a sign out or shutdown must not give Windows its gestures back.
    if !matches!(event, CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        setup::write_back();
    }
    log::flush();
    std::process::exit(0);
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
        Ok(s) if s.success() => info!("des-k task started"),
        Ok(s) => warn!("des-k task did not start ({s}): install des-k first, with its setup or install.ps1"),
        Err(e) => warn!("schtasks did not run: {e}"),
    }
}

/// Hands the backdrop each screen's pictures when its shown workspace, the settings or the pause
/// changed since `seen` (spec 0026). Each reload gets a new version number; never its address,
/// which a freed copy can hand on (spec 0032). While paused every screen wishes for none, so your
/// Windows wallpaper shows, and resume draws them again (spec 0039 AC-4).
fn backdrop_check(seen: &mut Option<(u64, u64, bool)>) {
    let s = settings::get();
    let paused = ws::pause::paused();
    let now = (ws::views::SHOWN.load(std::sync::atomic::Ordering::Relaxed), s.version(), paused);
    if *seen == Some(now) {
        return;
    }
    *seen = Some(now);
    let wants = ws::views::shown().into_iter().map(|(rect, id, shown)| backdrop::Want {
        rect,
        shown,
        files: (0..s.workspaces).map(|i| s.wallpaper_for(&id, i).filter(|_| !paused).and_then(backdrop::Source::of)).collect(),
        intervals: (0..s.workspaces).map(|i| s.interval_for(i)).collect(),
        id,
    });
    let look = backdrop::Look { fit: s.wallpaper_fit, dim: s.wallpaper_dim, fade: s.wallpaper_fades(), fastest: s.fastest, shuffle: s.wallpaper_shuffle };
    backdrop::want(wants.collect(), look);
}

/// Starts a thread timer of `ms` into `timer` when `on` and none runs yet.
fn timer_on(timer: &mut usize, on: bool, ms: u32) {
    if *timer == 0 && on {
        *timer = unsafe { SetTimer(None, 0, ms, None) };
    }
}

fn main() -> windows::core::Result<()> {
    let running = unsafe { FindWindowW(indicator::CLASS, None) }.ok();
    if std::env::args().nth(1).as_deref() == Some("--start") {
        match running {
            Some(_) => info!("des-k is already running"),
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
                log::open(true);
                unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
                ws::store::rescue();
                setup::write_back();
                log::flush();
            }
        }
        return Ok(());
    }
    if running.is_some() {
        // Two keyboard hooks would switch twice per key press.
        info!("des-k is already running");
        return Ok(());
    }
    log::open(false); // only now: a second copy must not truncate the running one's log
    #[cfg(feature = "drill")]
    drill::arm();
    match elevated() {
        Ok(true) => info!("des-k runs as admin: its keys work whatever window has focus"),
        Ok(false) => info!("des-k runs without admin: its keys stop while an admin window (Task Manager, an admin terminal) has focus"),
        Err(e) => warn!("cannot tell whether des-k runs as admin: {e}"),
    }

    // Real pixels on scaled monitors, so the indicator is sharp and correctly placed.
    unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    indicator::init();
    settings::store::load(); // before anything that reads it: ws, keys, the indicator
    // Lets you try a Windows des-k does not know on your own screens (spec 0030 AC-14).
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--pretend-unknown-windows") {
        cloak::pretend_unknown();
    }
    // Before ws::restore::load, so adopting a saved layout already knows whether hiding works (AC-3).
    cloak::check_at_start(&ws::views::app_windows());
    let restoring = ws::restore::load();
    apps::sync(); // after ws::restore::load, so the windows it adopted count as already open (spec 0027 AC-3)
    // Always on: the finder's recency needs it even with follow_activation off (spec 0007).
    // Skipping our own process keeps des-k's own focus changes from triggering a follow.
    // Never unhooked: the hook dies with the process.
    let hook = unsafe {
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
    };
    if hook.is_invalid() {
        warn!("foreground hook not installed: no follow, and the finder only counts its own picks as recent");
    }

    // Any thread: a des-k missing its keyboard hook or touchpad reader must not run on, so it
    // always ends here, after showing every window.
    std::panic::set_hook(Box::new(move |info| {
        log::panic(info);
        ws::restore::restore_after_panic();
        setup::write_back();
        log::flush();
        std::process::exit(101);
    }));
    unsafe { let _ = SetConsoleCtrlHandler(Some(on_console_close), true); }

    keys::spawn(unsafe { GetCurrentThreadId() });
    setup::start(); // before the touchpad thread reads Windows' gestures
    touchpad::spawn(unsafe { GetCurrentThreadId() });
    tray::init();
    // What the last update from the tray did, and its files and task cleared (spec 0018).
    tray::installed(install::outcome());
    // A start that comes up paused says so once (spec 0039 AC-5), over an update's balloon: des-k
    // doing nothing is the thing to know. load logged it. At logon Windows drops the balloon, so
    // des-k's own note follows at your first input.
    if ws::pause::paused() {
        tray::started_paused();
        ws::pause::remind_at_input();
    }
    install::cleanup();
    // Read at start only; written the moment it opens, so a crash can never make it loop
    // (spec 0017 AC-9).
    let first_run = settings::get().first_run_seen != Some(true);
    if first_run {
        welcome::show();
        if let Err(why) = settings::edit::write(settings::edit::Edit::Value("first_run_seen", true.into())) {
            error!("first run: could not write first_run_seen = true, so it opens again at the next start ({why})");
        }
    }
    // After an update, never with the first run: a start opens one screen at most (spec 0029).
    whats_new::at_start(first_run);
    info!("des-k running: the shortcuts switch the monitor under the mouse ({} workspaces each).", settings::get().workspaces);

    // Only now, with every exit path in place, may restore hide anything. The first tick
    // runs at once, so windows already open are matched like later ones (spec 0003).
    let mut restore_timer = 0;
    // While a window a rule moved to another screen is watched (spec 0027 AC-6), and while a
    // ruled app's window waits for its title or app id (AC-16).
    let (mut settle_timer, mut pending_timer) = (0, 0);
    if restoring == ws::restore::Restoring::NewSignIn {
        apps::place_open(); // restore leaves ruled apps to their rule (spec 0027 AC-11)
        timer_on(&mut settle_timer, ws::rules::settling(), 250);
    }
    if restoring != ws::restore::Restoring::No {
        restore_timer = unsafe { SetTimer(None, 0, 500, None) };
        if !ws::restore::restore_tick() && restore_timer != 0 {
            unsafe { let _ = KillTimer(None, restore_timer); }
            restore_timer = 0;
        }
    }
    // Hand edits to settings.toml apply live (spec 0012 AC-5). Never killed: dies with the process.
    let watch_timer = unsafe { SetTimer(None, 0, 1000, None) };
    if watch_timer == 0 {
        warn!("settings: no file watch, hand edits apply at the next start");
    }

    let mut shown = None;
    backdrop_check(&mut shown);
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        // Paused: no switch, send, follow, rule, swipe or mark acts, and none logs (spec 0039 AC-3,
        // AC-4). The hook posts no key while paused; these can still be queued from before.
        if ws::pause::paused()
            && matches!(
                msg.message,
                WM_APP | keys::WM_SEND | keys::WM_JUMP | keys::WM_BACK | keys::WM_SEND_TO | keys::WM_PIN | keys::WM_HINTS | keys::WM_FINDER | keys::WM_OVERVIEW | touchpad::WM_GESTURE | apps::WM_UNCLOAKED | indicator::WM_ATTENTION
            )
        {
            continue;
        }
        // Any switch, jump, back, send, pin, hints key or screen change closes the finder, the
        // overview and the tray menu first, focus back (specs 0007, 0010, 0017, 0024).
        if matches!(msg.message, WM_APP | indicator::WM_DISPLAY_CHANGED | keys::WM_SEND | keys::WM_JUMP | keys::WM_BACK | keys::WM_SEND_TO | keys::WM_PIN | keys::WM_HINTS) {
            finder::close(true);
            overview::close(true);
        }
        // The tray menu also closes for a gesture, a follow, the finder and the overview (AC-TRY-9).
        if matches!(msg.message, WM_APP | indicator::WM_DISPLAY_CHANGED | keys::WM_SEND | keys::WM_JUMP | keys::WM_BACK | keys::WM_SEND_TO | keys::WM_PIN | keys::WM_HINTS | keys::WM_PAUSE | keys::WM_FINDER | keys::WM_OVERVIEW | touchpad::WM_GESTURE | WM_FOLLOW) {
            tray_menu::close(msg.message != WM_FOLLOW); // a follow already moved focus
        }
        match msg.message {
            WM_APP => {
                #[cfg(feature = "drill")]
                let started = std::time::Instant::now();
                ws::switch::switch(msg.wParam.0 as isize, ws::switch::Source::Keys);
                #[cfg(feature = "drill")]
                drill::switched(started.elapsed());
                welcome::tried(welcome::Try::Switch);
            }
            indicator::WM_DISPLAY_CHANGED => {
                ws::screens::check_screens(false);
                settings_page::tick();
                welcome::tick();
            }
            WM_FOLLOW => {
                let h = HWND(msg.wParam.0 as *mut _);
                finder::used(h);
                if settings::get().follow_activation && !ws::pause::paused() {
                    ws::switch::follow(h);
                }
            }
            keys::WM_SEND => ws::moves::send(ws::moves::To::Step(msg.wParam.0 as isize), msg.lParam.0 != 0),
            keys::WM_JUMP => ws::switch::jump(msg.wParam.0, ws::switch::Source::Keys),
            keys::WM_BACK => ws::switch::back(ws::switch::Source::Keys),
            keys::WM_SEND_TO => ws::moves::send(ws::moves::To::Index(msg.wParam.0), msg.lParam.0 != 0),
            keys::WM_PIN => ws::moves::pin(),
            keys::WM_PAUSE => ws::pause::toggle(),
            // Paused: a new window stays where Windows put it, and counts as shown, so its rule is
            // never applied after the fact (spec 0039 AC-6).
            apps::WM_OPENED if ws::pause::paused() => {
                ws::rules::first_show(HWND(msg.wParam.0 as *mut _));
            }
            apps::WM_OPENED => {
                apps::opened(HWND(msg.wParam.0 as *mut _), msg.lParam.0 as u32);
                timer_on(&mut settle_timer, ws::rules::settling(), 250);
                timer_on(&mut pending_timer, apps::waiting(), 15);
            }
            apps::WM_UNCLOAKED => {
                let h = HWND(msg.wParam.0 as *mut _);
                ws::rules::recloak(h);
                apps::uncloaked(h);
                timer_on(&mut settle_timer, ws::rules::settling(), 250);
            }
            indicator::WM_ATTENTION => {
                let p = ws::focus::attention(HWND(msg.wParam.0 as *mut _));
                overview::wants_changed();
                // The mark is set either way; only the popup waits (spec 0022 AC-3, AC-7).
                if let Some(p) = p
                    && !overview::is_open()
                    && !indicator::busy(p.mon)
                {
                    // Never the title: it often carries the message itself (spec 0022 AC-9).
                    let text = settings::attention_line(&finder::names::app_name(p.hwnd).unwrap_or_default());
                    indicator::show(p.mon, p.card.with(indicator::Line::Wants { text, ws: p.ws }));
                }
            }
            keys::WM_FINDER => {
                overview::close(true);
                finder::toggle(ws::switch::Source::Keys);
            }
            // To the window that is recording: the settings page, the first run or the hints window.
            keys::WM_RECORDED => match keys::recording_window() {
                Some(h) if hints::owns(h) => hints::recorded(msg.wParam.0 as u16, msg.lParam.0 as u8),
                Some(h) if welcome::owns(h) => welcome::recorded(msg.wParam.0 as u16, msg.lParam.0 as u8),
                Some(h) if settings_page::owns(h) => settings_page::recording::recorded(msg.wParam.0 as u16, msg.lParam.0 as u8),
                _ => {}
            },
            keys::WM_HINTS => hints::toggle(ws::switch::Source::Keys),
            keys::WM_OVERVIEW => {
                finder::close(true);
                overview::toggle();
                if overview::is_open() {
                    welcome::tried(welcome::Try::Overview);
                }
            }
            touchpad::WM_GESTURE => gestures::run(msg.wParam.0),
            touchpad::WM_TOUCHPADS_CHANGED => {
                welcome::refresh();
                hints::refresh();
            }
            // Indicator timers carry its window and go to DispatchMessageW below.
            WM_TIMER if msg.hwnd.is_invalid() && restore_timer != 0 && msg.wParam.0 == restore_timer => {
                if !ws::restore::restore_tick() {
                    unsafe { let _ = KillTimer(None, restore_timer); }
                    restore_timer = 0;
                }
            }
            WM_TIMER if msg.hwnd.is_invalid() && settle_timer != 0 && msg.wParam.0 == settle_timer => {
                if !ws::rules::settle_tick() {
                    unsafe { let _ = KillTimer(None, settle_timer); }
                    settle_timer = 0;
                }
            }
            WM_TIMER if msg.hwnd.is_invalid() && pending_timer != 0 && msg.wParam.0 == pending_timer => {
                if !apps::pending_tick() {
                    unsafe { let _ = KillTimer(None, pending_timer); }
                    pending_timer = 0;
                }
                timer_on(&mut settle_timer, ws::rules::settling(), 250);
            }
            WM_TIMER if msg.hwnd.is_invalid() && watch_timer != 0 && msg.wParam.0 == watch_timer => {
                // The theme broadcast may never reach an admin des-k (spec 0017 AC-2).
                if ui::refresh() {
                    ui::repaint_all();
                }
                settings::store::watch();
                ws::screens::check_screens(true);
                ws::restore::resync();
                ws::pause::tick();
                ws::pause::remind();
                tray::warn_if_broken();
                if ws::store::cant_save_due() {
                    tray::cant_save();
                }
                settings_page::tick();
                welcome::tick();
                whats_new::tick(); // opens What's new once you're not busy (spec 0029 AC-7)
            }
            // Posted by `des-k --quit` (to the indicator) and the tray's Quit. The finder and
            // the overview post WM_CLOSE to any window they close, des-k's own settings page
            // and welcome screen too: those go to their window, never end des-k.
            WM_CLOSE if matches!(ws::views::class_name(msg.hwnd).as_str(), "des-k-indicator" | "des-k-tray") => break,
            // Indicator, tray, tray menu, finder, overview, welcome screen, hints window and settings
            // page messages. TranslateMessage makes the finder's typed characters and IME input;
            // the finder and the overview take their own keys before that, and the hints
            // window's go through IsDialogMessageW (Tab, Enter, Esc). The settings page, the
            // first run and the tray menu take their keys in their wndprocs.
            _ if hints::key(&msg) || finder::key(&msg) || overview::key(&msg) => {}
            _ => unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            },
        }
        backdrop_check(&mut shown);
    }
    ws::restore::restore_all();
    setup::write_back(); // Quit and --quit
    tray::remove();
    #[cfg(feature = "drill")]
    drill::report();
    info!("des-k stopped, all windows shown");
    log::flush();
    Ok(())
}
