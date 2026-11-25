// The color picker's card, wheel, ring and square, and where the card goes (spec 0021).
use super::*;
use crate::settings_page::cards::colors::{ACCENT, ws_index};

impl Ctx<'_, '_> {
    /// The open color picker, over everything (spec 0021 AC-4, sizes from the approved design):
    /// the header, the 12 presets, the wheel, the status line, Done and Cancel. Hidden while
    /// the page eases to make room for it.
    pub(in crate::settings_page) fn pick_card(&mut self) {
        let Some(p) = self.w.pick.as_ref() else { return };
        let Some(at) = p.placed else { return };
        if self.w.ease.is_some_and(|e| e.opening) {
            return;
        }
        let t = self.c.t;
        let b = t.b;
        let (x, y, kb) = (at.x, at.y, self.w.kb);
        let wi = ws_index(p.key);
        // The page's popup, as its menus are: edge, shadow, card.
        if !t.hc {
            self.c.fill_rr((x, y + 4.0, PICK_W, PICK_H), 8.0, ui::alpha(0, if t.dark { 0.30 } else { 0.08 }));
        }
        self.c.fill_rr((x - 1.0, y - 1.0, PICK_W + 2.0, PICK_H + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        self.c.fill_rr((x, y, PICK_W, PICK_H), 8.0, op(b.card));
        // Header: the chip (start and now once they differ), the title, the hex.
        let changed = p.color != p.start;
        let pale = if t.hc { op(b.tx) } else if t.dark { op(mix(b.bg, b.tx, 0.10)) } else { ui::alpha(0, 0.10) };
        let (hx, hy) = (x + 16.0, y + 12.0);
        let chip_w = match wi {
            None => 22.0,
            Some(_) if changed => 40.0,
            Some(_) => 28.0,
        };
        if wi.is_none() {
            self.c.circle(hx + 11.0, hy + 14.0, 11.0, Some(op(p.color)), None);
            self.c.circle(hx + 11.0, hy + 14.0, 10.5, None, Some((pale, 1.0)));
        } else {
            self.c.fill_rr((hx, hy + 3.0, chip_w, 22.0), 4.0, op(if changed { p.start } else { p.color }));
            if changed {
                self.c.fill_rr((hx + 20.0, hy + 3.0, 20.0, 22.0), 4.0, op(p.color));
                self.c.fill_rr((hx + 20.0, hy + 3.0, 4.0, 22.0), 0.0, op(p.color));
                self.c.fill_rr((hx + 20.0, hy + 3.0, 1.0, 22.0), 0.0, op(b.card));
            }
            // Above the fill, so a pale color still has an outline.
            self.c.stroke_rr((hx + 0.5, hy + 3.5, chip_w - 1.0, 21.0), 3.5, 1.0, pale);
        }
        let title = wi.map_or("Accent".to_string(), |i| format!("Workspace {}", i + 1));
        self.c.text(hx + chip_w + 10.0, hy, 28.0, &title, F::Bold, b.tx);
        let now = format!("#{:06X}", p.color);
        let right = x + PICK_W - 16.0 - self.c.width(&now, F::Mono);
        self.c.text(right, hy, 28.0, &now, F::Mono, b.tx);
        if changed {
            let was = format!("#{:06X} → ", p.start);
            self.c.text(right - self.c.width(&was, F::Mono), hy, 28.0, &was, F::Mono, b.tx2);
        }
        self.c.text(x + 16.0, y + 44.0, 20.0, "des-k colors", F::SmallBold, b.tx2);
        self.c.text(x + 172.0, y + 44.0, 20.0, "Any color", F::SmallBold, b.tx2);
        // The presets, 3 by 4: the one shown checked, a workspace's own color with its number.
        for (i, &c) in ui::PRESETS.iter().enumerate() {
            let (sx, sy) = (x + 16.0 + (i % 3) as f32 * 48.0, y + 68.0 + (i / 3) as f32 * 48.0);
            let bx = (sx, sy, 40.0, 40.0);
            let ink = ui::ink(c);
            self.c.fill_rr(bx, 4.0, op(c));
            if b.ws_edge > 0.0 {
                self.c.stroke_rr((sx + 0.5, sy + 0.5, 39.0, 39.0), 3.5, 1.0, op(mix(c, 0, b.ws_edge)));
            }
            if wi == Some(i) {
                self.c.text(sx + 5.0, sy + 22.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
            }
            if self.hovered(Ctl::Preset(i)) {
                self.cue(bx, ink);
            }
            if c == p.color {
                let (cx, cy) = (sx + 28.0, sy + 12.0);
                self.c.circle(cx, cy, 9.0, Some(op(b.card)), Some((ui::alpha(0, 0.12), 1.0)));
                let iw = self.c.width(I_OK, F::IconTiny);
                self.icon(cx - iw / 2.0, cy - 9.0, 18.0, I_OK, F::IconTiny, b.tx);
            }
            if kb && p.focus == Stop::Swatches(i) {
                self.c.stroke_rr((sx - 3.0, sy - 3.0, 46.0, 46.0), 7.0, 2.0, op(b.tx));
            }
            self.hit(Ctl::Preset(i), bx, 3);
        }
        // The wheel: the toned ring and the square at the hue, drawn pixel for pixel.
        let s = self.c.s;
        let ((cx, cy), sq) = wheel_at(at, s);
        let (n, ro, ri, _) = wheel_px(s);
        let half = n as f32 / 2.0 / s;
        RING.with_borrow_mut(|r| {
            if r.as_ref().is_none_or(|(k, _)| *k != s.to_bits()) {
                *r = Some((s.to_bits(), ring_px(s)));
            }
            if let Some((_, pic)) = r {
                self.c.image(cx - half, cy - half, pic);
            }
        });
        let hue = p.hsv.0;
        SQUARE.with_borrow_mut(|q| {
            if q.as_ref().is_none_or(|(k, h, _)| *k != s.to_bits() || *h != hue.to_bits()) {
                *q = Some((s.to_bits(), hue.to_bits(), square_px(s, hue)));
            }
            if let Some((_, _, pic)) = q {
                self.c.image(sq.0, sq.1, pic);
            }
        });
        if !t.dark && !t.hc {
            let line = ui::alpha(0, 0.10);
            self.c.circle(cx, cy, ro / s - 0.5, None, Some((line, 1.0)));
            self.c.circle(cx, cy, ri / s + 0.5, None, Some((line, 1.0)));
            self.c.stroke_rr((sq.0 + 0.5, sq.1 + 0.5, sq.2 - 1.0, sq.3 - 1.0), 2.5, 1.0, line);
        }
        let (h, sat, val) = p.hsv;
        let (mr, a) = ((ro + ri) / 2.0 / s, h.to_radians());
        self.mark(cx + a.sin() * mr, cy - a.cos() * mr, 9.0, ui::ring_color(h.round()), kb && p.focus == Stop::Ring);
        self.mark(sq.0 + sat * sq.2, sq.1 + (1.0 - val) * sq.3, 7.0, p.color, kb && p.focus == Stop::Square);
        self.hit(Ctl::Square, (sq.0 - 8.0, sq.1 - 8.0, sq.2 + 16.0, sq.3 + 16.0), 3);
        self.hit(Ctl::Ring, (cx - 106.0, cy - 106.0, 212.0, 212.0), 3);
        // Footer: the status line, Done, Cancel.
        let fy = y + 282.0;
        let cw = (self.c.width("Cancel", F::Body) + 28.0).max(96.0);
        let dw = (self.c.width("Done", F::Body) + 28.0).max(96.0);
        let (cancel_x, done_x) = (x + PICK_W - 16.0 - cw, x + PICK_W - 16.0 - cw - 8.0 - dw);
        let done = (done_x, fy, dw, 32.0);
        self.c.fill_rr(done, 6.0, op(if self.hovered(Ctl::PickDone) { mix(b.acc, b.card, 0.10) } else { b.acc }));
        if ui::contrast(b.acc, b.card) < 1.5 {
            self.c.stroke_rr((done_x + 0.5, fy + 0.5, dw - 1.0, 31.0), 5.5, 1.0, op(b.tx2));
        }
        let lw = self.c.width("Done", F::Body);
        self.c.text(done_x + (dw - lw) / 2.0, fy, 32.0, "Done", F::Body, b.on_acc);
        let cancel = (cancel_x, fy, cw, 32.0);
        self.field(cancel, if self.hovered(Ctl::PickCancel) { mix(b.card, b.tx, 0.05) } else { b.card }, None);
        let lw = self.c.width("Cancel", F::Body);
        self.c.text(cancel_x + (cw - lw) / 2.0, fy, 32.0, "Cancel", F::Body, b.tx);
        for (bx, stop) in [(done, Stop::Done), (cancel, Stop::Cancel)] {
            if kb && p.focus == stop {
                self.c.stroke_rr((bx.0 - 3.0, bx.1 - 3.0, bx.2 + 6.0, bx.3 + 6.0), 9.0, 2.0, op(b.tx));
            }
        }
        self.hit(Ctl::PickDone, done, 3);
        self.hit(Ctl::PickCancel, cancel, 3);
        let (sx, room) = (x + 16.0, done_x - 12.0 - x - 16.0);
        if p.failed.is_some() {
            let th = self.c.measuring().para(0.0, 0.0, room - 24.0, 16.0, &[(NOT_SAVED, F::SmallBold, b.tx)]);
            let top = fy + (32.0 - th) / 2.0;
            self.icon(sx, top, 16.0, I_WARN, F::IconSmall, b.warn);
            self.c.para(sx + 24.0, top, room - 24.0, 16.0, &[(NOT_SAVED, F::SmallBold, b.tx)]);
        } else if let Some(i) = wi
            && let Some((j, same)) = dup_of(p.color, i, &b.ws, self.s.workspaces)
        {
            let line = format!("Workspace {} {}", j + 1, if same { "uses this color too." } else { "has a very close color." });
            let th = self.c.measuring().para(0.0, 0.0, room - 30.0, 16.0, &[(&line, F::Small, b.tx2)]);
            let top = fy + (32.0 - th) / 2.0;
            self.c.fill_rr((sx, top, 22.0, 16.0), 3.0, op(b.ws[j]));
            if b.ws_edge > 0.0 {
                self.c.stroke_rr((sx + 0.5, top + 0.5, 21.0, 15.0), 2.5, 1.0, op(mix(b.ws[j], 0, b.ws_edge)));
            }
            self.c.text(sx + 4.0, top + 3.0, 13.0, &(j + 1).to_string(), F::TinyBold, b.on_ws[j]);
            self.c.para(sx + 30.0, top, room - 30.0, 16.0, &[(&line, F::Small, b.tx2)]);
        }
        // Last, so every part above wins the click.
        self.hit(Ctl::PickCard, (x, y, PICK_W, PICK_H), 3);
    }
}

/// The wheel's bitmap in pixels at scale `s`: its side, the ring's outer and inner radius, and
/// the square's side. Whole pixels, so the ring's center is a pixel corner at every scale (at
/// 125% the 90 dip middle line falls on half a pixel).
fn wheel_px(s: f32) -> (i32, f32, f32, i32) {
    (2 * (108.0 * s).round() as i32, (100.0 * s).round(), (80.0 * s).round(), 2 * (52.0 * s).round() as i32)
}

/// The wheel of a card at `at`: its center and the square, in dip, from the same whole pixels
/// the bitmaps are drawn at. The bitmap is 8 dip larger than the 200 dip wheel on every side,
/// so the focus circle around the ring's mark is never cut.
pub(super) fn wheel_at(at: Place, s: f32) -> ((f32, f32), Bx) {
    let (n, _, _, side) = wheel_px(s);
    let (ox, oy) = (((at.x + 164.0) * s).round(), ((at.y + 62.0) * s).round());
    let (cx, cy) = ((ox + n as f32 / 2.0) / s, (oy + n as f32 / 2.0) / s);
    let half = side as f32 / 2.0 / s;
    ((cx, cy), (cx - half, cy - half, 2.0 * half, 2.0 * half))
}

/// A color at opacity `a`, premultiplied.
fn premul(c: u32, a: f32) -> u32 {
    let ch = |shift: u32| ((c >> shift & 0xFF) as f32 * a).round() as u32;
    ((a * 255.0).round() as u32) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// The toned ring, hue 0 at the top going clockwise, its edges smoothed; clear inside and out.
fn ring_px(s: f32) -> ui::Picture {
    let (n, ro, ri, _) = wheel_px(s);
    let c = n as f32 / 2.0;
    let hues: Vec<u32> = (0..360).map(|h| ui::ring_color(h as f32)).collect();
    let mut px = vec![0u32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = dx.hypot(dy);
            let a = (ro - d + 0.5).min(d - ri + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                let h = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                px[(y * n + x) as usize] = premul(hues[h.round() as usize % 360], a);
            }
        }
    }
    ui::Picture { w: n, h: n, px }
}

/// The square at `hue`: saturation 0 to 1 left to right, value 1 to 0 top to bottom, its 3 dip
/// corners smoothed.
fn square_px(s: f32, hue: f32) -> ui::Picture {
    let (_, _, _, n) = wheel_px(s);
    let (r, side) = (3.0 * s, n as f32);
    let mut px = vec![0u32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (ox, oy) = ((r - fx).max(0.0).max(fx - (side - r)), (r - fy).max(0.0).max(fy - (side - r)));
            let a = (r - ox.hypot(oy) + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                px[(y * n + x) as usize] = premul(ui::rgb_of((hue, fx / side, 1.0 - fy / side)), a);
            }
        }
    }
    ui::Picture { w: n, h: n, px }
}

/// Which part of the wheel a point is on, given from the wheel's center in dip (the design's
/// hit areas): the square plus 8 dip, else the ring's band plus 6 dip either side.
pub(super) fn part_at((dx, dy): (f32, f32), half: f32) -> Option<Part> {
    if dx.abs() <= half + 8.0 && dy.abs() <= half + 8.0 {
        Some(Part::Square)
    } else {
        (74.0..=106.0).contains(&dx.hypot(dy)).then_some(Part::Ring)
    }
}

/// The card's window position, and the keep set it was placed with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::settings_page) struct Place {
    pub(super) x: f32,
    pub(super) y: f32,
    level: usize,
}

/// What the card is placed in, in window dip: the window's height, the notes over the column,
/// the card's least and most x, the scroll and its limit.
#[derive(Clone, Copy, Debug)]
pub(in crate::settings_page) struct Room {
    vh: f32,
    fixed: f32,
    lo: f32,
    hi: f32,
    scroll: f32,
    smax: f32,
}

/// One set the card keeps clear (AC-14): the box it sits by, and every box that must stay in
/// view and uncovered.
#[derive(Clone, Debug)]
pub(in crate::settings_page) struct Level {
    next_to: Bx,
    keep: Vec<Bx>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Way {
    Beside,
    Below,
    Above,
}

fn union(a: Bx, b: Bx) -> Bx {
    let (l, t) = (a.0.min(b.0), a.1.min(b.1));
    (l, t, (a.0 + a.2).max(b.0 + b.2) - l, (a.1 + a.3).max(b.1 + b.3) - t)
}

/// A kept box with its ring and hover edge, the page scrolled `d` further.
fn grown(r: Bx, d: f32) -> Bx {
    (r.0 - OUTSET, r.1 - d - OUTSET, r.2 + 2.0 * OUTSET, r.3 + 2.0 * OUTSET)
}

fn covers(r: Bx, (x, y): (f32, f32), (cw, ch): (f32, f32)) -> bool {
    r.0 < x + cw && x < r.0 + r.2 && r.1 < y + ch && y < r.1 + r.3
}

/// Every kept box is in view, 8 dip inside the window's bottom and below the notes.
fn in_view(room: &Room, keep: &[Bx]) -> bool {
    keep.iter().all(|r| r.1 >= room.fixed + MARGIN - 0.5 && r.1 + r.3 <= room.vh - MARGIN + 0.5)
}

/// Where the card fits beside, below or above `level`'s box with the page scrolled `d`
/// further, or None. `cover`: it may lie over the page's notes, from y 8, as the menus do.
/// Beside sits level with what you clicked, sliding only as far as the window and the kept
/// boxes need.
#[allow(clippy::too_many_arguments)]
fn spot(room: &Room, level: &Level, anchor: Bx, way: Way, cover: bool, d: f32, card: (f32, f32)) -> Option<(f32, f32)> {
    let (cw, ch) = card;
    let keep: Vec<Bx> = level.keep.iter().map(|r| grown(*r, d)).collect();
    if !in_view(room, &keep) {
        return None;
    }
    let (a, g) = ((anchor.0, anchor.1 - d, anchor.2, anchor.3), (level.next_to.0, level.next_to.1 - d, level.next_to.2, level.next_to.3));
    let (top, bottom) = (if cover { MARGIN } else { room.fixed + MARGIN }, room.vh - MARGIN - ch);
    let centered = (a.0 + a.2 / 2.0 - cw / 2.0).min(room.hi).max(room.lo);
    let (x, ys) = match way {
        Way::Below => (centered, vec![g.1 + g.3 + MARGIN]),
        Way::Above => (centered, vec![g.1 - MARGIN - ch]),
        Way::Beside => {
            let x = if g.0 - MARGIN - cw >= room.lo {
                g.0 - MARGIN - cw
            } else if g.0 + g.2 + MARGIN <= room.hi {
                g.0 + g.2 + MARGIN
            } else {
                return None;
            };
            let want = a.1 + a.3 / 2.0 - ch / 2.0;
            let mut ys: Vec<f32> = std::iter::once(want).chain(keep.iter().flat_map(|r| [r.1 + OUTSET - MARGIN - ch, r.1 + r.3 - OUTSET + MARGIN])).map(|y| y.min(bottom).max(top)).collect();
            ys.sort_by(|m, n| (m - want).abs().total_cmp(&(n - want).abs()));
            (x, ys)
        }
    };
    ys.into_iter().find(|&y| y >= top - 0.5 && y <= bottom + 0.5 && !keep.iter().any(|r| covers(*r, (x, y), card))).map(|y| (x, y))
}

/// Where the card opens (AC-14), each set tried widest first: a place as the page is, else (with
/// `scroll`) the least scroll that lets one fit, equal scrolls by the order of places; then the
/// same over the notes; only then the next, narrower set. Tiles and the picture: beside, below,
/// above; swatches: below, above, beside. Returns the place after the scroll, and the scroll's
/// change.
pub(in crate::settings_page) fn place(room: &Room, levels: &[Level], anchor: Bx, beside_first: bool, card: (f32, f32), scroll: bool) -> (Place, f32) {
    let order = if beside_first { [Way::Beside, Way::Below, Way::Above] } else { [Way::Below, Way::Above, Way::Beside] };
    let reach = if scroll { room.scroll.max(room.smax - room.scroll).ceil() as i32 } else { 0 };
    for (level, l) in levels.iter().enumerate() {
        for cover in [false, true] {
            for k in 0..=reach {
                for d in if k == 0 { vec![0.0] } else { vec![k as f32, -(k as f32)] } {
                    if room.scroll + d < -0.5 || room.scroll + d > room.smax + 0.5 {
                        continue;
                    }
                    for way in order {
                        if let Some((x, y)) = spot(room, l, anchor, way, cover, d, card) {
                            return (Place { x, y, level }, d);
                        }
                    }
                }
            }
        }
    }
    // Nothing fits even over the notes: below what you clicked, kept in the window.
    let x = (anchor.0 + anchor.2 / 2.0 - card.0 / 2.0).min(room.hi).max(room.lo);
    let y = (anchor.1 + anchor.3 + MARGIN).min(room.vh - MARGIN - card.1).max(MARGIN);
    (Place { x, y, level: levels.len().saturating_sub(1) }, 0.0)
}

/// The card can stay at `at`: inside the window, clear of its keep set, which is still in view.
fn kept(room: &Room, levels: &[Level], at: Place, card: (f32, f32)) -> bool {
    let Some(l) = levels.get(at.level) else { return false };
    let keep: Vec<Bx> = l.keep.iter().map(|r| grown(*r, 0.0)).collect();
    in_view(room, &keep)
        && at.y >= MARGIN - 0.5
        && at.y <= room.vh - MARGIN - card.1 + 0.5
        && at.x >= room.lo - 0.5
        && at.x <= room.hi + 0.5
        && !keep.iter().any(|r| covers(*r, (at.x, at.y), card))
}

/// What you clicked and the sets the card keeps clear, widest first (AC-14), from where the
/// page last drew them; None when what you clicked is gone.
pub(in crate::settings_page) fn levels_of(anchor: &Anchor, g: &Geo) -> Option<(Bx, Vec<Level>)> {
    let one = |b: Bx| Level { next_to: b, keep: vec![b] };
    match anchor {
        Anchor::Tile(dev, i) => {
            let tile = g.tiles.iter().find(|(d, j, _)| d == dev && j == i)?.2;
            let row = g.rows.iter().find(|(d, _)| d == dev).map_or(tile, |r| r.1);
            // Every plugged in screen's tiles: a screen not plugged in shows grey tiles, nothing to watch.
            let all = g.rows.iter().map(|r| r.1).reduce(union).unwrap_or(row);
            Some((tile, vec![one(all), one(row), one(tile)]))
        }
        Anchor::Show(dev, i) => {
            let tile = g.shows.iter().find(|(d, j, _)| d == dev && j == i)?.2;
            let four = g.groups.iter().find(|(d, _)| d == dev).map_or(tile, |r| r.1);
            Some((tile, vec![one(g.band.unwrap_or(four)), one(four), one(tile)]))
        }
        Anchor::Swatch(k) => {
            let sw = g.swatches.iter().find(|(x, _)| x == k)?.1;
            // Your accent first keeps the sample's legend clear too: it names what repaints.
            match g.legend.filter(|_| *k == ACCENT) {
                Some(legend) => Some((sw, vec![Level { next_to: sw, keep: vec![sw, legend] }, one(sw)])),
                None => Some((sw, vec![one(sw)])),
            }
        }
    }
}

pub(in crate::settings_page) fn room_of(w: &Win) -> Room {
    let cw = (w.vw - X0 - PAD_R).min(COL_MAX);
    Room { vh: w.vh, fixed: w.fixed, lo: X0, hi: X0 + cw - PICK_W, scroll: w.scroll, smax: (w.height - (w.vh - w.fixed)).max(0.0) }
}

pub(in crate::settings_page) enum Fit {
    Kept,
    Placed,
    Gone,
}

/// After a layout: the open picker keeps its window position while it still fits there; else it
/// is placed again, scrolling the page only when it first opens; it closes when what you
/// clicked is gone (see `layout`). Nothing while the page eases.
pub(in crate::settings_page) fn pick_fit() -> Option<Fit> {
    let (fit, to) = set(|w| {
        if w.ease.is_some() {
            return None;
        }
        let p = w.pick.as_ref()?;
        let Some((anchor, levels)) = levels_of(&p.anchor, &w.geo) else { return Some((Fit::Gone, None)) };
        let room = room_of(w);
        if p.placed.is_some_and(|at| kept(&room, &levels, at, (PICK_W, PICK_H))) {
            return Some((Fit::Kept, None));
        }
        let (first, beside) = (p.placed.is_none(), !matches!(p.anchor, Anchor::Swatch(_)));
        let (at, d) = place(&room, &levels, anchor, beside, (PICK_W, PICK_H), first);
        w.pick.as_mut()?.placed = Some(at);
        Some((Fit::Placed, (d != 0.0).then_some(w.scroll + d)))
    })
    .flatten()?;
    if let Some(to) = to {
        ease_to(to, true);
    }
    Some(fit)
}

#[cfg(test)]
mod tests;
