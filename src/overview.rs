// Workspace overview (spec 0010, look B by spec 0017): Win+Tab opens a popup on every screen at
// once, with a band of that screen's workspace tiles along the top and the windows of the
// workspace it shows spread out below. Windows draws every preview (DWM thumbnails) over
// des-k's painting; des-k paints the page, the band, the tiles' wallpaper, chips, rings,
// labels, the windows' shadows and the key bar, and answers Narrator. Lives on the main thread;
// where a window lives only ever changes through `ws`, and the overview never cloaks anything.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{COLORREF, ERROR_ACCESS_DENIED, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties},
        Gdi::*,
        GdiPlus as gp,
    },
    System::{LibraryLoader::GetModuleHandleW, SystemInformation::GetTickCount},
    UI::{
        Accessibility::{UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_PaneControlTypeId, UIA_TextControlTypeId},
        HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, PCWSTR, w};

use crate::finder;
use crate::settings::{self, Animation, Labels, Pick, Settings};
use crate::ui::{self, Theme, canvas::{Bx, Canvas}, color::{alpha, mix, op}, fonts::F};
use crate::uia::{self, Node, Tree};
use crate::wallpaper::{self, Face};
use crate::ws::{self, moves::Miss};

mod anim;
mod drag;
mod draw;
mod layout;

use anim::{Leaving, anim_tick, leave, step};
use drag::{Drag, Stage, aim_rects, cancel_drag, dim_source, drag_move, hover_changed, on_close_click, on_middle_click, on_move, on_press, on_release};
use draw::{Marks, paint, paint_x};
use layout::{Screen, bar_words, first, grow, nearest, tab_step, window_words};

const CLASS: PCWSTR = w!("des-k-overview");
const CLOSE_CLASS: PCWSTR = w!("des-k-overview-close");
const GHOST_CLASS: PCWSTR = w!("des-k-overview-ghost");
const WHO: &str = "des-k overview";
const CLOSING_TIMER: usize = 1;
const CLOSING_POLL_MS: u32 = 200; // as the finder's
const CLOSE_WAIT: Duration = Duration::from_secs(2); // still open after this (it asked to save, say): shown again
const NOTE_TIMER: usize = 2;
const NOTE_MS: u32 = 2000;
const REFRESH_TIMER: usize = 3;
const REFRESH_MS: u32 = 250;
const ANIM_TIMER: usize = 4;
const FRAME_MS: u32 = 10; // Windows rounds it up to its own tick, about 16 ms
const WALL_TIMER: usize = 5;
const WALL_POLL_MS: u32 = 50;
const SPEED_LIMIT_MS: u32 = 100; // spec 0017 AC-8
const KEY_W: VIRTUAL_KEY = VIRTUAL_KEY(b'W' as u16);
const WM_MOUSELEAVE: u32 = 0x02A3; // winuser.h, also in Win32_UI_Controls

// Sizes in dip, from the design (pdesk-design restart/overview-r3.html).
const TILE_TOP: f32 = 24.0;
const TILE_GAP: f32 = 16.0;
const TILE_HEIGHT: f32 = 0.12; // of the screen's height
const MARGIN: f32 = 48.0;
const CHIP_GAP: f32 = 8.0; // between a tile and its chip
const CHIP_H: f32 = 18.0;
const NAME_GAP: f32 = 6.0; // between a chip and its workspace's name (spec 0023)
const BAND_PAD: f32 = 16.0; // under the chips, down to the band's rule
const SPREAD_GAP: f32 = 24.0; // between the band and the spread
const BOTTOM: f32 = 64.0; // under the spread: the bar's room
const SLOT_GAP: f32 = 32.0;
const RING: f32 = 2.0; // a chosen or pointed at window's ring
const RING_OFF: f32 = 3.0; // its gap from the window
const TILE_RING_OFF: f32 = 2.0;
const WANT: f32 = 6.0; // a mark's dot under a tile's chip (spec 0022)
const WANT_GAP: f32 = 4.0;
const LABEL: f32 = 22.0; // the band under each spread window
const LABEL_PAD: f32 = 4.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 6.0;
const MINI_ICON: f32 = 12.0;
const CARD_ICON: f32 = 32.0;
const CARD_PAD: f32 = 8.0;
const CLOSE_BTN: f32 = 28.0;
const CLOSE_R: f32 = 6.0;
const CLOSE_INSET: f32 = 6.0; // from the hovered window's top right corner
const CLOSE_RED: u32 = 0xC42B1C; // Windows' own close button
const GHOST_W: f32 = 240.0;
const GHOST_MIN: f32 = 32.0; // over the band, at its size in a tile
const SPREAD_AIM: f32 = 16.0; // another screen's spread as a drop target: a ring this far out
const BAR_H: f32 = 40.0;
const BAR_UP: f32 = 12.0; // from the screen's bottom
const BAR_PAD: f32 = 16.0;
const BAR_GAP: f32 = 20.0;
const SHADOW_Y: f32 = 8.0; // look B's floating shadow, CSS's 0 8px 24px
const SHADOW_BLUR: f32 = 24.0;

/// What a note says (AC-OVR-8).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Note {
    CantMove,
    CantCloseAdmin,
    CantClose,
    DidntClose,
}

impl Note {
    /// Its icon, des-k's words, the grey line beside them, and whether it warns.
    fn words(self) -> (&'static str, &'static str, &'static str, bool) {
        match self {
            Note::CantMove => ("\u{E7BA}", "Can't move this window", "It stays where it was.", true),
            Note::CantCloseAdmin => ("\u{E7BA}", "Can't close an admin window", "It stays open.", true),
            Note::CantClose => ("\u{E7BA}", "Can't close this window", "It stays open.", true),
            Note::DidntClose => ("\u{E946}", "A window didn't close", "It may be asking you something. Enter goes to it.", false),
        }
    }
}

/// Only Windows refusing an admin window says so; any other failure may be anything.
fn close_note(e: &windows::core::Error) -> Note {
    if e.code() == ERROR_ACCESS_DENIED.to_hresult() { Note::CantCloseAdmin } else { Note::CantClose }
}

/// What the bar at a screen's bottom says.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Bar {
    Keys,
    Drag,
    Note(Note),
}

struct Session {
    screens: Vec<Screen>, // left to right, then top down
    came_from: HWND,
    hover: Option<(HWND, HWND)>, // (popup, window) of the spread window under the mouse
    tile_hover: Option<(HWND, usize)>, // (popup, tile) under the mouse
    selected: (usize, Option<HWND>), // (screen, spread window); no window when its spread is empty
    closing: Vec<(HWND, String, Instant, HWND)>, // sent WM_CLOSE and out of the layout, waiting to go: title for the log, popup it was asked on
    drag: Option<Drag>,
    seen: Vec<Seen>, // the list the layout was built from, for the refresh
    fading_in: Option<Instant>, // when the fade in started, while it runs (AC-11)
    alpha: u8, // the popups' now, so a close during the fade in fades out from there
    asked: u32, // the key message's time, for AC-8's clock
    logged: bool,
    wall_gen: u64, // backdrop::current_gen as the tiles last took their pictures (spec 0028 AC-10)
}

/// A listed window and where it lives, as the refresh compares them.
type Seen = (HWND, String, usize, bool);

fn seen(places: &[ws::views::Place]) -> Vec<Seen> {
    places.iter().map(|p| (p.hwnd, p.device.clone(), p.workspace, p.on_screen)).collect()
}

#[derive(Debug, PartialEq)]
enum Refresh {
    Nothing,
    Rebuild,        // live: something opened, went or moved
    Drop(Vec<HWND>), // not live: these went, the rest stay put
}

/// What a refresh does with the list `now`, given the one the layout came from (AC-10).
fn refresh_plan(live: bool, seen: &[Seen], now: &[Seen]) -> Refresh {
    if live {
        return if seen == now { Refresh::Nothing } else { Refresh::Rebuild };
    }
    let gone: Vec<HWND> = seen.iter().map(|s| s.0).filter(|h| !now.iter().any(|n| n.0 == *h)).collect();
    if gone.is_empty() { Refresh::Nothing } else { Refresh::Drop(gone) }
}

impl Session {
    /// (popup, window) of the selected spread window.
    fn selection(&self) -> Option<(HWND, HWND)> {
        let sc = self.screens.get(self.selected.0)?;
        Some((sc.popup, self.selected.1?))
    }

    /// After windows dropped out: a selection whose window is gone moves to the first one there.
    fn fix_selection(&mut self) {
        let Some(sc) = self.screens.get(self.selected.0) else { return };
        if !self.selected.1.is_some_and(|h| sc.slots.iter().any(|x| x.hwnd == h)) {
            self.selected.1 = sc.first();
        }
    }

    fn moving(&self) -> Option<&Drag> {
        self.drag.as_ref().filter(|d| matches!(d.stage, Stage::Moving(_)))
    }

    /// A note on its own screen; else the keys, or what a drop does, on the selected screen only
    /// (AC-OVR-7).
    fn bar(&self, i: usize) -> Option<Bar> {
        let sc = self.screens.get(i)?;
        if let Some(n) = sc.note {
            return Some(Bar::Note(n));
        }
        (i == self.selected.0).then(|| if self.moving().is_some() { Bar::Drag } else { Bar::Keys })
    }

    fn marks(&self, i: usize) -> Marks {
        let Some(sc) = self.screens.get(i) else { return Marks::default() };
        let mine = |at: Option<(HWND, HWND)>| at.filter(|(p, _)| *p == sc.popup).map(|(_, h)| h);
        let moving = self.moving();
        Marks {
            hover: mine(self.hover),
            // The chosen ring hides while a drag moves (AC-OVR-4).
            selected: if moving.is_some() { None } else { mine(self.selection()) },
            src: moving.filter(|d| d.from.0 == sc.device).map(|d| d.hwnd),
            aim: moving.and_then(|d| d.aim).filter(|(k, _)| *k == i).map(|(_, a)| a),
            tile_hover: self.tile_hover.filter(|(p, _)| *p == sc.popup).map(|(_, t)| t),
            bar: self.bar(i),
        }
    }

    /// Every place a drag's start or end changes: the chosen window's ring, the dragged one's
    /// fade, the target ring, and the selected screen's bar.
    fn drag_rects(&self) -> Vec<(HWND, RECT)> {
        let mut out = Vec::new();
        if let Some(sc) = self.screens.get(self.selected.0) {
            out.push((sc.popup, sc.bar_strip()));
            if let Some(r) = self.selected.1.and_then(|h| sc.mark_rect(h)) {
                out.push((sc.popup, r));
            }
        }
        if let Some(d) = &self.drag {
            for sc in self.screens.iter().filter(|sc| sc.device == d.from.0) {
                if let Some(r) = sc.mark_rect(d.hwnd).or_else(|| sc.all().find(|m| m.hwnd == d.hwnd).map(|m| grow(m.rect, 1))) {
                    out.push((sc.popup, r));
                }
            }
            if let Some((k, a)) = d.aim {
                out.extend(aim_rects(&self.screens[k], a).into_iter().map(|r| (self.screens[k].popup, r)));
            }
        }
        out
    }
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static LEAVING: RefCell<Option<Leaving>> = const { RefCell::new(None) };
    static X_HOT: Cell<bool> = const { Cell::new(false) };
    // What Narrator last heard: screen, chosen window, the bar's words.
    static SAID: RefCell<Option<(usize, isize, String)>> = const { RefCell::new(None) };
}

fn popup(r: RECT, layered: bool) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        if !REGISTERED.get() {
            let cursor = LoadCursorW(None, IDC_ARROW).ok()?;
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, hCursor: cursor, ..Default::default() });
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(close_wndproc), hInstance: inst.into(), lpszClassName: CLOSE_CLASS, hCursor: cursor, ..Default::default() });
            REGISTERED.set(true);
        }
        // Tool window: never in Alt+Tab or the taskbar, and never in ws lists. One of them
        // takes focus for the keys, so no WS_EX_NOACTIVATE. Layered only to animate, since
        // it shows nothing until its alpha is set. A whole screen: no rounded corners.
        let ex = if layered { WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED } else { WS_EX_TOPMOST | WS_EX_TOOLWINDOW };
        let h = CreateWindowExW(ex, CLASS, w!("des-k overview"), WS_POPUP, r.left, r.top, r.right - r.left, r.bottom - r.top, None, None, Some(inst.into()), None).ok()?;
        if layered {
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA);
        }
        Some(h)
    }
}

/// The hover X: its own popup, since a DWM preview always draws over its host's own painting.
/// Owned by the screen's popup, so it stays above it, and it never takes focus, so the keys
/// stay with the overview. Every pixel has its own opacity, for its round corners over the
/// live picture under it.
fn close_button(owner: HWND, scale: f32) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let d = (CLOSE_BTN * scale).round() as i32;
        let h = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED, CLOSE_CLASS, w!("des-k overview close"), WS_POPUP, 0, 0, d, d, Some(owner), None, Some(inst.into()), None).ok()?;
        paint_x(h, false);
        Some(h)
    }
}

fn cursor() -> POINT {
    let mut pt = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pt); }
    pt
}

/// A touchpad gesture asks (spec 0011).
pub fn is_open() -> bool {
    SESSION.with_borrow(Option::is_some)
}

/// Win+Tab: open on every screen, or close if open.
pub fn toggle() {
    if SESSION.with_borrow(Option::is_some) {
        close(true);
    } else {
        open();
    }
}

fn open() {
    // The key's own time: waiting in the queue counts against the 100 ms (spec 0017 AC-8).
    let asked = unsafe { GetMessageTime() } as u32;
    drop(LEAVING.take());
    let fg = unsafe { GetForegroundWindow() };
    let root = unsafe { GetAncestor(fg, GA_ROOTOWNER) };
    let came_from = if root.is_invalid() { fg } else { root };
    let (_, mouse) = ws::switch::under_mouse();
    let places = ws::views::windows();
    // Numbered like the finder's screens: left to right, then top down.
    let mut order = ws::views::screens();
    order.sort_by_key(|(_, r)| (r.left, r.top));
    let style = settings::get().overview_animation;
    ui::canvas::gdiplus();
    let mut screens = Vec::new();
    for (device, rect) in order {
        let Some(popup) = popup(rect, style != Animation::Off) else {
            warn!("overview: popup for {device} not created");
            continue;
        };
        let (mut dpi, mut dy) = (96u32, 96u32);
        unsafe { let _ = GetDpiForMonitor(MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
        let scale = dpi as f32 / 96.0;
        let close_btn = close_button(popup, scale).unwrap_or_else(|| {
            warn!("overview: close button for {device} not created");
            HWND::default()
        });
        let mut sc = Screen { device, popup, rect, scale, shown: 0, tiles: Vec::new(), slots: Vec::new(), cell_w: 0, labels: Labels::Hover, face: Face::Card, close_btn, note: None, painted: false, tree: Tree::default(), wants: Vec::new() };
        sc.build(&places);
        screens.push(sc);
    }
    take_wants(&mut screens, &ws::focus::wanted());
    let start = screens.iter().position(|s| s.device == mouse).unwrap_or(0);
    let Some(sc) = screens.get(start) else {
        return warn!("overview: no screen to open on");
    };
    let focus = sc.popup;
    // The window you came from when it is in this spread, else the first one there.
    let selected = (start, sc.slots.iter().any(|s| s.hwnd == came_from).then_some(came_from).or_else(|| sc.first()));
    let windows: usize = screens.iter().map(|s| s.slots.len()).sum();
    let popups: Vec<HWND> = screens.iter().map(|s| s.popup).collect();
    // Read off the open's path; the tiles take the wallpaper when it is ready.
    wallpaper::check(screens.iter().map(|s| (s.rect, s.tile_size(), s.files())).collect(), settings::get().wallpaper_fit);
    let fading_in = (style == Animation::Fade).then(Instant::now);
    let alpha = if fading_in.is_some() { 0 } else { 255 };
    SAID.set(None);
    SESSION.set(Some(Session { screens, came_from, hover: None, tile_hover: None, selected, closing: Vec::new(), drag: None, seen: seen(&places), fading_in, alpha, asked, logged: false, wall_gen: 0 }));
    // The first frame before anything shows.
    let animating = SESSION.with_borrow_mut(|s| {
        s.as_mut().is_some_and(|s| {
            step(s);
            s.fading_in.is_some()
        })
    });
    unsafe {
        for &p in &popups {
            let _ = SetWindowPos(p, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        // One timer each for every screen; they go with their popup on close.
        SetTimer(Some(focus), REFRESH_TIMER, REFRESH_MS, None);
        SetTimer(Some(focus), WALL_TIMER, WALL_POLL_MS, None);
        if animating {
            SetTimer(Some(focus), ANIM_TIMER, FRAME_MS, None);
        }
        let _ = SetForegroundWindow(focus);
        if GetForegroundWindow() != focus {
            close(false);
            return warn!("overview: Windows refused focus");
        }
    }
    info!("overview: opened on {} screen(s), {windows} window(s) shown", popups.len());
}

/// The log line every open writes (spec 0017 AC-8), easy to find when it is over the limit.
fn speed_line(screens: usize, ms: u32) -> String {
    let over = if ms > SPEED_LIMIT_MS { " (over the 100 ms limit)" } else { "" };
    format!("overview: shown on {screens} screens in {ms} ms{over}")
}

/// A popup's first full paint ended; after the last one, the open's time goes to the log.
fn first_paint(popup: HWND) {
    let line = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.iter_mut().find(|sc| sc.popup == popup)?;
        sc.painted = true;
        if s.logged || !s.screens.iter().all(|sc| sc.painted) {
            return None;
        }
        s.logged = true;
        Some(speed_line(s.screens.len(), unsafe { GetTickCount() }.wrapping_sub(s.asked)))
    });
    if let Some(l) = line {
        info!("{l}");
    }
}

/// Take every popup, preview and thumbnail down. `give_back` focuses the window you came
/// from (the desktop if it is gone); otherwise focus already went where it should.
pub fn close(give_back: bool) {
    let Some(s) = SESSION.with_borrow_mut(Option::take) else { return };
    if give_back {
        // Focus first: destroying the active popup would let Windows pick a window to activate.
        let to = if unsafe { IsWindow(Some(s.came_from)) }.as_bool() { s.came_from } else { unsafe { GetShellWindow() } };
        unsafe { let _ = SetForegroundWindow(to); }
    }
    leave(s); // outside the borrow: destroying a popup sends it messages
}

/// The wallpaper read finished something: the tiles show it.
fn wall_tick(popup: HWND) {
    let done = !wallpaper::busy(); // first: a read that just ended has stored its news
    if wallpaper::fresh() {
        let bands = SESSION.with_borrow_mut(|s| {
            let s = s.as_mut()?;
            Some(s.screens.iter_mut().map(|sc| {
                sc.read_face();
                (sc.popup, RECT { left: 0, top: 0, right: sc.size().0, bottom: sc.band_h() })
            }).collect::<Vec<_>>())
        });
        invalidate_rects(bands.unwrap_or_default());
    }
    if done {
        unsafe { let _ = KillTimer(Some(popup), WALL_TIMER); }
    }
}

fn is_ours(hwnd: HWND) -> bool {
    SESSION.with_borrow(|s| s.as_ref().is_some_and(|s| s.screens.iter().any(|sc| sc.popup == hwnd)))
}

/// Called by the main loop for every message before TranslateMessage. Returns true when
/// the overview took the key.
pub fn key(msg: &MSG) -> bool {
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) || !is_ours(msg.hwnd) {
        return false;
    }
    let held = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
    let selection = || SESSION.with_borrow(|s| s.as_ref().and_then(Session::selection));
    let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
    match vk {
        VK_ESCAPE if !cancel_drag() => close(true),
        VK_ESCAPE => {}
        VK_LEFT => arrow((-1, 0)),
        VK_RIGHT => arrow((1, 0)),
        VK_UP => arrow((0, -1)),
        VK_DOWN => arrow((0, 1)),
        VK_TAB => tab(if held(VK_SHIFT) { -1 } else { 1 }),
        VK_RETURN => {
            if let Some((_, h)) = selection() {
                go(h);
            }
        }
        KEY_W if held(VK_CONTROL) => {
            if let Some((popup, h)) = selection() {
                close_window(popup, h);
            }
        }
        _ => match vk.0 {
            0x31..=0x39 => jump((vk.0 - 0x31) as usize),
            v if (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&v) => jump((v - VK_NUMPAD1.0) as usize),
            _ => return false,
        },
    }
    true
}

/// Each screen's marks from `ws::focus::wanted` (spec 0022). Paint reads only these, never the layout,
/// so a new mark shows without moving a tile or a window.
fn take_wants(screens: &mut [Screen], wanted: &[(String, usize, HWND)]) {
    for sc in screens {
        sc.wants = wanted.iter().filter(|w| w.0 == sc.device).map(|w| (w.1, w.2)).collect();
    }
}

/// A window got a mark (spec 0022 AC-3): it shows at once in an open overview.
pub fn wants_changed() {
    if !is_open() {
        return;
    }
    let wanted = ws::focus::wanted();
    let popups = SESSION.with_borrow_mut(|s| {
        let Some(s) = s.as_mut() else { return Vec::new() };
        take_wants(&mut s.screens, &wanted);
        s.screens.iter().map(|sc| sc.popup).collect::<Vec<_>>()
    });
    invalidate(popups);
}

fn invalidate(popups: impl IntoIterator<Item = HWND>) {
    for p in popups {
        unsafe { let _ = InvalidateRect(Some(p), None, false); }
    }
}

/// Only what changed repaints: a popup is a whole screen (AC-OVR-10).
fn invalidate_rects(rects: impl IntoIterator<Item = (HWND, RECT)>) {
    for (p, r) in rects {
        unsafe { let _ = InvalidateRect(Some(p), Some(&r), false); }
    }
}

/// The selection moved on one screen from `old`: both rings and labels repaint.
fn selection_moved(old: Option<HWND>) {
    let rects = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let sc = s.screens.get(s.selected.0)?;
        Some([old, s.selected.1].into_iter().flatten().filter_map(|h| sc.mark_rect(h)).map(|r| (sc.popup, r)).collect::<Vec<_>>())
    });
    invalidate_rects(rects.unwrap_or_default());
}

/// An arrow: the nearest window that way on the selected screen.
fn arrow(dir: (i32, i32)) {
    let old = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.get(s.selected.0)?;
        let rects = sc.rects();
        let to = match s.selected.1.and_then(|h| sc.slots.iter().position(|x| x.hwnd == h)) {
            Some(from) => nearest(&rects, from, dir)?,
            None => first(&rects)?,
        };
        Some(s.selected.1.replace(sc.slots[to].hwnd))
    });
    if let Some(old) = old {
        selection_moved(old);
    }
}

/// Tab, Shift+Tab: the next or previous window on the selected screen, around the end and
/// never on to another screen (the mouse picks the screen, see `follow_mouse`).
fn tab(by: isize) {
    let old = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.get(s.selected.0)?;
        let from = s.selected.1.and_then(|h| sc.slots.iter().position(|x| x.hwnd == h));
        let to = sc.slots[tab_step(&sc.rects(), from, by)?].hwnd;
        Some(s.selected.1.replace(to))
    });
    if let Some(old) = old {
        selection_moved(old);
    }
}

/// A digit: switch the selected screen to that workspace, only within its own count (spec 0040
/// AC-4).
fn jump(i: usize) {
    let target = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let sc = s.screens.get(s.selected.0)?;
        Some((sc.device.clone(), sc.shown))
    });
    if let Some((device, shown)) = target
        && i < ws::views::count(&device)
    {
        show(&device, shown, i);
    }
}

/// Closed first, focus back, so the switch moves focus on exactly as a switch would; the
/// workspace the screen already shows just closes.
fn show(device: &str, shown: usize, i: usize) {
    close(true);
    if i != shown {
        ws::switch::show_workspace(device, i);
    }
}

/// Land on a window, wherever it lives, like the finder's Enter.
fn go(hwnd: HWND) {
    match ws::moves::go_to(hwnd) {
        Ok(()) => {
            finder::used(hwnd); // our own focus change: the foreground hook skips it
            close(false);
        }
        Err(Miss::Gone | Miss::CantMove | Miss::Unplugged) => {
            info!("overview: a window is gone");
            rebuild();
        }
    }
}

/// Every screen laid out again from the current window list, after windows moved or went:
/// one may sit in a tile on one screen and in the spread on another.
fn rebuild() {
    let mut places = ws::views::windows();
    let wanted = ws::focus::wanted();
    let (popups, old) = SESSION.with_borrow_mut(|s| {
        let Some(s) = s.as_mut() else { return (Vec::new(), None) };
        // Seen with the closing windows, so the refresh does not take them for a change.
        s.seen = seen(&places);
        places.retain(|p| !s.closing.iter().any(|c| c.0 == p.hwnd));
        for sc in &mut s.screens {
            sc.build(&places);
        }
        take_wants(&mut s.screens, &wanted);
        s.fix_selection();
        s.tile_hover = None;
        (s.screens.iter().map(|sc| sc.popup).collect(), s.hover.take())
    });
    hover_changed(old);
    invalidate(popups);
}

/// Workspace `i`'s picture on screen `id` for its tile: its own, or its slideshow's now. None
/// before the backdrop chose one: the tile shows that screen's Windows wallpaper, which is
/// what the screen shows then (spec 0028 AC-10).
fn tile_file(s: &Settings, id: &str, i: usize) -> Option<String> {
    match s.wallpaper_for(id, i)? {
        Pick::Picture(p) => Some(p.clone()),
        _ => crate::backdrop::current(id, i),
    }
}

/// A slideshow moved on since the tiles were made: each tile takes its picture now, decoded
/// on the wallpaper thread, and shows it once ready (spec 0028 AC-10).
fn slides_moved() {
    let moved = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let now = crate::backdrop::current_gen();
        if s.wall_gen == now {
            return None;
        }
        s.wall_gen = now;
        let st = settings::get();
        let mut any = false;
        for sc in &mut s.screens {
            let id = ws::views::id(&sc.device);
            for (i, t) in sc.tiles.iter_mut().enumerate() {
                let file = tile_file(&st, &id, i);
                any |= t.file != file;
                t.file = file;
            }
        }
        let popup = s.screens.first()?.popup;
        any.then(|| (popup, s.screens.iter().map(|sc| (sc.rect, sc.tile_size(), sc.files())).collect::<Vec<_>>()))
    });
    if let Some((popup, screens)) = moved {
        wallpaper::check(screens, settings::get().wallpaper_fit);
        unsafe { SetTimer(Some(popup), WALL_TIMER, WALL_POLL_MS, None) };
    }
}

/// Every 250 ms: catch windows that opened, went or moved since the layout was made (AC-10).
/// Skipped mid drag, so the spread never shifts under a press.
fn refresh() {
    if SESSION.with_borrow(|s| s.as_ref().is_none_or(|s| s.drag.is_some())) {
        return;
    }
    slides_moved();
    // A name typed in the settings page or the file shows here too (spec 0023 AC-7).
    let renamed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let mut popups = Vec::new();
        for sc in &mut s.screens {
            let names = ws::views::names(&sc.device);
            if sc.tiles.iter().zip(&names).any(|(t, n)| t.name != *n) {
                sc.tiles.iter_mut().zip(names).for_each(|(t, n)| t.name = n);
                popups.push(sc.popup);
            }
        }
        Some(popups)
    });
    invalidate(renamed.unwrap_or_default());
    let now = seen(&ws::views::windows());
    let plan = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let plan = refresh_plan(settings::get().overview_live, &s.seen, &now);
        if let Refresh::Drop(gone) = &plan {
            s.seen.retain(|x| !gone.contains(&x.0));
        }
        Some(plan)
    });
    match plan {
        Some(Refresh::Rebuild) => rebuild(),
        Some(Refresh::Drop(gone)) => drop_gone(&gone),
        Some(Refresh::Nothing) | None => {}
    }
}

/// X, middle click, Ctrl+W: ask the window to close as its own X button would, take it out of
/// the layout at once, then watch for it to go. The overview stays open.
fn close_window(popup: HWND, h: HWND) {
    let app = ws::views::who(h);
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        info!("overview: {app} is gone");
        return drop_gone(&[h]);
    }
    match unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
            info!("overview: asked {app} to close");
            SESSION.with_borrow_mut(|s| {
                if let Some(s) = s {
                    s.closing.push((h, app, Instant::now(), popup));
                }
            });
            // Some apps (Store ones like Settings) shrink what they draw while closing, and
            // the preview would stretch that to fill its slot.
            drop_gone(&[h]);
            unsafe { SetTimer(Some(popup), CLOSING_TIMER, CLOSING_POLL_MS, None) };
        }
        Err(e) => {
            warn!("overview: can't close {app}: {e}");
            note(popup, close_note(&e));
        }
    }
}

/// Windows asked to close that closed or hid themselves (apps that close to the tray) are done;
/// one still open after `CLOSE_WAIT` comes back into the layout, chosen, with a note on the
/// screen it was asked on (AC-OVR-8): its prompt is likely behind the overview.
fn closing_tick(popup: HWND) {
    let Some((gone, back, none_left)) = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let (gone, left): (Vec<_>, Vec<_>) = s.closing.drain(..).partition(|&(h, ..)| unsafe { !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() });
        let (back, left): (Vec<_>, Vec<_>) = left.into_iter().partition(|(_, _, at, _)| at.elapsed() >= CLOSE_WAIT);
        s.closing = left;
        Some((gone, back, s.closing.is_empty()))
    }) else {
        return;
    };
    for (_, app, ..) in &gone {
        info!("overview: closed {app}");
    }
    for (_, app, ..) in &back {
        info!("overview: {app} is still open, showing it again");
    }
    if let Some(&(h, _, _, asked_on)) = back.last() {
        rebuild();
        let picked = SESSION.with_borrow_mut(|s| {
            let s = s.as_mut()?;
            let i = s.screens.iter().position(|sc| sc.popup == asked_on && sc.slots.iter().any(|x| x.hwnd == h))?;
            s.selected = (i, Some(h));
            Some(())
        });
        if picked.is_some() {
            note(asked_on, Note::DidntClose);
        }
    }
    if none_left {
        unsafe { let _ = KillTimer(Some(popup), CLOSING_TIMER); }
    }
}

/// Gone windows leave the spread and the tiles; the rest stay put.
fn drop_gone(gone: &[HWND]) {
    let (popups, old) = SESSION.with_borrow_mut(|s| {
        let Some(s) = s.as_mut() else { return (Vec::new(), None) };
        let mut popups = Vec::new();
        for sc in &mut s.screens {
            let count = |sc: &Screen| sc.slots.len() + sc.tiles.iter().map(|t| t.minis.len()).sum::<usize>();
            let before = count(sc);
            sc.slots.retain(|x| !gone.contains(&x.hwnd));
            for t in &mut sc.tiles {
                t.minis.retain(|m| !gone.contains(&m.hwnd));
            }
            if count(sc) != before {
                popups.push(sc.popup);
            }
        }
        s.fix_selection();
        let old = s.hover.filter(|(_, h)| gone.contains(h));
        if old.is_some() {
            s.hover = None;
        }
        (popups, old)
    });
    if old.is_some() {
        hover_changed(old);
    }
    invalidate(popups);
}

/// A note in the bar of one screen, gone after about two seconds.
fn note(popup: HWND, n: Note) {
    let strip = SESSION.with_borrow_mut(|s| {
        let sc = s.iter_mut().flat_map(|s| s.screens.iter_mut()).find(|sc| sc.popup == popup)?;
        sc.note = Some(n);
        Some(sc.bar_strip())
    });
    unsafe { SetTimer(Some(popup), NOTE_TIMER, NOTE_MS, None) };
    invalidate_rects(strip.map(|r| (popup, r)));
}

fn note_gone(popup: HWND) {
    unsafe { let _ = KillTimer(Some(popup), NOTE_TIMER); }
    let strip = SESSION.with_borrow_mut(|s| {
        let sc = s.iter_mut().flat_map(|s| s.screens.iter_mut()).find(|sc| sc.popup == popup)?;
        sc.note = None;
        Some(sc.bar_strip())
    });
    invalidate_rects(strip.map(|r| (popup, r)));
}

/// After a paint: Narrator's picture of this screen, and what it should hear now (AC-OVR-11).
fn tell(popup: HWND) {
    if !uia::listening() {
        return;
    }
    let told = SESSION.with(|cell| {
        let s = cell.try_borrow().ok()?;
        let s = s.as_ref()?;
        let i = s.screens.iter().position(|sc| sc.popup == popup)?;
        let sc = &s.screens[i];
        let chosen = s.selection().filter(|(p, _)| *p == popup).map(|(_, h)| h);
        if let Ok(mut n) = sc.tree.try_lock() {
            *n = sc.node(chosen, s.bar(i));
        }
        let say = speech(s)?;
        Some((s.screens[s.selected.0].popup, say))
    });
    if let Some((p, (words, queue))) = told {
        uia::notify(p, &words, WHO, queue);
    }
}

/// What Narrator should hear after this change, if anything: the screen and its chosen window
/// when it opens or moves to another screen, the chosen window when the selection moves, and
/// a drag's or a note's words when the bar changes to them. The first words wait for what
/// Narrator is saying.
fn speech(s: &Session) -> Option<(String, bool)> {
    let i = s.selected.0;
    let sc = s.screens.get(i)?;
    let bar = s.bar(i);
    let words = bar.map(|b| bar_words(sc, b)).unwrap_or_default();
    let now = (i, s.selected.1.map_or(0, |h| h.0 as isize), words.clone());
    let before = SAID.replace(Some(now.clone()));
    if before.as_ref() == Some(&now) {
        return None;
    }
    let win = s.selected.1.and_then(|h| sc.slots.iter().find(|x| x.hwnd == h)).map_or_else(|| "No windows".to_string(), |x| window_words(x, &finder::names::program(x.hwnd), true));
    Some(match before {
        None => (format!("Overview. {}. {win}", sc.summary()), true),
        Some(b) if b.2 != words && !matches!(bar, Some(Bar::Keys) | None) => (words, false),
        Some(b) if b.0 != i => (format!("{}. {win}", sc.summary()), false),
        Some(b) if b.1 != now.1 => (win, false),
        Some(_) => return None,
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // Focus going to another window that is not one of the popups (the Start menu, say).
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && !is_ours(HWND(lp.0 as *mut _)) => {
                close(false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                on_press(hwnd, POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: (lp.0 >> 16) as i16 as i32 });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                on_release(hwnd, POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: (lp.0 >> 16) as i16 as i32 });
                LRESULT(0)
            }
            WM_RBUTTONDOWN => {
                cancel_drag();
                LRESULT(0)
            }
            // Lost to another window mid press: the release goes there, so the press is over.
            WM_CAPTURECHANGED => {
                let marks = SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default();
                if let Some(d) = SESSION.with_borrow_mut(|s| s.as_mut()?.drag.take()) {
                    dim_source(d.hwnd, false);
                    invalidate_rects(marks);
                }
                LRESULT(0)
            }
            WM_MBUTTONUP => {
                on_middle_click(hwnd, POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: (lp.0 >> 16) as i16 as i32 });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if !drag_move() {
                    on_move(hwnd, POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: (lp.0 >> 16) as i16 as i32 });
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == CLOSING_TIMER => {
                closing_tick(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == NOTE_TIMER => {
                note_gone(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == REFRESH_TIMER => {
                refresh();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == ANIM_TIMER => {
                anim_tick(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == WALL_TIMER => {
                wall_tick(hwnd);
                LRESULT(0)
            }
            // Windows' mode or contrast changed (or main's 1 s check saw it): repaint in it.
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                let _ = ui::refresh();
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_DPICHANGED => LRESULT(0), // each popup is sized for its own screen at open
            WM_GETOBJECT => {
                let tree = SESSION.with(|cell| cell.try_borrow().ok()?.as_ref()?.screens.iter().find(|sc| sc.popup == hwnd).map(|sc| sc.tree.clone()));
                match tree {
                    Some(t) => uia::answer_tree(hwnd, wp, lp, &t),
                    None => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn close_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // Red while pointed at (AC-OVR-2).
            WM_MOUSEMOVE if !X_HOT.get() => {
                X_HOT.set(true);
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
                paint_x(hwnd, true);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                X_HOT.set(false);
                paint_x(hwnd, false);
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                paint_x(hwnd, X_HOT.get());
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                on_close_click();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests;
