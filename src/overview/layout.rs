// The overview's layout: each screen's popup, its band of tiles and its spread of windows, the
// live previews in them, where a point lands, the reading order the keys walk, and the
// words Narrator reads for a tile, a window and the bar.
use super::*;

/// A live preview drawn by DWM into a popup; unregistered when dropped.
pub(super) struct Thumb(isize);

impl Drop for Thumb {
    fn drop(&mut self) {
        unsafe { let _ = DwmUnregisterThumbnail(self.0); }
    }
}

impl Thumb {
    /// A fade frame, or the dragged window's 40%: the preview made see through.
    pub(super) fn set_opacity(&self, opacity: u8) {
        let p = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_OPACITY, opacity, ..Default::default() };
        unsafe { let _ = DwmUpdateThumbnailProperties(self.0, &p); }
    }

    /// The whole preview drawn into `r` now (the ghost changing size).
    pub(super) fn place(&self, r: RECT) {
        let p = DWM_THUMBNAIL_PROPERTIES { dwFlags: DWM_TNP_RECTDESTINATION, rcDestination: r, ..Default::default() };
        unsafe { let _ = DwmUpdateThumbnailProperties(self.0, &p); }
    }
}

/// A window drawn into a popup: in the spread, or a mini inside a tile. A card is a window
/// Windows draws nothing for (AC-4), shown as its icon and title on a plain box.
pub(super) struct Slot {
    pub(super) hwnd: HWND,
    pub(super) rect: RECT, // in popup coordinates
    pub(super) title: String, // empty for minis: tiles never carry titles
    pub(super) icon: HICON, // borrowed, never destroyed; only fetched when drawn
    pub(super) pinned: bool,
    pub(super) card: bool,
    pub(super) thumb: Option<Thumb>,
}

pub(super) struct Tile {
    pub(super) rect: RECT,
    pub(super) minis: Vec<Slot>, // back to front, as the windows stack
    pub(super) name: String,     // its workspace's name on this screen; empty for none (spec 0023 AC-7)
    pub(super) file: Option<String>,               // its workspace's picture on this screen (spec 0026 AC-9)
    pub(super) picture: Option<Arc<ui::canvas::Picture>>, // that picture at tile size, once decoded
}

pub(super) struct Screen {
    pub(super) device: String,
    pub(super) popup: HWND,
    pub(super) rect: RECT, // the whole monitor, taskbar included
    pub(super) scale: f32,
    pub(super) shown: usize, // 0 based
    pub(super) tiles: Vec<Tile>,
    pub(super) slots: Vec<Slot>,
    pub(super) cell_w: i32, // a spread cell's width: a hover label may run that wide
    pub(super) labels: Labels, // as the layout was made for
    pub(super) face: Face, // behind the tiles' windows
    pub(super) close_btn: HWND, // the hover X, hidden until a spread window is hovered
    pub(super) note: Option<Note>,
    pub(super) painted: bool, // its first full paint is done (spec 0017 AC-8)
    pub(super) tree: Tree,
    pub(super) wants: Vec<(usize, HWND)>, // marked windows here, by workspace (spec 0022)
}

pub(super) fn inside(r: &RECT, pt: POINT) -> bool {
    pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
}

pub(super) fn grow(r: RECT, by: i32) -> RECT {
    RECT { left: r.left - by, top: r.top - by, right: r.right + by, bottom: r.bottom + by }
}

fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

pub(super) fn grow_bx((x, y, w, h): Bx, by: f32) -> Bx {
    (x - by, y - by, w + 2.0 * by, h + 2.0 * by)
}

/// A desktop that is still being read paints Windows' desktop color, with no wait (AC-OVR-3).
fn desktop_color() -> u32 {
    let c = unsafe { GetSysColor(COLOR_DESKTOP) };
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
}

impl Screen {
    pub(super) fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    pub(super) fn size(&self) -> (i32, i32) {
        (self.rect.right - self.rect.left, self.rect.bottom - self.rect.top)
    }

    /// The band's height, its rule included: the tiles, their chips, and room under them.
    pub(super) fn band_h(&self) -> i32 {
        self.tiles.first().map_or(self.px(TILE_TOP), |t| t.rect.bottom) + self.px(CHIP_GAP + CHIP_H + BAND_PAD)
    }

    pub(super) fn spread_area(&self) -> RECT {
        let (w, h) = self.size();
        RECT { left: self.px(MARGIN), top: self.band_h() + self.px(SPREAD_GAP), right: w - self.px(MARGIN), bottom: h - self.px(BOTTOM) }
    }

    pub(super) fn tile_size(&self) -> (i32, i32) {
        self.tiles.first().map_or((0, 0), |t| (t.rect.right - t.rect.left, t.rect.bottom - t.rect.top))
    }

    /// The tiles' face as cached now, or the desktop color while it is first read.
    pub(super) fn read_face(&mut self) {
        let size = self.tile_size();
        self.face = wallpaper::cached(self.rect, size).unwrap_or(Face::Color(desktop_color()));
        for t in &mut self.tiles {
            t.picture = t.file.as_deref().and_then(|f| wallpaper::cached_ws(self.rect, f, size));
        }
    }

    /// The pictures this screen's workspaces name, for the wallpaper thread to decode.
    pub(super) fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.tiles.iter().filter_map(|t| t.file.clone()).collect();
        files.sort();
        files.dedup();
        files
    }

    /// Lay out the strip and the spread from `places` (a `ws::windows()` list).
    pub(super) fn build(&mut self, places: &[ws::Place]) {
        let (w, h) = self.size();
        self.shown = ws::active(&self.device);
        // The old thumbnails go before the new ones come.
        self.tiles.clear();
        self.slots.clear();
        let mine: Vec<&ws::Place> = places.iter().filter(|p| p.device == self.device).collect();
        let names = ws::names(&self.device);
        let id = ws::id(&self.device);
        let s = settings::get();
        for (i, rect) in strip(w, h, settings::get().workspaces, self.scale).into_iter().enumerate() {
            // ws::windows() lists front first (on screen in z order, hidden top first), and
            // DWM draws the thumbnail registered last on top, so register back to front. A
            // pinned window is in the shown workspace's tile only.
            let here = |p: &&&ws::Place| if p.pinned { i == self.shown } else { p.workspace == i };
            let minis = mine.iter().rev().filter(here).filter_map(|p| self.mini(rect, p.hwnd)).collect();
            let file = tile_file(&s, &id, i);
            self.tiles.push(Tile { rect, minis, name: names.get(i).cloned().unwrap_or_default(), file, picture: None });
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
                let icon = if card || labels == Labels::Always { finder::names::icon(p.hwnd) } else { HICON::default() };
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
        let icon = if card { finder::names::icon(hwnd) } else { HICON::default() };
        Some(Slot { hwnd, rect, title: String::new(), icon, pinned: false, card, thumb })
    }

    pub(super) fn hit(&self, pt: POINT) -> Hit {
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
    pub(super) fn drop_target(&self, pt: POINT) -> Option<usize> {
        self.tiles.iter().position(|t| inside(&t.rect, pt)).or_else(|| inside(&self.spread_area(), pt).then_some(self.shown))
    }

    /// The top left spread window, where a selection starts.
    pub(super) fn first(&self) -> Option<HWND> {
        first(&self.rects()).map(|i| self.slots[i].hwnd)
    }

    pub(super) fn rects(&self) -> Vec<RECT> {
        self.slots.iter().map(|s| s.rect).collect()
    }

    /// The band under a spread window, where its label goes: as wide as the window with
    /// labels "Always", as wide as its cell for a hover or chosen label (AC-OVR-6).
    pub(super) fn label_rect(&self, s: &Slot, always: bool) -> RECT {
        let (left, right) = if always {
            (s.rect.left, s.rect.right)
        } else {
            let mid = (s.rect.left + s.rect.right) / 2;
            (mid - self.cell_w / 2, mid + self.cell_w / 2)
        };
        RECT { left, top: s.rect.bottom, right, bottom: s.rect.bottom + self.px(LABEL) }
    }

    /// What a spread window's marks cover: its ring and its widest label.
    pub(super) fn mark_rect(&self, hwnd: HWND) -> Option<RECT> {
        let s = self.slots.iter().find(|s| s.hwnd == hwnd)?;
        Some(union(grow(s.rect, self.px(RING_OFF + RING) + 1), self.label_rect(s, false)))
    }

    /// What a tile's ring covers, at its widest (high contrast's 3 px target ring).
    pub(super) fn tile_ring(&self, i: usize) -> Option<RECT> {
        Some(grow(self.tiles.get(i)?.rect, self.px(TILE_RING_OFF + 3.0) + 1))
    }

    /// The bottom of the screen: the bar, its shadow included.
    pub(super) fn bar_strip(&self) -> RECT {
        let (w, h) = self.size();
        RECT { left: 0, top: h - self.px(BOTTOM + SHADOW_BLUR), right: w, bottom: h }
    }

    /// The four thin sides of another screen's spread ring, not the spread inside it.
    pub(super) fn spread_ring(&self) -> [RECT; 4] {
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
    pub(super) fn all(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().chain(self.tiles.iter().flat_map(|t| &t.minis))
    }

    /// Where the hover X goes for `hwnd`, in screen coordinates. None when its window is
    /// too small to hold it.
    pub(super) fn close_spot(&self, hwnd: HWND) -> Option<POINT> {
        let r = self.slots.iter().find(|s| s.hwnd == hwnd)?.rect;
        let (d, inset) = (self.px(CLOSE_BTN), self.px(CLOSE_INSET));
        (r.right - r.left >= d + 2 * inset && r.bottom - r.top >= d + 2 * inset).then(|| POINT { x: self.rect.left + r.right - inset - d, y: self.rect.top + r.top + inset })
    }

    /// "Workspace 2 of 4 shown, 3 windows": what Narrator says first about this screen.
    pub(super) fn summary(&self) -> String {
        format!("Workspace {} of {} shown, {}", self.shown + 1, self.tiles.len(), count(self.slots.len()))
    }

    /// This screen as Narrator sees it, in popup pixels: the tiles, the spread windows with
    /// the chosen one selected, and the bar's words.
    pub(super) fn node(&self, chosen: Option<HWND>, bar: Option<Bar>) -> Node {
        let text = |name: String, rect: RECT| Node { role: UIA_TextControlTypeId, name, rect, ..Node::default() };
        let item = |name: String, rect: RECT, selected: bool| Node { role: UIA_ListItemControlTypeId, name, rect, selected: Some(selected), ..Node::default() };
        let tiles = (self.tiles.iter().enumerate())
            .map(|(i, t)| {
                item(tile_words(i, t, i == self.shown), t.rect, i == self.shown)
            })
            .collect();
        let band = RECT { left: 0, top: 0, right: self.size().0, bottom: self.band_h() };
        let mut kids = vec![Node { role: UIA_ListControlTypeId, name: "Workspaces".into(), rect: band, selected: None, kids: tiles, ..Node::default() }];
        let windows = self.slots.iter().map(|s| item(window_words(s, &finder::names::program(s.hwnd), chosen == Some(s.hwnd)), s.rect, chosen == Some(s.hwnd))).collect();
        kids.push(Node { role: UIA_ListControlTypeId, name: "Windows".into(), rect: self.spread_area(), selected: None, kids: windows, ..Node::default() });
        if self.slots.is_empty() {
            kids.push(text("No windows".into(), self.spread_area()));
        }
        if let Some(b) = bar {
            kids.push(text(bar_words(self, b), self.bar_strip()));
        }
        let (w, h) = self.size();
        Node { role: UIA_PaneControlTypeId, name: "Overview".into(), rect: RECT { left: 0, top: 0, right: w, bottom: h }, selected: None, kids, ..Node::default() }
    }
}

/// A tile as Narrator reads it: its chip, its name as painted under it, then the rest
/// (spec 0023 AC-15).
fn tile_words(i: usize, t: &Tile, shown: bool) -> String {
    let name = if t.name.is_empty() { String::new() } else { format!(", {}", t.name) };
    let how = if shown { "shown" } else { "not shown" };
    format!("Workspace {}{name}, {how}, {}", i + 1, count(t.minis.len()))
}

fn count(n: usize) -> String {
    match n {
        0 => "no windows".into(),
        1 => "1 window".into(),
        n => format!("{n} windows"),
    }
}

/// A spread window as Narrator reads it: the title it paints, then its app.
pub(super) fn window_words(s: &Slot, program: &str, chosen: bool) -> String {
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

pub(super) fn bar_words(sc: &Screen, bar: Bar) -> String {
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

pub(super) enum Hit {
    Slot(HWND),
    Mini(usize, HWND), // (tile, window)
    Tile(usize),
    Background,
}

/// The strip: `count` tiles with the screen's aspect, 12% of its height, in a centered row
/// near the top, shrunk to fit between the side margins.
pub(super) fn strip(w: i32, h: i32, count: usize, scale: f32) -> Vec<RECT> {
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
pub(super) fn spread(area: RECT, reals: &[RECT], gap: i32, band: i32) -> (Vec<RECT>, i32) {
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
pub(super) fn place_in(real: RECT, screen: RECT, tile: RECT) -> RECT {
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
pub(super) fn first(rects: &[RECT]) -> Option<usize> {
    reading_order(rects).first().copied()
}

/// The window `by` Tab stops from `rects[from]` in reading order, around the end; the first
/// one when there is no `from`. None on an empty screen, so Tab there does nothing.
pub(super) fn tab_step(rects: &[RECT], from: Option<usize>, by: isize) -> Option<usize> {
    let order = reading_order(rects);
    let Some(p) = from.and_then(|f| order.iter().position(|&k| k == f)) else { return order.first().copied() };
    Some(order[(p as isize + by).rem_euclid(order.len() as isize) as usize])
}

/// The nearest of `rects` from `rects[from]` in direction `(dx, dy)` (one of them 1 or -1,
/// the other 0), by centers. Being off the line counts double, so Right goes along the row
/// before it drops to the next one. None when nothing lies that way.
pub(super) fn nearest(rects: &[RECT], from: usize, (dx, dy): (i32, i32)) -> Option<usize> {
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

pub(super) fn intersect(a: RECT, b: RECT) -> Option<RECT> {
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
pub(super) fn thumb(host: HWND, src: HWND, full: RECT, shown: RECT) -> Option<Thumb> {
    let t = Thumb(unsafe { DwmRegisterThumbnail(host, src) }.inspect_err(|e| warn!("overview: no preview for {}: {e}", ws::who(src))).ok()?);
    let size = unsafe { DwmQueryThumbnailSourceSize(t.0) }.inspect_err(|e| warn!("overview: no preview size for {}: {e}", ws::who(src))).ok()?;
    if size.cx <= 0 || size.cy <= 0 {
        warn!("overview: Windows draws nothing for {}, showing a card", ws::who(src));
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
    unsafe { DwmUpdateThumbnailProperties(t.0, &p) }.inspect_err(|e| warn!("overview: preview for {} not placed: {e}", ws::who(src))).ok()?;
    Some(t)
}


#[cfg(test)]
mod tests;
