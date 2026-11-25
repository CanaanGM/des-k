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

use crate::install::InstallFail;
use crate::settings::Combo;
use crate::ui::{self, Theme, canvas::{Bx, Canvas}, color::{mix, op}, fonts::F};
use crate::uia::{self, Node, Tree};
use crate::update::{Fail, Version};
use crate::ws::views::TrayScreen;

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
    /// The installed admin copy installs it itself (spec 0018 AC-1, AC-3, AC-6).
    Install(Version),
    Downloading(Version),
    Installing(Version),
    /// Refused this run, with why (spec 0031 AC-5): a click checks again, never installs it.
    Refused(Version, InstallFail),
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
            Check::Install(v) => {
                update = Item { icon: "\u{E896}", label: format!("Install {v}"), line: "Downloads and installs it".into(), new: true, ..update };
            }
            Check::Downloading(v) => {
                update.label = format!("Downloading des-k {v}…");
                update.on = false;
            }
            Check::Installing(v) => {
                update.label = format!("Installing des-k {v}…");
                update.on = false;
            }
            Check::Refused(v, f) => {
                let line = if f == InstallFail::Mismatch { "Did not match its signed checksum" } else { "Not signed by des-k's publisher" };
                update = Item { icon: "\u{E7BA}", label: format!("des-k {v} refused"), line: line.into(), ..update };
            }
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
    chips: Vec<(Bx, f32, String, String)>, // one or two screens: the chip, where its words start, the screen and workspace names as they fit
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

const SEP: &str = " · ";

/// A chip line's screen and workspace names cut to `room` (spec 0023 handoff trayCut): the
/// screen is the line's subject, so the workspace's name is cut first, down to 10 letters, then
/// the screen at a word, keeping at least its first word and a twin's " #n". No name: today's cut.
fn tray_cut(c: &Canvas, screen: &str, name: &str, room: f32) -> (String, String) {
    if name.is_empty() {
        return (c.fit(screen, F::Small, room), String::new());
    }
    let w = |s: &str| c.width(s, F::Small);
    let ten: String = name.chars().take(10).collect();
    let floor = if name.chars().count() > 10 { c.fit_name(name, F::Small, w(&format!("{ten}…"))) } else { name.to_string() };
    let name_room = room - w(screen) - w(SEP);
    if name_room >= w(&floor) {
        return (screen.to_string(), c.fit_name(name, F::Small, name_room));
    }
    let (base, end) = match screen.rsplit_once(" #") {
        Some((base, n)) if !n.is_empty() && n.chars().all(|d| d.is_ascii_digit()) => (base, format!(" #{n}")),
        _ => (screen, String::new()),
    };
    let first = base.split(' ').next().unwrap_or(base);
    let mut cut = c.fit_name(base, F::Small, room - w(SEP) - w(&floor) - w(&end));
    if cut.trim_end_matches('…').chars().count() < first.chars().count() {
        cut = format!("{first}…");
    }
    (cut + &end, floor)
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
            let (screen, name) = tray_cut(c, &s.name, &s.ws_name, RIGHT - IN_X - cw - 6.0);
            let named = if name.is_empty() { 0.0 } else { c.width(SEP, F::Small) + c.width(&name, F::Small) };
            let span = cw + 6.0 + c.width(&screen, F::Small) + named;
            if x > IN_X && x + span > RIGHT {
                (x, line) = (IN_X, line + 18.0 + 4.0);
            }
            l.chips.push(((x, line, cw, 18.0), x + cw + 6.0, screen, name));
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

    /// A screen as Narrator and its tooltip name it. The tooltip says the workspace only when it
    /// has a name, which then follows its number in both (spec 0023 AC-10).
    fn screen_words(&self, i: usize, workspace: bool) -> String {
        let s = &self.menu.screens[i];
        let name = if s.ws_name.is_empty() { String::new() } else { format!(", {}", s.ws_name) };
        let ws = if workspace || !name.is_empty() { format!(", workspace {}{name}", s.active + 1) } else { String::new() };
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
    let Some(popup) = window() else { return warn!("tray menu: window not created") };
    ui::canvas::gdiplus();
    let anchor = match (keyboard, icon) {
        (true, Some(r)) => POINT { x: (r.left + r.right) / 2, y: r.top },
        _ => at,
    };
    let (mut dpi, mut dy) = (96u32, 96u32);
    let mon = unsafe { MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST) };
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
    let (scale, theme) = (dpi as f32 / 96.0, ui::system_theme());
    let fonts = ui::fonts::fonts(scale);
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
            warn!("tray menu: Windows refused focus, so it closes when you click elsewhere only");
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
    let fonts = ui::fonts::fonts(scale);
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
    for (i, ((x, y, w, h), name_x, screen, name)) in s.lay.chips.iter().enumerate() {
        let sc = &m.screens[i];
        let (fill, edge) = ws_fill(sc.active);
        let n = (sc.active + 1).to_string();
        c.boxed((*x, *y, *w, *h), 4.0, fill, edge);
        c.text(x + (w - c.width(&n, F::SmallBold)) / 2.0, *y, *h, &n, F::SmallBold, b.on_ws[sc.active.min(8)]);
        c.text(*name_x, *y, *h, screen, F::Small, b.tx2);
        if !name.is_empty() {
            // The screen and " · " grey, the workspace's name in the text color.
            let sep_x = name_x + c.width(screen, F::Small);
            c.text(sep_x, *y, *h, SEP, F::Small, b.tx2);
            c.text(sep_x + c.width(SEP, F::Small), *y, *h, name, F::Small, b.tx);
        }
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
            c.text(x + (w - c.width(&n, f)) / 2.0, y, ih, &n, f, b.on_ws[sc.active.min(8)]);
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
        ui::canvas::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
            let _ = ui::get(&SESSION, "tray menu paint", |s| {
                let fonts = ui::fonts::fonts(s.scale);
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
                    ui::canvas::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
                        let fonts = ui::fonts::fonts(scale);
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
mod tests;
