// The tray menu (spec 0017, 0017-tray-menu.md): des-k's own look B popup at the tray icon in
// place of Windows' menu, drawn to the approved design (pdesk-design restart/tray-r4.html):
// des-k's version, your screens (chips, or a map from three on), the hiding warning, and the
// items. Not modal: any des-k action, a lost focus or a second click on the icon closes it.
// Lives on the main thread. tray.rs owns the icon and runs what you pick; this file only shows
// the menu, takes its keys and clicks, and tells Narrator what it shows.
use std::cell::{Cell, RefCell};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::Gdi::*,
    System::SystemInformation::GetTickCount,
    UI::{
        Accessibility::{UIA_MenuClosedEventId, UIA_MenuControlTypeId, UIA_MenuItemControlTypeId, UIA_MenuOpenedEventId, UIA_TextControlTypeId},
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings::Combo;
use crate::ui::{self, Bx, Canvas, F, Theme, mix, op};
use crate::uia::{self, Node, Tree};
use crate::update::{Fail, Version};
use crate::ws::TrayScreen;

const CLASS: PCWSTR = w!("des-k-tray-menu");
const TIP_CLASS: PCWSTR = w!("des-k-tip");
const WHO: &str = "des-k tray menu";
const TIP_TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3; // winuser.h, also in Win32_UI_Controls

// Sizes in dip, from the design.
const W: f32 = 288.0;
const PAD: f32 = 4.0;
const HEAD: f32 = 40.0;
const IN_X: f32 = PAD + 12.0; // the header's, the screens' and an item's content
const RIGHT: f32 = W - PAD - 12.0;
const LABEL_X: f32 = IN_X + 16.0 + 12.0; // after an item's icon
const ITEM: f32 = 32.0;
const MAP_W: f32 = 264.0;
const MAP_H: f32 = 88.0;
const MIN_SIDE: f32 = 8.0; // no screen in the map is drawn smaller (AC-TRY-4)
const BAR: f32 = 3.0; // the bar under the screen the menu is on

/// What you can pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Id {
    Hints,
    Settings,
    Log,
    Update,
    Quit,
}

/// The update item (spec 0015; AC-TRY-7 keeps every answer on it while hiding is broken).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Check {
    Ready,
    Busy,
    Newer(Version),
    Newest(Version), // the one you have
    Failed(Fail),
}

/// What Quit does to the touchpad, from setup::state() (spec 0015 AC-1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Quit {
    Plain,
    Saved, // Windows' gestures come back at the next sign in
    Stops, // none saved: the touchpad has no gestures until you turn Windows' on
}

/// Everything the menu shows, read once when it opens.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Menu {
    pub(crate) version: String,
    pub(crate) screens: Vec<TrayScreen>,
    pub(crate) broken: bool,
    pub(crate) hints_key: Option<Combo>,
    pub(crate) log: bool,
    pub(crate) check: Check,
    pub(crate) quit: Quit,
}

/// One item as drawn: its icon, words, key caps or New, and whether it works now.
struct Item {
    id: Id,
    icon: &'static str,
    label: String,
    line: String,
    keys: Vec<String>,
    new: bool,
    on: bool,
}

/// "Couldn't check: no connection to GitHub." from the balloon's words.
fn failed_line(f: Fail) -> String {
    let words = f.words();
    let what = words.trim_end_matches(" des-k carries on as before.");
    // "GitHub did not answer in time." keeps its capital: it is a name.
    let what = if what.starts_with("GitHub") {
        what.to_string()
    } else {
        let mut c = what.chars();
        c.next().map_or(String::new(), |f| f.to_lowercase().chain(c).collect())
    };
    format!("Couldn't check: {what}")
}

impl Menu {
    /// The items in order: Shortcut hints and Settings, Open log and the update item, Quit.
    fn items(&self) -> Vec<Item> {
        let item = |id, icon, label: &str| Item { id, icon, label: label.into(), line: String::new(), keys: vec![], new: false, on: true };
        let mut update = item(Id::Update, "\u{E895}", "Check for updates");
        match self.check {
            Check::Ready => {}
            Check::Busy => {
                update.label = "Checking for updates…".into();
                update.on = false;
            }
            Check::Newer(v) => {
                update = Item { icon: "\u{E8A7}", label: format!("Update to {v}"), line: "Opens its page on github.com".into(), new: true, ..update };
            }
            Check::Newest(v) => update.line = format!("You have the newest ({v})."),
            Check::Failed(f) => update.line = failed_line(f),
        }
        let quit = match self.quit {
            Quit::Plain => "Every hidden window comes back.",
            Quit::Saved => "Every hidden window comes back. Windows' touchpad gestures return at your next sign in.",
            Quit::Stops => "Every hidden window comes back. Your touchpad has no gestures until you turn Windows' on.",
        };
        let keys = self.hints_key.map_or_else(Vec::new, |c| c.to_string().split('+').map(String::from).collect());
        vec![
            Item { keys, ..item(Id::Hints, "\u{E765}", "Shortcut hints") },
            item(Id::Settings, "\u{E713}", "Settings"),
            Item { on: self.log, ..item(Id::Log, "\u{E8A5}", "Open log") },
            update,
            Item { line: quit.into(), ..item(Id::Quit, "\u{E7E8}", "Quit des-k") },
        ]
    }
}

/// Where the menu's parts sit, in dip from its top left.
#[derive(Default)]
struct Lay {
    height: f32,
    chips: Vec<(Bx, f32, String)>, // one or two screens: the chip, where its name starts, the name as it fits
    map: Vec<Bx>,                  // three or more: each screen's box
    warn: Option<Bx>,
    rules: Vec<f32>,
    items: Vec<Bx>,
}

const WARN_TITLE: &str = "des-k cannot hide windows";
const WARN_TEXT: &str = "A Windows update most likely changed what des-k uses. Switching hides nothing until des-k is updated.";

/// Each screen's box in the map, from the map's top left, and the map's height: Windows'
/// arrangement fitted into 264 × 88, 2 dip apart, but never a screen under 8 dip on a side:
/// then the whole map grows until none is, and stays centred (AC-TRY-4).
fn map(rects: &[RECT]) -> (Vec<Bx>, f32) {
    let (Some(x0), Some(y0)) = (rects.iter().map(|r| r.left).min(), rects.iter().map(|r| r.top).min()) else { return (vec![], 0.0) };
    let w1 = (rects.iter().map(|r| r.right).max().unwrap_or(0) - x0).max(1) as f32;
    let h1 = (rects.iter().map(|r| r.bottom).max().unwrap_or(0) - y0).max(1) as f32;
    let side = rects.iter().map(|r| (r.right - r.left).min(r.bottom - r.top)).min().unwrap_or(1).max(1) as f32;
    let k = (MAP_W / w1).min(MAP_H / h1).max((MIN_SIDE + 2.0) / side);
    let ox = ((MAP_W - w1 * k) / 2.0).round();
    let boxes = rects
        .iter()
        .map(|r| {
            let (x, y) = (ox + ((r.left - x0) as f32 * k).round() + 1.0, ((r.top - y0) as f32 * k).round() + 1.0);
            (x, y, ((r.right - r.left) as f32 * k).round() - 2.0, ((r.bottom - r.top) as f32 * k).round() - 2.0)
        })
        .collect();
    (boxes, (h1 * k).round())
}

/// The New badge's or the key caps' width on an item's right, 0 for none.
fn right_w(c: &Canvas, it: &Item) -> f32 {
    if it.new {
        return c.width("New", F::SmallBold) + 12.0;
    }
    let caps: f32 = it.keys.iter().map(|k| c.measuring().key_cap(0.0, 0.0, 20.0, k, false) + 4.0).sum();
    (caps - 4.0).max(0.0)
}

/// How wide an item's words may be: up to its caps or New, 12 dip clear of them.
fn words_w(c: &Canvas, it: &Item) -> f32 {
    let r = right_w(c, it);
    RIGHT - LABEL_X - if r > 0.0 { r + 12.0 } else { 0.0 }
}

fn layout(c: &Canvas, m: &Menu) -> Lay {
    let mut l = Lay::default();
    let mut y = PAD + HEAD;
    if m.screens.len() >= 3 {
        let (boxes, h) = map(&m.screens.iter().map(|s| s.rect).collect::<Vec<_>>());
        l.map = boxes.into_iter().map(|(x, by, w, bh)| (IN_X - 4.0 + x, y + by, w, bh)).collect();
        y += h + 10.0;
    } else {
        // Chips with names, wrapping as the design's flex line does.
        let (mut x, mut line) = (IN_X, y);
        for s in &m.screens {
            let n = (s.active + 1).to_string();
            let cw = (c.width(&n, F::SmallBold) + 10.0).max(20.0);
            let name = c.fit(&s.name, F::Small, RIGHT - IN_X - cw - 6.0);
            let span = cw + 6.0 + c.width(&name, F::Small);
            if x > IN_X && x + span > RIGHT {
                (x, line) = (IN_X, line + 18.0 + 4.0);
            }
            l.chips.push(((x, line, cw, 18.0), x + cw + 6.0, name));
            x += span + 12.0;
        }
        y = line + 18.0 + 10.0;
    }
    if m.broken {
        let h = 8.0 + 20.0 + c.measuring().para(0.0, 0.0, W - 16.0 - 24.0 - 28.0, 16.0, &[(WARN_TEXT, F::Small, 0)]) + 8.0;
        l.warn = Some((PAD + 4.0, y, W - 16.0, h));
        y += h + 4.0;
    }
    for (i, it) in m.items().iter().enumerate() {
        if matches!(i, 0 | 2 | 4) {
            l.rules.push(y + 4.0);
            y += 9.0;
        }
        let h = if it.line.is_empty() { ITEM } else { 6.0 + 20.0 + c.measuring().para(0.0, 0.0, words_w(c, it), 16.0, &[(&it.line, F::Small, 0)]) + 6.0 };
        l.items.push((PAD, y, W - 2.0 * PAD, h));
        y += h;
    }
    l.height = y + PAD;
    l
}

struct Session {
    menu: Menu,
    lay: Lay,
    scale: f32,
    theme: Theme,
    hot: Option<usize>,    // the item under the mouse
    focus: Option<usize>,  // the item the keyboard moved to: it gets the ring
    landed: Option<usize>, // the screen the menu is on: its bar in the map
    tip: Option<usize>,    // the map screen under the mouse
    came_from: HWND,
}

impl Session {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x as f32 / self.scale, y as f32 / self.scale);
        self.lay.items.iter().position(|&(bx, by, bw, bh)| x >= bx && x < bx + bw && y >= by && y < by + bh)
    }

    fn screen_at(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x as f32 / self.scale, y as f32 / self.scale);
        self.lay.map.iter().position(|&(bx, by, bw, bh)| x >= bx && x < bx + bw && y >= by && y < by + bh)
    }

    /// A screen as Narrator and its tooltip name it.
    fn screen_words(&self, i: usize, workspace: bool) -> String {
        let s = &self.menu.screens[i];
        let ws = if workspace { format!(", workspace {}", s.active + 1) } else { String::new() };
        format!("{}{ws}{}", s.name, if s.main { ", main display" } else { "" })
    }

    fn tree(&self) -> Node {
        let rect = |(x, y, w, h): Bx| RECT { left: self.px(x), top: self.px(y), right: self.px(x + w), bottom: self.px(y + h) };
        let text = |name: String, b: Bx| Node { role: UIA_TextControlTypeId, name, rect: rect(b), ..Node::default() };
        let mut kids = vec![text(format!("des-k {}", self.menu.version), (PAD, PAD, W - 2.0 * PAD, HEAD))];
        let boxes: Vec<Bx> = if self.lay.map.is_empty() { self.lay.chips.iter().map(|c| c.0).collect() } else { self.lay.map.clone() };
        kids.extend(boxes.into_iter().enumerate().map(|(i, b)| text(self.screen_words(i, true), b)));
        if let Some(b) = self.lay.warn {
            kids.push(text(format!("{WARN_TITLE}. {WARN_TEXT}"), b));
        }
        for (i, (it, b)) in self.menu.items().into_iter().zip(&self.lay.items).enumerate() {
            let keys = it.keys.join("+");
            let help = [it.line.as_str(), keys.as_str()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(". ");
            kids.push(Node { role: UIA_MenuItemControlTypeId, name: it.label, rect: rect(*b), help, greyed: !it.on, focusable: true, focused: self.focus == Some(i), ..Node::default() });
        }
        Node { role: UIA_MenuControlTypeId, name: "des-k".into(), rect: rect((0.0, 0.0, W, self.lay.height)), kids, ..Node::default() }
    }
}

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static TIP: Cell<Option<HWND>> = const { Cell::new(None) };
    static TIP_TEXT: RefCell<String> = const { RefCell::new(String::new()) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static TREE: Tree = Tree::default();
    /// When the menu last closed because it lost focus: the click on the icon that took its
    /// focus must not open it again (AC-TRY-9).
    static LOST: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Never hold the borrow across a call that can send the menu a message.
fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    ui::set(&SESSION, "tray menu", f)
}

pub(crate) fn is_open() -> bool {
    SESSION.with_borrow(Option::is_some)
}

/// The menu closed by losing focus within a double click's time: an open now is the same click.
pub(crate) fn just_lost() -> bool {
    LOST.get().is_some_and(|t| unsafe { GetTickCount().wrapping_sub(t) < GetDoubleClickTime() })
}

/// Opens the menu by the icon: at `at` (where the click was), clear of `icon`, the icon's
/// rectangle; a keyboard open starts from the icon and puts focus on the first item.
pub(crate) fn open(menu: Menu, at: POINT, icon: Option<RECT>, keyboard: bool) {
    let Some(popup) = window() else { return eprintln!("tray menu: window not created") };
    ui::gdiplus();
    let anchor = match (keyboard, icon) {
        (true, Some(r)) => POINT { x: (r.left + r.right) / 2, y: r.top },
        _ => at,
    };
    let (mut dpi, mut dy) = (96u32, 96u32);
    let mon = unsafe { MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST) };
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
    let (scale, theme) = (dpi as f32 / 96.0, ui::system_theme());
    let fonts = ui::fonts(scale);
    let lay = unsafe {
        let dc = GetDC(None);
        let lay = layout(&Canvas::new(dc, false, scale, theme, &fonts), &menu);
        ReleaseDC(None, dc);
        lay
    };
    let size = SIZE { cx: (W * scale).round() as i32, cy: (lay.height * scale).round() as i32 };
    // As Windows places a tray menu: in the work area, above or below the icon, never on it.
    let side = if unsafe { GetSystemMetrics(SM_MENUDROPALIGNMENT) } != 0 { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
    let flags = TPM_WORKAREA | TPM_BOTTOMALIGN | TPM_VERTICAL | side;
    let mut r = RECT::default();
    if unsafe { CalculatePopupWindowPosition(&anchor, &size, flags.0, icon.as_ref().map(|r| r as *const RECT), &mut r) }.is_err() {
        r = RECT { left: anchor.x - size.cx, top: anchor.y - size.cy, right: anchor.x, bottom: anchor.y };
    }
    let landed = unsafe {
        let m = MonitorFromPoint(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        menu.screens.iter().position(|s| s.rect == mi.rcMonitor)
    };
    let came_from = unsafe { GetForegroundWindow() };
    let focus = keyboard.then_some(0);
    SESSION.set(Some(Session { menu, lay, scale, theme, hot: None, focus, landed, tip: None, came_from }));
    LOST.set(None);
    unsafe {
        ui::border(popup, theme.b.edge);
        let _ = SetWindowPos(popup, Some(HWND_TOPMOST), r.left, r.top, size.cx, size.cy, SWP_NOACTIVATE);
        refresh();
        let _ = ShowWindow(popup, SW_SHOW);
        let _ = SetForegroundWindow(popup);
        if GetForegroundWindow() != popup {
            eprintln!("tray menu: Windows refused focus, so it closes when you click elsewhere only");
        }
    }
    TREE.with(|t| uia::raise(popup, t, &[], UIA_MenuOpenedEventId, WHO));
    if keyboard {
        TREE.with(|t| uia::focus_moved(popup, t, WHO));
    }
}

/// Hides the menu. `give_back`: focus goes back where it was (the taskbar, mostly).
pub(crate) fn close(give_back: bool) {
    let Some(s) = SESSION.with_borrow_mut(Option::take) else { return };
    let Some(popup) = WND.get() else { return };
    hide_tip();
    TREE.with(|t| uia::raise(popup, t, &[], UIA_MenuClosedEventId, WHO));
    unsafe {
        let _ = KillTimer(Some(popup), TIP_TIMER);
        // Focus first: hiding the active menu would let Windows pick a window to activate.
        if give_back && IsWindow(Some(s.came_from)).as_bool() {
            let _ = SetForegroundWindow(s.came_from);
        }
        let _ = ShowWindow(popup, SW_HIDE);
    }
}

/// Repaints and gives Narrator what is shown now.
fn refresh() {
    let Some(popup) = WND.get() else { return };
    let Some(tree) = with(|s| s.tree()) else { return };
    TREE.with(|t| {
        if let Ok(mut n) = t.try_lock() {
            *n = tree;
        }
    });
    unsafe { let _ = InvalidateRect(Some(popup), None, false); }
}

/// Runs item `i` if it works now: the menu closes first, then tray.rs does it.
fn run(i: usize) {
    let Some(Some(id)) = with(|s| s.menu.items().get(i).filter(|it| it.on).map(|it| it.id)) else { return };
    close(true);
    crate::tray::act(id);
}

/// Keyboard focus to item `i`: the ring moves there and Narrator reads it.
fn focus(i: usize) {
    let Some(popup) = WND.get() else { return };
    let moved = with(|s| {
        s.hot = None;
        s.focus.replace(i) != Some(i)
    });
    refresh();
    if moved == Some(true) {
        TREE.with(|t| uia::focus_moved(popup, t, WHO));
    }
}

/// Arrows, Home, End, Enter, Space and Esc; a letter only moves to the next item that starts
/// with it, so Q never quits at once (AC-TRY-8).
fn on_key(vk: VIRTUAL_KEY) {
    let Some((n, at, letters)) = with(|s| {
        let items = s.menu.items();
        let letters: Vec<char> = items.iter().map(|it| it.label.chars().next().unwrap_or(' ').to_ascii_uppercase()).collect();
        (items.len(), s.focus.or(s.hot), letters)
    }) else {
        return;
    };
    match vk {
        VK_DOWN => focus(at.map_or(0, |i| (i + 1) % n)),
        VK_UP => focus(at.map_or(n - 1, |i| (i + n - 1) % n)),
        VK_HOME => focus(0),
        VK_END => focus(n - 1),
        VK_RETURN | VK_SPACE => {
            if let Some(i) = at {
                run(i);
            }
        }
        VK_ESCAPE => close(true),
        v if (b'A' as u16..=b'Z' as u16).contains(&v.0) => {
            let from = at.map_or(0, |i| i + 1);
            if let Some(i) = (0..n).map(|k| (from + k) % n).find(|&i| letters[i] as u16 == v.0) {
                focus(i);
            }
        }
        _ => {}
    }
}

fn on_mouse(popup: HWND, x: i32, y: i32) {
    let Some((changed, screen)) = with(|s| {
        let hot = s.item_at(x, y);
        let changed = hot != s.hot || (hot.is_some() && s.focus.is_some());
        if hot.is_some() {
            s.focus = None; // one highlight: the mouse's
        }
        s.hot = hot;
        let screen = s.screen_at(x, y);
        // Some(new screen) when the mouse moved onto another map screen, or off the map.
        let moved = (screen != s.tip).then(|| {
            s.tip = screen;
            screen
        });
        (changed, moved)
    }) else {
        return;
    };
    if changed {
        refresh();
    }
    if let Some(screen) = screen {
        hide_tip();
        unsafe {
            let _ = KillTimer(Some(popup), TIP_TIMER);
            if screen.is_some() {
                SetTimer(Some(popup), TIP_TIMER, GetDoubleClickTime(), None);
            }
        }
    }
    let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: popup, dwHoverTime: 0 };
    unsafe { let _ = TrackMouseEvent(&mut t); }
}

fn on_leave(popup: HWND) {
    if with(|s| {
        s.tip = None;
        s.hot.take().is_some()
    }) == Some(true)
    {
        refresh();
    }
    hide_tip();
    unsafe { let _ = KillTimer(Some(popup), TIP_TIMER); }
}

/// A map screen's name, just under the mouse, after Windows' tooltip wait.
fn show_tip(popup: HWND) {
    unsafe { let _ = KillTimer(Some(popup), TIP_TIMER); }
    let Some(Some((words, scale, theme))) = with(|s| s.tip.map(|i| (s.screen_words(i, false), s.scale, s.theme))) else { return };
    let Some(tip) = tip_window() else { return };
    TIP_TEXT.set(words.clone());
    let fonts = ui::fonts(scale);
    let tw = unsafe {
        let dc = GetDC(None);
        let w = Canvas::new(dc, false, scale, theme, &fonts).width(&words, F::Small);
        ReleaseDC(None, dc);
        w
    };
    let (w, h) = (((tw + 16.0) * scale).round() as i32, (28.0 * scale).round() as i32);
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &mut mi);
        let wk = mi.rcWork;
        let below = p.y + (20.0 * scale) as i32;
        let y = if below + h > wk.bottom { p.y - h - (4.0 * scale) as i32 } else { below };
        let x = p.x.min(wk.right - w).max(wk.left);
        ui::border(tip, theme.b.edge);
        let _ = SetWindowPos(tip, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(tip), None, false);
    }
}

fn hide_tip() {
    if let Some(t) = TIP.get() {
        unsafe { let _ = ShowWindow(t, SW_HIDE); }
    }
}

/// An item's hover or focus color: the menu grey (Highlight in high contrast).
fn hover(t: &Theme) -> u32 {
    let b = t.b;
    if t.hc { b.acc } else { mix(b.card, b.tx, if t.dark { 0.07 } else { 0.06 }) }
}

/// The whole menu at 0, 0.
fn draw(c: &Canvas, s: &Session) {
    let (t, b) = (c.t, c.t.b);
    let m = &s.menu;
    c.fill_rr((0.0, 0.0, W, s.lay.height), 0.0, op(b.card));
    // The header: the mark, des-k, the version.
    c.mark(IN_X, PAD + (HEAD - 16.0) / 2.0);
    c.text(IN_X + 24.0, PAD, HEAD, "des-k", F::Bold, b.tx);
    c.text(RIGHT - c.width(&m.version, F::Small), PAD, HEAD, &m.version, F::Small, b.tx2);
    // The screens.
    let ws_fill = |n: usize| {
        let color = b.ws[n.min(8)];
        (color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)))
    };
    for (i, ((x, y, w, h), name_x, name)) in s.lay.chips.iter().enumerate() {
        let sc = &m.screens[i];
        let (fill, edge) = ws_fill(sc.active);
        let n = (sc.active + 1).to_string();
        c.boxed((*x, *y, *w, *h), 4.0, fill, edge);
        c.text(x + (w - c.width(&n, F::SmallBold)) / 2.0, *y, *h, &n, F::SmallBold, b.on_ws);
        c.text(*name_x, *y, *h, name, F::Small, b.tx2);
    }
    for (i, &(x, y, w, h)) in s.lay.map.iter().enumerate() {
        let sc = &m.screens[i];
        let (fill, edge) = ws_fill(sc.active);
        c.boxed((x, y, w, h), 2.0, fill, edge);
        let landed = s.landed == Some(i);
        if landed {
            c.fill_rr((x, y + h - BAR, w, BAR), 1.5, op(b.tx));
        }
        let side = w.min(h);
        if side >= 12.0 {
            let f = if side < 18.0 { F::TinyBold } else { F::SmallBold };
            let n = (sc.active + 1).to_string();
            let ih = if landed { h - BAR } else { h }; // the bar never covers the number
            c.text(x + (w - c.width(&n, f)) / 2.0, y, ih, &n, f, b.on_ws);
        }
    }
    // The hiding warning: the finder's note box.
    if let Some((x, y, w, h)) = s.lay.warn {
        c.boxed((x, y, w, h), 6.0, b.bg, Some(b.line));
        c.text(x + 12.0, y + 8.0, 20.0, "\u{E7BA}", F::Icon, b.warn);
        c.text(x + 40.0, y + 8.0, 20.0, WARN_TITLE, F::Bold, b.tx);
        c.para(x + 40.0, y + 28.0, w - 24.0 - 28.0, 16.0, &[(WARN_TEXT, F::Small, b.tx2)]);
    }
    for &y in &s.lay.rules {
        c.rule(PAD, y, W - 2.0 * PAD, op(b.line));
    }
    // The items.
    for (i, (it, &(x, y, w, h))) in m.items().iter().zip(&s.lay.items).enumerate() {
        let (hot, ring) = (s.hot == Some(i) || s.focus == Some(i), s.focus == Some(i));
        if hot {
            c.fill_rr((x, y, w, h), 4.0, op(hover(&t)));
        }
        if ring {
            c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, h - 2.0), 3.0, 2.0, op(b.tx));
        }
        let ink = |color: u32| match () {
            _ if !it.on => b.tx3,
            _ if hot && t.hc => b.on_acc,
            _ => color,
        };
        c.text(IN_X, y, h, it.icon, F::Icon, ink(b.tx));
        let ww = words_w(c, it);
        let top = if it.line.is_empty() { y + (h - 20.0) / 2.0 } else { y + 6.0 };
        c.text(LABEL_X, top, 20.0, &c.fit(&it.label, F::Body, ww), F::Body, ink(b.tx));
        if !it.line.is_empty() {
            c.para(LABEL_X, top + 20.0, ww, 16.0, &[(&it.line, F::Small, ink(b.tx2))]);
        }
        let rw = right_w(c, it);
        if it.new {
            let bx = (RIGHT - rw, y + (h - 18.0) / 2.0, rw, 18.0);
            c.boxed(bx, 4.0, b.acc, t.hc.then_some(b.on_acc));
            c.text(bx.0 + 6.0, bx.1, 18.0, "New", F::SmallBold, b.on_acc);
        }
        let mut kx = RIGHT - rw;
        for k in &it.keys {
            kx += c.key_cap(kx, y + (h - 20.0) / 2.0, 20.0, k, false) + 4.0;
        }
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        ui::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
            let _ = ui::get(&SESSION, "tray menu paint", |s| {
                let fonts = ui::fonts(s.scale);
                draw(&Canvas::new(mem, true, s.scale, s.theme, &fonts), s);
            });
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let (x, y) = ((lp.0 & 0xFFFF) as i16 as i32, (lp.0 >> 16) as i16 as i32);
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_KEYDOWN => {
                on_key(VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            // Alt or F10 closes it, as it closes Windows' menus.
            WM_SYSKEYDOWN if matches!(VIRTUAL_KEY(wp.0 as u16), VK_MENU | VK_F10) => {
                close(true);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                on_mouse(hwnd, x, y);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                on_leave(hwnd);
                LRESULT(0)
            }
            // Like Windows' menus: an item runs when the button comes up on it.
            WM_LBUTTONUP | WM_RBUTTONUP => {
                if let Some(Some(i)) = with(|s| s.item_at(x, y)) {
                    run(i);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIP_TIMER => {
                show_tip(hwnd);
                LRESULT(0)
            }
            // A click outside, or any other window taking focus (AC-TRY-9).
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
                if is_open() {
                    LOST.set(Some(GetTickCount()));
                    close(false);
                }
                LRESULT(0)
            }
            // Windows' mode or contrast changed (or main's 1 s check saw it).
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
                let t = ui::system_theme();
                if with(|s| s.theme = t).is_some() {
                    ui::border(hwnd, t.b.edge);
                    refresh();
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_GETOBJECT => TREE.with(|t| uia::answer_tree(hwnd, wp, lp, t)),
            WM_DPICHANGED => LRESULT(0), // open sizes the menu for its screen itself
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn tip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let scale = ui::get(&SESSION, "tray tip paint", |s| (s.scale, s.theme));
                if let Some((scale, t)) = scale {
                    ui::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
                        let fonts = ui::fonts(scale);
                        let c = Canvas::new(mem, true, scale, t, &fonts);
                        c.fill_rr((0.0, 0.0, rc.right as f32 / scale, rc.bottom as f32 / scale), 0.0, op(t.b.card));
                        TIP_TEXT.with_borrow(|words| c.text(8.0, 0.0, rc.bottom as f32 / scale, words, F::Small, t.b.tx));
                    });
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// The menu, made on first use and kept.
fn window() -> Option<HWND> {
    if let Some(h) = WND.get() {
        return Some(h);
    }
    let h = ui::popup(CLASS, Some(wndproc), WS_EX_TOPMOST, false)?;
    unsafe { let _ = SetWindowTextW(h, &HSTRING::from("des-k")); } // what Narrator calls the window
    WND.set(Some(h));
    Some(h)
}

/// The map's tooltip, made on first use: it never takes focus, and the mouse goes through it.
fn tip_window() -> Option<HWND> {
    if let Some(h) = TIP.get() {
        return Some(h);
    }
    let h = ui::popup(TIP_CLASS, Some(tip_proc), WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, false)?;
    TIP.set(Some(h));
    Some(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CTRL, WIN};
    use crate::ui::{DARK, Fonts, LIGHT};

    fn screen(name: &str, (x, y, w, h): (i32, i32, i32, i32), active: usize, main: bool) -> TrayScreen {
        TrayScreen { name: name.into(), rect: RECT { left: x, top: y, right: x + w, bottom: y + h }, active, main }
    }

    fn menu(screens: Vec<TrayScreen>) -> Menu {
        Menu { version: "0.3.0".into(), screens, broken: false, hints_key: None, log: true, check: Check::Ready, quit: Quit::Plain }
    }

    /// Screens on a grid of 16 × 9 units, like the design's: (x, y, w, h, workspace number).
    fn wall(units: &[(i32, i32, i32, i32, usize)]) -> Vec<TrayScreen> {
        units.iter().enumerate().map(|(i, &(x, y, w, h, n))| screen(&format!("Screen {}", i + 1), (x * 240, y * 240, w * 240, h * 240), n - 1, i == 0)).collect()
    }

    // AC-TRY-4: the map must fit the menu's 264 × 88 however many screens you have, keep their
    // arrangement, and never draw a screen too small to see or point at.
    #[test]
    fn the_map_fits_and_no_screen_is_too_small() {
        let twelve: Vec<RECT> = (0..12).map(|i| RECT { left: (i % 4) * 3840, top: (i / 4) * 2160, right: (i % 4 + 1) * 3840, bottom: (i / 4 + 1) * 2160 }).collect();
        let (boxes, h) = map(&twelve);
        assert!(h <= MAP_H);
        assert!(boxes.iter().all(|&(x, y, w, bh)| x >= 0.0 && x + w <= MAP_W && y + bh <= MAP_H && w >= MIN_SIDE && bh >= MIN_SIDE), "{boxes:?}");
        assert!(boxes[0].0 < boxes[1].0 && boxes[0].1 < boxes[4].1, "the arrangement is kept");
        // A small screen beside three huge ones: the map grows rather than lose it.
        let odd = [RECT { left: 0, top: 0, right: 20000, bottom: 8000 }, RECT { left: 20000, top: 0, right: 20400, bottom: 300 }];
        let (boxes, _) = map(&odd);
        assert!(boxes[1].2 >= MIN_SIDE - 0.5 && boxes[1].3 >= MIN_SIDE - 0.5, "{boxes:?}");
    }

    // AC-TRY-5, AC-TRY-7: a greyed item can't run; a newer release says where it goes; Q's
    // letter only moves to Quit (the label starts with Q), and the hints key shows as caps.
    #[test]
    fn the_items_say_what_they_do_now() {
        let mut m = menu(vec![]);
        m.log = false;
        m.check = Check::Busy;
        m.hints_key = Some(Combo { mods: WIN | CTRL, vk: b'H' as u16 });
        let items = m.items();
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [Id::Hints, Id::Settings, Id::Log, Id::Update, Id::Quit]);
        assert_eq!((items[2].on, items[3].on, items[3].label.as_str()), (false, false, "Checking for updates…"));
        assert_eq!(items[0].keys, ["Win", "Ctrl", "H"]);
        m.check = Check::Newer(Version(0, 4, 0));
        let up = &m.items()[3];
        assert_eq!((up.label.as_str(), up.line.as_str(), up.new, up.on), ("Update to 0.4.0", "Opens its page on github.com", true, true));
        m.check = Check::Failed(Fail::NoConnection);
        assert_eq!(m.items()[3].line, "Couldn't check: no connection to GitHub.");
        m.check = Check::Failed(Fail::Timeout);
        assert_eq!(m.items()[3].line, "Couldn't check: GitHub did not answer in time.");
        m.quit = Quit::Stops;
        assert!(m.items()[4].line.contains("no gestures until you turn Windows' on"));
    }

    fn twelve_mixed() -> Vec<TrayScreen> {
        let mut s = wall(&[(9, 0, 16, 9, 1), (25, 0, 16, 9, 2), (41, 0, 16, 9, 3), (57, 0, 16, 9, 4), (9, 9, 16, 9, 5), (25, 9, 16, 9, 6), (41, 9, 16, 9, 7), (57, 9, 16, 9, 8), (0, 1, 9, 16, 9), (73, 1, 9, 16, 1), (26, 18, 14, 8, 2), (42, 18, 14, 8, 3)]);
        for (i, x) in s.iter_mut().enumerate() {
            x.main = i == 10;
        }
        s
    }

    /// Draws the design's states to target/tray/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/trr4-* (spec 0017 AC-1, AC-TRY-1 to AC-TRY-8).
    #[test]
    #[ignore]
    fn render() {
        ui::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tray");
        std::fs::create_dir_all(&dir).unwrap();
        let two = || vec![screen("DELL U2720Q", (0, 0, 3840, 2160), 1, true), screen("Built-in display", (3840, 600, 2560, 1600), 0, false)];
        let three = wall(&[(0, 0, 16, 9, 2), (16, 0, 16, 9, 5), (32, 0, 16, 9, 1)]);
        let wall12 = wall(&[(0, 0, 16, 9, 3), (16, 0, 16, 9, 1), (32, 0, 16, 9, 7), (48, 0, 16, 9, 2), (0, 9, 16, 9, 4), (16, 9, 16, 9, 2), (32, 9, 16, 9, 6), (48, 9, 16, 9, 1), (0, 18, 16, 9, 5), (16, 18, 16, 9, 9), (32, 18, 16, 9, 3), (48, 18, 16, 9, 8)]);
        type Setup = fn(&mut Session);
        let states: Vec<(&str, Vec<TrayScreen>, Option<usize>, Setup)> = vec![
            ("pointed", two(), Some(0), |s| s.hot = Some(0)),
            ("hints-key", two(), Some(0), |s| {
                s.menu.hints_key = Some(Combo { mods: WIN | CTRL, vk: b'H' as u16 });
                s.hot = Some(0);
            }),
            ("checking", two(), Some(0), |s| s.menu.check = Check::Busy),
            ("broken-newer", two(), Some(0), |s| {
                s.menu.broken = true;
                s.menu.check = Check::Newer(Version(0, 4, 0));
                s.menu.log = false;
                s.hot = Some(3);
            }),
            ("broken-newest", two(), Some(0), |s| {
                s.menu.broken = true;
                s.menu.check = Check::Newest(Version(0, 3, 0));
            }),
            ("broken-failed", two(), Some(0), |s| {
                s.menu.broken = true;
                s.menu.check = Check::Failed(Fail::NoConnection);
            }),
            ("quit-saved", two(), Some(0), |s| {
                s.menu.quit = Quit::Saved;
                s.hot = Some(4);
            }),
            ("quit-stops", two(), Some(0), |s| s.menu.quit = Quit::Stops),
            ("keyboard", two(), Some(0), |s| s.focus = Some(0)),
            ("one", vec![screen("Built-in display", (0, 0, 2560, 1600), 0, true)], Some(0), |_| {}),
            ("three", three, Some(0), |s| s.hot = Some(1)),
            ("twelve-wall", wall12, Some(8), |s| s.hot = Some(1)),
            ("twelve-mixed", twelve_mixed(), Some(10), |_| {}),
        ];
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let themes: [(&str, Theme, f32); 5] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5)];
        for (tname, t, scale) in themes {
            let fonts = Fonts::new(scale);
            for (name, screens, landed, setup) in &states {
                let m = menu(screens.clone());
                let lay = unsafe {
                    let dc = GetDC(None);
                    let l = layout(&Canvas::new(dc, false, scale, t, &fonts), &m);
                    ReleaseDC(None, dc);
                    l
                };
                let mut s = Session { menu: m, lay, scale, theme: t, hot: None, focus: None, landed: *landed, tip: None, came_from: HWND::default() };
                setup(&mut s);
                s.lay = unsafe {
                    let dc = GetDC(None);
                    let l = layout(&Canvas::new(dc, false, scale, t, &fonts), &s.menu);
                    ReleaseDC(None, dc);
                    l
                };
                let (w, h) = (s.px(W), s.px(s.lay.height));
                ui::tests::save_png(w, h, &dir.join(format!("{name}-{tname}.png")), |dc| {
                    let c = Canvas::new(dc, true, scale, t, &fonts);
                    draw(&c, &s);
                    // Windows' 1 px edge, as ui::border asks for it.
                    c.stroke_rr((0.5 / scale, 0.5 / scale, W - 1.0 / scale, s.lay.height - 1.0 / scale), 8.0, 1.0 / scale, op(t.b.edge));
                });
            }
        }
    }
}
