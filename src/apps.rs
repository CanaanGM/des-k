// App rules (spec 0027): a new main window of an app with an [apps] rule goes to its workspace
// and screen the moment Windows shows it. Owns the show and uncloak hooks and matching a window
// to a rule; ws does the placing and holds the hide. A rule only ever names an app to compare with, never one to start.
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::HICON,
    System::Threading::GetCurrentThreadId,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            CHILDID_SELF, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, GA_ROOT, GetAncestor, GetWindowThreadProcessId, IsWindow, OBJID_WINDOW, PostThreadMessageW, WINEVENT_OUTOFCONTEXT,
            WINEVENT_SKIPOWNPROCESS, WM_APP,
        },
    },
};

use crate::{finder, restore, settings, ws};

/// Thread message: a top level window showed. wParam = the window, lParam = the event's tick count.
pub const WM_OPENED: u32 = WM_APP + 17;
/// Thread message: a top level window was uncloaked. wParam = the window (AC-17).
pub const WM_UNCLOAKED: u32 = WM_APP + 18;

thread_local! {
    // The show hook, then the uncloak hook: both in, or neither.
    static HOOK: Cell<Option<[HWINEVENTHOOK; 2]>> = const { Cell::new(None) };
    // Windows of ruled apps waiting for their title or app id: (hwnd, since, the show's tick count).
    static PENDING: RefCell<Vec<(isize, Instant, u32)>> = const { RefCell::new(Vec::new()) };
}

/// How long a window may wait for its title or app id before it is left where it is (AC-16).
const WAIT_FOR: Duration = Duration::from_secs(1);

/// Out of context, so this runs on the main thread inside GetMessageW. It only posts: the ws
/// lock is not reentrant (AGENTS.md).
unsafe extern "system" fn on_show(_: HWINEVENTHOOK, _: u32, hwnd: HWND, obj: i32, child: i32, _: u32, time: u32) {
    post_top(WM_OPENED, hwnd, obj, child, time);
}

/// The same for an uncloak: Windows' shell can show a window des-k just hid (AC-17).
unsafe extern "system" fn on_uncloak(_: HWINEVENTHOOK, _: u32, hwnd: HWND, obj: i32, child: i32, _: u32, time: u32) {
    post_top(WM_UNCLOAKED, hwnd, obj, child, time);
}

fn post_top(msg: u32, hwnd: HWND, obj: i32, child: i32, time: u32) {
    if !hwnd.is_invalid() && obj == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 && unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd {
        unsafe { let _ = PostThreadMessageW(GetCurrentThreadId(), msg, WPARAM(hwnd.0 as usize), LPARAM(time as isize)); }
    }
}

/// Puts the show and uncloak hooks in while any rule exists and takes them out when none do (AC-8). Main
/// thread, after the settings load and after each change to `[apps]`.
pub fn sync() {
    let count = settings::get().apps.len();
    match (HOOK.get(), count > 0) {
        (None, true) => {
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
            let show = unsafe { SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(on_show), 0, 0, flags) };
            let uncloak = unsafe { SetWinEventHook(EVENT_OBJECT_UNCLOAKED, EVENT_OBJECT_UNCLOAKED, None, Some(on_uncloak), 0, 0, flags) };
            if show.is_invalid() || uncloak.is_invalid() {
                for h in [show, uncloak].into_iter().filter(|h| !h.is_invalid()) {
                    unsafe { let _ = UnhookWinEvent(h); }
                }
                return error!("apps: show hook not installed, app rules do nothing");
            }
            // After the hook: a window that shows in between is then both seen and heard, never missed.
            ws::rules::seed_opened();
            HOOK.set(Some([show, uncloak]));
            info!("apps: show hook in for {count} rule(s)");
        }
        (Some(hooks), false) => {
            for h in hooks {
                unsafe { let _ = UnhookWinEvent(h); }
            }
            HOOK.set(None);
            PENDING.take();
            ws::rules::forget_opened();
            info!("apps: no rules, show hook out");
        }
        // Only apps left alone: no hook is needed for them (spec 0041 AC-3, AC-12).
        (None, false) if !settings::get().alone.is_empty() => info!("apps: 0 rules, no show hook"),
        _ => {}
    }
}

/// A top level window showed (`WM_OPENED`): the first time it ever shows, a window of an app with
/// a rule goes where the rule says (AC-1 to AC-3). `shown` is the event's tick count.
pub fn opened(h: HWND, shown: u32) {
    if HOOK.get().is_none() || !unsafe { IsWindow(Some(h)) }.as_bool() || !ws::rules::first_show(h) {
        return;
    }
    if let Try::Wait(_) = try_place(h, Some(shown)) {
        PENDING.with_borrow_mut(|p| p.push((h.0 as isize, Instant::now(), shown)));
    }
}

/// Whether a window still waits for its title or app id, so main keeps its 15 ms timer.
pub fn waiting() -> bool {
    PENDING.with_borrow(|p| !p.is_empty())
}

/// Every 15 ms while a window waits: places each one whose title or app id has come, and leaves
/// one that waited `WAIT_FOR` where it is (AC-4, AC-16). Returns whether any still waits.
pub fn pending_tick() -> bool {
    // Paused: no rule acts, so the waiting windows stay where they opened and the timer stops (spec 0039 AC-4).
    if ws::pause::paused() {
        PENDING.take();
        return false;
    }
    let mut left = Vec::new();
    for (raw, since, shown) in PENDING.take() {
        let h = HWND(raw as *mut _);
        if HOOK.get().is_none() || !unsafe { IsWindow(Some(h)) }.as_bool() {
            continue;
        }
        match try_place(h, Some(shown)) {
            Try::Done => {}
            Try::Wait(what) if since.elapsed() >= WAIT_FOR => info!("apps: a {} window left where it opened: {what} after 1 s", ws::views::who(h)),
            Try::Wait(_) => left.push((raw, since, shown)),
        }
    }
    PENDING.with_borrow_mut(|p| p.extend(left));
    waiting()
}

/// A window was uncloaked (`WM_UNCLOAKED`): one waiting for that is placed now rather than at the
/// next 15 ms tick, so it shows for at most a frame (AC-4).
pub fn uncloaked(h: HWND) {
    let raw = h.0 as isize;
    let Some(shown) = PENDING.with_borrow(|p| p.iter().find(|w| w.0 == raw).map(|w| w.2)) else { return };
    if let Try::Done = try_place(h, Some(shown)) {
        PENDING.with_borrow_mut(|p| p.retain(|w| w.0 != raw));
    }
}

/// After a fresh sign in, while restore runs: windows of apps with a rule that opened before
/// des-k started go by their rule once, since restore would have placed them anyway and now
/// leaves them alone (AC-11). Main calls this once every exit path is in place.
pub fn place_open() {
    if HOOK.get().is_none() {
        return;
    }
    for h in ws::views::app_windows() {
        // Opened from now on, so a show event still queued for it never places it twice.
        ws::rules::first_show(h);
        let _ = try_place(h, None);
    }
}

/// An app with a window open now, as the settings page shows it (AC-13).
#[derive(Clone, PartialEq, Debug)]
pub struct Seen {
    pub key: String,
    pub name: String,
    pub icon: HICON, // borrowed from the finder's cache, never destroyed
    /// Where its front window is: the screen's device, and the window's own workspace.
    pub device: String,
    pub ws: usize,
}

/// Every app with a window open now, shown or hidden, once each by its front window: the
/// page's Add menu, and the icon and name on its rule rows. `open` is `ws::views::View::open`, so no
/// ws lock is held while this asks the apps; des-k's own windows are never in it.
pub fn seen(open: &[(isize, String, usize)]) -> Vec<Seen> {
    let mut out: Vec<Seen> = Vec::new();
    for (raw, device, ws) in open {
        let h = HWND(*raw as *mut _);
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let Some(key) = key_from(&restore::app(h, pid)) else { continue };
        if out.iter().any(|s| s.key == key) {
            continue;
        }
        let name = finder::names::app_name(h).or_else(|| finder::names::store_name(&key)).unwrap_or_else(|| key.trim_end_matches(".exe").to_string());
        out.push(Seen { key, name, icon: finder::names::icon(h), device: device.clone(), ws: *ws });
    }
    out
}

enum Try {
    Done,
    /// Its title or app id has not come yet: what is missing, for the log.
    Wait(&'static str),
}

/// Places `h` if its app has a rule and it is a main window now. `shown` is the tick count of
/// its show event, None for a window open before des-k started.
fn try_place(h: HWND, shown: Option<u32>) -> Try {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    let app = restore::app(h, pid);
    if app.is_empty() {
        return Try::Done; // its process can't be read
    }
    let set = settings::get();
    let Some(key) = key_from(&app) else {
        // A Store app's frame window shows before its app id can be read (AC-16).
        return if set.apps.iter().any(|(k, _)| k.contains('!')) { Try::Wait("app id not readable") } else { Try::Done };
    };
    let Some((_, rule)) = set.apps.iter().find(|(k, _)| *k == key) else { return Try::Done };
    // Only the kind of window app_windows lists: never a dialog or popup on its own (AC-3).
    // Many apps show their window first and give it a title a moment later.
    match ws::views::app_window(h) {
        Err("no title") => return Try::Wait("no title"),
        // A Store app's frame window shows cloaked by Windows, which uncloaks it about 300 ms
        // later, once the app has drawn (measured 2026-11-19). Its key is an app id, with a `!`.
        Err("cloaked") if app.contains('!') => return Try::Wait("still cloaked by Windows"),
        Err(why) => {
            info!("apps: a {key} window left where it opened: {why}");
            return Try::Done;
        }
        Ok(()) => {}
    }
    // A Store app has no CoreWindow yet as it lands, so its name comes from its package.
    let name = finder::names::app_name(h).or_else(|| finder::names::store_name(&key)).unwrap_or_else(|| key.trim_end_matches(".exe").to_string());
    ws::rules::place_new(h, rule, name, shown);
    Try::Done
}

/// The key a rule names an app by, from `restore::app`'s lowercased exe path or Store app id:
/// the exe file name, or the app id (AC-16). None for a Store app's frame window whose app id
/// can't be read yet, and for an empty one.
pub fn key_from(app: &str) -> Option<String> {
    let key = app.rsplit('\\').next().unwrap_or_default();
    (!key.is_empty() && key != "applicationframehost.exe").then(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::key_from;

    // A rule names an app by its exe file name, never its folder: one rule must match wherever
    // the app is installed, and a setting never carries a path.
    #[test]
    fn exe_key_is_the_file_name() {
        assert_eq!(key_from(r"c:\users\me\appdata\local\slack\app-4.41.0\slack.exe").as_deref(), Some("slack.exe"));
        assert_eq!(key_from(r"c:\windows\system32\notepad.exe").as_deref(), Some("notepad.exe"));
    }

    // A Store app is ruled by its app id, which restore::app reads in place of the frame host's path.
    #[test]
    fn store_app_key_is_its_app_id() {
        let id = "microsoft.windowscalculator_8wekyb3d8bbwe!app";
        assert_eq!(key_from(id).as_deref(), Some(id));
    }

    // A frame window with no app id yet must wait for it, never match as the frame host itself.
    #[test]
    fn frame_host_without_app_id_has_no_key() {
        assert_eq!(key_from(r"c:\windows\system32\applicationframehost.exe"), None);
        assert_eq!(key_from(""), None);
    }
}
