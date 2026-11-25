// The Shortcut hints window (spec 0017, 0017-shortcut-hints.md): every des-k shortcut and
// gesture as settings.toml has them now, drawn to the approved design (pdesk-design
// restart/hints-r3.html), with its own key set, changed or removed in the footer through the
// settings page's recorder rules. Opens from the tray menu or its key, centred on the screen
// under the mouse. Owned by the tray's hidden window, so it is never in the taskbar or Alt+Tab.
// Lives on the main thread; main's loop passes its messages through IsDialogMessageW
// (`key`), so Tab, Enter and Esc work. Sizes are in dip (1/96 inch).
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::{UIA_PaneControlTypeId, UIA_TextControlTypeId},
        Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::{GetActiveWindow, GetFocus, IsWindowEnabled, SetFocus, VIRTUAL_KEY, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT},
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::controls::record;
use crate::settings::{self, Action, Combo, Motion, Owner, Settings, edit::Edit};
use crate::setup::{self, GaveBack, Row as PadRow, State};
use crate::ui::{self, Bx, Canvas, F, Theme, buffered, dark_title, mix, op};
use crate::uia::{self, Node, Tree};

mod draw;
pub(crate) mod rows;

use draw::{Fit, Place, compose, paint_button};
use rows::{Does, Pad, View, list, pad_words, read_now, recorded_rec};

const CLASS: PCWSTR = w!("des-k-hints");
const TITLE: &str = "Shortcut hints";
// No minimize box: with no taskbar button (AC-HNT-7) a minimized window would have nowhere to come back from.
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_CLIPCHILDREN.0);

// The buttons, created in Tab order (AC-HNT-6). Esc is IDCANCEL, never one of these.
const ID_SET: i32 = 101; // Set key, or Change
const ID_REMOVE: i32 = 102;
const ID_NOTE: i32 = 103; // the touchpad note's: Touchpad setup or Turn reading on
const ID_MOVE: i32 = 104; // the clash's Move it here
const ID_CANCEL: i32 = 105; // recording's and the clash's Cancel
const ID_SETTINGS: i32 = 106;
const IDS: [i32; 6] = [ID_SET, ID_REMOVE, ID_NOTE, ID_MOVE, ID_CANCEL, ID_SETTINGS];

// Sizes from the design: a 960 × 672 window, 640 of it under Windows' title bar.
const W: f32 = 960.0;
const H: f32 = 640.0;
const SIDE: f32 = 32.0;
const MID_TOP: f32 = 88.0; // where the scrolling middle starts, under the title and its line
const LEFT_W: f32 = 480.0; // the keyboard column; the touchpad column is the rest after 16
const RIGHT_X: f32 = SIDE + LEFT_W + 16.0;
const RIGHT_W: f32 = W - SIDE - RIGHT_X;
const HEAD: f32 = 37.0; // a card's tinted header, its line included
const KROW: f32 = 36.0;
const BTN_H: f32 = 32.0;
const CAP: f32 = 22.0;
const RING: f32 = 4.0; // a button's window reaches this far past its pill, for the focus ring
const MIN_H: f32 = 320.0; // the least client height, on the shortest screen

const COPILOT: &str = "The Copilot key works as Right Ctrl, so Win Ctrl keys work with it.";

/// The recorder in the footer (AC-HNT-4): nothing is saved until you choose.
#[derive(Clone, PartialEq, Debug)]
enum Rec {
    Idle,
    Recording(Option<(String, String)>), // a refused key: its title and line
    Clash { combo: Combo, title: String, sub: String, other: Option<(&'static str, Vec<Combo>)> },
}

struct Win {
    hwnd: HWND,
    dpi: u32,
    scale: f32,
    theme: Theme,
    view: View,
    rec: Rec,
    error: Option<String>, // a save that failed
    places: Vec<Place>,
    buttons: Vec<(i32, HWND)>,
    focus: Option<HWND>, // the focused button while the window is not active
    fit: Fit,
    scroll: f32,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
    static TREE: Tree = Tree::default();
}

// Never hold the borrow across a call that sends this window a message: a button redraws
// itself at once on focus, enable and text changes, and that comes back as WM_DRAWITEM.
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&WIN, "hints get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&WIN, "hints set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

// ---- The window ----

impl Win {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn tree(&self) -> Node {
        let rect = |(x, y, w, h): Bx| RECT { left: self.px(x), top: self.px(y), right: self.px(x + w), bottom: self.px(y + h) };
        let text = |name: String, b: Bx| Node { role: UIA_TextControlTypeId, name, rect: rect(b), ..Node::default() };
        let keys = |k: &[Vec<String>]| if k.is_empty() { "No key".to_string() } else { k.iter().map(|c| c.join(" ")).collect::<Vec<_>>().join(", or ") };
        let mid = (0.0, MID_TOP, W, self.fit.shown);
        let mut kids = vec![text(TITLE.into(), (SIDE, 16.0, W - 2.0 * SIDE, 32.0))];
        for r in self.view.around.iter().chain(&self.view.windows) {
            kids.push(text(format!("{}: {}", r.title, keys(&r.keys)), (SIDE, MID_TOP, LEFT_W, self.fit.shown)));
        }
        if self.view.copilot {
            kids.push(text(COPILOT.into(), mid));
        }
        let (status, words, _) = pad_words(self.view.pad);
        kids.push(text(format!("Touchpad: {status}"), (RIGHT_X, MID_TOP, RIGHT_W, 20.0)));
        if let Some((_, title, line, _)) = words {
            kids.push(text(format!("{title}. {line}"), (RIGHT_X, MID_TOP, RIGHT_W, self.fit.shown)));
        }
        for (f, acts) in &self.view.cards {
            for g in acts {
                let what = match &g.once {
                    Does::Name(n) => n.clone(),
                    Does::Nothing => "Nothing".into(),
                    Does::Types(k) => format!("Types {}", k.join(" ")),
                };
                let how = if g.motion == Motion::Tap { "tap".to_string() } else { format!("swipe {}", g.motion.word()) };
                let twice = g.twice.as_ref().map_or(String::new(), |t| format!(", {}", t.to_lowercase()));
                kids.push(text(format!("{f} fingers, {how}: {what}{twice}"), (RIGHT_X, MID_TOP, RIGHT_W, self.fit.shown)));
            }
        }
        let opens = if self.view.key.is_empty() { "Opens with: no key".to_string() } else { format!("Opens with {}", keys(&list(&self.view.key))) };
        kids.push(text(opens, (SIDE, self.fit.height - BTN_H - 20.0, 400.0, BTN_H)));
        Node { role: UIA_PaneControlTypeId, name: TITLE.into(), rect: rect((0.0, 0.0, W, self.fit.height)), kids, ..Node::default() }
    }
}

/// The work area of the screen `at` is on, else the window's own screen.
fn work_area(h: HWND, at: Option<RECT>) -> Option<RECT> {
    unsafe {
        let mon = match at {
            Some(r) => MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST),
        };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(mon, &mut mi).as_bool().then_some(mi.rcWork)
    }
}

/// The client height, in dip, that fits under the title bar in `work` px of screen height.
fn client_cap(work: i32, dpi: u32, s: f32) -> f32 {
    let mut r = RECT::default();
    unsafe { let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi); }
    (work - (r.bottom - r.top)) as f32 / s
}

/// Lays the window out again and fits it and its buttons: 640 dip tall, or its screen's work
/// area when that is less (AC-HNT-1); `to` (WM_DPICHANGED's rectangle) moves it too.
fn relayout(to: Option<RECT>) {
    let Some((h, s, dpi)) = get(|w| (w.hwnd, w.scale, w.dpi)) else { return };
    let work = work_area(h, to);
    let cap = work.map_or(H, |r| client_cap(r.bottom - r.top, dpi, s));
    let height = H.min(cap).max(MIN_H);
    let Some((places, buttons, tree)) = set(|w| {
        let fonts = ui::fonts(w.scale);
        let mut places = vec![];
        unsafe {
            let dc = GetDC(None);
            let c = Canvas::new(dc, false, w.scale, w.theme, &fonts);
            w.fit = compose(&c, &mut places, &w.view, &w.rec, w.error.as_deref(), height, w.scroll);
            drop(c);
            ReleaseDC(None, dc);
        }
        w.scroll = w.scroll.clamp(0.0, (w.fit.content - w.fit.shown).max(0.0));
        w.places.clone_from(&places);
        (places, w.buttons.clone(), w.tree())
    }) else {
        return;
    };
    TREE.with(|t| {
        if let Ok(mut n) = t.try_lock() {
            *n = tree;
        }
    });
    let px = |v: f32| (v * s).round() as i32;
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: px(W), bottom: px(height) };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut now = RECT::default();
        let _ = GetWindowRect(h, &mut now);
        let (x, mut y) = to.map_or((now.left, now.top), |t| (t.left, t.top));
        if let Some(wk) = work.filter(|_| wh > now.bottom - now.top || to.is_some()) {
            y = y.min(wk.bottom - wh).max(wk.top);
        }
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        let (mid_top, mid_bottom) = (px(MID_TOP), px(MID_TOP) + get(|w| px(w.fit.shown)).unwrap_or(0));
        for (id, b) in &buttons {
            let Some(p) = places.iter().find(|p| p.id == *id) else {
                let _ = ShowWindow(*b, SW_HIDE);
                continue;
            };
            let (bx, by) = (px(p.bx.0 - RING), px(p.bx.1 - RING));
            let (bw, bh) = (px(p.bx.0 + p.bx.2 + RING) - bx, px(p.bx.1 + p.bx.3 + RING) - by);
            let _ = SetWindowTextW(*b, &HSTRING::from(p.label));
            let _ = MoveWindow(*b, bx, by, bw, bh, true);
            // A button in the middle shows only the part of it inside the middle.
            if p.scrolls {
                let (top, bottom) = ((mid_top - by).max(0), (mid_bottom - by).min(bh));
                if bottom <= top {
                    let _ = ShowWindow(*b, SW_HIDE);
                    continue;
                }
                let rgn = CreateRectRgn(0, top, bw, bottom);
                SetWindowRgn(*b, Some(rgn), true); // the window owns it now
            }
            let _ = ShowWindow(*b, SW_SHOWNA);
            let _ = InvalidateRect(Some(*b), None, false);
        }
        let _ = InvalidateRect(Some(h), None, false);
        // A layout that hid the focused button moves focus to the first one that shows.
        let usable = |b: HWND| buttons.iter().any(|(_, x)| *x == b) && IsWindowVisible(b).as_bool() && IsWindowEnabled(b).as_bool();
        let first = IDS.iter().copied().find(|id| places.iter().any(|p| p.id == *id && !p.scrolls));
        let fallback = buttons.iter().find(|(id, _)| Some(*id) == first).map(|(_, b)| *b);
        if let Some(d) = fallback {
            if GetActiveWindow() == h && !usable(GetFocus()) {
                let _ = SetFocus(Some(d));
            }
            let _ = set(|w| {
                if w.focus.is_some_and(|f| !usable(f)) {
                    w.focus = Some(d);
                }
            });
        }
    }
}

/// Scrolls the middle by `dy` dip.
fn scroll_by(dy: f32) {
    let moved = set(|w| {
        let s = (w.scroll + dy).clamp(0.0, (w.fit.content - w.fit.shown).max(0.0));
        (s != w.scroll).then(|| w.scroll = s)
    });
    if moved.flatten().is_some() {
        relayout(None);
    }
}

/// Settings or Windows' touchpad state changed: show it at once, if the window is open
/// (settings::store::reload, the touchpad thread, getting active).
pub fn refresh() {
    if hwnd().is_none() {
        return;
    }
    let view = read_now();
    if set(|w| std::mem::replace(&mut w.view, view.clone()) != view) == Some(true) {
        relayout(None);
    }
}

/// Main loop: the window's keys (Tab, Enter, Esc) through the dialog manager, and Page Up and
/// Page Down scrolling the middle.
pub fn key(msg: &MSG) -> bool {
    let Some(h) = hwnd() else { return false };
    unsafe {
        if msg.message == WM_KEYDOWN && (msg.hwnd == h || IsChild(h, msg.hwnd).as_bool()) {
            let page = get(|w| (w.fit.shown - 40.0).max(20.0)).unwrap_or(0.0);
            let dy = match VIRTUAL_KEY(msg.wParam.0 as u16) {
                VK_PRIOR => Some(-page),
                VK_NEXT => Some(page),
                _ => None,
            };
            if let Some(dy) = dy {
                scroll_by(dy);
                return true;
            }
        }
        let took = IsDialogMessageW(h, msg).as_bool();
        if took {
            reveal_focus();
        }
        took
    }
}

/// Tab moved focus to the touchpad note's button while it is scrolled out of view: scroll to it.
fn reveal_focus() {
    let focused = unsafe { GetFocus() };
    let dy = get(|w| {
        let note = w.buttons.iter().find(|(id, b)| *id == ID_NOTE && *b == focused)?;
        let p = w.places.iter().find(|p| p.id == note.0)?;
        let (top, bottom) = (MID_TOP + RING, MID_TOP + w.fit.shown - RING);
        if p.bx.1 < top {
            Some(p.bx.1 - top)
        } else if p.bx.1 + p.bx.3 > bottom {
            Some(p.bx.1 + p.bx.3 - bottom)
        } else {
            None
        }
    });
    if let Some(Some(dy)) = dy {
        scroll_by(dy);
    }
}

/// The screen under the mouse: its monitor and work area.
fn under_mouse() -> (HMONITOR, RECT) {
    let (mon, _) = crate::ws::under_mouse();
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { let _ = GetMonitorInfoW(mon, &mut mi); }
    (mon, mi.rcWork)
}

/// Centres the window in `work`.
fn centre(h: HWND, work: RECT) {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Moves the open window to the screen under the mouse, in front with focus.
fn bring(h: HWND) {
    let (mon, work) = under_mouse();
    unsafe {
        if MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) != mon {
            // Twice: the first move may change its dpi, and so its size.
            let _ = SetWindowPos(h, None, work.left, work.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            centre(h, work);
        }
        let _ = SetForegroundWindow(h);
    }
}

/// Tray menu › Shortcut hints: opens the window, or brings the open one here.
pub fn show() {
    match hwnd() {
        Some(h) => bring(h),
        None => open(),
    }
}

/// Its key (WM_HINTS): opens it; closes it while it is open on the screen under the mouse,
/// and moves it here while it is open on another screen (AC-HNT-2).
pub fn toggle() {
    let Some(h) = hwnd() else { return open() };
    let (mon, _) = under_mouse();
    if unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) } == mon {
        close(h);
    } else {
        bring(h);
    }
}

fn open() {
    ui::gdiplus();
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc); // fails harmlessly when the window opens a second time
        let (_, work) = under_mouse();
        // Owned by the tray's hidden window: never in the taskbar or Alt+Tab (AC-HNT-7).
        let owner = crate::tray::window();
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, &HSTRING::from(TITLE), STYLE, work.left, work.top, 100, 100, owner, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return warn!("hints: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let style = WINDOW_STYLE(WS_CHILD.0 | WS_TABSTOP.0 | BS_OWNERDRAW as u32);
        let make = |id: i32| {
            let menu = HMENU(id as isize as *mut core::ffi::c_void);
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!(""), style, 0, 0, 0, 0, Some(h), Some(menu), Some(inst.into()), None).ok().map(|b| (id, b))
        };
        let buttons: Vec<(i32, HWND)> = IDS.into_iter().filter_map(make).collect();
        let dpi = GetDpiForWindow(h);
        let theme = ui::theme();
        dark_title(h, &theme);
        ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        let view = read_now();
        WIN.with_borrow_mut(|w| *w = Some(Win { hwnd: h, dpi, scale: dpi as f32 / 96.0, theme, view, rec: Rec::Idle, error: None, places: vec![], buttons, focus: None, fit: Fit::default(), scroll: 0.0 }));
        relayout(None);
        centre(h, work);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
    info!("hints: opened");
}

/// Esc (with nothing to cancel), the title bar's X, or its key.
fn close(h: HWND) {
    unsafe { let _ = DestroyWindow(h); }
}

/// Starts recording a key for the window; the hook swallows every key until it stops.
fn start_rec() {
    let Some(h) = hwnd() else { return };
    crate::settings_page::recording::cancel_recording(); // one recording at a time
    crate::welcome::cancel_recording();
    let _ = set(|w| {
        w.rec = Rec::Recording(None);
        w.error = None;
    });
    record::start(Some(h));
    relayout(None);
    focus_button(ID_CANCEL);
}

/// Ends recording or the clash prompt, saving nothing.
fn stop_rec() {
    let Some(h) = hwnd() else { return };
    record::stop(Some(h));
    let _ = set(|w| w.rec = Rec::Idle);
    relayout(None);
    focus_button(ID_SET);
}

/// The settings page starts recording, so a recording here stops (AC-HNT-4).
pub fn cancel_recording() {
    if get(|w| w.rec != Rec::Idle) == Some(true) {
        stop_rec();
    }
}

fn focus_button(id: i32) {
    if let Some(Some(b)) = get(|w| w.buttons.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)) {
        unsafe { let _ = SetFocus(Some(b)); }
    }
}

/// Saves an edit; a failure shows in the footer until the next action.
fn save(edit: Edit) {
    let failed = settings::edit::write(edit).err();
    let _ = set(|w| w.error = failed);
    refresh();
    relayout(None);
}

/// Whether the hints window is the one recording: main hands it WM_RECORDED.
pub fn owns(h: HWND) -> bool {
    hwnd() == Some(h)
}

/// The settings page's refusals, two of them in this window's own words.
const WORDS: record::Words = record::Words { not_savable: ("This key can't be saved; try another", "Use a letter, a number row digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Still recording."), no_mods: ("Hold Win, Ctrl, Alt or Shift with the key.", "A combo needs at least one of them. Still recording; Esc alone cancels."), ..record::WORDS };

/// The hook's WM_RECORDED while this window records: the settings page's rules (spec 0012
/// AC-7, AC-9). Main thread.
pub fn recorded(vk: u16, mods: u8) {
    if !matches!(get(|w| w.rec.clone()), Some(Rec::Recording(_))) {
        return;
    }
    if vk == VK_ESCAPE.0 && mods == 0 {
        return stop_rec();
    }
    if let Err((title, sub)) = record::check(vk, mods, record::Kind::Combo, &WORDS) {
        let _ = set(|w| w.rec = Rec::Recording(Some((title.into(), sub.into()))));
        return relayout(None);
    }
    let Some(h) = hwnd() else { return };
    record::stop(Some(h));
    let combo = Combo { mods, vk };
    match recorded_rec(&settings::get(), combo) {
        None => stop_rec(),
        Some(Rec::Idle) => {
            let _ = set(|w| w.rec = Rec::Idle);
            save(Edit::Combos("shortcuts.hints", vec![combo]));
            focus_button(ID_SET);
        }
        Some(clash) => {
            let _ = set(|w| w.rec = clash);
            relayout(None);
            focus_button(ID_CANCEL); // as the settings page's prompt: Cancel has focus
        }
    }
}

/// Move it here: the key comes here and the other shortcut loses it, in one write.
fn move_here() {
    let Some(Rec::Clash { combo, other, .. }) = get(|w| w.rec.clone()) else { return };
    let mine = Edit::Combos("shortcuts.hints", vec![combo]);
    let edit = match other {
        Some((key, rest)) => Edit::Pair(Box::new(mine), Box::new(Edit::Combos(key, rest))),
        None => mine,
    };
    let _ = set(|w| w.rec = Rec::Idle);
    save(edit);
    focus_button(ID_SET);
}

fn on_command(h: HWND, id: i32) {
    match id {
        ID_SET => start_rec(),
        ID_REMOVE => save(Edit::Combos("shortcuts.hints", vec![])),
        ID_NOTE if matches!(get(|w| w.view.pad), Some(Pad::Off | Pad::OffWin)) => save(Edit::Value("touchpad.enabled", true.into())),
        ID_NOTE => crate::settings_page::open(Some(crate::settings_page::rows::Section::Touchpad)),
        ID_MOVE => move_here(),
        ID_CANCEL => stop_rec(),
        ID_SETTINGS => crate::settings_page::open(Some(crate::settings_page::rows::Section::Shortcuts)),
        // Esc: backs out of the clash prompt first, like a dialog; else closes.
        _ if id == IDCANCEL.0 => match get(|w| w.rec.clone()) {
            Some(Rec::Idle) | None => close(h),
            Some(_) => stop_rec(),
        },
        _ => {}
    }
}

fn on_draw_item(dis: &DRAWITEMSTRUCT) {
    let _ = get(|w| {
        let Some(p) = w.places.iter().find(|p| p.id == dis.CtlID as i32) else { return };
        let r = dis.rcItem;
        let (pw, ph) = (r.right - r.left, r.bottom - r.top);
        let state = dis.itemState;
        let fonts = ui::fonts(w.scale);
        buffered(dis.hDC, r.left, r.top, pw, ph, |dc| {
            let c = Canvas::new(dc, true, w.scale, w.theme, &fonts);
            let (bw, bh) = (pw as f32 / w.scale, ph as f32 / w.scale);
            c.fill_rr((0.0, 0.0, bw + 1.0, bh + 1.0), 0.0, op(p.back));
            let focus = state.0 & ODS_FOCUS.0 != 0 && state.0 & ODS_NOFOCUSRECT.0 == 0;
            paint_button(&c, p, (RING, RING, bw - 2.0 * RING, bh - 2.0 * RING), state.0 & ODS_SELECTED.0 != 0, focus);
        });
    });
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(h, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let _ = get(|w| {
                    let fonts = ui::fonts(w.scale);
                    buffered(dc, 0, 0, r.right, r.bottom, |mem| {
                        let c = Canvas::new(mem, true, w.scale, w.theme, &fonts);
                        compose(&c, &mut vec![], &w.view, &w.rec, w.error.as_deref(), w.fit.height, w.scroll);
                    });
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // A button with focus hands the wheel on to here.
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_DRAWITEM => {
                on_draw_item(&*(lp.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
            }
            WM_COMMAND if (wp.0 >> 16) & 0xFFFF == BN_CLICKED as usize => {
                on_command(h, (wp.0 & 0xFFFF) as i32);
                LRESULT(0)
            }
            // Enter presses the focused button, like any Windows dialog; with none, nothing.
            DM_GETDEFID => {
                let focused = GetFocus();
                let id = get(|w| w.buttons.iter().find(|(_, b)| *b == focused).map(|(id, _)| *id)).flatten();
                match id {
                    Some(id) => LRESULT(((DC_HASDEFID as isize) << 16) | id as isize),
                    None => LRESULT(0),
                }
            }
            WM_ACTIVATE => {
                if wp.0 & 0xFFFF == WA_INACTIVE as usize {
                    // Recording ends when the window loses focus, as on the settings page (spec 0012 AC-7).
                    if matches!(get(|w| w.rec.clone()), Some(Rec::Recording(_))) {
                        stop_rec();
                    }
                    let focused = GetFocus();
                    let _ = set(|w| w.focus = w.buttons.iter().any(|(_, b)| *b == focused).then_some(focused));
                } else if (wp.0 >> 16) & 0xFFFF == 0 {
                    refresh(); // back from Windows Settings: the touchpad may read differently
                    if let Some(Some(b)) = get(|w| w.focus) {
                        let _ = SetFocus(Some(b));
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                close(h);
                LRESULT(0)
            }
            WM_DESTROY => {
                crate::keys::record(h, false);
                drop(WIN.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_GETOBJECT => TREE.with(|t| uia::answer_tree(h, wp, lp, t)),
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                let to = *(lp.0 as *const RECT);
                let _ = set(|w| {
                    w.dpi = dpi;
                    w.scale = dpi as f32 / 96.0;
                });
                relayout(Some(to));
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
                let t = ui::theme();
                if set(|w| std::mem::replace(&mut w.theme, t) != t) == Some(true) {
                    dark_title(h, &t);
                    relayout(None);
                }
                DefWindowProcW(h, m, wp, lp)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests;
