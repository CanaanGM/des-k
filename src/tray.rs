// Tray icon near the clock: shows des-k is running, opens des-k's own menu (tray_menu.rs, spec
// 0017) with Shortcut hints, Settings, Open log, Check for updates (spec 0015) and Quit, and
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
    Graphics::Gdi::{CreateBitmap, DeleteObject},
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
    /// What a click on the current balloon does, if anything.
    static BALLOON: RefCell<Option<Click>> = const { RefCell::new(None) };
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
const BROKEN_TIP: &str = "des-k: hiding windows does not work, see the log";

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
                    None => eprintln!("tray: an install message it cannot read ({:#x}, {:#x})", wp.0, lp.0),
                }
                LRESULT(0)
            }
            // Explorer restarted (or the taskbar changed scale): the old icon is gone, and the
            // new taskbar may list hidden windows again.
            _ if msg == TASKBAR_CREATED.get() => {
                add(hwnd);
                crate::ws::resync_soon();
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
                        eprintln!("tray: message {m:#x} not let in from explorer: {e}");
                    }
                }
                WND.set(Some(h));
                add(h);
            }
            Err(e) => eprintln!("tray: window not created: {e}"),
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
    copy(&mut nid.szTip, if BROKEN.get() { BROKEN_TIP } else { TIP });
    unsafe {
        // At login explorer may not be ready yet; its TaskbarCreated brings us back here.
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() && !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            return eprintln!("tray: icon not added");
        }
        // Set again after every add: a restarted explorer forgets it. Version 4 tells a
        // keyboard open from a click (spec 0017 AC-TRY-8).
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if !Shell_NotifyIconW(NIM_SETVERSION, &nid).as_bool() {
            eprintln!("tray: icon version 4 refused, the menu can't tell a keyboard open from a click");
        }
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
/// that stays, so a switch that hides nothing is never a mystery. Main's 1 s check.
pub fn warn_if_broken() {
    if BROKEN.get() || !crate::cloak::broken() {
        return;
    }
    BROKEN.set(true);
    eprintln!("hiding windows does not work: Windows refuses des-k's SetCloak call, most likely after an update changed it. Switching hides nothing until des-k is updated.");
    balloon(
        "des-k cannot hide windows",
        "A Windows update most likely changed what des-k uses to hide windows. Switching hides nothing until des-k is updated. The log says more.",
        true,
        None,
    );
}

/// Screens came or went and windows moved with them (spec 0016 AC-13): one balloon, a line per
/// screen. Never over the warning that hiding does not work: then the indicator and the log say it.
pub fn screens_changed(lines: &[String]) {
    if BROKEN.get() {
        return println!("tray: no balloon over the hiding warning: {}", lines.join("; "));
    }
    balloon("Screens changed", &lines.join("\n"), false, None);
}

/// What a balloon click does: open a page or file through explorer, or install a release.
#[derive(Clone, Debug)]
enum Click {
    Open(String),
    Install(Version),
}

/// A balloon from the icon, and what a click on it does. Sets the tooltip too, so the broken
/// one stays (BROKEN is set before its balloon).
fn balloon(title: &str, text: &str, warn: bool, click: Option<Click>) {
    BALLOON.replace(click);
    let Some(h) = WND.get() else { return };
    let mut nid = data(h);
    // Without NIF_SHOWTIP, version 4 hides the tooltip from this balloon on.
    nid.uFlags = NIF_TIP | NIF_INFO | NIF_SHOWTIP;
    nid.dwInfoFlags = if warn { NIIF_WARNING } else { NIIF_INFO };
    copy(&mut nid.szTip, if BROKEN.get() { BROKEN_TIP } else { TIP });
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    unsafe {
        if !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            eprintln!("tray: balloon {title:?} not shown");
        }
    }
}

/// des-k's version: the one update.rs reads, else the one it was built as.
fn version() -> String {
    update::ours().map_or_else(|| env!("CARGO_PKG_VERSION").to_string(), |v| v.to_string())
}

/// The update check's answer (spec 0015). The worker already logged it. While the broken
/// warning shows, a balloon would replace it, so the answer goes on the menu's item instead
/// (spec 0015 AC-9, spec 0017 AC-TRY-7).
fn answered(a: Answer) {
    let ours = version();
    if BROKEN.get() || matches!(a, Answer::Newer(_)) {
        ANSWER.set(Some(a));
    }
    if BROKEN.get() {
        return;
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

/// The install worker's word (spec 0018). Started only changes the item, which reads the
/// phase. A failure says why, never over the broken warning (AC-9): the log has it, and the
/// item offers the install again.
fn install_said(m: Msg, v: Version) {
    let Msg::Failed(f) = m else { return };
    if BROKEN.get() {
        return println!("tray: no balloon over the hiding warning: the install of {v} failed ({f:?})");
    }
    match f {
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
        return println!("tray: no balloon over the hiding warning: {title}: {text}");
    }
    balloon(title, &text, warn, click);
}

/// Four rounded squares in one blend, purple at the bottom left to red at the top right.
/// Both colors are strong enough to read on light and dark taskbars.
pub fn draw_icon() -> Option<HICON> {
    const PURPLE: [u32; 3] = [0x8E, 0x3F, 0xD9];
    const RED: [u32; 3] = [0xE5, 0x39, 0x35];
    let s = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
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
    // Ignored when the color bitmap has alpha, but CreateIconIndirect requires one.
    let mask = vec![0u8; ((s + 15) / 16 * 2 * s) as usize];
    unsafe {
        let color = CreateBitmap(s, s, 1, 32, Some(px.as_ptr().cast()));
        let hmask = CreateBitmap(s, s, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO { fIcon: BOOL(1), hbmMask: hmask, hbmColor: color, ..Default::default() };
        let icon = CreateIconIndirect(&info); // copies both bitmaps
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(hmask.into());
        icon.inspect_err(|e| eprintln!("tray: icon not drawn: {e}")).ok()
    }
}

/// What the menu shows now (spec 0017 AC-TRY-2 to AC-TRY-7).
fn menu() -> Menu {
    let ours = update::ours();
    let check = match (install::phase(), ANSWER.get()) {
        (Phase::Downloading(v), _) => Check::Downloading(v),
        (Phase::Installing(v), _) => Check::Installing(v),
        _ if update::running() => Check::Busy,
        (_, Some(Answer::Newer(v))) if install::can_install() => Check::Install(v),
        (_, Some(Answer::Newer(v))) => Check::Newer(v),
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
        screens: crate::ws::tray_screens(),
        broken: BROKEN.get(),
        hints_key: crate::settings::get().hints.first().copied(),
        // Debug builds with a console log there instead of to the file.
        log: log_path().exists(),
        check,
        quit,
    }
}

fn log_path() -> std::path::PathBuf {
    crate::ws::data_dir().join("des-k.log")
}

/// Runs what you picked in the menu, which has closed already.
pub fn act(id: Id) {
    let Some(hwnd) = WND.get() else { return };
    match id {
        Id::Hints => crate::hints::show(),
        Id::Settings => crate::settings_page::open(None),
        Id::Log => open(&log_path()),
        Id::Update => match ANSWER.get() {
            Some(Answer::Newer(v)) if install::can_install() => install::start(hwnd, v),
            Some(Answer::Newer(v)) => open(Path::new(&update::page(v))),
            // A check you start answers anew: the old answer goes.
            _ => {
                ANSWER.set(None);
                update::check(hwnd);
            }
        },
        // Main's loop breaks on WM_CLOSE, the same path as `des-k --quit`.
        Id::Quit => unsafe { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); },
    }
}

/// Opened by explorer, so the editor runs as you even when des-k runs as admin (spec 0008).
/// Never from des-k itself: an admin ShellExecute follows file associations anything running
/// as you can rewrite. Also takes a settings page like `ms-settings:devices-touchpad`.
pub fn open(path: &Path) {
    if let Err(e) = unsafe { open_by_explorer(path) } {
        eprintln!("tray: explorer did not open {} ({e}); open it yourself, or try again once explorer runs", path.display());
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
