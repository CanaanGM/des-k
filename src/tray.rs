// Tray icon near the clock: shows des-k is running, opens des-k's own menu (tray_menu.rs, spec
// 0017) with Shortcut hints, Settings, Open log, Check for updates (spec 0015), Pause (spec 0039)
// and Quit, and
// runs what you pick there, an update install too (spec 0018). Warns once when hiding windows stopped working. Lives on the main
// thread (its messages come through main's loop), adds itself again whenever explorer
// restarts, and has des-k hide its windows again then.
use crate::tray_menu::{self, Check, Id, Menu, Quit};
use crate::install::{self, InstallFail, Msg, Outcome, Phase};
use crate::update::{self, Answer, Version};
use std::cell::{Cell, RefCell};
use std::path::Path;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{CreateBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, FIXED, GDI_ERROR, GGO_GRAY8_BITMAP, GLYPHMETRICS, GetGlyphOutlineW, MAT2, SelectObject},
    System::{
        Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider},
        LibraryLoader::GetModuleHandleW,
        SystemInformation::GetTickCount,
        Variant::{VARIANT, VT_I4},
    },
    UI::{
        Input::KeyboardAndMouse::GetDoubleClickTime,
        Shell::{
            CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO,
            NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
            NOTIFYICONIDENTIFIER, SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows, Shell_NotifyIconGetRect, Shell_NotifyIconW,
        },
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, BSTR, Interface, PCWSTR, w};

const CLASS: PCWSTR = w!("des-k-tray");
/// Window message (not a thread message): the shell's callback for clicks on the icon. Its
/// own id all the same: main's loop matches thread messages by id alone, and normal programs
/// may post this one to the admin des-k (spec 0008).
const WM_TRAY: u32 = WM_APP + 11;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static ICON: Cell<Option<HICON>> = const { Cell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static BROKEN: Cell<bool> = const { Cell::new(false) }; // warned that hiding does not work
    /// The tooltip while that warning shows, built once with it (spec 0030 AC-7).
    static BROKEN_TIP: RefCell<String> = const { RefCell::new(String::new()) };
    /// An update check the warning's click started: its answer shows over the warning (AC-7).
    static ASKED: Cell<bool> = const { Cell::new(false) };
    static WAITING: Cell<bool> = const { Cell::new(false) }; // no taskbar yet, said so once
    /// What a click on the current balloon does, if anything.
    static BALLOON: RefCell<Option<Click>> = const { RefCell::new(None) };
    /// A balloon asked for before the taskbar was there (title, text, warning): the icon it
    /// needs comes with TaskbarCreated, and the balloon with it. At logon a start that came up
    /// paused lost its balloon this way (spec 0039 AC-5, found 2026-11-26).
    static HELD: RefCell<Option<(String, String, bool)>> = const { RefCell::new(None) };
    /// An update answer while the broken warning shows (spec 0015 AC-9, spec 0017 AC-TRY-7):
    /// the menu's update item says it instead of a balloon, until the next check. A newer
    /// release is always kept, so the item offers it after the balloon is gone (spec 0018 AC-1).
    static ANSWER: Cell<Option<Answer>> = const { Cell::new(None) };
    /// The icon's last mouse or key event (mouse moves aside), and the last one that opened
    /// or closed the menu with its time (AC-TRY-8).
    static PREV: Cell<u32> = const { Cell::new(0) };
    static TOGGLED: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
}

const TIP: &str = "des-k";
const PAUSED_TIP: &str = "des-k · paused";

/// The tooltip: the warning's while it shows, else whether des-k is paused (spec 0039 AC-5).
fn tip() -> String {
    match () {
        _ if BROKEN.get() => BROKEN_TIP.with_borrow(Clone::clone),
        _ if crate::ws::pause::paused() => PAUSED_TIP.to_string(),
        _ => TIP.to_string(),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // Version 4: the event is lParam's low word, the icon's point is in wParam.
            WM_TRAY => {
                let event = (lp.0 & 0xFFFF) as u32;
                match event {
                    NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU => {
                        let at = POINT { x: (wp.0 & 0xFFFF) as i16 as i32, y: (wp.0 >> 16 & 0xFFFF) as i16 as i32 };
                        clicked(hwnd, event, at);
                    }
                    NIN_BALLOONUSERCLICK => match BALLOON.take() {
                        Some(Click::Open(what)) => open(Path::new(&what)),
                        Some(Click::Install(v)) => install::start(hwnd, v),
                        Some(Click::Check) => {
                            ANSWER.set(None);
                            ASKED.set(true);
                            update::check(hwnd);
                        }
                        Some(Click::Resume) => crate::ws::pause::resume(),
                        None => {}
                    },
                    _ => {}
                }
                if event != WM_MOUSEMOVE {
                    PREV.set(event);
                }
                LRESULT(0)
            }
            update::WM_UPDATE => {
                answered(update::unpack(wp, lp).unwrap_or(Answer::Failed(update::Fail::Crashed)));
                LRESULT(0)
            }
            install::WM_INSTALL => {
                match install::unpack(wp, lp) {
                    Some((m, v)) => install_said(m, v),
                    None => warn!("tray: an install message it cannot read ({:#x}, {:#x})", wp.0, lp.0),
                }
                LRESULT(0)
            }
            // Explorer restarted (or the taskbar changed scale): the old icon is gone, and the
            // new taskbar may list hidden windows again.
            _ if msg == TASKBAR_CREATED.get() => {
                add(hwnd);
                crate::ws::restore::resync_soon();
                crate::backdrop::explorer_restarted();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// A hidden top level window, since TaskbarCreated never reaches message only windows.
/// Tool window, so des-k never manages it.
pub fn init() {
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        match CreateWindowExW(WS_EX_TOOLWINDOW, CLASS, w!("des-k"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None) {
            Ok(h) => {
                // Explorer runs without admin; an admin des-k would never hear its clicks or
                // its restart otherwise (spec 0008). Only these two, nothing else.
                for m in [WM_TRAY, TASKBAR_CREATED.get()] {
                    if let Err(e) = ChangeWindowMessageFilterEx(h, m, MSGFLT_ALLOW, None) {
                        warn!("tray: message {m:#x} not let in from explorer: {e}");
                    }
                }
                WND.set(Some(h));
                add(h);
            }
            Err(e) => warn!("tray: window not created: {e}"),
        }
    }
}

/// On a clean exit only; after a hard kill the icon lingers until the mouse passes over it.
pub fn remove() {
    if let Some(h) = WND.get() {
        unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &data(h)); }
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

fn add(hwnd: HWND) {
    // Drawn again each time, so a taskbar scale change gets a sharp icon.
    if let Some(old) = ICON.take() {
        unsafe { let _ = DestroyIcon(old); }
    }
    ICON.set(draw_icon());
    let mut nid = data(hwnd);
    // Version 4 hides the tooltip unless asked to show it.
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = ICON.get().unwrap_or_default();
    copy(&mut nid.szTip, &tip());
    unsafe {
        // At login explorer may not be ready yet; its TaskbarCreated brings us back here.
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() && !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            // No taskbar at all is explorer starting or stopped, not a failure: the icon comes
            // with its TaskbarCreated (tried 2026-11-22, des-k started with explorer stopped).
            if FindWindowW(w!("Shell_TrayWnd"), None).is_err() {
                if !WAITING.replace(true) {
                    info!("tray: waiting for the taskbar (explorer is starting)");
                }
                return;
            }
            return error!("tray: icon not added");
        }
        if WAITING.replace(false) {
            info!("tray: icon added");
        }
        // Set again after every add: a restarted explorer forgets it. Version 4 tells a
        // keyboard open from a click (spec 0017 AC-TRY-8).
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if !Shell_NotifyIconW(NIM_SETVERSION, &nid).as_bool() {
            warn!("tray: icon version 4 refused, the menu can't tell a keyboard open from a click");
        }
    }
    if let Some((title, text, warn)) = HELD.take() {
        show(hwnd, &title, &text, warn);
    }
}

/// A click on the icon, Enter or Space on it, or its context menu: opens the menu, or closes
/// it. A repeat of the same event within a double click's time is ignored (Enter often sends
/// NIN_KEYSELECT twice), and so is an open right after the menu lost focus, which was this
/// same click on the icon (AC-TRY-9).
fn clicked(hwnd: HWND, event: u32, at: POINT) {
    let (now, dbl) = unsafe { (GetTickCount(), GetDoubleClickTime()) };
    if TOGGLED.get().is_some_and(|(e, t)| e == event && now.wrapping_sub(t) < dbl) {
        return;
    }
    TOGGLED.set(Some((event, now)));
    if tray_menu::is_open() {
        return tray_menu::close(true);
    }
    if tray_menu::just_lost() {
        return;
    }
    // A right click sends WM_RBUTTONUP just before its WM_CONTEXTMENU; Shift+F10 and the menu key don't.
    let keyboard = event == NIN_KEYSELECT || (event == WM_CONTEXTMENU && PREV.get() != WM_RBUTTONUP);
    let id = NOTIFYICONIDENTIFIER { cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: hwnd, uID: 1, ..Default::default() };
    let icon: Option<RECT> = unsafe { Shell_NotifyIconGetRect(&id) }.ok();
    tray_menu::open(menu(), at, icon, keyboard);
}

/// The tray's hidden window: it owns the hints window, which keeps that out of the taskbar
/// and Alt+Tab (spec 0017 AC-HNT-7).
pub fn window() -> Option<HWND> {
    WND.get()
}

/// `text` into a fixed buffer, cut to fit, always ending in a 0.
fn copy(to: &mut [u16], text: &str) {
    let last = to.len() - 1;
    for (d, s) in to[..last].iter_mut().zip(text.encode_utf16().chain([0])) {
        *d = s;
    }
}

/// Once, when cloak.rs says a Windows update broke hiding: a warning balloon and a tooltip
/// that stays, so a switch that hides nothing is never a mystery, and a click on the balloon
/// checks for the des-k that fixes it (spec 0030 AC-7, AC-8). Main's 1 s check.
pub fn warn_if_broken() {
    if BROKEN.get() {
        return;
    }
    let Some(cause) = crate::cloak::cause() else { return };
    let check = "Click here to check for updates.";
    let (title, text, tip) = match cause {
        // cloak.rs logged it as it found out (AC-9).
        crate::cloak::Cause::Unknown(..) => {
            let build = crate::log::windows_build();
            // Nothing is hidden on this layout, so a listed window is one an earlier des-k hid (AC-10).
            // Windows 11's toast shows 4 lines of text and cuts the rest, so with that note the
            // same meaning goes in fewer words (tried 2026-11-24: the long form lost the note).
            let text = match crate::ws::views::hidden_count() {
                Some(n) if n > 0 => {
                    let them = if n == 1 { "1 window".to_string() } else { format!("{n} windows") };
                    format!("{build} is new to des-k, so switching hides nothing until an update. Click to check for one. Sign out and in to see {them} it hid.")
                }
                _ => format!("des-k does not know {build} yet, so switching hides nothing until des-k is updated. {check}"),
            };
            ("des-k does not know this Windows", text, format!("des-k: hiding is off, {build} is new to des-k"))
        }
        crate::cloak::Cause::Refused => {
            warn!("hiding windows does not work: Windows refuses des-k's SetCloak call, most likely after an update changed it. Switching hides nothing until des-k is updated.");
            let text = format!("A Windows update most likely changed what des-k uses to hide windows. Switching hides nothing until des-k is updated. The log says more. {check}");
            ("des-k cannot hide windows", text, "des-k: hiding windows does not work, see the log".to_string())
        }
    };
    BROKEN_TIP.set(tip);
    BROKEN.set(true);
    balloon(title, &text, true, Some(Click::Check));
}

/// Screens came or went and windows moved with them (spec 0016 AC-13): one balloon, a line per
/// screen. Never over the warning that hiding does not work: then the indicator and the log say it.
pub fn screens_changed(lines: &[String]) {
    if BROKEN.get() {
        // Counted, not quoted: the lines name screens as you renamed them (spec 0023 AC-15).
        return info!("tray: no balloon over the hiding warning, {} screen notes not shown", lines.len());
    }
    balloon("Screens changed", &lines.join("\n"), false, None);
}

/// Pause or resume happened: the icon goes gray or back to color, and the tooltip follows (spec
/// 0039 AC-5). Never under the ws lock.
pub fn paused_changed() {
    if let Some(h) = WND.get() {
        add(h);
    }
}

/// A start that came up paused (spec 0039 AC-5): once, and a click resumes.
pub fn started_paused() {
    balloon("des-k is paused", "Click to resume", false, Some(Click::Resume));
}

/// state.txt could not be saved, so des-k hid nothing (spec 0034 AC-2). Once a run, from main's
/// 1 s timer, never under the ws lock.
pub fn cant_save() {
    balloon("des-k can't save your layout", "Check the free space on your system drive. Until it can save, des-k hides nothing.", true, None);
}

/// What a balloon click does: open a page or file through explorer, install a release, check
/// for updates (the hiding warning's), or resume a pause (spec 0039).
#[derive(Clone, Debug)]
enum Click {
    Open(String),
    Install(Version),
    Check,
    Resume,
}

/// A balloon from the icon, and what a click on it does. Sets the tooltip too, so the broken
/// one stays (BROKEN is set before its balloon).
fn balloon(title: &str, text: &str, warn: bool, click: Option<Click>) {
    BALLOON.replace(click);
    // No taskbar yet: no icon to carry it, so it waits for the icon. The latest one wins, as a
    // new balloon replaces the one showing.
    if WAITING.get() {
        HELD.replace(Some((title.into(), text.into(), warn)));
        return;
    }
    let Some(h) = WND.get() else { return };
    show(h, title, text, warn);
}

fn show(h: HWND, title: &str, text: &str, warn: bool) {
    let mut nid = data(h);
    // Without NIF_SHOWTIP, version 4 hides the tooltip from this balloon on.
    nid.uFlags = NIF_TIP | NIF_INFO | NIF_SHOWTIP;
    nid.dwInfoFlags = if warn { NIIF_WARNING } else { NIIF_INFO };
    copy(&mut nid.szTip, &tip());
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    unsafe {
        if !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            warn!("tray: balloon {title:?} not shown");
        }
    }
}

/// des-k's version: the one update.rs reads, else the one it was built as.
fn version() -> String {
    update::ours().map_or_else(|| env!("CARGO_PKG_VERSION").to_string(), |v| v.to_string())
}

/// The update check's answer (spec 0015). The worker already logged it. While the broken
/// warning shows, a balloon would replace it, so the answer goes on the menu's item instead
/// (spec 0015 AC-9, spec 0017 AC-TRY-7), unless the warning's own click asked (spec 0030 AC-7).
fn answered(a: Answer) {
    let ours = version();
    if BROKEN.get() || matches!(a, Answer::Newer(_)) {
        ANSWER.set(Some(a));
    }
    if BROKEN.get() && !ASKED.replace(false) {
        return;
    }
    // Found again after a refusal: say so again, never download it (spec 0031 AC-5).
    if let Answer::Newer(v) = a
        && let Some(f) = refused(v)
    {
        return refused_balloon(v, f);
    }
    match a {
        Answer::Newer(v) if install::can_install() => balloon("des-k update", &format!("des-k {v} is out (you have {ours}). Click here to install it."), false, Some(Click::Install(v))),
        Answer::Newer(v) => balloon("des-k update", &format!("des-k {v} is out (you have {ours}). Click here to open its page."), false, Some(Click::Open(update::page(v)))),
        Answer::Newest(_) => balloon("des-k is up to date", &format!("You have the newest des-k ({ours})."), false, None),
        Answer::Failed(f) => balloon("Could not check for updates", &f.words(), true, None),
    }
}

/// `setup.log` when the setup wrote one, else the release's page (spec 0018 AC-8).
fn log_or_page(v: Version) -> Click {
    Click::Open(install::setup_log().map_or_else(|| update::page(v), |p| p.to_string_lossy().into_owned()))
}

/// The kind `v` was refused with this run, if it was.
fn refused(v: Version) -> Option<InstallFail> {
    install::refused().filter(|&(r, _)| r == v).map(|(_, f)| f)
}

/// A release that failed the publisher's proof (spec 0031 AC-5): no click, and never its page,
/// which is exactly what an attacker would control.
fn refused_balloon(v: Version, f: InstallFail) {
    balloon(&format!("des-k refused {v}"), &f.reason(), true, None);
}

/// The install worker's word (spec 0018). Started only changes the item, which reads the
/// phase. A failure says why, never over the broken warning (AC-9): the log has it, and the
/// item offers the install again, or says it was refused (spec 0031).
fn install_said(m: Msg, v: Version) {
    let Msg::Failed(f) = m else { return };
    if BROKEN.get() {
        return info!("tray: no balloon over the hiding warning: the install of {v} failed ({f:?})");
    }
    match f {
        f if f.refused() => refused_balloon(v, f),
        InstallFail::SetupStopped => balloon("des-k was not updated", &format!("The update to {v} did not install. Click here to open the setup's log."), true, Some(log_or_page(v))),
        InstallFail::Slow => balloon("des-k was not updated", "The setup is taking unusually long. des-k carries on.", true, Some(log_or_page(v))),
        f => balloon(&format!("Could not install des-k {v}"), &format!("{} Click here to open its page.", f.reason()), true, Some(Click::Open(update::page(v)))),
    }
}

/// At start: what the last install did, once (spec 0018 AC-7, AC-8, AC-9).
pub fn installed(o: Outcome) {
    let (title, text, warn, click) = match o {
        Outcome::Nothing => return,
        Outcome::Updated(v) => ("des-k updated", format!("You now have des-k {v}."), false, None),
        Outcome::NotUpdated(v) => ("des-k was not updated", format!("The update to {v} did not install. Click here to open the setup's log."), true, Some(log_or_page(v))),
    };
    if BROKEN.get() {
        return info!("tray: no balloon over the hiding warning: {title}: {text}");
    }
    balloon(title, &text, warn, click);
}

/// Four rounded squares in one blend, purple at the bottom left to red at the top right.
/// Both colors are strong enough to read on light and dark taskbars.
pub fn draw_icon() -> Option<HICON> {
    let s = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let px = if crate::ws::pause::paused() { gray(&logo_pixels(s)) } else { logo_pixels(s) };
    // Ignored when the color bitmap has alpha, but CreateIconIndirect requires one.
    let mask = vec![0u8; ((s + 15) / 16 * 2 * s) as usize];
    unsafe {
        let color = CreateBitmap(s, s, 1, 32, Some(px.as_ptr().cast()));
        let hmask = CreateBitmap(s, s, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO { fIcon: BOOL(1), hbmMask: hmask, hbmColor: color, ..Default::default() };
        let icon = CreateIconIndirect(&info); // copies both bitmaps
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(hmask.into());
        icon.inspect_err(|e| warn!("tray: icon not drawn: {e}")).ok()
    }
}

/// The logo `s` px square, 0xAARRGGBB, top row first.
fn logo_pixels(s: i32) -> Vec<u32> {
    const PURPLE: [u32; 3] = [0x8E, 0x3F, 0xD9];
    const RED: [u32; 3] = [0xE5, 0x39, 0x35];
    let span = (2 * (s - 1)) as u32;
    let blend = |x: i32, y: i32| {
        let t = (x + s - 1 - y) as u32; // 0 at the bottom left, span at the top right
        let c = |i: usize| (PURPLE[i] * (span - t) + RED[i] * t) / span;
        0xFF00_0000 | c(0) << 16 | c(1) << 8 | c(2) // 0xAARRGGBB
    };
    let gap = (s / 8).max(2);
    let q = (s - gap) / 2;
    let off = (s - 2 * q - gap) / 2;
    let mut px = vec![0u32; (s * s) as usize];
    for oy in [off, off + q + gap] {
        for ox in [off, off + q + gap] {
            for dy in 0..q {
                for dx in 0..q {
                    let (ex, ey) = (dx == 0 || dx == q - 1, dy == 0 || dy == q - 1);
                    if ex && ey {
                        continue; // corner pixel: rounds the square
                    }
                    px[((oy + dy) * s + ox + dx) as usize] = blend(ox + dx, oy + dy);
                }
            }
        }
    }
    px
}

/// The logo in gray, for a paused des-k (spec 0039 AC-5): each pixel's own lightness, so the
/// squares keep their shape and blend, and alpha stays as it was.
fn gray(px: &[u32]) -> Vec<u32> {
    px.iter()
        .map(|&p| {
            let ch = |shift: u32| (p >> shift & 0xFF) as f32;
            let l = (0.299 * ch(16) + 0.587 * ch(8) + 0.114 * ch(0)).round() as u32;
            p & 0xFF00_0000 | l << 16 | l << 8 | l
        })
        .collect()
}

/// A workspace chip's geometry, in px of a 16 px icon (spec 0025 AC-10). Not wired to the
/// icon yet: the design pass renders the candidates, and the approved one is wired in task 2.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct Shape {
    /// The chip's margin inside the icon, and its corner radius (half the side: a circle).
    pub(crate) inset: f32,
    pub(crate) radius: f32,
    /// The digit's em size, and whether it is Segoe UI Variable's Small cut, drawn for small text.
    pub(crate) em: f32,
    pub(crate) small: bool,
    /// The attention dot's center and radius, and the clear ring cut around it.
    pub(crate) dot: (f32, f32, f32),
    pub(crate) gap: f32,
}

/// A pixel being drawn: straight (not premultiplied) red, green, blue, alpha, 0 to 1.
type Px = [f32; 4];

fn unpack(c: u32) -> [f32; 3] {
    [(c >> 16 & 0xFF) as f32 / 255.0, (c >> 8 & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0]
}

/// `c` at coverage `a` laid over `d`.
fn over(d: &mut Px, c: u32, a: f32) {
    let out = a + d[3] * (1.0 - a);
    if out > 0.0 {
        let c = unpack(c);
        for i in 0..3 {
            d[i] = (c[i] * a + d[i] * d[3] * (1.0 - a)) / out;
        }
    }
    d[3] = out;
}

/// Straight alpha 0xAARRGGBB, as an icon takes it: premultiplied edges would draw a dark fringe.
fn pack(p: &[Px]) -> Vec<u32> {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    p.iter().map(|d| ch(d[3]) << 24 | ch(d[0]) << 16 | ch(d[1]) << 8 | ch(d[2])).collect()
}

/// How much of pixel x, y a rounded box covers (center, half side, radius, all in px): 1 inside,
/// 0 outside, smooth across the one pixel on its edge.
fn cover(x: i32, y: i32, c: f32, half: f32, r: f32, grow: f32) -> f32 {
    let (qx, qy) = ((x as f32 + 0.5 - c).abs() - (half - r), (y as f32 + 0.5 - c).abs() - (half - r));
    let d = (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)) - r - grow;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Windows' own hinted grayscale glyph for `ch`, `em` px: its width, height and coverage (0 to 64),
/// row by row. The same shape Windows draws grey smoothed text with, so a 9 px digit stays sharp.
fn glyph(ch: char, em: f32, small: bool) -> Option<(i32, i32, Vec<u8>)> {
    let face = match (crate::ui::fonts::variable(), small) {
        (true, true) => "Segoe UI Variable Small Semibold",
        (true, false) => "Segoe UI Variable Display Semibold",
        (false, _) => "Segoe UI Semibold",
    };
    let font = crate::ui::fonts::font(PCWSTR(windows::core::HSTRING::from(face).as_ptr()), em, 600);
    let one = FIXED { fract: 0, value: 1 };
    let mat = MAT2 { eM11: one, eM22: one, ..Default::default() };
    let mut gm = GLYPHMETRICS::default();
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, font.into());
        let size = GetGlyphOutlineW(dc, ch as u32, GGO_GRAY8_BITMAP, &mut gm, 0, None, &mat);
        let mut buf = vec![0u8; if size == GDI_ERROR as u32 { 0 } else { size as usize }];
        let got = !buf.is_empty() && GetGlyphOutlineW(dc, ch as u32, GGO_GRAY8_BITMAP, &mut gm, size, Some(buf.as_mut_ptr().cast()), &mat) != GDI_ERROR as u32;
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(font.into());
        if !got {
            return None;
        }
        // Rows are padded to whole DWORDs.
        let (w, h) = (gm.gmBlackBoxX as i32, gm.gmBlackBoxY as i32);
        let pitch = ((w + 3) & !3) as usize;
        Some((w, h, (0..h as usize).flat_map(|y| buf[y * pitch..y * pitch + w as usize].to_vec()).collect()))
    }
}

/// The chip for workspace `n` (1 to 9), `size` px square: `fill` with `n` in `ink`, and in light
/// a one pixel `edge` inside its rim. None when Windows gives no glyph.
#[allow(dead_code)]
pub(crate) fn chip_pixels(size: i32, sh: &Shape, fill: u32, ink: u32, edge: Option<u32>, n: usize) -> Option<Vec<u32>> {
    let s = size as f32 / 16.0;
    let (c, half, r) = (size as f32 / 2.0, (8.0 - sh.inset) * s, sh.radius * s);
    let rim = s.floor().max(1.0);
    let mut px = vec![[0.0f32; 4]; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let a = cover(x, y, c, half, r, 0.0);
            let p = &mut px[(y * size + x) as usize];
            match edge {
                Some(e) => {
                    over(p, e, a);
                    let inner = cover(x, y, c, half, r, -rim);
                    let k = unpack(fill);
                    let e = unpack(e);
                    for i in 0..3 {
                        p[i] = e[i] + (k[i] - e[i]) * inner;
                    }
                }
                None => over(p, fill, a),
            }
        }
    }
    // Whole pixels, centered on the glyph's own ink: the hinted shape stays on the grid.
    let (w, h, g) = glyph(char::from_digit(n as u32, 10)?, sh.em * s, sh.small)?;
    let (left, top) = (((size - w) as f32 / 2.0).round() as i32, ((size - h) as f32 / 2.0).round() as i32);
    for gy in 0..h {
        for gx in 0..w {
            let (x, y) = (left + gx, top + gy);
            let k = g[(gy * w + gx) as usize] as f32 / 64.0;
            if k > 0.0 && (0..size).contains(&x) && (0..size).contains(&y) {
                let p = &mut px[(y * size + x) as usize];
                let i = unpack(ink);
                for j in 0..3 {
                    p[j] += (i[j] - p[j]) * k;
                }
            }
        }
    }
    Some(pack(&px))
}

/// The attention dot in `color` on an icon's pixels, with a clear ring cut around it so it
/// reads on any chip and any taskbar (spec 0025 AC-6).
#[allow(dead_code)]
pub(crate) fn with_dot(icon: &[u32], size: i32, sh: &Shape, color: u32) -> Vec<u32> {
    let s = size as f32 / 16.0;
    let (cx, cy, r) = (sh.dot.0 * s, sh.dot.1 * s, sh.dot.2 * s);
    let round = |x: i32, y: i32, r: f32| {
        let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) - r;
        (0.5 - d).clamp(0.0, 1.0)
    };
    let mut px: Vec<Px> = icon.iter().map(|&c| {
        let [r, g, b] = unpack(c);
        [r, g, b, (c >> 24) as f32 / 255.0]
    }).collect();
    for y in 0..size {
        for x in 0..size {
            let p = &mut px[(y * size + x) as usize];
            p[3] *= 1.0 - round(x, y, r + sh.gap * s);
            over(p, color, round(x, y, r));
        }
    }
    pack(&px)
}

/// What the menu shows now (spec 0017 AC-TRY-2 to AC-TRY-7).
fn menu() -> Menu {
    let ours = update::ours();
    let check = match (install::phase(), ANSWER.get()) {
        (Phase::Downloading(v), _) => Check::Downloading(v),
        (Phase::Installing(v), _) => Check::Installing(v),
        _ if update::running() => Check::Busy,
        (_, Some(Answer::Newer(v))) => match refused(v) {
            Some(f) => Check::Refused(v, f),
            None if install::can_install() => Check::Install(v),
            None => Check::Newer(v),
        },
        (_, Some(Answer::Newest(v))) => Check::Newest(ours.unwrap_or(v)),
        (_, Some(Answer::Failed(f))) => Check::Failed(f),
        (_, None) => Check::Ready,
    };
    // With Windows' gestures off, quitting leaves the touchpad with none: say for how long.
    let st = crate::setup::state();
    let quit = match (st.off, st.saved) {
        (true, true) => Quit::Saved,
        (true, false) => Quit::Stops,
        _ => Quit::Plain,
    };
    Menu {
        version: version(),
        screens: crate::ws::views::tray_screens(),
        broken: BROKEN.get(),
        hints_key: crate::settings::get().hints.first().copied(),
        paused: crate::ws::pause::paused(),
        // Windows' gestures stay off while paused, so a swipe does nothing (spec 0039 AC-4).
        swipes_off: crate::touchpad::present() && st.off,
        // Debug builds with a console log there instead of to the file.
        log: log_path().exists(),
        check,
        quit,
    }
}

fn log_path() -> std::path::PathBuf {
    crate::ws::store::data_dir().join("des-k.log")
}

/// Runs what you picked in the menu, which has closed already.
pub fn act(id: Id) {
    let Some(hwnd) = WND.get() else { return };
    match id {
        Id::Hints => crate::hints::show(),
        Id::Settings => crate::settings_page::open(None),
        Id::Log => open(&log_path()),
        Id::Update => match ANSWER.get() {
            // A refused release is neither installed nor linked; its item checks again (spec 0031).
            Some(Answer::Newer(v)) if refused(v).is_none() && install::can_install() => install::start(hwnd, v),
            Some(Answer::Newer(v)) if refused(v).is_none() => open(Path::new(&update::page(v))),
            // A check you start answers anew: the old answer goes.
            _ => {
                ANSWER.set(None);
                update::check(hwnd);
            }
        },
        Id::Pause => crate::ws::pause::toggle(),
        // Main's loop breaks on WM_CLOSE, the same path as `des-k --quit`.
        Id::Quit => unsafe { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); },
    }
}

/// Opened by explorer, so the editor runs as you even when des-k runs as admin (spec 0008).
/// Never from des-k itself: an admin ShellExecute follows file associations anything running
/// as you can rewrite. Also takes a settings page like `ms-settings:devices-touchpad`.
pub fn open(path: &Path) {
    if let Err(e) = unsafe { open_by_explorer(path) } {
        warn!("tray: explorer did not open {} ({e}); open it yourself, or try again once explorer runs", path.file_name().unwrap_or_default().display());
    }
}

/// The desktop's shell view hands out its IShellDispatch2, whose ShellExecute runs inside
/// explorer. Documented COM, the route Raymond Chen describes for starting unelevated.
unsafe fn open_by_explorer(path: &Path) -> windows::core::Result<()> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let mut loc = VARIANT::default();
        (*loc.Anonymous.Anonymous).vt = VT_I4;
        (*loc.Anonymous.Anonymous).Anonymous.lVal = CSIDL_DESKTOP as i32;
        let empty = VARIANT::default();
        let mut hwnd = 0;
        let desktop = windows.FindWindowSW(&loc, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
        let browser: IShellBrowser = desktop.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
        // The view only hands out IDispatch: asking for IShellFolderViewDual directly fails with E_NOINTERFACE.
        let view: IShellFolderViewDual = browser.QueryActiveShellView()?.GetItemObject::<IDispatch>(SVGIO_BACKGROUND)?.cast()?;
        let shell: IShellDispatch2 = view.Application()?.cast()?;
        shell.ShellExecute(&BSTR::from(path.to_string_lossy().as_ref()), &empty, &empty, &empty, &empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{self, Theme, color::mix};
    use windows::Win32::Graphics::GdiPlus as gp;

    /// The design pass's candidates (spec 0025 build plan task 1): the approved one stays.
    const SHAPES: [(&str, Shape); 3] = [
        ("tile", Shape { inset: 1.0, radius: 3.0, em: 12.0, small: false, dot: (14.0, 2.0, 2.0), gap: 1.0 }),
        ("full", Shape { inset: 0.0, radius: 3.5, em: 13.0, small: false, dot: (14.0, 2.0, 2.0), gap: 1.0 }),
        ("round", Shape { inset: 0.5, radius: 7.5, em: 12.0, small: false, dot: (14.0, 2.0, 2.0), gap: 1.0 }),
    ];

    /// Saves straight alpha pixels as a see through PNG.
    fn save(w: i32, h: i32, px: &[u32], path: &std::path::Path) {
        unsafe {
            let mut image = std::ptr::null_mut();
            gp::GdipCreateBitmapFromScan0(w, h, w * 4, 0x0026_200A, Some(px.as_ptr().cast()), &mut image); // straight ARGB
            let png = windows::core::GUID::from_u128(0x557cf406_1a04_11d3_9a73_0000f81ef32e);
            let status = gp::GdipSaveImageToFile(image.cast(), &windows::core::HSTRING::from(path.as_os_str()), &png, std::ptr::null());
            assert_eq!(status.0, 0, "{}", path.display());
            gp::GdipDisposeImage(image.cast());
        }
    }

    /// One theme's cells: workspaces 1 to 9, 3 with the dot, your deep blue and pale yellow
    /// (spec 0020's render colors), the logo, the logo with the dot.
    fn cells(size: i32, sh: &Shape, t: &Theme, yours: &Theme) -> Vec<Vec<u32>> {
        let chip = |t: &Theme, i: usize| {
            let b = t.b;
            let edge = (b.ws_edge > 0.0).then(|| mix(b.ws[i], 0, b.ws_edge));
            chip_pixels(size, sh, b.ws[i], b.on_ws[i], edge, i + 1).expect("a glyph")
        };
        let mut v: Vec<Vec<u32>> = (0..9).map(|i| chip(t, i)).collect();
        v.push(with_dot(&chip(t, 2), size, sh, t.b.acc));
        v.push(chip(yours, 0));
        v.push(chip(yours, 1));
        v.push(logo_pixels(size));
        v.push(with_dot(&logo_pixels(size), size, sh, t.b.acc));
        v
    }

    /// Spec 0039 AC-5: the logo running and paused, side by side, at each tray size, in
    /// target/tray/icon/paused-<size>.png. Gray must still read as des-k's four squares.
    #[test]
    #[ignore]
    fn render_paused_icon() {
        ui::canvas::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tray").join("icon");
        std::fs::create_dir_all(&dir).unwrap();
        for size in [16, 20, 24, 32] {
            let (on, off) = (logo_pixels(size), gray(&logo_pixels(size)));
            let w = size * 3;
            let mut strip = vec![0u32; (w * size) as usize];
            for y in 0..size as usize {
                let (row, s) = (y * w as usize, size as usize);
                strip[row..row + s].copy_from_slice(&on[y * s..(y + 1) * s]);
                strip[row + 2 * s..row + 3 * s].copy_from_slice(&off[y * s..(y + 1) * s]);
            }
            save(w, size, &strip, &dir.join(format!("paused-{size}.png")));
        }
    }

    // Spec 0039 AC-5: a paused icon is gray, every pixel, yet keeps the logo's shape exactly.
    #[test]
    fn the_paused_icon_is_the_logo_in_gray() {
        let (on, off) = (logo_pixels(16), gray(&logo_pixels(16)));
        for (a, b) in on.iter().zip(&off) {
            assert_eq!(a >> 24, b >> 24, "same shape");
            let (r, g, bl) = (b >> 16 & 0xFF, b >> 8 & 0xFF, b & 0xFF);
            assert!(r == g && g == bl, "gray: {b:#010x}");
        }
        assert_ne!(on, off);
    }

    /// Writes target/tray/icon/<shape>-<theme>-<size>.png strips for the design page
    /// (pdesk-design/tray-number/), one cell a gap of one icon apart.
    #[test]
    #[ignore]
    fn render_icons() {
        ui::canvas::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tray").join("icon");
        std::fs::create_dir_all(&dir).unwrap();
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let (rose, rose_dark) = (ui::tests::rose(true), ui::tests::rose(false));
        let themes = [("light", ui::LIGHT, rose), ("dark", ui::DARK, rose_dark), ("aquatic", aquatic, aquatic), ("desert", desert, desert)];
        for (name, sh) in &SHAPES {
            for (tn, t, yours) in &themes {
                for size in [16, 20, 24, 32] {
                    let cells = cells(size, sh, t, yours);
                    let w = size * (2 * cells.len() as i32 - 1);
                    let mut strip = vec![0u32; (w * size) as usize];
                    for (k, cell) in cells.iter().enumerate() {
                        for y in 0..size {
                            let at = (y * w + 2 * size * k as i32) as usize;
                            strip[at..at + size as usize].copy_from_slice(&cell[(y * size) as usize..((y + 1) * size) as usize]);
                        }
                    }
                    save(w, size, &strip, &dir.join(format!("{name}-{tn}-{size}.png")));
                }
            }
        }
    }

    // Spec 0039 AC-5: at logon des-k starts before the taskbar, which is exactly when a start
    // comes up paused. Its balloon must wait for the icon, click and all, or the one reminder
    // that des-k is paused is lost (found after a reboot, 2026-11-26).
    #[test]
    fn a_balloon_before_the_taskbar_waits_for_the_icon() {
        WAITING.set(true);
        started_paused();
        assert_eq!(HELD.take(), Some(("des-k is paused".into(), "Click to resume".into(), false)));
        assert!(matches!(BALLOON.take(), Some(Click::Resume)), "a click on it still resumes");
        WAITING.set(false);
    }
}
