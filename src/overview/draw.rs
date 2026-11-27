// The overview's drawing: look B's floating shadow, the bar, the workspace chips and names, the
// window labels, the X, and each screen's paint.
use super::*;
use super::drag::Aim;
use super::layout::{Screen, Slot, grow_bx};

thread_local! {
    // The floating shadow's picture per scale and strength (AC-OVR-3): a memory DC holding it,
    // and how far it reaches past its box. Made once and kept, like the fonts.
    static SHADOWS: RefCell<HashMap<(u32, u8), (HDC, i32)>> = RefCell::new(HashMap::new());
}

/// What one screen's painting marks, besides its windows and tiles.
#[derive(Default)]
pub(super) struct Marks {
    pub(super) hover: Option<HWND>,
    pub(super) selected: Option<HWND>,
    pub(super) src: Option<HWND>, // the window being dragged, on its own screen
    pub(super) aim: Option<Aim>,
    pub(super) tile_hover: Option<usize>,
    pub(super) bar: Option<Bar>,
}

/// The X as the design draws it, 28 dip square at x, y: card color with an edge, Windows'
/// close red when pointed at (AC-OVR-2). Two strokes rather than the glyph: text on a per
/// pixel alpha popup would come out see through.
pub(super) fn draw_x(c: &Canvas, x: f32, y: f32, hot: bool) {
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

pub(super) fn paint_x(h: HWND, hot: bool) {
    // Its owner covers the screen it sits on; the X itself is made on the main screen.
    let owner = unsafe { GetWindow(h, GW_OWNER) }.unwrap_or(h);
    let scale = unsafe { GetDpiForWindow(owner) } as f32 / 96.0;
    let mut r = RECT::default();
    unsafe { let _ = GetWindowRect(h, &mut r); }
    let d = (CLOSE_BTN * scale).round() as i32;
    ui::canvas::layered(h, (r.left, r.top, d, d), scale, ui::theme(), |c| draw_x(c, 0.0, 0.0, hot));
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
pub(super) fn bar_box(c: &Canvas, sc: &Screen, bar: Bar) -> Bx {
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

fn chip_w(c: &Canvas, ws: usize) -> f32 {
    (c.width(&(ws + 1).to_string(), F::SmallBold) + 10.0).max(20.0)
}

/// A tile's chip and its workspace's name (spec 0023 AC-7), centered together under the tile
/// `r`, the name cut to the tile's width. Returns the chip's center, where its mark hangs.
fn chip_and_name(c: &Canvas, r: Bx, top: f32, ws: usize, name: &str, full: bool) -> f32 {
    let cw = chip_w(c, ws);
    let name = c.fit_name(name, F::Small, r.2 - cw - NAME_GAP);
    let total = cw + if name.is_empty() { 0.0 } else { NAME_GAP + c.width(&name, F::Small) };
    let left = r.0 + (r.2 - total) / 2.0;
    chip(c, left + cw / 2.0, top, ws, full);
    if !name.is_empty() {
        c.text(left + cw + NAME_GAP, top, CHIP_H, &name, F::Small, c.t.b.tx);
    }
    left + cw / 2.0
}

/// A tile's workspace chip, centered on `cx`: full in its color on the shown workspace, soft
/// on the others (AC-OVR-1).
fn chip(c: &Canvas, cx: f32, top: f32, ws: usize, full: bool) {
    let (b, hc) = (c.t.b, c.t.hc);
    let color = b.ws[ws.min(8)];
    let label = (ws + 1).to_string();
    let tw = c.width(&label, F::SmallBold);
    let w = chip_w(c, ws);
    let at = (cx - w / 2.0, top, w, CHIP_H);
    let (fill, edge, ink) = match () {
        _ if full => (color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)), b.on_ws[ws.min(8)]),
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
        match (&t.picture, &sc.face) {
            // Dimmed as on the screen; your Windows wallpaper never is (spec 0026 AC-18).
            (Some(p), _) => {
                c.picture_rr(r, 4.0, p);
                let dim = crate::backdrop::paint::dim_alpha(settings::get().wallpaper_dim) as u32;
                if dim > 0 {
                    c.fill_rr(r, 4.0, dim << 24);
                }
            }
            (None, Face::Picture(p)) => c.picture_rr(r, 4.0, p),
            (None, Face::Color(color)) => c.fill_rr(r, 4.0, op(*color)),
            (None, Face::Card) => c.fill_rr(r, 4.0, op(b.card)),
        }
        for mini in t.minis.iter().filter(|x| x.card) {
            let mr = bx(mini.rect);
            c.boxed(mr, 0.0, b.card, Some(b.line));
            if mr.2 >= MINI_ICON && mr.3 >= MINI_ICON {
                c.icon(mr.0 + (mr.2 - MINI_ICON) / 2.0, mr.1 + (mr.3 - MINI_ICON) / 2.0, MINI_ICON, mini.icon);
            }
        }
        let cx = chip_and_name(c, r, r.1 + r.3 + CHIP_GAP, i, &t.name, i == sc.shown);
        // The indicator's mark, under the workspace (design attention-r2): a ring around a mini
        // would read as a chosen window or a drop target, purple's only meanings here. Under
        // the chip, not the tile, so it stays with the number when a name follows it.
        if sc.wants.iter().any(|w| w.0 == i) {
            let top = r.1 + r.3 + CHIP_GAP + CHIP_H + WANT_GAP;
            c.fill_rr((cx - WANT / 2.0, top, WANT, WANT), WANT / 2.0, op(b.acc));
        }
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
pub(super) fn paint_screen(mem: HDC, size: (i32, i32), sc: &Screen, m: &Marks, (ox, oy): (i32, i32), t: Theme) {
    let fonts = ui::fonts::fonts(sc.scale);
    unsafe {
        let bg = CreateSolidBrush(ui::color::cref(t.b.bg));
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

pub(super) fn paint(hwnd: HWND) {
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
        ui::canvas::buffered(hdc, dirty.left, dirty.top, size.0, size.1, |mem| {
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


#[cfg(test)]
mod tests;
