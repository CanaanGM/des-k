// Workspace overview (spec 0010, look B by spec 0017): Win+Tab opens a popup on every screen at
// once, with a band of that screen's workspace tiles along the top and the windows of the
// workspace it shows spread out below. Windows draws every preview (DWM thumbnails) over
// des-k's painting; des-k paints the page, the band, the tiles' wallpaper, chips, rings,
// labels, the windows' shadows and the key bar, and answers Narrator. Lives on the main thread;
// where a window lives only ever changes through `ws`, and the overview never cloaks anything.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
use crate::settings::{self, Animation, Labels};
use crate::ui::{self, Bx, Canvas, F, Theme, alpha, mix, op};
use crate::uia::{self, Node, Tree};
use crate::wallpaper::{self, Face};
use crate::ws::{self, Miss};

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
const BAND_PAD: f32 = 16.0; // under the chips, down to the band's rule
const SPREAD_GAP: f32 = 24.0; // between the band and the spread
const BOTTOM: f32 = 64.0; // under the spread: the bar's room
const SLOT_GAP: f32 = 32.0;
const RING: f32 = 2.0; // a chosen or pointed at window's ring
const RING_OFF: f32 = 3.0; // its gap from the window
const TILE_RING_OFF: f32 = 2.0;
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

/// A live preview drawn by DWM into a popup; unregistered when dropped.
struct Thumb(isize);

impl Drop for Thumb {
    fn drop(&mut self) {
        unsafe { let _ = DwmUnregisterThumbnail(self.0); }
    }
}

impl Thumb {
    /// A fade frame, or the dragged window's 40%: the preview made see through.
    fn set_opacity(&self, opacity: u8) {
        let p = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_OPACITY, opacity, ..Default::default() };
        unsafe { let _ = DwmUpdateThumbnailProperties(self.0, &p); }
    }

    /// The whole preview drawn into `r` now (the ghost changing size).
    fn place(&self, r: RECT) {
        let p = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_RECTDESTINATION, rcDestination: r, ..Default::default() };
        unsafe { let _ = DwmUpdateThumbnailProperties(self.0, &p); }
    }
}

/// A window drawn into a popup: in the spread, or a mini inside a tile. A card is a window
/// Windows draws nothing for (AC-4), shown as its icon and title on a plain box.
struct Slot {
    hwnd: HWND,
    rect: RECT, // in popup coordinates
    title: String, // empty for minis: tiles never carry titles
    icon: HICON, // borrowed, never destroyed; only fetched when drawn
    pinned: bool,
    card: bool,
    thumb: Option<Thumb>,
}

struct Tile {
    rect: RECT,
    minis: Vec<Slot>, // back to front, as the windows stack
}

struct Screen {
    device: String,
    popup: HWND,
    rect: RECT, // the whole monitor, taskbar included
    scale: f32,
    shown: usize, // 0 based
    tiles: Vec<Tile>,
    slots: Vec<Slot>,
    cell_w: i32, // a spread cell's width: a hover label may run that wide
    labels: Labels, // as the layout was made for
    face: Face, // behind the tiles' windows
    close_btn: HWND, // the hover X, hidden until a spread window is hovered
    note: Option<Note>,
    painted: bool, // its first full paint is done (spec 0017 AC-8)
    tree: Tree,
}

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

/// Where a drag would drop, when the drop changes anything.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Aim {
    Tile(usize),
    Spread,
}

fn inside(r: &RECT, pt: POINT) -> bool {
    pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
}

fn grow(r: RECT, by: i32) -> RECT {
    RECT { left: r.left - by, top: r.top - by, right: r.right + by, bottom: r.bottom + by }
}

fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

fn grow_bx((x, y, w, h): Bx, by: f32) -> Bx {
    (x - by, y - by, w + 2.0 * by, h + 2.0 * by)
}

/// A desktop that is still being read paints Windows' desktop color, with no wait (AC-OVR-3).
fn desktop_color() -> u32 {
    let c = unsafe { GetSysColor(COLOR_DESKTOP) };
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
}

impl Screen {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn size(&self) -> (i32, i32) {
        (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top)
    }

    /// The band's height, its rule included: the tiles, their chips, and room under them.
    fn band_h(&self) -> i32 {
        self.tiles.first().map_or(self.px(TILE_TOP), |t| t.rect.bottom) + self.px(CHIP_GAP + CHIP_H + BAND_PAD)
    }

    fn spread_area(&self) -> RECT {
        let (w, h) = self.size();
        RECT { left: self.px(MARGIN), top: self.band_h() + self.px(SPREAD_GAP), right: w - self.px(MARGIN), bottom: h - self.px(BOTTOM) }
    }

    fn tile_size(&self) -> (i32, i32) {
        self.tiles.first().map_or((0, 0), |t| (t.rect.right - t.rect.left, t.rect.bottom - t.rect.top))
    }

    /// The tiles' face as cached now, or the desktop color while it is first read.
    fn read_face(&mut self) {
        self.face = wallpaper::cached(self.rect, self.tile_size()).unwrap_or(Face::Color(desktop_color()));
    }

    /// Lay out the strip and the spread from `places` (a `ws::windows()` list).
    fn build(&mut self, places: &[ws::Place]) {
        let (w, h) = self.size();
        self.shown = ws::active(&self.device);
        // The old thumbnails go before the new ones come.
        self.tiles.clear();
        self.slots.clear();
        let mine: Vec<&ws::Place> = places.iter().filter(|p| p.device == self.device).collect();
        for (i, rect) in strip(w, h, settings::get().workspaces, self.scale).into_iter().enumerate() {
            // ws::windows() lists front first (on screen in z order, hidden top first), and
            // DWM draws the thumbnail registered last on top, so register back to front. A
            // pinned window is in the shown workspace's tile only.
            let here = |p: &&&ws::Place| if p.pinned { i == self.shown } else { p.workspace == i };
            let minis = mine.iter().rev().filter(here).filter_map(|p| self.mini(rect, p.hwnd)).collect();
            self.tiles.push(Tile { rect, minis });
        }
        let labels = settings::get().overview_labels;
        self.labels = labels;
        let band = if labels == Labels::None { 0 } else { self.px(LABEL) };
        let shown: Vec<&ws::Place> = mine.iter().filter(|p| p.on_screen).copied().collect();
        let reals: Vec<RECT> = shown.iter().map(|p| ws::real_rect(p.hwnd)).collect();
        let (rects, cell_w) = spread(self.spread_area(), &reals, self.px(SLOT_GAP), band);
        self.cell_w = cell_w;
        self.slots = shown
            .into_iter()
            .zip(rects)
            .map(|(p, rect)| {
                let thumb = thumb(self.popup, p.hwnd, rect, rect);
                let card = thumb.is_none();
                let icon = if card || labels == Labels::Always { finder::icon(p.hwnd) } else { HICON::default() };
                Slot { hwnd: p.hwnd, rect, title: ws::title(p.hwnd), icon, pinned: p.pinned, card, thumb }
            })
            .collect();
        self.read_face();
    }

    /// A window inside `tile` at its real place on this screen, cut at the tile's edge so it
    /// never draws onto the next tile. None when none of it falls inside.
    fn mini(&self, tile: RECT, hwnd: HWND) -> Option<Slot> {
        let full = place_in(ws::real_rect(hwnd), self.rect, tile);
        let rect = intersect(full, tile)?;
        let thumb = thumb(self.popup, hwnd, full, rect);
        let card = thumb.is_none();
        let icon = if card { finder::icon(hwnd) } else { HICON::default() };
        Some(Slot { hwnd, rect, title: String::new(), icon, pinned: false, card, thumb })
    }

    fn hit(&self, pt: POINT) -> Hit {
        if let Some(s) = self.slots.iter().find(|s| inside(&s.rect, pt)) {
            return Hit::Slot(s.hwnd);
        }
        let Some(i) = self.tiles.iter().position(|t| inside(&t.rect, pt)) else { return Hit::Background };
        // Front to back, so the window on top takes the click.
        match self.tiles[i].minis.iter().rev().find(|m| inside(&m.rect, pt)) {
            Some(m) => Hit::Mini(i, m.hwnd),
            None => Hit::Tile(i),
        }
    }

    /// The workspace a drop at `pt` lands on: a tile's, else the shown one when it is in the
    /// spread. None anywhere else, which cancels the drop.
    fn drop_target(&self, pt: POINT) -> Option<usize> {
        self.tiles.iter().position(|t| inside(&t.rect, pt)).or_else(|| inside(&self.spread_area(), pt).then_some(self.shown))
    }

    /// The top left spread window, where a selection starts.
    fn first(&self) -> Option<HWND> {
        first(&self.rects()).map(|i| self.slots[i].hwnd)
    }

    fn rects(&self) -> Vec<RECT> {
        self.slots.iter().map(|s| s.rect).collect()
    }

    /// The band under a spread window, where its label goes: as wide as the window with
    /// labels "Always", as wide as its cell for a hover or chosen label (AC-OVR-6).
    fn label_rect(&self, s: &Slot, always: bool) -> RECT {
        let (left, right) = if always {
            (s.rect.left, s.rect.right)
        } else {
            let mid = (s.rect.left + s.rect.right) / 2;
            (mid - self.cell_w / 2, mid + self.cell_w / 2)
        };
        RECT { left, top: s.rect.bottom, right, bottom: s.rect.bottom + self.px(LABEL) }
    }

    /// What a spread window's marks cover: its ring and its widest label.
    fn mark_rect(&self, hwnd: HWND) -> Option<RECT> {
        let s = self.slots.iter().find(|s| s.hwnd == hwnd)?;
        Some(union(grow(s.rect, self.px(RING_OFF + RING) + 1), self.label_rect(s, false)))
    }

    /// What a tile's ring covers, at its widest (high contrast's 3 px target ring).
    fn tile_ring(&self, i: usize) -> Option<RECT> {
        Some(grow(self.tiles.get(i)?.rect, self.px(TILE_RING_OFF + 3.0) + 1))
    }

    /// The bottom of the screen: the bar, its shadow included.
    fn bar_strip(&self) -> RECT {
        let (w, h) = self.size();
        RECT { left: 0, top: h - self.px(BOTTOM + SHADOW_BLUR), right: w, bottom: h }
    }

    /// The four thin sides of another screen's spread ring, not the spread inside it.
    fn spread_ring(&self) -> [RECT; 4] {
        let o = grow(self.spread_area(), self.px(SPREAD_AIM + RING) + 1);
        let t = self.px(RING) + 2;
        [
            RECT { bottom: o.top + t, ..o },
            RECT { top: o.bottom - t, ..o },
            RECT { right: o.left + t, ..o },
            RECT { left: o.right - t, ..o },
        ]
    }

    /// Every preview on this screen: the spread first, then the tiles' minis.
    fn all(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().chain(self.tiles.iter().flat_map(|t| &t.minis))
    }

    /// Where the hover X goes for `hwnd`, in screen coordinates. None when its window is
    /// too small to hold it.
    fn close_spot(&self, hwnd: HWND) -> Option<POINT> {
        let r = self.slots.iter().find(|s| s.hwnd == hwnd)?.rect;
        let (d, inset) = (self.px(CLOSE_BTN), self.px(CLOSE_INSET));
        (r.right - r.left >= d + 2 * inset && r.bottom - r.top >= d + 2 * inset).then(|| POINT { x: self.rect.left + r.right - inset - d, y: self.rect.top + r.top + inset })
    }

    /// "Workspace 2 of 4 shown, 3 windows": what Narrator says first about this screen.
    fn summary(&self) -> String {
        format!("Workspace {} of {} shown, {}", self.shown + 1, self.tiles.len(), count(self.slots.len()))
    }

    /// This screen as Narrator sees it, in popup pixels: the tiles, the spread windows with
    /// the chosen one selected, and the bar's words.
    fn node(&self, chosen: Option<HWND>, bar: Option<Bar>) -> Node {
        let text = |name: String, rect: RECT| Node { role: UIA_TextControlTypeId, name, rect, ..Node::default() };
        let item = |name: String, rect: RECT, selected: bool| Node { role: UIA_ListItemControlTypeId, name, rect, selected: Some(selected), kids: vec![] };
        let tiles = (self.tiles.iter().enumerate())
            .map(|(i, t)| {
                let how = if i == self.shown { "shown" } else { "not shown" };
                item(format!("Workspace {}, {how}, {}", i + 1, count(t.minis.len())), t.rect, i == self.shown)
            })
            .collect();
        let band = RECT { left: 0, top: 0, right: self.size().0, bottom: self.band_h() };
        let mut kids = vec![Node { role: UIA_ListControlTypeId, name: "Workspaces".into(), rect: band, selected: None, kids: tiles }];
        let windows = self.slots.iter().map(|s| item(window_words(s, &finder::program(s.hwnd), chosen == Some(s.hwnd)), s.rect, chosen == Some(s.hwnd))).collect();
        kids.push(Node { role: UIA_ListControlTypeId, name: "Windows".into(), rect: self.spread_area(), selected: None, kids: windows });
        if self.slots.is_empty() {
            kids.push(text("No windows".into(), self.spread_area()));
        }
        if let Some(b) = bar {
            kids.push(text(bar_words(self, b), self.bar_strip()));
        }
        let (w, h) = self.size();
        Node { role: UIA_PaneControlTypeId, name: "Overview".into(), rect: RECT { left: 0, top: 0, right: w, bottom: h }, selected: None, kids }
    }
}

fn count(n: usize) -> String {
    match n {
        0 => "no windows".into(),
        1 => "1 window".into(),
        n => format!("{n} windows"),
    }
}

/// A spread window as Narrator reads it: the title it paints, then its app.
fn window_words(s: &Slot, program: &str, chosen: bool) -> String {
    let mut w = s.title.clone();
    if !program.is_empty() {
        w += &format!(", {program}");
    }
    if s.pinned {
        w += ", pinned";
    }
    if chosen {
        w += ", chosen";
    }
    w
}

fn bar_words(sc: &Screen, bar: Bar) -> String {
    match bar {
        Bar::Keys => format!("Enter, go to. Drag, move. 1 to {}, show workspace. Ctrl W, close window. Esc, cancel.", sc.tiles.len()),
        Bar::Drag => "Drop on a workspace. Esc, cancel.".into(),
        Bar::Note(n) => {
            let (_, line, under, _) = n.words();
            format!("{line}. {under}")
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        // Thumbnails before the popup they draw into.
        self.slots.clear();
        self.tiles.clear();
        unsafe {
            let _ = DestroyWindow(self.close_btn);
            let _ = DestroyWindow(self.popup);
        }
    }
}

enum Hit {
    Slot(HWND),
    Mini(usize, HWND), // (tile, window)
    Tile(usize),
    Background,
}

/// The small live preview under the mouse during a drag (AC-7); its own popup, above every
/// screen's overview, with Windows' rounded corners, shadow, and a 1 px edge in the text color
/// (AC-OVR-5). Taken down when dropped.
struct Ghost {
    hwnd: HWND,
    size: (i32, i32),
    real: (i32, i32), // the window's own size
    small: bool, // over a band: the size it will have in a tile
    thumb: Option<Thumb>, // none: a plain card
}

impl Drop for Ghost {
    fn drop(&mut self) {
        self.thumb = None; // before the popup it draws into
        unsafe { let _ = DestroyWindow(self.hwnd); }
    }
}

/// A `w` wide picture of a `real` sized window.
fn ghost_size(real: (i32, i32), w: i32) -> (i32, i32) {
    (w, (w as i64 * real.1.max(1) as i64 / real.0.max(1) as i64) as i32)
}

enum Stage {
    Pressed,        // not yet past the drag distance: still a click
    Moving(Ghost),
    Cancelled,      // Esc or a right click: the release does nothing
}

/// A press on a window, from its press to the button's release. The pressed popup holds the
/// mouse capture throughout, so the drag can cross screens.
struct Drag {
    hwnd: HWND,
    from: (String, usize), // where it lives: screen and workspace
    start: POINT,          // screen coordinates
    scale: f32,            // the pressed screen's, for the ghost's size
    stage: Stage,
    aim: Option<(usize, Aim)>, // (screen, where) a drop now would land
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
}

/// The popups of a closed session while they fade out: no focus, and clicks go through them.
struct Leaving {
    screens: Vec<Screen>,
    start: Instant,
    from: u8, // the alpha the fade starts at
}

/// A listed window and where it lives, as the refresh compares them.
type Seen = (HWND, String, usize, bool);

fn seen(places: &[ws::Place]) -> Vec<Seen> {
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

/// What one screen's painting marks, besides its windows and tiles.
#[derive(Default)]
struct Marks {
    hover: Option<HWND>,
    selected: Option<HWND>,
    src: Option<HWND>, // the window being dragged, on its own screen
    aim: Option<Aim>,
    tile_hover: Option<usize>,
    bar: Option<Bar>,
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

fn aim_rects(sc: &Screen, a: Aim) -> Vec<RECT> {
    match a {
        Aim::Tile(i) => sc.tile_ring(i).into_iter().collect(),
        Aim::Spread => sc.spread_ring().to_vec(),
    }
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static LEAVING: RefCell<Option<Leaving>> = const { RefCell::new(None) };
    static X_HOT: Cell<bool> = const { Cell::new(false) };
    // What Narrator last heard: screen, chosen window, the bar's words.
    static SAID: RefCell<Option<(usize, isize, String)>> = const { RefCell::new(None) };
    // The floating shadow's picture per scale and strength (AC-OVR-3): a memory DC holding it,
    // and how far it reaches past its box. Made once and kept, like the fonts.
    static SHADOWS: RefCell<HashMap<(u32, u8), (HDC, i32)>> = RefCell::new(HashMap::new());
}

/// The strip: `count` tiles with the screen's aspect, 12% of its height, in a centered row
/// near the top, shrunk to fit between the side margins.
fn strip(w: i32, h: i32, count: usize, scale: f32) -> Vec<RECT> {
    let px = |v: f32| (v * scale).round() as i32;
    let (n, gap) = (count as i32, px(TILE_GAP));
    let mut th = (h as f32 * TILE_HEIGHT).round() as i32;
    let mut tw = (th as i64 * w as i64 / h.max(1) as i64) as i32;
    let room = w - 2 * px(MARGIN);
    if n * tw + (n - 1) * gap > room {
        tw = (room - (n - 1) * gap) / n;
        th = (tw as i64 * h as i64 / w.max(1) as i64) as i32;
    }
    let left = (w - (n * tw + (n - 1) * gap)) / 2;
    let top = px(TILE_TOP);
    (0..n).map(|i| RECT { left: left + i * (tw + gap), top, right: left + i * (tw + gap) + tw, bottom: top + th }).collect()
}

/// The spread as a grid of equal cells inside `area`, with the row count that makes the
/// windows largest (fewest rows on a tie). Rows fill by each window's real center (`reals`,
/// screen pixels) top to bottom, then left to right within a row, so a window keeps its
/// side of the screen. Each is scaled to fit its cell above a `band` for its label, keeping
/// its aspect, never above its real size, and centered there; rows are centered. Returned
/// in the order of `reals`, with the cells' width.
fn spread(area: RECT, reals: &[RECT], gap: i32, band: i32) -> (Vec<RECT>, i32) {
    let n = reals.len() as i32;
    if n == 0 {
        return (Vec::new(), 0);
    }
    let sizes: Vec<(i32, i32)> = reals.iter().map(|r| (r.right - r.left, r.bottom - r.top)).collect();
    let (aw, ah) = (area.right - area.left, area.bottom - area.top);
    let fit = |cw: i32, ch: i32, (w, h): (i32, i32)| (cw as f64 / w.max(1) as f64).min(ch as f64 / h.max(1) as f64).min(1.0);
    // (cell width, room for the window above its label, columns)
    let cell = |rows: i32| {
        let cols = (n + rows - 1) / rows;
        ((aw - gap * (cols - 1)) / cols, (ah - gap * (rows - 1)) / rows - band, cols)
    };
    let shown_area = |rows: i32| {
        let (cw, ch, _) = cell(rows);
        if cw <= 0 || ch <= 0 {
            return 0.0;
        }
        sizes.iter().map(|&s| fit(cw, ch, s).powi(2) * s.0 as f64 * s.1 as f64).sum::<f64>()
    };
    let mut rows = 1;
    for r in 2..=n {
        if shown_area(r) > shown_area(rows) {
            rows = r;
        }
    }
    let (cw, ch, cols) = cell(rows);
    let center = |i: usize| ((reals[i].left + reals[i].right) / 2, (reals[i].top + reals[i].bottom) / 2);
    // Stable sorts: windows level with each other keep their front to back order.
    let mut order: Vec<usize> = (0..reals.len()).collect();
    order.sort_by_key(|&i| center(i).1);
    for row in order.chunks_mut(cols as usize) {
        row.sort_by_key(|&i| center(i).0);
    }
    let mut out = vec![RECT::default(); reals.len()];
    for (k, &i) in order.iter().enumerate() {
        let (row, col) = (k as i32 / cols, k as i32 % cols);
        let in_row = (n - row * cols).min(cols);
        let x = area.left + (aw - (in_row * cw + (in_row - 1) * gap)) / 2 + col * (cw + gap);
        let y = area.top + row * (ch + band + gap);
        let f = fit(cw.max(0), ch.max(0), sizes[i]);
        let (w, h) = ((sizes[i].0 as f64 * f).round() as i32, (sizes[i].1 as f64 * f).round() as i32);
        let (left, top) = (x + (cw - w) / 2, y + (ch - h) / 2);
        out[i] = RECT { left, top, right: left + w, bottom: top + h };
    }
    (out, cw.max(0))
}

/// Where `real` (screen pixels) lands inside `tile`, a small copy of `screen`.
fn place_in(real: RECT, screen: RECT, tile: RECT) -> RECT {
    let sx = (tile.right - tile.left) as f64 / (screen.right - screen.left).max(1) as f64;
    let sy = (tile.bottom - tile.top) as f64 / (screen.bottom - screen.top).max(1) as f64;
    let x = |v: i32| tile.left + ((v - screen.left) as f64 * sx).round() as i32;
    let y = |v: i32| tile.top + ((v - screen.top) as f64 * sy).round() as i32;
    RECT { left: x(real.left), top: y(real.top), right: x(real.right), bottom: y(real.bottom) }
}

fn center(r: &RECT) -> (i32, i32) {
    ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
}

/// `rects` in reading order by center: rows top down, left to right within a row. A row
/// is windows whose centers fall inside the one above's height, not one exact line, since
/// centering each window in its cell rounds its center by a pixel either way.
fn reading_order(rects: &[RECT]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by_key(|&i| center(&rects[i]).1);
    for row in order.chunk_by_mut(|&a, &b| center(&rects[b]).1 < rects[a].bottom) {
        row.sort_by_key(|&i| center(&rects[i]).0);
    }
    order
}

/// The top left of `rects`, where a selection starts.
fn first(rects: &[RECT]) -> Option<usize> {
    reading_order(rects).first().copied()
}

/// The window `by` Tab stops from `rects[from]` in reading order, around the end; the first
/// one when there is no `from`. None on an empty screen, so Tab there does nothing.
fn tab_step(rects: &[RECT], from: Option<usize>, by: isize) -> Option<usize> {
    let order = reading_order(rects);
    let Some(p) = from.and_then(|f| order.iter().position(|&k| k == f)) else { return order.first().copied() };
    Some(order[(p as isize + by).rem_euclid(order.len() as isize) as usize])
}

/// The nearest of `rects` from `rects[from]` in direction `(dx, dy)` (one of them 1 or -1,
/// the other 0), by centers. Being off the line counts double, so Right goes along the row
/// before it drops to the next one. None when nothing lies that way.
fn nearest(rects: &[RECT], from: usize, (dx, dy): (i32, i32)) -> Option<usize> {
    let (fx, fy) = center(&rects[from]);
    (0..rects.len())
        .filter(|&i| i != from)
        .filter_map(|i| {
            let (x, y) = center(&rects[i]);
            let along = (x - fx) * dx + (y - fy) * dy;
            let off = ((x - fx) * dy).abs() + ((y - fy) * dx).abs();
            (along > 0).then_some((along + 2 * off, i))
        })
        .min()
        .map(|(_, i)| i)
}

/// Ease out: fast at first, then settling. 0 at the start, exactly 1 at the end. Squared,
/// not cubed: a cube covers most of the way in the first frames and looks like no animation.
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

/// `[overview] fade_ms` long (spec 0017 AC-7); done at once with `fastest` on (AC-6).
fn progress(start: Instant) -> f32 {
    let ms = ui::motion_ms(settings::get().overview_fade_ms);
    if ms == 0 {
        return 1.0;
    }
    ease((start.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0))
}

fn intersect(a: RECT, b: RECT) -> Option<RECT> {
    let r = RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) };
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

/// The part of a `src` sized picture that lands in `shown` when the whole picture fills `full`.
fn source_rect(full: RECT, shown: RECT, src: SIZE) -> RECT {
    let fw = (full.right - full.left).max(1) as f64;
    let fh = (full.bottom - full.top).max(1) as f64;
    let x = |v: i32| ((v - full.left) as f64 * src.cx as f64 / fw).round() as i32;
    let y = |v: i32| ((v - full.top) as f64 * src.cy as f64 / fh).round() as i32;
    RECT { left: x(shown.left), top: y(shown.top), right: x(shown.right), bottom: y(shown.bottom) }
}

/// A live preview of `src` as if it filled `full`, drawn only inside `shown` (a mini cut at
/// its tile's edge; the same rect in the spread). None when Windows draws nothing for it.
fn thumb(host: HWND, src: HWND, full: RECT, shown: RECT) -> Option<Thumb> {
    let t = Thumb(unsafe { DwmRegisterThumbnail(host, src) }.inspect_err(|e| eprintln!("overview: no preview for \"{}\": {e}", ws::title(src))).ok()?);
    let size = unsafe { DwmQueryThumbnailSourceSize(t.0) }.inspect_err(|e| eprintln!("overview: no preview size for \"{}\": {e}", ws::title(src))).ok()?;
    if size.cx <= 0 || size.cy <= 0 {
        eprintln!("overview: Windows draws nothing for \"{}\", showing a card", ws::title(src));
        return None;
    }
    let mut p = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
        rcDestination: shown,
        fVisible: BOOL(1),
        opacity: 255,
        ..Default::default()
    };
    if shown != full {
        p.dwFlags |= DWM_TNP_RECTSOURCE;
        p.rcSource = source_rect(full, shown, size);
    }
    unsafe { DwmUpdateThumbnailProperties(t.0, &p) }.inspect_err(|e| eprintln!("overview: preview for \"{}\" not placed: {e}", ws::title(src))).ok()?;
    Some(t)
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

/// The X as the design draws it, 28 dip square at x, y: card color with an edge, Windows'
/// close red when pointed at (AC-OVR-2). Two strokes rather than the glyph: text on a per
/// pixel alpha popup would come out see through.
fn draw_x(c: &Canvas, x: f32, y: f32, hot: bool) {
    let (b, hc) = (c.t.b, c.t.hc);
    let (fill, edge, ink) = match (hot, hc) {
        (true, false) => (CLOSE_RED, None, 0xFFFFFF),
        (true, true) => (b.acc, None, b.on_acc),
        _ => (b.card, Some(b.line), b.tx),
    };
    c.boxed((x, y, CLOSE_BTN, CLOSE_BTN), CLOSE_R, fill, edge);
    let (m, k) = (CLOSE_BTN / 2.0, 4.0); // the 10 px glyph's arms
    c.lines(&[(x + m - k, y + m - k), (x + m + k, y + m + k)], 1.0, op(ink));
    c.lines(&[(x + m + k, y + m - k), (x + m - k, y + m + k)], 1.0, op(ink));
}

fn paint_x(h: HWND, hot: bool) {
    // Its owner covers the screen it sits on; the X itself is made on the main screen.
    let owner = unsafe { GetWindow(h, GW_OWNER) }.unwrap_or(h);
    let scale = unsafe { GetDpiForWindow(owner) } as f32 / 96.0;
    let mut r = RECT::default();
    unsafe { let _ = GetWindowRect(h, &mut r); }
    let d = (CLOSE_BTN * scale).round() as i32;
    ui::layered(h, (r.left, r.top, d, d), scale, ui::theme(), |c| draw_x(c, 0.0, 0.0, hot));
}

/// The drag ghost for `src`: 240 dip wide at `scale`, the window's aspect, not shown yet.
fn ghost(src: HWND, scale: f32) -> Option<Ghost> {
    let real = ws::real_rect(src);
    let real = ((real.right - real.left).max(1), (real.bottom - real.top).max(1));
    let (w, h) = ghost_size(real, (GHOST_W * scale).round() as i32);
    let hwnd = ui::popup(GHOST_CLASS, Some(ghost_wndproc), WS_EX_TOPMOST | WS_EX_NOACTIVATE, false)?;
    ui::border(hwnd, ui::theme().b.tx);
    unsafe { let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE); }
    let full = RECT { left: 0, top: 0, right: w, bottom: h };
    Some(Ghost { hwnd, size: (w, h), real, small: false, thumb: thumb(hwnd, src, full, full) })
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
    let (_, mouse) = ws::under_mouse();
    let places = ws::windows();
    // Numbered like the finder's screens: left to right, then top down.
    let mut order = ws::screens();
    order.sort_by_key(|(_, r)| (r.left, r.top));
    let style = settings::get().overview_animation;
    ui::gdiplus();
    let mut screens = Vec::new();
    for (device, rect) in order {
        let Some(popup) = popup(rect, style != Animation::Off) else {
            eprintln!("overview: popup for {device} not created");
            continue;
        };
        let (mut dpi, mut dy) = (96u32, 96u32);
        unsafe { let _ = GetDpiForMonitor(MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
        let scale = dpi as f32 / 96.0;
        let close_btn = close_button(popup, scale).unwrap_or_else(|| {
            eprintln!("overview: close button for {device} not created");
            HWND::default()
        });
        let mut sc = Screen { device, popup, rect, scale, shown: 0, tiles: Vec::new(), slots: Vec::new(), cell_w: 0, labels: Labels::Hover, face: Face::Card, close_btn, note: None, painted: false, tree: Tree::default() };
        sc.build(&places);
        screens.push(sc);
    }
    let start = screens.iter().position(|s| s.device == mouse).unwrap_or(0);
    let Some(sc) = screens.get(start) else {
        return eprintln!("overview: no screen to open on");
    };
    let focus = sc.popup;
    // The window you came from when it is in this spread, else the first one there.
    let selected = (start, sc.slots.iter().any(|s| s.hwnd == came_from).then_some(came_from).or_else(|| sc.first()));
    let windows: usize = screens.iter().map(|s| s.slots.len()).sum();
    let popups: Vec<HWND> = screens.iter().map(|s| s.popup).collect();
    // Read off the open's path; the tiles take the wallpaper when it is ready.
    wallpaper::check(screens.iter().map(|s| (s.rect, s.tile_size())).collect());
    let fading_in = (style == Animation::Fade).then(Instant::now);
    let alpha = if fading_in.is_some() { 0 } else { 255 };
    SAID.set(None);
    SESSION.set(Some(Session { screens, came_from, hover: None, tile_hover: None, selected, closing: Vec::new(), drag: None, seen: seen(&places), fading_in, alpha, asked, logged: false }));
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
            return eprintln!("overview: Windows refused focus");
        }
    }
    println!("overview: opened on {} screen(s), {windows} window(s) shown", popups.len());
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
        println!("{l}");
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

/// Off: every popup goes at once. Fade: they stay a moment, without focus and letting clicks
/// through, while they fade out (AC-11).
fn leave(s: Session) {
    drop(LEAVING.take());
    if settings::get().overview_animation == Animation::Off {
        return drop(s);
    }
    let Session { screens, drag, alpha, .. } = s;
    if drag.is_some() {
        unsafe { let _ = ReleaseCapture(); }
    }
    drop(drag); // and its ghost
    let Some(first) = screens.first().map(|sc| sc.popup) else { return };
    for sc in &screens {
        unsafe {
            let _ = ShowWindow(sc.close_btn, SW_HIDE);
            for id in [CLOSING_TIMER, NOTE_TIMER, REFRESH_TIMER, ANIM_TIMER, WALL_TIMER] {
                let _ = KillTimer(Some(sc.popup), id);
            }
            SetWindowLongPtrW(sc.popup, GWL_EXSTYLE, GetWindowLongPtrW(sc.popup, GWL_EXSTYLE) | WS_EX_TRANSPARENT.0 as isize);
        }
    }
    LEAVING.set(Some(Leaving { screens, start: Instant::now(), from: alpha }));
    unsafe { SetTimer(Some(first), ANIM_TIMER, FRAME_MS, None) };
}

/// The whole popup at `alpha`, previews too (Windows may not carry the popup's alpha over to
/// the previews it draws into it).
fn set_alpha(sc: &Screen, alpha: u8) {
    unsafe { let _ = SetLayeredWindowAttributes(sc.popup, COLORREF(0), alpha, LWA_ALPHA); }
    for t in sc.all().filter_map(|x| x.thumb.as_ref()) {
        t.set_opacity(alpha);
    }
}

/// One frame of the session's fade in; true when it just ended.
fn step(s: &mut Session) -> bool {
    let Some(start) = s.fading_in else { return false };
    let e = progress(start);
    s.alpha = (255.0 * e).round() as u8;
    for sc in &s.screens {
        set_alpha(sc, s.alpha);
    }
    let done = e >= 1.0;
    if done {
        s.fading_in = None;
    }
    done
}

/// The animation timer: a frame of the open session, or of the popups leaving.
fn anim_tick(popup: HWND) {
    if !is_ours(popup) {
        return leaving_tick();
    }
    if SESSION.with_borrow_mut(|s| s.as_mut().is_some_and(step)) {
        unsafe { let _ = KillTimer(Some(popup), ANIM_TIMER); }
    }
}

fn leaving_tick() {
    let done = LEAVING.with_borrow(|l| {
        let Some(l) = l else { return true };
        let e = progress(l.start);
        let alpha = (l.from as f32 * (1.0 - e)).round() as u8;
        for sc in &l.screens {
            set_alpha(sc, alpha);
        }
        e >= 1.0
    });
    if done {
        drop(LEAVING.take()); // outside the borrow: destroying a popup sends it messages
    }
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

/// A digit: switch the selected screen to that workspace.
fn jump(i: usize) {
    if i >= settings::get().workspaces {
        return;
    }
    let target = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let sc = s.screens.get(s.selected.0)?;
        Some((sc.device.clone(), sc.shown))
    });
    if let Some((device, shown)) = target {
        show(&device, shown, i);
    }
}

/// Closed first, focus back, so the switch moves focus on exactly as a switch would; the
/// workspace the screen already shows just closes.
fn show(device: &str, shown: usize, i: usize) {
    close(true);
    if i != shown {
        ws::show_workspace(device, i);
    }
}

/// Land on a window, wherever it lives, like the finder's Enter.
fn go(hwnd: HWND) {
    match ws::go_to(hwnd) {
        Ok(()) => {
            finder::used(hwnd); // our own focus change: the foreground hook skips it
            close(false);
        }
        Err(Miss::Gone | Miss::CantMove | Miss::Unplugged) => {
            println!("overview: a window is gone");
            rebuild();
        }
    }
}

/// Every screen laid out again from the current window list, after windows moved or went:
/// one may sit in a tile on one screen and in the spread on another.
fn rebuild() {
    let mut places = ws::windows();
    let (popups, old) = SESSION.with_borrow_mut(|s| {
        let Some(s) = s.as_mut() else { return (Vec::new(), None) };
        // Seen with the closing windows, so the refresh does not take them for a change.
        s.seen = seen(&places);
        places.retain(|p| !s.closing.iter().any(|c| c.0 == p.hwnd));
        for sc in &mut s.screens {
            sc.build(&places);
        }
        s.fix_selection();
        s.tile_hover = None;
        (s.screens.iter().map(|sc| sc.popup).collect(), s.hover.take())
    });
    hover_changed(old);
    invalidate(popups);
}

/// Every 250 ms: catch windows that opened, went or moved since the layout was made (AC-10).
/// Skipped mid drag, so the spread never shifts under a press.
fn refresh() {
    if SESSION.with_borrow(|s| s.as_ref().is_none_or(|s| s.drag.is_some())) {
        return;
    }
    let now = seen(&ws::windows());
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
    let title = ws::title(h);
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        println!("overview: \"{title}\" is gone");
        return drop_gone(&[h]);
    }
    match unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
            println!("overview: asked \"{title}\" to close");
            SESSION.with_borrow_mut(|s| {
                if let Some(s) = s {
                    s.closing.push((h, title, Instant::now(), popup));
                }
            });
            // Some apps (Store ones like Settings) shrink what they draw while closing, and
            // the preview would stretch that to fill its slot.
            drop_gone(&[h]);
            unsafe { SetTimer(Some(popup), CLOSING_TIMER, CLOSING_POLL_MS, None) };
        }
        Err(e) => {
            eprintln!("overview: can't close \"{title}\": {e}");
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
    for (_, title, ..) in &gone {
        println!("overview: closed \"{title}\"");
    }
    for (_, title, ..) in &back {
        println!("overview: \"{title}\" is still open, showing it again");
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

fn on_click(popup: HWND, pt: POINT) {
    let hit = SESSION.with_borrow(|s| {
        let sc = s.as_ref()?.screens.iter().find(|sc| sc.popup == popup)?;
        Some((sc.device.clone(), sc.shown, sc.hit(pt)))
    });
    match hit {
        Some((_, _, Hit::Slot(h) | Hit::Mini(_, h))) => go(h),
        Some((device, shown, Hit::Tile(i))) => show(&device, shown, i),
        Some((.., Hit::Background)) => close(true),
        None => {}
    }
}

fn on_middle_click(popup: HWND, pt: POINT) {
    // During a drag the point is relative to the pressed popup, whichever screen it is on.
    let hit = SESSION.with_borrow(|s| s.as_ref().filter(|s| s.drag.is_none())?.screens.iter().find(|sc| sc.popup == popup).map(|sc| sc.hit(pt)));
    if let Some(Hit::Slot(h)) = hit {
        close_window(popup, h);
    }
}

/// A press on a window may become a drag: the popup takes the mouse capture, so the moves
/// and the release come back to it from any screen.
fn on_press(popup: HWND, pt: POINT) {
    let start = cursor();
    let started = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.iter().find(|sc| sc.popup == popup)?;
        let (hwnd, ws) = match sc.hit(pt) {
            Hit::Slot(h) => (h, sc.shown),
            Hit::Mini(i, h) => (h, i),
            _ => return None,
        };
        s.drag = Some(Drag { hwnd, from: (sc.device.clone(), ws), start, scale: sc.scale, stage: Stage::Pressed, aim: None });
        Some(())
    });
    if started.is_some() {
        unsafe { SetCapture(popup) };
    }
}

/// The dragged window's preview at 40% while it moves, as it was after (AC-OVR-4). High
/// contrast keeps it whole and outlines it instead.
fn dim_source(hwnd: HWND, dim: bool) {
    let opacity = if dim && !ui::theme().hc { 102 } else { 255 };
    SESSION.with_borrow(|s| {
        for sc in s.iter().flat_map(|s| &s.screens) {
            for t in sc.all().filter(|x| x.hwnd == hwnd).filter_map(|x| x.thumb.as_ref()) {
                t.set_opacity(opacity);
            }
        }
    });
}

/// Where a drop at `pt` (screen coordinates) would land when it changes anything, for a window
/// living at `from`: a tile, or a spread. Never its own tile or spread (AC-OVR-4). Also the
/// screen under the mouse and whether `pt` is over that screen's band.
fn aim_at(s: &Session, pt: POINT, from: &(String, usize)) -> Option<(usize, Option<Aim>, bool)> {
    let i = s.screens.iter().position(|sc| inside(&sc.rect, pt))?;
    let sc = &s.screens[i];
    let local = POINT { x: pt.x - sc.rect.left, y: pt.y - sc.rect.top };
    let own = |ws: usize| (sc.device.as_str(), ws) == (from.0.as_str(), from.1);
    let aim = match sc.tiles.iter().position(|t| inside(&t.rect, local)) {
        Some(k) => (!own(k)).then_some(Aim::Tile(k)),
        None => (inside(&sc.spread_area(), local) && !own(sc.shown)).then_some(Aim::Spread),
    };
    Some((i, aim, local.y < sc.band_h()))
}

/// A move with a window pressed: past the drag distance the ghost appears, then follows the
/// mouse, and the place a drop would land gets its ring. False when nothing is pressed, so the
/// move is an ordinary one.
fn drag_move() -> bool {
    let pt = cursor();
    let pressed = SESSION.with_borrow(|s| {
        let d = s.as_ref()?.drag.as_ref()?;
        let moving = match &d.stage {
            Stage::Pressed => false,
            Stage::Moving(_) => true,
            Stage::Cancelled => return Some(None),
        };
        Some(Some((d.hwnd, d.start, d.scale, moving)))
    });
    let Some(pressed) = pressed else { return false };
    // Cancelled: nothing moves until the release.
    let Some((hwnd, start, scale, moving)) = pressed else { return true };
    if !moving {
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
        if (pt.x - start.x).abs() <= cx && (pt.y - start.y).abs() <= cy {
            return true;
        }
        let Some(g) = ghost(hwnd, scale) else {
            eprintln!("overview: drag preview for \"{}\" not created", ws::title(hwnd));
            return true;
        };
        // The X would sit on a window that is now moving.
        let old = SESSION.with_borrow_mut(|s| {
            let s = s.as_mut()?;
            s.drag.as_mut()?.stage = Stage::Moving(g);
            s.hover.take()
        });
        hover_changed(old);
        dim_source(hwnd, true);
        // The chosen ring hides and the bar says what a drop does.
        invalidate_rects(SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default());
    }
    // The bar speaks on the screen under the mouse.
    let on = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.screens.iter().find(|sc| inside(&sc.rect, pt)).map(|sc| sc.popup)));
    if let Some(p) = on {
        follow_mouse(p);
    }
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let from = s.drag.as_ref()?.from.clone();
        let (i, aim, over_band) = aim_at(s, pt, &from).unwrap_or((usize::MAX, None, false));
        let before = s.drag.as_ref()?.aim;
        let now = aim.map(|a| (i, a));
        let rects: Vec<(HWND, RECT)> = [before, now].into_iter().flatten().filter(|_| before != now).flat_map(|(k, a)| aim_rects(&s.screens[k], a).into_iter().map(move |r| (k, r))).map(|(k, r)| (s.screens[k].popup, r)).collect();
        // Over a band the picture takes its size in that screen's tiles, so the tile it aims
        // at stays in view; it changes size only on crossing the band's rule (AC-OVR-5).
        let tile = s.screens.get(i).map(|sc| (sc.tile_size().0, sc.size().0, sc.scale));
        let d = s.drag.as_mut()?;
        d.aim = now;
        let Stage::Moving(g) = &mut d.stage else { return None };
        let mut resized = None;
        if let Some((tw, sw, sc_scale)) = tile
            && over_band != g.small
        {
            g.small = over_band;
            let w = if over_band { ((g.real.0 as i64 * tw as i64 / sw.max(1) as i64) as i32).max((GHOST_MIN * sc_scale).round() as i32) } else { (GHOST_W * d.scale).round() as i32 };
            g.size = ghost_size(g.real, w);
            if let Some(t) = &g.thumb {
                t.place(RECT { left: 0, top: 0, right: g.size.0, bottom: g.size.1 });
            }
            resized = Some(g.size);
        }
        Some((g.hwnd, g.size, resized, rects))
    });
    let Some((ghost_hwnd, (w, h), resized, rects)) = changed else { return true };
    invalidate_rects(rects);
    let flags = if resized.is_some() { SWP_NOACTIVATE | SWP_SHOWWINDOW } else { SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW };
    unsafe { let _ = SetWindowPos(ghost_hwnd, Some(HWND_TOPMOST), pt.x - w / 2, pt.y - h / 2, w, h, flags); }
    true
}

/// The release: a drop after a drag, a click otherwise, nothing after a cancel.
fn on_release(popup: HWND, pt: POINT) {
    let marks = SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default();
    let drag = SESSION.with_borrow_mut(|s| s.as_mut()?.drag.take());
    // After the take, so the capture change finds no drag to cancel.
    unsafe { let _ = ReleaseCapture(); }
    if let Some(d) = &drag
        && !matches!(d.stage, Stage::Pressed)
    {
        dim_source(d.hwnd, false);
        invalidate_rects(marks);
    }
    match drag {
        Some(Drag { stage: Stage::Cancelled, .. }) => {}
        Some(d @ Drag { stage: Stage::Moving(_), .. }) => drop_at(d),
        _ => on_click(popup, pt),
    }
}

/// Esc or a right click during a drag: the ghost goes, and the release does nothing. False
/// when no drag is under way, so Esc closes the overview as usual.
fn cancel_drag() -> bool {
    let marks = SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default();
    let old = SESSION.with_borrow_mut(|s| {
        let d = s.as_mut()?.drag.as_mut()?;
        d.aim = None;
        matches!(d.stage, Stage::Moving(_)).then(|| (d.hwnd, std::mem::replace(&mut d.stage, Stage::Cancelled)))
    });
    // The ghost goes here, outside the borrow.
    let Some((hwnd, _)) = old else { return false };
    dim_source(hwnd, false);
    invalidate_rects(marks);
    true
}

/// Move the dragged window to the workspace under the mouse, on any screen. Its own place,
/// or anywhere that is neither a tile nor a spread, changes nothing.
fn drop_at(d: Drag) {
    let pt = cursor();
    let target = SESSION.with_borrow(|s| {
        s.as_ref()?.screens.iter().find_map(|sc| {
            let r = sc.rect;
            inside(&r, pt).then(|| (sc.popup, sc.device.clone(), sc.drop_target(POINT { x: pt.x - r.left, y: pt.y - r.top })))
        })
    });
    let (hwnd, from) = (d.hwnd, d.from.clone());
    drop(d); // the ghost goes before the window moves
    let Some((popup, device, Some(to))) = target else { return };
    if (device.as_str(), to) == (from.0.as_str(), from.1) {
        return;
    }
    match ws::place(hwnd, &device, to) {
        Ok(()) => {
            // Hidden now: Esc must not hand focus to a window you can't see.
            if to != ws::active(&device) {
                SESSION.with_borrow_mut(|s| {
                    if let Some(s) = s.as_mut().filter(|s| s.came_from == hwnd) {
                        s.came_from = HWND::default();
                    }
                });
            }
            rebuild();
        }
        Err(Miss::Gone) => {
            println!("overview: a window is gone");
            rebuild();
        }
        Err(Miss::CantMove | Miss::Unplugged) => note(popup, Note::CantMove),
    }
}

/// The selection follows the mouse onto another screen, at that screen's first window, so
/// the keys always act on the screen you are looking at; the bar moves with it.
fn follow_mouse(popup: HWND) {
    let rects = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let i = s.screens.iter().position(|sc| sc.popup == popup)?;
        if i == s.selected.0 {
            return None;
        }
        let marks = |s: &Session| {
            let sc = s.screens.get(s.selected.0)?;
            Some([Some(sc.bar_strip()), s.selected.1.and_then(|h| sc.mark_rect(h))].into_iter().flatten().map(|r| (sc.popup, r)).collect::<Vec<_>>())
        };
        let mut out = marks(s).unwrap_or_default();
        s.selected = (i, s.screens[i].first());
        out.extend(marks(s).unwrap_or_default());
        Some(out)
    });
    invalidate_rects(rects.unwrap_or_default());
}

/// Track the spread window and the tile under the mouse, for their marks, the X and the
/// `"hover"` labels.
fn on_move(popup: HWND, pt: POINT) {
    follow_mouse(popup);
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.iter().find(|sc| sc.popup == popup)?;
        let hit = sc.hit(pt);
        let new = match hit {
            Hit::Slot(h) => Some((popup, h)),
            _ => None,
        };
        let tile = match hit {
            Hit::Tile(i) | Hit::Mini(i, _) => Some((popup, i)),
            _ => None,
        };
        let old_tile = std::mem::replace(&mut s.tile_hover, tile);
        let tiles: Vec<(HWND, RECT)> = if old_tile != tile {
            [old_tile, tile].into_iter().flatten().filter_map(|(p, i)| s.screens.iter().find(|sc| sc.popup == p)?.tile_ring(i).map(|r| (p, r))).collect()
        } else {
            Vec::new()
        };
        let old = std::mem::replace(&mut s.hover, new);
        Some(((old != new).then_some(old), tiles))
    });
    let Some((hover, tiles)) = changed else { return };
    invalidate_rects(tiles);
    if let Some(old) = hover {
        hover_changed(old);
    }
}

/// After `hover` changed from `old`: the X moves to the hovered window (or hides), and the
/// old and new rings and labels redraw. Every screen, since the mouse may have crossed onto
/// another one.
fn hover_changed(old: Option<(HWND, HWND)>) {
    let Some((buttons, marks)) = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let at = |sc: &Screen| s.hover.filter(|(p, _)| *p == sc.popup).and_then(|(_, h)| sc.close_spot(h));
        // A window no longer laid out repaints its whole popup.
        let mark = |(p, h): (HWND, HWND)| {
            let sc = s.screens.iter().find(|sc| sc.popup == p);
            (p, sc.and_then(|sc| sc.mark_rect(h)))
        };
        Some((s.screens.iter().map(|sc| (sc.close_btn, at(sc))).collect::<Vec<_>>(), [old, s.hover].into_iter().flatten().map(mark).collect::<Vec<_>>()))
    }) else {
        return;
    };
    unsafe {
        for (b, at) in buttons {
            match at {
                Some(pt) => { let _ = SetWindowPos(b, Some(HWND_TOPMOST), pt.x, pt.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
                None => { let _ = ShowWindow(b, SW_HIDE); }
            }
        }
        for (p, r) in marks {
            let _ = InvalidateRect(Some(p), r.as_ref().map(|r| r as *const RECT), false);
        }
    }
}

/// The X's click closes the window it sits on.
fn on_close_click() {
    if let Some((popup, h)) = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.hover)) {
        close_window(popup, h);
    }
}

/// Look B's floating shadow under a box (AC-OVR-3), as alphas: `4e + 1` square, the box from
/// `e` to `3e + 1`, a Gaussian blur of `sigma` px at strength `a`. Its corners and the one
/// pixel middle row and column stretch to any box (nine slice).
fn shadow_alphas(e: i32, sigma: f32, a: f32) -> Vec<u8> {
    // Abramowitz and Stegun 7.1.26: the normal curve's area, to 1.5e-7.
    let erf = |x: f32| {
        let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
        let y = 1.0 - ((((1.061_405_4 * t - 1.453_152) * t + 1.421_413_8) * t - 0.284_496_74) * t + 0.254_829_6) * t * (-x * x).exp();
        y.copysign(x)
    };
    let cover = |d: f32| 0.5 * (1.0 + erf(d / (sigma * std::f32::consts::SQRT_2)));
    let n = 4 * e + 1;
    let (near, far) = (e as f32, (3 * e + 1) as f32);
    let g: Vec<f32> = (0..n).map(|p| p as f32 + 0.5).map(|c| (cover(c - near) + cover(far - c) - 1.0).max(0.0)).collect();
    (0..n * n).map(|k| (a * g[(k % n) as usize] * g[(k / n) as usize] * 255.0).round().clamp(0.0, 255.0) as u8).collect()
}

/// The shadow picture for `scale` and strength `a` (0 to 255), made once and kept.
fn shadow_picture(scale: f32, a: u8) -> Option<(HDC, i32)> {
    let key = ((scale * 100.0).round() as u32, a);
    if let Some(&made) = SHADOWS.with_borrow(|m| m.get(&key).copied()).as_ref() {
        return Some(made);
    }
    let sigma = SHADOW_BLUR / 2.0 * scale;
    let e = (3.0 * sigma).ceil() as i32;
    let n = 4 * e + 1;
    let alphas = shadow_alphas(e, sigma, a as f32 / 255.0);
    unsafe {
        let mem = CreateCompatibleDC(None);
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: n, biHeight: -n, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            return None;
        };
        // Premultiplied black: only the alpha byte.
        let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (n * n) as usize);
        for (p, &a) in px.iter_mut().zip(&alphas) {
            *p = (a as u32) << 24;
        }
        SelectObject(mem, bmp.into()); // kept selected for des-k's life
        SHADOWS.with_borrow_mut(|m| m.insert(key, (mem, e)));
        Some((mem, e))
    }
}

/// The floating shadow under `r` (buffer pixels), 8 dip down.
fn shadow(dc: HDC, r: RECT, scale: f32, a: u8) {
    let Some((src, e)) = shadow_picture(scale, a) else { return };
    let dy = (SHADOW_Y * scale).round() as i32;
    let d = RECT { left: r.left - e, top: r.top + dy - e, right: r.right + e, bottom: r.bottom + dy + e };
    let (dw, dh) = (d.right - d.left, d.bottom - d.top);
    // A box smaller than the corners squeezes them.
    let (cx, cy) = ((2 * e).min(dw / 2), (2 * e).min(dh / 2));
    let xs = [(d.left, cx, 0, 2 * e), (d.left + cx, dw - 2 * cx, 2 * e, 1), (d.right - cx, cx, 2 * e + 1, 2 * e)];
    let ys = [(d.top, cy, 0, 2 * e), (d.top + cy, dh - 2 * cy, 2 * e, 1), (d.bottom - cy, cy, 2 * e + 1, 2 * e)];
    let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8, ..Default::default() };
    for (x, w, sx, sw) in xs {
        for (y, h, sy, sh) in ys {
            if w > 0 && h > 0 {
                unsafe { let _ = AlphaBlend(dc, x, y, w, h, src, sx, sy, sw, sh, blend); }
            }
        }
    }
}

/// The shadow's strength in `t`: none in high contrast.
fn shadow_strength(t: &Theme) -> Option<u8> {
    match () {
        _ if t.hc || settings::get().fastest => None, // fastest skips the painting (AC-6)
        _ if t.dark => Some(128), // rgba(0, 0, 0, .5)
        _ => Some(41),            // rgba(0, 0, 0, .16)
    }
}

/// A piece of the bar.
enum Piece {
    Key(String),
    Mouse(String), // a mouse action's dashed cap
    Text(String, F, u32),
    Icon(&'static str, u32),
}

/// The bar's items, each its pieces and whether it is greyed, and the gap inside an item.
fn bar_items(sc: &Screen, bar: Bar, t: &Theme) -> (Vec<(Vec<Piece>, bool)>, f32) {
    let b = t.b;
    let key = |k: &str| Piece::Key(k.into());
    let small = |s: &str| Piece::Text(s.into(), F::Small, b.tx2);
    match bar {
        Bar::Keys => {
            let empty = sc.slots.is_empty();
            let nothing = empty && sc.tiles.iter().all(|t| t.minis.is_empty());
            let n = sc.tiles.len().to_string();
            let items = vec![
                (vec![key("Enter"), small("Go to")], empty),
                (vec![Piece::Mouse("Drag".into()), small("Move")], nothing),
                (vec![key("1"), small("–"), key(&n), small("Show workspace")], false),
                (vec![key("Ctrl"), key("W"), small("Close window")], empty),
                (vec![key("Esc"), small("Cancel")], false),
            ];
            (items, 8.0)
        }
        Bar::Drag => (vec![(vec![small("Drop on a workspace")], false), (vec![key("Esc"), small("Cancel")], false)], 8.0),
        Bar::Note(n) => {
            let (icon, line, under, warn) = n.words();
            (vec![(vec![Piece::Icon(icon, if warn { b.warn } else { b.tx2 }), Piece::Text(line.into(), F::Bold, b.tx), small(under)], false)], 12.0)
        }
    }
}

fn cap_w(c: &Canvas, k: &str) -> f32 {
    (c.width(k, F::Small) + 12.0).max(20.0)
}

fn piece_w(c: &Canvas, p: &Piece) -> f32 {
    match p {
        Piece::Key(k) | Piece::Mouse(k) => cap_w(c, k),
        Piece::Text(s, f, _) => c.width(s, *f),
        Piece::Icon(i, _) => c.width(i, F::Icon),
    }
}

/// The bar's box in dip: centered, 12 above the screen's bottom.
fn bar_box(c: &Canvas, sc: &Screen, bar: Bar) -> Bx {
    let (items, gap) = bar_items(sc, bar, &c.t);
    let inner: f32 = items.iter().map(|(ps, _)| ps.iter().map(|p| piece_w(c, p)).sum::<f32>() + gap * (ps.len() as f32 - 1.0)).sum();
    let w = 2.0 * BAR_PAD + inner + BAR_GAP * (items.len() as f32 - 1.0);
    let (sw, sh) = sc.size();
    let (sw, sh) = (sw as f32 / sc.scale, sh as f32 / sc.scale);
    ((sw - w) / 2.0, sh - BAR_UP - BAR_H, w, BAR_H)
}

fn draw_bar(c: &Canvas, sc: &Screen, bar: Bar, at: Bx) {
    let (b, s) = (c.t.b, c.s);
    c.fill_rr(at, 8.0, op(b.card));
    c.stroke_rr(grow_bx(at, 0.5 / s), 8.5, 1.0 / s, ring_color(&c.t));
    let (items, gap) = bar_items(sc, bar, &c.t);
    let ky = at.1 + (BAR_H - 20.0) / 2.0;
    let mut x = at.0 + BAR_PAD;
    for (pieces, grey) in items {
        let fade = |color: u32| if grey { mix(b.card, color, 0.45) } else { color };
        for (k, p) in pieces.iter().enumerate() {
            if k > 0 {
                x += gap;
            }
            let w = piece_w(c, p);
            match p {
                Piece::Key(key) => {
                    c.fill_rr((x, ky + 1.0, w, 20.0), 4.0, op(fade(b.key_edge))); // its 1 px shadow below
                    c.boxed((x, ky, w, 20.0), 4.0, fade(b.key), Some(fade(b.key_edge)));
                    c.text(x + (w - c.width(key, F::Small)) / 2.0, ky, 20.0, key, F::Small, fade(b.tx));
                }
                Piece::Mouse(key) => {
                    c.stroke_dash(grow_bx((x, ky, w, 20.0), -0.5 / s), 4.0, 1.0 / s, op(fade(b.key_edge)), gp::DashStyleDash);
                    c.text(x + (w - c.width(key, F::Small)) / 2.0, ky, 20.0, key, F::Small, fade(b.tx));
                }
                Piece::Text(text, f, color) => {
                    let lh = if matches!(f, F::Small) { 16.0 } else { 20.0 };
                    c.text(x, at.1 + (BAR_H - lh) / 2.0, lh, text, *f, fade(*color));
                }
                Piece::Icon(icon, color) => c.text(x, at.1, BAR_H, icon, F::Icon, *color),
            }
            x += w;
        }
        x += BAR_GAP;
    }
}

/// Look B's 1 px ring around a floating thing: 10% black, 10% white in dark, the text
/// color in high contrast.
fn ring_color(t: &Theme) -> u32 {
    match () {
        _ if t.hc => op(t.b.tx),
        _ if t.dark => alpha(0xFFFFFF, 0.10),
        _ => alpha(0x000000, 0.10),
    }
}

/// A tile's workspace chip, centered under it: full in its color on the shown workspace, soft
/// on the others (AC-OVR-1).
fn chip(c: &Canvas, cx: f32, top: f32, ws: usize, full: bool) {
    let (b, hc) = (c.t.b, c.t.hc);
    let color = b.ws[ws.min(8)];
    let label = (ws + 1).to_string();
    let tw = c.width(&label, F::SmallBold);
    let w = (tw + 10.0).max(20.0);
    let at = (cx - w / 2.0, top, w, CHIP_H);
    let (fill, edge, ink) = match () {
        _ if full => (color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)), b.on_ws),
        _ if hc => (b.card, Some(b.tx), b.tx),
        _ => (mix(b.card, color, 0.4), Some(mix(b.card, color, 0.7)), b.tx),
    };
    c.boxed(at, 4.0, fill, edge);
    c.text(at.0 + (w - tw) / 2.0, top, CHIP_H, &label, F::SmallBold, ink);
}

/// A spread window's label: its pin and title, after its icon with labels "Always", centered
/// together and cut with … to fit.
fn label(c: &Canvas, sc: &Screen, sl: &Slot, always: bool) {
    let (b, s) = (c.t.b, c.s);
    let r = sc.label_rect(sl, always);
    let (x, y, w) = (r.left as f32 / s + LABEL_PAD, r.top as f32 / s, (r.right - r.left) as f32 / s - 2.0 * LABEL_PAD);
    let pin = "\u{E718}";
    let pin_w = c.width(pin, F::IconSmall);
    let lead = if always { ICON + ICON_GAP } else { 0.0 } + if sl.pinned { pin_w + ICON_GAP } else { 0.0 };
    let title = c.fit(&sl.title, F::Body, (w - lead).max(0.0));
    let mut cx = x + ((w - lead - c.width(&title, F::Body)) / 2.0).max(0.0);
    if always {
        c.icon(cx, y + (LABEL - ICON) / 2.0, ICON, sl.icon);
        cx += ICON + ICON_GAP;
    }
    if sl.pinned {
        c.text(cx, y, LABEL, pin, F::IconSmall, b.tx2);
        cx += pin_w + ICON_GAP;
    }
    c.text(cx, y, LABEL, &title, F::Body, b.tx);
}

/// Everything des-k paints on one screen, in dip. DWM draws the live previews over it.
fn draw(c: &Canvas, sc: &Screen, m: &Marks) {
    let (b, hc, s) = (c.t.b, c.t.hc, c.s);
    let d = |v: i32| v as f32 / s;
    let bx = |r: RECT| (d(r.left), d(r.top), d(r.right - r.left), d(r.bottom - r.top));
    let px1 = 1.0 / s;
    let (w, _) = sc.size();
    let band = sc.band_h();
    // The band and its rule.
    c.fill_rr((0.0, 0.0, d(w), d(band)), 0.0, op(b.card));
    c.rule(0.0, d(band - 1), d(w), op(b.line));
    for (i, t) in sc.tiles.iter().enumerate() {
        let r = bx(t.rect);
        let aimed = m.aim == Some(Aim::Tile(i));
        // One meaning per mark (AC-OVR-4): the shown workspace's ring in the text color, a drop
        // target's in purple, a thin edge where the mouse points.
        if aimed || i == sc.shown {
            let (color, width) = if aimed { (b.acc, if hc { 3.0 } else { RING }) } else { (b.tx, RING) };
            c.fill_rr(grow_bx(r, TILE_RING_OFF + width), 4.0 + TILE_RING_OFF + width, op(color));
            c.fill_rr(grow_bx(r, TILE_RING_OFF), 4.0 + TILE_RING_OFF, op(b.card));
        } else if m.tile_hover == Some(i) && hc {
            c.stroke_dash(grow_bx(r, px1 / 2.0), 4.0, px1, op(b.tx), gp::DashStyleDot);
        } else {
            c.fill_rr(grow_bx(r, px1), 4.0 + px1, op(if m.tile_hover == Some(i) { b.tx2 } else { b.line }));
        }
        match &sc.face {
            Face::Picture(p) => c.picture_rr(r, 4.0, p),
            Face::Color(color) => c.fill_rr(r, 4.0, op(*color)),
            Face::Card => c.fill_rr(r, 4.0, op(b.card)),
        }
        for mini in t.minis.iter().filter(|x| x.card) {
            let mr = bx(mini.rect);
            c.boxed(mr, 0.0, b.card, Some(b.line));
            if mr.2 >= MINI_ICON && mr.3 >= MINI_ICON {
                c.icon(mr.0 + (mr.2 - MINI_ICON) / 2.0, mr.1 + (mr.3 - MINI_ICON) / 2.0, MINI_ICON, mini.icon);
            }
        }
        chip(c, r.0 + r.2 / 2.0, r.1 + r.3 + CHIP_GAP, i, i == sc.shown);
    }
    for sl in &sc.slots {
        let r = bx(sl.rect);
        let src = m.src == Some(sl.hwnd);
        if sl.card {
            c.boxed(r, 0.0, b.card, Some(b.line));
            let top = r.1 + (r.3 - (CARD_ICON + 8.0 + 20.0)) / 2.0;
            c.icon(r.0 + (r.2 - CARD_ICON) / 2.0, top, CARD_ICON, sl.icon);
            let title = c.fit(&sl.title, F::Body, r.2 - 2.0 * CARD_PAD);
            c.text(r.0 + (r.2 - c.width(&title, F::Body)) / 2.0, top + CARD_ICON + 8.0, 20.0, &title, F::Body, b.tx);
            if src && !hc {
                c.fill_rr(r, 0.0, alpha(b.bg, 0.6));
            }
        } else {
            c.stroke_rr(grow_bx(r, px1 / 2.0), 0.0, px1, ring_color(&c.t));
        }
        if src && hc {
            c.stroke_dash(grow_bx(r, RING_OFF + RING / 2.0), 0.0, RING, op(b.tx), gp::DashStyleDash);
        }
        let (chosen, pointed) = (m.selected == Some(sl.hwnd), m.hover == Some(sl.hwnd));
        if chosen {
            c.stroke_rr(grow_bx(r, RING_OFF + RING / 2.0), 0.0, RING, op(b.acc));
        } else if pointed && hc {
            c.stroke_dash(grow_bx(r, RING_OFF + px1 / 2.0), 0.0, px1, op(b.tx), gp::DashStyleDot);
        } else if pointed {
            c.stroke_rr(grow_bx(r, RING_OFF + RING / 2.0), 0.0, RING, op(b.tx3));
        }
        match sc.labels {
            Labels::Always => label(c, sc, sl, true),
            Labels::Hover if chosen || pointed => label(c, sc, sl, false),
            _ => {}
        }
    }
    let area = bx(sc.spread_area());
    if sc.slots.is_empty() {
        let tw = c.width("No windows", F::Body);
        c.text(area.0 + (area.2 - tw) / 2.0, area.1 + (area.3 - 20.0) / 2.0, 20.0, "No windows", F::Body, b.tx2);
    }
    if m.aim == Some(Aim::Spread) {
        c.stroke_rr(grow_bx(area, SPREAD_AIM + RING / 2.0), 8.0 + RING / 2.0, RING, op(b.acc));
    }
    if let Some(bar) = m.bar {
        draw_bar(c, sc, bar, bar_box(&c.measuring(), sc, bar));
    }
}

/// One screen's painting into a buffer whose top left is popup pixel `ox`, `oy`: the page
/// and the shadows through GDI first, then the rest on a Canvas (AC-OVR-3).
fn paint_screen(mem: HDC, size: (i32, i32), sc: &Screen, m: &Marks, (ox, oy): (i32, i32), t: Theme) {
    let fonts = ui::fonts(sc.scale);
    unsafe {
        let bg = CreateSolidBrush(ui::cref(t.b.bg));
        FillRect(mem, &RECT { left: 0, top: 0, right: size.0, bottom: size.1 }, bg);
        let _ = DeleteObject(bg.into());
    }
    if let Some(a) = shadow_strength(&t) {
        let shift = |r: RECT| RECT { left: r.left - ox, top: r.top - oy, right: r.right - ox, bottom: r.bottom - oy };
        for sl in sc.slots.iter().filter(|x| !x.card) {
            shadow(mem, shift(sl.rect), sc.scale, a);
        }
        if let Some(bar) = m.bar {
            let (x, y, w, h) = bar_box(&Canvas::new(mem, false, sc.scale, t, &fonts), sc, bar);
            let p = |v: f32| (v * sc.scale).round() as i32;
            shadow(mem, shift(RECT { left: p(x), top: p(y), right: p(x + w), bottom: p(y + h) }), sc.scale, a);
        }
    }
    draw(&Canvas::new(mem, true, sc.scale, t, &fonts).shifted(ox, oy), sc, m);
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let mut rc = RECT::default();
    unsafe {
        let hdc = BeginPaint(hwnd, &mut ps);
        let _ = GetClientRect(hwnd, &mut rc);
        // Drawn off screen first, so a repaint never flickers. The buffer holds only the part
        // that needs painting (a hover ring, say), not the whole screen.
        let dirty = ps.rcPaint;
        let size = (dirty.right - dirty.left, dirty.bottom - dirty.top);
        let t = ui::theme();
        ui::buffered(hdc, dirty.left, dirty.top, size.0, size.1, |mem| {
            let drawn = SESSION.with_borrow(|s| {
                let Some(s) = s.as_ref() else { return false };
                let Some(i) = s.screens.iter().position(|sc| sc.popup == hwnd) else { return false };
                paint_screen(mem, size, &s.screens[i], &s.marks(i), (dirty.left, dirty.top), t);
                true
            });
            if !drawn {
                // A popup fading out keeps its picture, without the marks.
                LEAVING.with_borrow(|l| {
                    if let Some(sc) = l.iter().flat_map(|l| &l.screens).find(|sc| sc.popup == hwnd) {
                        paint_screen(mem, size, sc, &Marks::default(), (dirty.left, dirty.top), t);
                    }
                });
            }
        });
        let _ = EndPaint(hwnd, &ps);
        if ps.rcPaint == rc {
            first_paint(hwnd);
        }
    }
    tell(hwnd);
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
    let win = s.selected.1.and_then(|h| sc.slots.iter().find(|x| x.hwnd == h)).map_or_else(|| "No windows".to_string(), |x| window_words(x, &finder::program(x.hwnd), true));
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

unsafe extern "system" fn ghost_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // What a window Windows draws nothing for shows: a plain card.
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let card = CreateSolidBrush(ui::cref(ui::theme().b.card));
                FillRect(dc, &ps.rcPaint, card);
                let _ = DeleteObject(card.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::border(hwnd, ui::theme().b.tx);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{DARK, Fonts, LIGHT};
    use windows::core::Error;

    fn overlap(a: &RECT, b: &RECT) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    // AC-2: N tiles must all fit on the screen and still look like little copies of it, so a
    // window's place inside a tile can match its place on the screen.
    #[test]
    fn strip_tiles_keep_the_screen_aspect_and_fit_between_the_margins() {
        let tiles = strip(1920, 1080, 4, 1.0);
        assert_eq!(tiles.len(), 4);
        assert_eq!(tiles[0], RECT { left: 474, top: 24, right: 705, bottom: 154 });
        // Nine tiles on a narrow screen: shrunk to fit, aspect kept, still centered.
        let tiles = strip(1080, 1920, 9, 1.0);
        assert!(tiles[0].left >= 48 && tiles[8].right <= 1080 - 48, "{:?}", (tiles[0], tiles[8]));
        let (w, h) = (tiles[0].right - tiles[0].left, tiles[0].bottom - tiles[0].top);
        assert!((w as f64 / h as f64 - 1080.0 / 1920.0).abs() < 0.02);
        assert!((tiles[0].left - (1080 - tiles[8].right)).abs() <= 1);
    }

    // AC-3: every window must be visible at once and none may hide another, or the spread
    // fails at what it is for; and a small window is never blown up past its real size.
    fn sized(w: i32, h: i32) -> RECT {
        RECT { left: 0, top: 0, right: w, bottom: h }
    }

    #[test]
    fn spread_never_overlaps_never_enlarges_and_stays_inside() {
        let area = RECT { left: 48, top: 200, right: 1872, bottom: 1032 };
        let reals = [sized(1920, 1080), sized(800, 600), sized(300, 200), sized(1200, 1000), sized(640, 480)];
        let (got, cell_w) = spread(area, &reals, 32, 22);
        assert_eq!(got.len(), reals.len());
        for (i, r) in got.iter().enumerate() {
            // The label band under each window stays inside too (AC-5), and a hover label as
            // wide as the cell never runs into the next one (AC-OVR-6).
            assert!(r.left >= area.left && r.right <= area.right && r.top >= area.top && r.bottom + 22 <= area.bottom, "{r:?}");
            assert!(r.right - r.left <= reals[i].right && r.bottom - r.top <= reals[i].bottom);
            assert!(r.right - r.left <= cell_w);
            let with_label = |r: &RECT| RECT { bottom: r.bottom + 22, ..*r };
            assert!(got.iter().skip(i + 1).all(|o| !overlap(&with_label(r), &with_label(o))), "{r:?}");
        }
        assert_eq!(got[2].right - got[2].left, 300); // small enough to show at real size
    }

    // Four screen sized windows on a wide screen read best as 2 by 2, not as a thin row of four.
    #[test]
    fn spread_picks_the_row_count_that_shows_windows_largest() {
        let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
        let (got, _) = spread(area, &[sized(1920, 1080); 4], 0, 0);
        assert_eq!(got[0].top, got[1].top);
        assert!(got[2].top > got[0].top);
        assert_eq!(got[0].right - got[0].left, 800);
        // One window alone gets the whole area, centered.
        let (one, _) = spread(area, &[sized(3200, 900)], 0, 0);
        assert_eq!(one, [RECT { left: 0, top: 225, right: 1600, bottom: 675 }]);
        assert!(spread(area, &[], 0, 0).0.is_empty());
    }

    // AC-3: you find a window in the spread by where you left it, so a window on the right of
    // the screen must land right of one on the left, whatever order the z order lists them in.
    #[test]
    fn spread_keeps_each_window_on_its_side_of_the_screen() {
        let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
        let at = |x: i32, y: i32| RECT { left: x, top: y, right: x + 800, bottom: y + 600 };
        let (got, _) = spread(area, &[at(1400, 0), at(0, 0), at(0, 700)], 0, 0);
        assert!(got[0].left > got[1].left && got[0].left > got[2].left, "{got:?}");
        // Four in a 2 by 2: the lower windows take the lower row.
        let (got, _) = spread(area, &[at(0, 700), at(1000, 700), at(1000, 0), at(0, 0)], 0, 0);
        assert!(got[0].top > got[3].top && got[1].top > got[2].top, "{got:?}");
        assert!(got[1].left > got[0].left && got[2].left > got[3].left, "{got:?}");
    }

    // AC-2: a tile is a small copy of its screen, so a window filling the right half of the
    // second screen fills the right half of the tile, not some place offset by the screen's position.
    #[test]
    fn a_mini_sits_where_its_window_sits_on_its_own_screen() {
        let screen = RECT { left: 1920, top: 0, right: 3840, bottom: 1080 };
        let tile = RECT { left: 100, top: 24, right: 292, bottom: 132 };
        let right_half = RECT { left: 2880, top: 0, right: 3840, bottom: 1080 };
        assert_eq!(place_in(right_half, screen, tile), RECT { left: 196, top: 24, right: 292, bottom: 132 });
    }

    // A maximized window hangs a few pixels past its screen; cut at the tile's edge, its preview
    // must show the matching part of the picture, or it would draw onto the next tile.
    #[test]
    fn a_cut_mini_shows_the_matching_part_of_the_picture() {
        let full = RECT { left: -10, top: 0, right: 90, bottom: 100 };
        let shown = intersect(full, RECT { left: 0, top: 0, right: 200, bottom: 200 }).unwrap();
        assert_eq!(shown, RECT { left: 0, top: 0, right: 90, bottom: 100 });
        assert_eq!(source_rect(full, shown, SIZE { cx: 200, cy: 200 }), RECT { left: 20, top: 0, right: 200, bottom: 200 });
        assert_eq!(intersect(full, RECT { left: 90, top: 0, right: 200, bottom: 200 }), None);
    }

    // AC-8: the arrows walk the spread the way it looks. Right from the top left window must
    // reach its neighbour in the same row, not a closer looking one in the row below, and an
    // arrow with nothing that way keeps the selection where it is.
    #[test]
    fn arrows_move_to_the_nearest_window_that_way() {
        let at = |x: i32, y: i32, w: i32, h: i32| RECT { left: x, top: y, right: x + w, bottom: y + h };
        // Two rows; from the top left one, the window below is closer as the crow flies than
        // its row mate on the right.
        let rects = [at(0, 0, 400, 300), at(600, 100, 400, 100), at(100, 400, 400, 300), at(700, 400, 300, 300)];
        assert_eq!(nearest(&rects, 0, (1, 0)), Some(1));
        assert_eq!(nearest(&rects, 0, (0, 1)), Some(2));
        assert_eq!(nearest(&rects, 1, (0, 1)), Some(3));
        assert_eq!(nearest(&rects, 3, (-1, 0)), Some(2));
        assert_eq!(nearest(&rects, 0, (-1, 0)), None);
        assert_eq!(nearest(&rects, 0, (0, -1)), None);
    }

    fn screen(w: i32, h: i32, scale: f32, shown: usize, workspaces: usize) -> Screen {
        let rect = RECT { left: 0, top: 0, right: w, bottom: h };
        let tiles = strip(w, h, workspaces, scale).into_iter().map(|rect| Tile { rect, minis: Vec::new() }).collect();
        Screen { device: "D".into(), popup: HWND::default(), rect, scale, shown, tiles, slots: Vec::new(), cell_w: 0, labels: Labels::Hover, face: Face::Card, close_btn: HWND::default(), note: None, painted: false, tree: Tree::default() }
    }

    // AC-7: a drop lands where you let go: on a tile, that workspace; anywhere in the spread,
    // the one the screen shows; on the margins nothing, so a missed drop never moves a window.
    #[test]
    fn a_drop_lands_on_the_tile_or_the_spread_under_the_mouse() {
        let sc = screen(1920, 1080, 1.0, 2, 4);
        let mid = |r: RECT| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
        assert_eq!(sc.drop_target(mid(sc.tiles[0].rect)), Some(0));
        assert_eq!(sc.drop_target(mid(sc.tiles[3].rect)), Some(3));
        assert_eq!(sc.drop_target(POINT { x: 960, y: 700 }), Some(2), "the spread means the shown workspace");
        assert_eq!(sc.drop_target(POINT { x: 10, y: 700 }), None, "a side margin is neither");
        assert_eq!(sc.drop_target(POINT { x: 10, y: 10 }), None);
        assert_eq!(sc.drop_target(POINT { x: 960, y: 1080 - 30 }), None, "the bar's room is not the spread");
    }

    // AC-OVR-2: the spread starts 24 dip under the band (tiles, chips, 16 under them) and ends
    // 64 dip above the bottom, where the bar sits, at every scale; a window there never runs
    // under the band or the bar.
    #[test]
    fn the_spread_leaves_room_for_the_band_and_the_bar() {
        for scale in [1.0, 1.5] {
            let (w, h) = ((1280.0 * scale) as i32, (720.0 * scale) as i32);
            let sc = screen(w, h, scale, 1, 4);
            let px = |v: f32| (v * scale).round() as i32;
            let tile = sc.tiles[0].rect;
            assert_eq!(sc.band_h(), tile.bottom + px(8.0 + 18.0 + 16.0));
            let area = sc.spread_area();
            assert_eq!(area.top, sc.band_h() + px(24.0));
            assert_eq!(area.bottom, h - px(64.0));
            assert_eq!((area.left, area.right), (px(48.0), w - px(48.0)));
            // The bar's box sits inside those 64 dip.
            let fonts = Fonts::new(scale);
            let dc = unsafe { CreateCompatibleDC(None) };
            let c = Canvas::new(dc, false, scale, LIGHT, &fonts);
            let (_, y, _, bh) = bar_box(&c, &sc, Bar::Keys);
            assert!(y * scale >= area.bottom as f32 && (y + bh) * scale <= h as f32, "{y} {bh}");
            unsafe { let _ = DeleteDC(dc); }
        }
    }

    // AC-OVR-8: "admin" is a claim: say it only when Windows refused for that reason, else a
    // note blames the wrong thing.
    #[test]
    fn only_access_denied_says_admin() {
        assert_eq!(close_note(&Error::from_hresult(ERROR_ACCESS_DENIED.to_hresult())), Note::CantCloseAdmin);
        assert_eq!(close_note(&Error::from_hresult(windows::Win32::Foundation::E_FAIL)), Note::CantClose);
        assert_eq!(Note::CantCloseAdmin.words().1, "Can't close an admin window");
        assert_eq!(Note::CantClose.words().1, "Can't close this window");
        // The come back note is information, not a warning: nothing failed.
        assert!(!Note::DidntClose.words().3 && Note::CantMove.words().3);
    }

    // AC-8: every open says how long it took, and one over the limit must be easy to find.
    #[test]
    fn the_speed_line_flags_an_open_over_the_limit() {
        assert_eq!(speed_line(3, 100), "overview: shown on 3 screens in 100 ms");
        assert_eq!(speed_line(3, 101), "overview: shown on 3 screens in 101 ms (over the 100 ms limit)");
    }

    // AC-OVR-3: the shadow must fade to nothing at its edge (no hard line where the picture
    // ends) and reach full strength under the window, getting lighter the farther out.
    #[test]
    fn the_shadow_fades_out_and_is_full_under_the_box() {
        let (e, sigma) = (18, 6.0);
        let a = shadow_alphas(e, sigma, 0.5);
        let n = (4 * e + 1) as usize;
        let at = |x: usize, y: usize| a[y * n + x];
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(0, n / 2), 0, "the outer edge");
        assert!((126..=128).contains(&at(n / 2, n / 2)), "the middle, under the box: {}", at(n / 2, n / 2));
        let row: Vec<u8> = (0..n / 2).map(|x| at(x, n / 2)).collect();
        assert!(row.windows(2).all(|p| p[0] <= p[1]), "{row:?}");
        // Symmetric, so stretching the middle never shows a seam.
        assert_eq!(at(3, n / 2), at(n - 4, n / 2));
    }

    // AC-OVR-7: the keys only on the screen the keys act on, a note on its own screen, and
    // "Drop on a workspace" while dragging.
    #[test]
    fn the_bar_shows_on_the_selected_screen_and_a_note_on_its_own() {
        let mut s = Session {
            screens: vec![screen(1280, 720, 1.0, 0, 4), screen(1280, 720, 1.0, 0, 4)],
            came_from: HWND::default(),
            hover: None,
            tile_hover: None,
            selected: (1, None),
            closing: Vec::new(),
            drag: None,
            seen: Vec::new(),
            fading_in: None,
            alpha: 255,
            asked: 0,
            logged: false,
        };
        assert_eq!((s.bar(0), s.bar(1)), (None, Some(Bar::Keys)));
        s.screens[0].note = Some(Note::CantMove);
        assert_eq!((s.bar(0), s.bar(1)), (Some(Bar::Note(Note::CantMove)), Some(Bar::Keys)));
        s.drag = Some(Drag { hwnd: HWND::default(), from: ("D".into(), 0), start: POINT::default(), scale: 1.0, stage: Stage::Cancelled, aim: None });
        assert_eq!(s.bar(1), Some(Bar::Keys), "a press that is not moving says nothing new");
    }

    // AC-OVR-7: Enter and Ctrl+W need a window in the spread, Drag a window anywhere on the
    // screen; greying them tells you before you try.
    #[test]
    fn the_bar_greys_what_cannot_act() {
        let mut sc = screen(1280, 720, 1.0, 0, 4);
        let grey = |sc: &Screen| bar_items(sc, Bar::Keys, &LIGHT).0.iter().map(|(_, g)| *g).collect::<Vec<_>>();
        assert_eq!(grey(&sc), [true, true, false, true, false]);
        sc.tiles[2].minis.push(Slot { hwnd: HWND::default(), rect: RECT::default(), title: String::new(), icon: HICON::default(), pinned: false, card: false, thumb: None });
        assert_eq!(grey(&sc), [true, false, false, true, false], "a hidden window can still be dragged");
    }

    // The fade must end exactly at full strength, or the overview would stay a little see
    // through for as long as it is open; and exactly at nothing on close (AC-11).
    #[test]
    fn a_fade_eases_out_and_ends_exactly_at_full_strength() {
        assert_eq!((ease(0.0), ease(1.0)), (0.0, 1.0));
        assert!(ease(0.5) > 0.5, "ease out covers most of the way early, then settles");
    }

    // AC-8: with no window you came from on that screen, the selection starts where you start
    // reading, the top left window, whatever order the z order listed them in.
    #[test]
    fn the_first_window_is_the_top_left_one() {
        let at = |x: i32, y: i32| RECT { left: x, top: y, right: x + 100, bottom: y + 100 };
        assert_eq!(first(&[at(500, 400), at(500, 0), at(0, 400), at(0, 0)]), Some(3));
        assert_eq!(first(&[]), None);
    }

    // AC-8: Tab reads one screen like a page and wraps there, never on to another screen,
    // since mixing every screen's windows into one cycle was what you rejected.
    #[test]
    fn tab_reads_one_screen_like_a_page_and_wraps_around_on_it() {
        // Row one centers at y 50 and 49 (a pixel of rounding), so the right one must
        // still come after the left one, not first; then row two.
        let r = |x: i32, top: i32, h: i32| RECT { left: x, top, right: x + 100, bottom: top + h };
        let rects = [r(0, 300, 100), r(200, 0, 99), r(0, 0, 100)];
        assert_eq!(tab_step(&rects, None, 1), Some(2));
        assert_eq!(tab_step(&rects, Some(2), 1), Some(1));
        assert_eq!(tab_step(&rects, Some(1), 1), Some(0));
        assert_eq!(tab_step(&rects, Some(0), 1), Some(2), "Tab wraps to the top of this screen");
        assert_eq!(tab_step(&rects, Some(2), -1), Some(0), "Shift+Tab wraps to the bottom");
        assert_eq!(tab_step(&[], None, 1), None, "an empty screen has no stop");
    }

    // AC-10: live rebuilds only when the list really changed, since a rebuild registers every
    // preview again and would blink four times a second; not live drops just what went and
    // never reshuffles the rest, which is the whole point of turning live off.
    #[test]
    fn a_refresh_rebuilds_only_on_a_change_and_not_live_only_drops() {
        let h = |n: usize| HWND(std::ptr::without_provenance_mut(n));
        let w = |n: usize, ws: usize, on: bool| (h(n), "D".to_string(), ws, on);
        let before = [w(1, 0, true), w(2, 1, false), w(3, 0, true)];
        assert_eq!(refresh_plan(true, &before, &before), Refresh::Nothing);
        let moved = [w(1, 0, true), w(2, 2, false), w(3, 0, true)];
        assert_eq!(refresh_plan(true, &before, &moved), Refresh::Rebuild);
        let opened = [w(4, 0, true), w(1, 0, true), w(2, 1, false), w(3, 0, true)];
        assert_eq!(refresh_plan(true, &before, &opened), Refresh::Rebuild);
        assert_eq!(refresh_plan(false, &before, &opened), Refresh::Nothing, "new windows wait for the next open");
        assert_eq!(refresh_plan(false, &before, &moved), Refresh::Nothing, "the rest stay put");
        let went = [w(4, 0, true), w(3, 0, true)];
        assert_eq!(refresh_plan(false, &before, &went), Refresh::Drop(vec![h(1), h(2)]));
    }

    // The design's Built-in display: 1280 by 720 dip, its windows and where they sit (dip).
    fn design_windows() -> Vec<(&'static str, usize, RECT, bool)> {
        let r = |l: i32, t: i32, rr: i32, b: i32| RECT { left: l, top: t, right: rr, bottom: b };
        vec![
            ("Administrator: PowerShell", 0, r(0, 0, 1280, 672), false),
            ("Settings", 0, r(180, 70, 900, 610), false),
            ("des-k issues · GitHub - Google Chrome", 1, r(0, 0, 760, 672), false),
            ("overview.rs - des-k - Visual Studio Code", 1, r(600, 0, 1280, 672), false),
            ("Downloads - File Explorer", 1, r(120, 300, 660, 640), false),
            ("notes.txt - Notepad", 1, r(880, 360, 1240, 660), false),
            ("Spotify Premium", 1, r(240, 80, 1060, 600), false),
            ("Inbox - Outlook", 2, r(80, 30, 1200, 660), false),
        ]
    }

    /// A screen laid out as `build` would, from stand in windows: (title, workspace, real
    /// rect in dip, pinned). `cards` draw nothing for Windows.
    fn laid_out(scale: f32, shown: usize, workspaces: usize, wins: &[(&str, usize, RECT, bool)], cards: &[&str], labels: Labels) -> Screen {
        let (w, h) = ((1280.0 * scale) as i32, (720.0 * scale) as i32);
        let mut sc = screen(w, h, scale, shown, workspaces);
        let px = |r: RECT| RECT { left: (r.left as f32 * scale) as i32, top: (r.top as f32 * scale) as i32, right: (r.right as f32 * scale) as i32, bottom: (r.bottom as f32 * scale) as i32 };
        let hwnd = |i: usize| HWND(std::ptr::without_provenance_mut(i + 10));
        for (i, (_, ws, r, pinned)) in wins.iter().enumerate() {
            for (k, t) in sc.tiles.iter_mut().enumerate() {
                if (*pinned && k == shown) || (!pinned && *ws == k) {
                    let full = place_in(px(*r), sc.rect, t.rect);
                    if let Some(rect) = intersect(full, t.rect) {
                        t.minis.push(Slot { hwnd: hwnd(i), rect, title: String::new(), icon: HICON::default(), pinned: false, card: cards.contains(&wins[i].0), thumb: None });
                    }
                }
            }
        }
        let mine: Vec<usize> = (0..wins.len()).filter(|&i| wins[i].1 == shown || wins[i].3).collect();
        let band = if labels == Labels::None { 0 } else { sc.px(LABEL) };
        let (rects, cell_w) = spread(sc.spread_area(), &mine.iter().map(|&i| px(wins[i].2)).collect::<Vec<_>>(), sc.px(SLOT_GAP), band);
        sc.cell_w = cell_w;
        sc.labels = labels;
        sc.slots = mine.iter().zip(rects).map(|(&i, rect)| Slot { hwnd: hwnd(i), rect, title: wins[i].0.into(), icon: HICON::default(), pinned: wins[i].3, card: cards.contains(&wins[i].0), thumb: None }).collect();
        sc
    }

    /// `paint_screen` into a fresh w by h buffer whose top left is popup pixel `at`; its pixels.
    fn pixels(sc: &Screen, m: &Marks, t: Theme, (w, h): (i32, i32), at: (i32, i32)) -> Vec<u32> {
        unsafe {
            let mem = CreateCompatibleDC(None);
            let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(mem, bmp.into());
            paint_screen(mem, (w, h), sc, m, at, t);
            let _ = GdiFlush();
            let out = std::slice::from_raw_parts(bits.cast::<u32>(), (w * h) as usize).to_vec();
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            out
        }
    }

    // AC-OVR-10: hover, selection and drags repaint only the rectangle that changed, into a
    // buffer that size. That is only safe if a part painted alone matches the same part of a
    // whole paint, pixel for pixel: shapes, text, icons and shadows all shifted alike.
    #[test]
    fn a_partial_repaint_matches_the_whole_paint() {
        ui::gdiplus();
        let code = HWND(std::ptr::without_provenance_mut(13));
        let web = HWND(std::ptr::without_provenance_mut(12));
        let sc = laid_out(1.5, 1, 4, &design_windows(), &[], Labels::Hover);
        let m = Marks { selected: Some(code), hover: Some(web), tile_hover: Some(2), bar: Some(Bar::Keys), ..Marks::default() };
        let (w, h) = sc.size();
        let whole = pixels(&sc, &m, LIGHT, (w, h), (0, 0));
        // A hover mark, a tile's ring and the bar: text, shapes and a shadow.
        for part in [sc.mark_rect(web).unwrap(), sc.tile_ring(2).unwrap(), sc.bar_strip()] {
            let (pw, ph) = (part.right - part.left, part.bottom - part.top);
            let alone = pixels(&sc, &m, LIGHT, (pw, ph), (part.left, part.top));
            for y in 0..ph {
                for x in 0..pw {
                    let (a, b) = (alone[(y * pw + x) as usize], whole[((part.top + y) * w + part.left + x) as usize]);
                    assert_eq!(a & 0xFFFFFF, b & 0xFFFFFF, "{part:?} at {x}, {y}");
                }
            }
        }
    }

    /// Draws the states the design shows to target/overview/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/ovr3-* (spec 0017 AC-1). DWM draws the live previews and
    /// the wallpaper cannot be read here, so both are flat stand ins.
    #[test]
    #[ignore]
    fn render() {
        ui::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("overview");
        std::fs::create_dir_all(&dir).unwrap();
        let code = HWND(std::ptr::without_provenance_mut(13));
        let web = HWND(std::ptr::without_provenance_mut(12));
        let note = HWND(std::ptr::without_provenance_mut(15));
        let mut many = design_windows();
        for (i, t) in ["Pull requests · des-k - Google Chrome", "des-k - File Explorer", "todo.txt - Notepad", "Calendar", "Photos"].iter().enumerate() {
            many.push((t, 1, RECT { left: 60 * i as i32, top: 40, right: 60 * i as i32 + 700, bottom: 560 }, false));
        }
        let mut pinned = design_windows();
        pinned.push(("Chat | Microsoft Teams", 0, RECT { left: 700, top: 120, right: 1240, bottom: 600 }, true));
        pinned.push(("Calculator", 1, RECT { left: 960, top: 60, right: 1280, bottom: 560 }, false));
        let one = vec![design_windows()[3]];
        let three = design_windows().into_iter().filter(|w| w.1 == 1).take(3).collect::<Vec<_>>();
        type Set = (Vec<(&'static str, usize, RECT, bool)>, usize, usize, Labels, &'static [&'static str]);
        let states: Vec<(&str, Set, Marks)> = vec![
            ("opened", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("hover", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), hover: Some(web), bar: Some(Bar::Keys), ..Marks::default() }),
            ("always", (design_windows(), 1, 4, Labels::Always, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("none", (design_windows(), 1, 4, Labels::None, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("tile-hover", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), tile_hover: Some(2), bar: Some(Bar::Keys), ..Marks::default() }),
            ("empty", (design_windows(), 3, 4, Labels::Hover, &[]), Marks { bar: Some(Bar::Keys), ..Marks::default() }),
            ("nine", (design_windows(), 1, 9, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("pinned-card", (pinned, 1, 4, Labels::Always, &["Calculator"]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("one", (one, 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("three", (three, 1, 4, Labels::Hover, &[]), Marks { selected: Some(web), bar: Some(Bar::Keys), ..Marks::default() }),
            ("twelve", (many, 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
            ("drag-tile", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { src: Some(note), aim: Some(Aim::Tile(2)), bar: Some(Bar::Drag), ..Marks::default() }),
            ("drag-spread", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { aim: Some(Aim::Spread), bar: Some(Bar::Drag), ..Marks::default() }),
            ("cant-move", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantMove)), ..Marks::default() }),
            ("cant-close-admin", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantCloseAdmin)), ..Marks::default() }),
            ("cant-close", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantClose)), ..Marks::default() }),
            ("didnt-close", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(note), bar: Some(Bar::Note(Note::DidntClose)), ..Marks::default() }),
        ];
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let themes: [(&str, Theme, f32, u32); 5] = [("light", LIGHT, 1.0, 0xCFD9EE), ("dark", DARK, 1.0, 0x1A2030), ("hc", aquatic, 1.0, 0x000000), ("desert", desert, 1.0, 0xFFFAEF), ("light-150", LIGHT, 1.5, 0xCFD9EE)];
        for (tname, t, scale, wall) in themes {
            for (name, (wins, shown, n, labels, cards), m) in &states {
                let mut sc = laid_out(scale, *shown, *n, wins, cards, *labels);
                sc.face = Face::Color(wall);
                let (w, h) = sc.size();
                let path = dir.join(format!("{name}-{tname}.png"));
                ui::tests::save_png(w, h, &path, |dc| {
                    paint_screen(dc, (w, h), &sc, m, (0, 0), t);
                    // What DWM would draw: each live preview, as a flat stand in.
                    let fonts = Fonts::new(scale);
                    let c = Canvas::new(dc, true, scale, t, &fonts);
                    let stand = if t.hc { 0x000000 } else if t.dark { 0x3A3F4A } else { 0xFFFFFF };
                    for sl in sc.slots.iter().chain(sc.tiles.iter().flat_map(|t| &t.minis)).filter(|x| !x.card) {
                        let r = sl.rect;
                        let faded = m.src == Some(sl.hwnd) && !t.hc;
                        let color = if faded { mix(t.b.bg, stand, 0.4) } else { stand };
                        c.fill_rr((r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale), 0.0, op(color));
                        c.fill_rr((r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, ((r.bottom - r.top) as f32 / scale).min(12.0)), 0.0, op(mix(color, 0x808080, 0.25)));
                    }
                    // The hover X, its own popup on screen.
                    if let Some(at) = m.hover.and_then(|h| sc.close_spot(h)) {
                        draw_x(&c, at.x as f32 / scale, at.y as f32 / scale, false);
                    }
                });
            }
        }
    }

}
