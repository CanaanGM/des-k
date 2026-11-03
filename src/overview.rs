// Workspace overview (spec 0010): Win+Tab opens a popup on every screen at once, with a
// strip of that screen's workspaces along the top and the windows of the workspace it shows
// spread out below. Windows draws every preview (DWM thumbnails); des-k paints only the
// background, the tiles and the text. Lives on the main thread; where a window lives only
// ever changes through `ws`, and the overview never cloaks anything itself.
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties},
        Gdi::*,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, PCWSTR, w};

use crate::finder::{self, BG, DIM, TEXT};
use crate::indicator::ACCENT;
use crate::settings::{self, Animation, Labels};
use crate::ui::{self, text};
use crate::ws::{self, Miss};

const CLASS: PCWSTR = w!("des-k-overview");
const CLOSE_CLASS: PCWSTR = w!("des-k-overview-close");
const GHOST_CLASS: PCWSTR = w!("des-k-overview-ghost");
const TILE_BG: COLORREF = COLORREF(0x0034_3434); // the finder's line grey
const CLOSING_TIMER: usize = 1;
const CLOSING_POLL_MS: u32 = 200; // as the finder's
const CLOSE_WAIT: Duration = Duration::from_secs(2); // still open after this (it asked to save, say): shown again
const NOTE_TIMER: usize = 2;
const NOTE_MS: u32 = 2000;
const REFRESH_TIMER: usize = 3;
const REFRESH_MS: u32 = 250;
const ANIM_TIMER: usize = 4;
const FRAME_MS: u32 = 10; // Windows rounds it up to its own tick, about 16 ms
const FADE_MS: u32 = 250;
const KEY_W: VIRTUAL_KEY = VIRTUAL_KEY(b'W' as u16);

// Sizes at 96 dpi, from the spec's layout.
const TILE_TOP: f32 = 24.0;
const TILE_GAP: f32 = 16.0;
const TILE_HEIGHT: f32 = 0.12; // of the screen's height
const MARGIN: f32 = 48.0;
const STRIP_GAP: f32 = 32.0; // between the strip and the spread
const SLOT_GAP: f32 = 32.0;
const OUTLINE: f32 = 3.0;
const FONT_PX: f32 = 16.0;
const LABEL: f32 = 22.0; // the band under each spread window
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 6.0;
const CARD_ICON: f32 = 32.0;
const CARD_PAD: f32 = 8.0;
const CLOSE_BTN: f32 = 24.0;
const CLOSE_INSET: f32 = 6.0; // from the hovered window's top right corner
const GHOST_W: f32 = 240.0;

/// A live preview drawn by DWM into a popup; unregistered when dropped.
struct Thumb(isize);

impl Drop for Thumb {
    fn drop(&mut self) {
        unsafe { let _ = DwmUnregisterThumbnail(self.0); }
    }
}

impl Thumb {
    /// A fade frame: the preview made see through.
    fn set_opacity(&self, opacity: u8) {
        let p = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_OPACITY, opacity, ..Default::default() };
        unsafe { let _ = DwmUpdateThumbnailProperties(self.0, &p); }
    }
}

/// A window drawn into a popup: in the spread, or a mini inside a tile. No thumbnail means
/// Windows draws nothing for it, so it shows as a card (AC-4).
struct Slot {
    hwnd: HWND,
    rect: RECT, // in popup coordinates
    title: String, // empty for minis: tiles never carry titles
    icon: HICON, // borrowed, never destroyed; only fetched when drawn
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
    font: HFONT,
    shown: usize, // 0 based
    tiles: Vec<Tile>,
    slots: Vec<Slot>,
    close_btn: HWND, // the hover X, hidden until a spread window is hovered
    note: Option<&'static str>,
}

impl Screen {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn size(&self) -> (i32, i32) {
        (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top)
    }

    fn spread_area(&self) -> RECT {
        let (w, h) = self.size();
        let top = self.tiles.first().map_or(0, |t| t.rect.bottom) + self.px(STRIP_GAP);
        RECT { left: self.px(MARGIN), top, right: w - self.px(MARGIN), bottom: h - self.px(MARGIN) }
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
            // DWM draws the thumbnail registered last on top, so register back to front.
            let minis = mine.iter().rev().filter(|p| p.workspace == i).filter_map(|p| self.mini(rect, p.hwnd)).collect();
            self.tiles.push(Tile { rect, minis });
        }
        let labels = settings::get().overview_labels;
        let band = if labels == Labels::None { 0 } else { self.px(LABEL) };
        let shown: Vec<HWND> = mine.iter().filter(|p| p.on_screen).map(|p| p.hwnd).collect();
        let reals: Vec<RECT> = shown.iter().map(|&h| ws::real_rect(h)).collect();
        let rects = spread(self.spread_area(), &reals, self.px(SLOT_GAP), band);
        self.slots = shown
            .into_iter()
            .zip(rects)
            .map(|(hwnd, rect)| {
                let thumb = thumb(self.popup, hwnd, rect, rect);
                let icon = if thumb.is_none() || labels == Labels::Always { finder::icon(hwnd) } else { HICON::default() };
                Slot { hwnd, rect, title: ws::title(hwnd), icon, thumb }
            })
            .collect();
    }

    /// A window inside `tile` at its real place on this screen, cut at the tile's edge so it
    /// never draws onto the next tile. None when none of it falls inside.
    fn mini(&self, tile: RECT, hwnd: HWND) -> Option<Slot> {
        let full = place_in(ws::real_rect(hwnd), self.rect, tile);
        let rect = intersect(full, tile)?;
        let thumb = thumb(self.popup, hwnd, full, rect);
        let icon = if thumb.is_none() { finder::icon(hwnd) } else { HICON::default() };
        Some(Slot { hwnd, rect, title: String::new(), icon, thumb })
    }

    fn hit(&self, pt: POINT) -> Hit {
        let inside = |r: &RECT| pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
        if let Some(s) = self.slots.iter().find(|s| inside(&s.rect)) {
            return Hit::Slot(s.hwnd);
        }
        let Some(i) = self.tiles.iter().position(|t| inside(&t.rect)) else { return Hit::Background };
        // Front to back, so the window on top takes the click.
        match self.tiles[i].minis.iter().rev().find(|m| inside(&m.rect)) {
            Some(m) => Hit::Mini(i, m.hwnd),
            None => Hit::Tile(i),
        }
    }

    /// The workspace a drop at `pt` lands on: a tile's, else the shown one when it is in the
    /// spread. None anywhere else, which cancels the drop.
    fn drop_target(&self, pt: POINT) -> Option<usize> {
        let inside = |r: &RECT| pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
        self.tiles.iter().position(|t| inside(&t.rect)).or_else(|| inside(&self.spread_area()).then_some(self.shown))
    }

    /// The top left spread window, where a selection starts.
    fn first(&self) -> Option<HWND> {
        first(&self.rects()).map(|i| self.slots[i].hwnd)
    }

    fn rects(&self) -> Vec<RECT> {
        self.slots.iter().map(|s| s.rect).collect()
    }

    /// The band under a spread window, where its label goes.
    fn band(&self, s: &Slot) -> RECT {
        RECT { left: s.rect.left, top: s.rect.bottom, right: s.rect.right, bottom: s.rect.bottom + self.px(LABEL) }
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
}

impl Drop for Screen {
    fn drop(&mut self) {
        // Thumbnails before the popup they draw into.
        self.slots.clear();
        self.tiles.clear();
        unsafe {
            let _ = DestroyWindow(self.close_btn);
            let _ = DestroyWindow(self.popup);
            let _ = DeleteObject(self.font.into());
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
/// screen's overview. Taken down when dropped.
struct Ghost {
    hwnd: HWND,
    size: (i32, i32),
    thumb: Option<Thumb>, // none: a plain card
}

impl Drop for Ghost {
    fn drop(&mut self) {
        self.thumb = None; // before the popup it draws into
        unsafe { let _ = DestroyWindow(self.hwnd); }
    }
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
}

struct Session {
    screens: Vec<Screen>, // left to right, then top down
    came_from: HWND,
    hover: Option<(HWND, HWND)>, // (popup, window) of the spread window under the mouse
    selected: (usize, Option<HWND>), // (screen, spread window); no window when its spread is empty
    closing: Vec<(HWND, String, Instant)>, // sent WM_CLOSE and out of the layout, waiting to go; titles for the log
    drag: Option<Drag>,
    seen: Vec<Seen>, // the list the layout was built from, for the refresh
    fading_in: Option<Instant>, // when the fade in started, while it runs (AC-11)
    alpha: u8, // the popups' now, so a close during the fade in fades out from there
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
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static LEAVING: RefCell<Option<Leaving>> = const { RefCell::new(None) };
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
/// in the order of `reals`.
fn spread(area: RECT, reals: &[RECT], gap: i32, band: i32) -> Vec<RECT> {
    let n = reals.len() as i32;
    if n == 0 {
        return Vec::new();
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
    out
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

fn progress(start: Instant) -> f32 {
    ease((start.elapsed().as_secs_f32() * 1000.0 / FADE_MS as f32).min(1.0))
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
            // The class brush lives as long as des-k; it is what a card ghost shows.
            let card = CreateSolidBrush(TILE_BG);
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(ghost_wndproc), hInstance: inst.into(), lpszClassName: GHOST_CLASS, hCursor: cursor, hbrBackground: card, ..Default::default() });
            REGISTERED.set(true);
        }
        // Tool window: never in Alt+Tab or the taskbar, and never in ws lists. One of them
        // takes focus for the keys, so no WS_EX_NOACTIVATE. Layered only to animate, since
        // it shows nothing until its alpha is set.
        let ex = if layered { WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED } else { WS_EX_TOPMOST | WS_EX_TOOLWINDOW };
        let h = CreateWindowExW(ex, CLASS, w!("des-k overview"), WS_POPUP, r.left, r.top, r.right - r.left, r.bottom - r.top, None, None, Some(inst.into()), None).ok()?;
        if layered {
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA);
        }
        Some(h)
    }
}

/// The round hover X: its own popup, since a DWM preview always draws over its host's own
/// painting. Owned by the screen's popup, so it stays above it, and it never takes focus, so
/// the keys stay with the overview.
fn close_button(owner: HWND, scale: f32) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let d = (CLOSE_BTN * scale).round() as i32;
        let h = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, CLOSE_CLASS, w!("des-k overview close"), WS_POPUP, 0, 0, d, d, Some(owner), None, Some(inst.into()), None).ok()?;
        // Windows owns the region from here on.
        let _ = SetWindowRgn(h, Some(CreateEllipticRgn(0, 0, d + 1, d + 1)), false);
        Some(h)
    }
}

/// The drag ghost for `src`: 240 px wide at `scale`, the window's aspect, not shown yet.
fn ghost(src: HWND, scale: f32) -> Option<Ghost> {
    let real = ws::real_rect(src);
    let w = (GHOST_W * scale).round() as i32;
    let h = ((w as i64 * (real.bottom - real.top).max(1) as i64) / (real.right - real.left).max(1) as i64) as i32;
    let hwnd = unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, GHOST_CLASS, w!("des-k overview drag"), WS_POPUP, 0, 0, w, h, None, None, Some(inst.into()), None).ok()?
    };
    let full = RECT { left: 0, top: 0, right: w, bottom: h };
    Some(Ghost { hwnd, size: (w, h), thumb: thumb(hwnd, src, full, full) })
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
        let mut sc = Screen { device, popup, rect, scale, font: ui::font(w!("Segoe UI"), FONT_PX * scale, FW_SEMIBOLD.0 as i32), shown: 0, tiles: Vec::new(), slots: Vec::new(), close_btn, note: None };
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
    let fading_in = (style == Animation::Fade).then(Instant::now);
    let alpha = if fading_in.is_some() { 0 } else { 255 };
    SESSION.set(Some(Session { screens, came_from, hover: None, selected, closing: Vec::new(), drag: None, seen: seen(&places), fading_in, alpha }));
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
        // One timer for every screen; it goes with its popup on close.
        SetTimer(Some(focus), REFRESH_TIMER, REFRESH_MS, None);
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
            for id in [CLOSING_TIMER, NOTE_TIMER, REFRESH_TIMER, ANIM_TIMER] {
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

/// An arrow: the nearest window that way on the selected screen.
fn arrow(dir: (i32, i32)) {
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.get(s.selected.0)?;
        let rects = sc.rects();
        let to = match s.selected.1.and_then(|h| sc.slots.iter().position(|x| x.hwnd == h)) {
            Some(from) => nearest(&rects, from, dir)?,
            None => first(&rects)?,
        };
        s.selected.1 = Some(sc.slots[to].hwnd);
        Some(sc.popup)
    });
    invalidate(changed);
}

/// Tab, Shift+Tab: the next or previous window on the selected screen, around the end and
/// never on to another screen (the mouse picks the screen, see `follow_mouse`).
fn tab(by: isize) {
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.get(s.selected.0)?;
        let from = s.selected.1.and_then(|h| sc.slots.iter().position(|x| x.hwnd == h));
        s.selected.1 = Some(sc.slots[tab_step(&sc.rects(), from, by)?].hwnd);
        Some(sc.popup)
    });
    invalidate(changed);
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
        Err(Miss::Gone | Miss::CantMove) => {
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
                    s.closing.push((h, title, Instant::now()));
                }
            });
            // Some apps (Store ones like Settings) shrink what they draw while closing, and
            // the preview would stretch that to fill its slot.
            drop_gone(&[h]);
            unsafe { SetTimer(Some(popup), CLOSING_TIMER, CLOSING_POLL_MS, None) };
        }
        Err(e) => {
            eprintln!("overview: can't close \"{title}\": {e}");
            note(popup, "Can't close this window");
        }
    }
}

/// Windows asked to close that closed or hid themselves (apps that close to the tray) are done;
/// one still open after `CLOSE_WAIT` comes back into the layout.
fn closing_tick(popup: HWND) {
    let Some((gone, back, none_left)) = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let (gone, left): (Vec<_>, Vec<_>) = s.closing.drain(..).partition(|&(h, ..)| unsafe { !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() });
        let (back, left): (Vec<_>, Vec<_>) = left.into_iter().partition(|(.., at)| at.elapsed() >= CLOSE_WAIT);
        s.closing = left;
        Some((gone, back, s.closing.is_empty()))
    }) else {
        return;
    };
    for (_, title, _) in &gone {
        println!("overview: closed \"{title}\"");
    }
    for (_, title, _) in &back {
        println!("overview: \"{title}\" is still open, showing it again");
    }
    if !back.is_empty() {
        rebuild();
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

/// A short note at the bottom of one screen, gone after about two seconds.
fn note(popup: HWND, text: &'static str) {
    SESSION.with_borrow_mut(|s| {
        if let Some(sc) = s.iter_mut().flat_map(|s| s.screens.iter_mut()).find(|sc| sc.popup == popup) {
            sc.note = Some(text);
        }
    });
    unsafe { SetTimer(Some(popup), NOTE_TIMER, NOTE_MS, None) };
    invalidate([popup]);
}

fn note_gone(popup: HWND) {
    unsafe { let _ = KillTimer(Some(popup), NOTE_TIMER); }
    SESSION.with_borrow_mut(|s| {
        if let Some(sc) = s.iter_mut().flat_map(|s| s.screens.iter_mut()).find(|sc| sc.popup == popup) {
            sc.note = None;
        }
    });
    invalidate([popup]);
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
        s.drag = Some(Drag { hwnd, from: (sc.device.clone(), ws), start, scale: sc.scale, stage: Stage::Pressed });
        Some(())
    });
    if started.is_some() {
        unsafe { SetCapture(popup) };
    }
}

/// A move with a window pressed: past the drag distance the ghost appears, then follows the
/// mouse. False when nothing is pressed, so the move is an ordinary one.
fn drag_move() -> bool {
    let pt = cursor();
    let pressed = SESSION.with_borrow(|s| {
        let d = s.as_ref()?.drag.as_ref()?;
        let ghost = match &d.stage {
            Stage::Pressed => None,
            Stage::Moving(g) => Some((g.hwnd, g.size)),
            Stage::Cancelled => return Some(None),
        };
        Some(Some((d.hwnd, d.start, d.scale, ghost)))
    });
    let Some(pressed) = pressed else { return false };
    // Cancelled: nothing moves until the release.
    let Some((hwnd, start, scale, moving)) = pressed else { return true };
    let (ghost_hwnd, (w, h)) = match moving {
        Some(g) => g,
        None => {
            let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            if (pt.x - start.x).abs() <= cx && (pt.y - start.y).abs() <= cy {
                return true;
            }
            let Some(g) = ghost(hwnd, scale) else {
                eprintln!("overview: drag preview for \"{}\" not created", ws::title(hwnd));
                return true;
            };
            let made = (g.hwnd, g.size);
            // The X would sit on a window that is now moving.
            let old = SESSION.with_borrow_mut(|s| {
                let s = s.as_mut()?;
                s.drag.as_mut()?.stage = Stage::Moving(g);
                s.hover.take()
            });
            hover_changed(old);
            made
        }
    };
    unsafe { let _ = SetWindowPos(ghost_hwnd, Some(HWND_TOPMOST), pt.x - w / 2, pt.y - h / 2, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
    true
}

/// The release: a drop after a drag, a click otherwise, nothing after a cancel.
fn on_release(popup: HWND, pt: POINT) {
    let drag = SESSION.with_borrow_mut(|s| s.as_mut()?.drag.take());
    // After the take, so the capture change finds no drag to cancel.
    unsafe { let _ = ReleaseCapture(); }
    match drag {
        Some(Drag { stage: Stage::Cancelled, .. }) => {}
        Some(d @ Drag { stage: Stage::Moving(_), .. }) => drop_at(d),
        _ => on_click(popup, pt),
    }
}

/// Esc or a right click during a drag: the ghost goes, and the release does nothing. False
/// when no drag is under way, so Esc closes the overview as usual.
fn cancel_drag() -> bool {
    let old = SESSION.with_borrow_mut(|s| {
        let d = s.as_mut()?.drag.as_mut()?;
        matches!(d.stage, Stage::Moving(_)).then(|| std::mem::replace(&mut d.stage, Stage::Cancelled))
    });
    // The ghost goes here, outside the borrow.
    old.is_some()
}

/// Move the dragged window to the workspace under the mouse, on any screen. Its own place,
/// or anywhere that is neither a tile nor a spread, changes nothing.
fn drop_at(d: Drag) {
    let pt = cursor();
    let target = SESSION.with_borrow(|s| {
        s.as_ref()?.screens.iter().find_map(|sc| {
            let r = sc.rect;
            let on = pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
            on.then(|| (sc.popup, sc.device.clone(), sc.drop_target(POINT { x: pt.x - r.left, y: pt.y - r.top })))
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
        Err(Miss::CantMove) => note(popup, "Can't move this window"),
    }
}

/// The selection follows the mouse onto another screen, at that screen's first window, so
/// the keys always act on the screen you are looking at.
fn follow_mouse(popup: HWND) {
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let i = s.screens.iter().position(|sc| sc.popup == popup)?;
        if i == s.selected.0 {
            return None;
        }
        let old = s.screens.get(s.selected.0)?.popup;
        s.selected = (i, s.screens[i].first());
        Some([old, popup])
    });
    invalidate(changed.into_iter().flatten());
}

/// Track the spread window under the mouse, for its X and the `"hover"` labels.
fn on_move(popup: HWND, pt: POINT) {
    follow_mouse(popup);
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let new = match s.screens.iter().find(|sc| sc.popup == popup)?.hit(pt) {
            Hit::Slot(h) => Some((popup, h)),
            _ => None,
        };
        let old = std::mem::replace(&mut s.hover, new);
        (old != new).then_some(old)
    });
    if let Some(old) = changed {
        hover_changed(old);
    }
}

/// After `hover` changed from `old`: the X moves to the hovered window (or hides), and the
/// hover label redraws. Every screen, since the mouse may have crossed onto another one.
fn hover_changed(old: Option<(HWND, HWND)>) {
    let Some((buttons, bands)) = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let at = |sc: &Screen| s.hover.filter(|(p, _)| *p == sc.popup).and_then(|(_, h)| sc.close_spot(h));
        // Only the old and the new label change, so only their bands repaint: a whole popup
        // is a whole screen. A window no longer laid out repaints its whole popup.
        let band = |(p, h): (HWND, HWND)| {
            let sc = s.screens.iter().find(|sc| sc.popup == p);
            (p, sc.and_then(|sc| Some(sc.band(sc.slots.iter().find(|x| x.hwnd == h)?))))
        };
        Some((s.screens.iter().map(|sc| (sc.close_btn, at(sc))).collect::<Vec<_>>(), [old, s.hover].into_iter().flatten().map(band).collect::<Vec<_>>()))
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
    }
    if settings::get().overview_labels == Labels::Hover {
        for (p, r) in bands {
            unsafe { let _ = InvalidateRect(Some(p), r.as_ref().map(|r| r as *const RECT), false); }
        }
    }
}

/// The X's click closes the window it sits on.
fn on_close_click() {
    if let Some((popup, h)) = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.hover)) {
        close_window(popup, h);
    }
}

fn icon(dc: HDC, icon: HICON, x: i32, y: i32, size: i32) {
    unsafe { let _ = DrawIconEx(dc, x, y, icon, size, size, 0, None, DI_NORMAL); }
}

/// The band under a spread window: its title, after its icon when `with_icon`, centered
/// together and cut with … when too long.
fn label(dc: HDC, sc: &Screen, s: &Slot, with_icon: bool) {
    let mut r = sc.band(s);
    if with_icon {
        let (size, gap) = (sc.px(ICON), sc.px(ICON_GAP));
        let wide: Vec<u16> = s.title.encode_utf16().collect();
        let mut tw = SIZE::default();
        unsafe { let _ = GetTextExtentPoint32W(dc, &wide, &mut tw); }
        let left = r.left + ((r.right - r.left) - (size + gap + tw.cx)).max(0) / 2;
        icon(dc, s.icon, left, r.top + (r.bottom - r.top - size) / 2, size);
        r.left = left + size + gap;
        return text(dc, &s.title, r, TEXT, DT_LEFT);
    }
    text(dc, &s.title, r, TEXT, DT_CENTER);
}

/// A border `o` thick just outside `r`, leaving `r` itself alone.
fn frame(dc: HDC, r: RECT, o: i32, brush: HBRUSH) {
    for side in [
        RECT { left: r.left - o, top: r.top - o, right: r.right + o, bottom: r.top },
        RECT { left: r.left - o, top: r.bottom, right: r.right + o, bottom: r.bottom + o },
        RECT { left: r.left - o, top: r.top, right: r.left, bottom: r.bottom },
        RECT { left: r.right, top: r.top, right: r.right + o, bottom: r.bottom },
    ] {
        unsafe { FillRect(dc, &side, brush) };
    }
}

fn draw(dc: HDC, sc: &Screen, hover: Option<HWND>, selected: Option<HWND>) {
    let labels = settings::get().overview_labels;
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SelectObject(dc, sc.font.into());
        let (tile, accent, card) = (CreateSolidBrush(TILE_BG), CreateSolidBrush(ACCENT), CreateSolidBrush(BG));
        for (i, t) in sc.tiles.iter().enumerate() {
            let (r, o) = (t.rect, sc.px(OUTLINE));
            if i == sc.shown {
                FillRect(dc, &RECT { left: r.left - o, top: r.top - o, right: r.right + o, bottom: r.bottom + o }, accent);
            }
            FillRect(dc, &r, tile);
            // Under the tile, since its windows' previews would cover a number inside it.
            text(dc, &(i + 1).to_string(), RECT { top: r.bottom + o, bottom: r.bottom + sc.px(STRIP_GAP), ..r }, TEXT, DT_CENTER);
            // Cards only: DWM draws every preview over this painting anyway.
            let size = sc.px(ICON);
            for m in t.minis.iter().filter(|m| m.thumb.is_none()) {
                FillRect(dc, &m.rect, card);
                if m.rect.right - m.rect.left >= size && m.rect.bottom - m.rect.top >= size {
                    icon(dc, m.icon, (m.rect.left + m.rect.right - size) / 2, (m.rect.top + m.rect.bottom - size) / 2, size);
                }
            }
        }
        for s in &sc.slots {
            if selected == Some(s.hwnd) {
                frame(dc, s.rect, sc.px(OUTLINE), accent);
            }
            if s.thumb.is_none() {
                let (size, mid) = (sc.px(CARD_ICON), (s.rect.top + s.rect.bottom) / 2);
                FillRect(dc, &s.rect, tile);
                icon(dc, s.icon, (s.rect.left + s.rect.right - size) / 2, mid - size, size);
                let pad = sc.px(CARD_PAD);
                text(dc, &s.title, RECT { left: s.rect.left + pad, top: mid + pad, right: s.rect.right - pad, bottom: mid + pad + sc.px(LABEL) }, TEXT, DT_CENTER);
            }
            match labels {
                Labels::Always => label(dc, sc, s, true),
                Labels::Hover if hover == Some(s.hwnd) => label(dc, sc, s, false),
                _ => {}
            }
        }
        let _ = DeleteObject(tile.into());
        let _ = DeleteObject(accent.into());
        let _ = DeleteObject(card.into());
        if sc.slots.is_empty() {
            text(dc, "No windows", sc.spread_area(), DIM, DT_CENTER);
        }
        if let Some(note) = sc.note {
            let (w, h) = sc.size();
            text(dc, note, RECT { left: 0, top: h - sc.px(MARGIN), right: w, bottom: h }, TEXT, DT_CENTER);
        }
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // Drawn off screen first, like the finder, so a repaint never flickers. The buffer
        // holds only the part that needs painting (a hover label's band, say), not the whole
        // screen, and is shifted so the drawing keeps popup coordinates.
        let dirty = ps.rcPaint;
        ui::buffered(hdc, dirty.left, dirty.top, dirty.right - dirty.left, dirty.bottom - dirty.top, |mem| {
            let _ = SetViewportOrgEx(mem, -dirty.left, -dirty.top, None);
            let bg = CreateSolidBrush(BG);
            FillRect(mem, &rc, bg);
            let _ = DeleteObject(bg.into());
            let drawn = SESSION.with_borrow(|s| {
                let Some(s) = s.as_ref() else { return false };
                let Some(sc) = s.screens.iter().find(|sc| sc.popup == hwnd) else { return false };
                let mine = |at: Option<(HWND, HWND)>| at.filter(|(p, _)| *p == hwnd).map(|(_, h)| h);
                draw(mem, sc, mine(s.hover), mine(s.selection()));
                true
            });
            if !drawn {
                // A popup fading out keeps its picture, without the marks.
                LEAVING.with_borrow(|l| {
                    if let Some(sc) = l.iter().flat_map(|l| &l.screens).find(|sc| sc.popup == hwnd) {
                        draw(mem, sc, None, None);
                    }
                });
            }
        });
        let _ = EndPaint(hwnd, &ps);
    }
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
                drop(SESSION.with_borrow_mut(|s| s.as_mut()?.drag.take()));
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
            WM_DPICHANGED => LRESULT(0), // each popup is sized for its own screen at open
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn close_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                // The window's round region does the clipping.
                let bg = CreateSolidBrush(TILE_BG);
                FillRect(dc, &rc, bg);
                let _ = DeleteObject(bg.into());
                let (d, i) = (rc.right, rc.right / 3);
                let pen = CreatePen(PS_SOLID, (d / 12).max(1), TEXT);
                let old = SelectObject(dc, pen.into());
                let _ = MoveToEx(dc, i, i, None);
                let _ = LineTo(dc, d - i, d - i);
                let _ = MoveToEx(dc, d - i, i, None);
                let _ = LineTo(dc, i, d - i);
                SelectObject(dc, old);
                let _ = DeleteObject(pen.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
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
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let got = spread(area, &reals, 32, 22);
        assert_eq!(got.len(), reals.len());
        for (i, r) in got.iter().enumerate() {
            // The label band under each window stays inside too (AC-5).
            assert!(r.left >= area.left && r.right <= area.right && r.top >= area.top && r.bottom + 22 <= area.bottom, "{r:?}");
            assert!(r.right - r.left <= reals[i].right && r.bottom - r.top <= reals[i].bottom);
            let with_label = |r: &RECT| RECT { bottom: r.bottom + 22, ..*r };
            assert!(got.iter().skip(i + 1).all(|o| !overlap(&with_label(r), &with_label(o))), "{r:?}");
        }
        assert_eq!(got[2].right - got[2].left, 300); // small enough to show at real size
    }

    // Four screen sized windows on a wide screen read best as 2 by 2, not as a thin row of four.
    #[test]
    fn spread_picks_the_row_count_that_shows_windows_largest() {
        let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
        let got = spread(area, &[sized(1920, 1080); 4], 0, 0);
        assert_eq!(got[0].top, got[1].top);
        assert!(got[2].top > got[0].top);
        assert_eq!(got[0].right - got[0].left, 800);
        // One window alone gets the whole area, centered.
        let one = spread(area, &[sized(3200, 900)], 0, 0);
        assert_eq!(one, [RECT { left: 0, top: 225, right: 1600, bottom: 675 }]);
        assert!(spread(area, &[], 0, 0).is_empty());
    }

    // AC-3: you find a window in the spread by where you left it, so a window on the right of
    // the screen must land right of one on the left, whatever order the z order lists them in.
    #[test]
    fn spread_keeps_each_window_on_its_side_of_the_screen() {
        let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
        let at = |x: i32, y: i32| RECT { left: x, top: y, right: x + 800, bottom: y + 600 };
        let got = spread(area, &[at(1400, 0), at(0, 0), at(0, 700)], 0, 0);
        assert!(got[0].left > got[1].left && got[0].left > got[2].left, "{got:?}");
        // Four in a 2 by 2: the lower windows take the lower row.
        let got = spread(area, &[at(0, 700), at(1000, 700), at(1000, 0), at(0, 0)], 0, 0);
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

    // AC-7: a drop lands where you let go: on a tile, that workspace; anywhere in the spread,
    // the one the screen shows; on the margins nothing, so a missed drop never moves a window.
    #[test]
    fn a_drop_lands_on_the_tile_or_the_spread_under_the_mouse() {
        let rect = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let tiles = strip(1920, 1080, 4, 1.0).into_iter().map(|rect| Tile { rect, minis: Vec::new() }).collect();
        let sc = Screen { device: "D".into(), popup: HWND::default(), rect, scale: 1.0, font: HFONT::default(), shown: 2, tiles, slots: Vec::new(), close_btn: HWND::default(), note: None };
        let mid = |r: RECT| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
        assert_eq!(sc.drop_target(mid(sc.tiles[0].rect)), Some(0));
        assert_eq!(sc.drop_target(mid(sc.tiles[3].rect)), Some(3));
        assert_eq!(sc.drop_target(POINT { x: 960, y: 700 }), Some(2), "the spread means the shown workspace");
        assert_eq!(sc.drop_target(POINT { x: 10, y: 700 }), None, "a side margin is neither");
        assert_eq!(sc.drop_target(POINT { x: 10, y: 10 }), None);
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
}
