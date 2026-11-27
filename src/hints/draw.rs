// The hints window's drawing: key caps, the keyboard and gesture cards, notes, the scrolling middle,
// the footer, and its buttons where the layout placed them.
use super::*;
use super::rows::{Does, Gact, KeyRow, Pad, View, caps, list, pad_words};

/// A button as the layout placed it: its pill, in dip.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Place {
    pub(super) id: i32,
    pub(super) bx: Bx,
    pub(super) back: u32, // the color behind it
    pub(super) label: &'static str,
    pub(super) scrolls: bool, // in the middle: clipped to it
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Fit {
    pub(super) height: f32,
    pub(super) content: f32, // the middle's whole height
    pub(super) shown: f32,   // how much of it shows
}

// ---- Drawing ----

/// Key caps for `combos`, each combo's caps 4 apart and "or" between combos: their width.
pub(super) fn keys_w(c: &Canvas, combos: &[Vec<String>]) -> f32 {
    let m = c.measuring();
    let caps: f32 = combos.iter().map(|k| k.iter().map(|x| m.key_cap(0.0, 0.0, CAP, x, false) + 4.0).sum::<f32>() - 4.0).sum();
    caps + combos.len().saturating_sub(1) as f32 * (16.0 + c.width("or", F::Small))
}

/// Draws `combos` from x, centred on `cy`; returns where they end.
pub(super) fn draw_keys(c: &Canvas, x: f32, cy: f32, combos: &[Vec<String>], held: bool) -> f32 {
    let mut x = x;
    for (i, combo) in combos.iter().enumerate() {
        if i > 0 {
            c.text(x + 8.0, cy - 8.0, 16.0, "or", F::Small, c.t.b.tx2);
            x += 16.0 + c.width("or", F::Small);
        }
        for (j, k) in combo.iter().enumerate() {
            if j > 0 {
                x += 4.0;
            }
            x += c.key_cap(x, cy - CAP / 2.0, CAP, k, held);
        }
    }
    x
}

/// A card's tinted header with its line under it; `n`: a finger count in a circle.
pub(super) fn header(c: &Canvas, (x, y, w): (f32, f32, f32), title: &str, n: Option<u8>, dim: bool) -> f32 {
    let (t, b) = (c.t, c.t.b);
    let h = if n.is_some() { 39.0 } else { HEAD };
    let tint = match () {
        _ if t.hc => b.card,
        _ if dim => b.track,
        _ => mix(b.card, b.acc_a, 0.09),
    };
    c.fill_rr((x + 1.0, y + 1.0, w - 2.0, h - 1.0), 7.0, op(tint));
    c.fill_rr((x + 1.0, y + h / 2.0, w - 2.0, h / 2.0 - 1.0), 0.0, op(tint));
    c.rule(x, y + h - 1.0, w, op(b.line));
    let mut tx = x + 16.0;
    if let Some(n) = n {
        let fill = if dim { b.tx3 } else { b.acc_b };
        c.circle(tx + 11.0, y + h / 2.0, 11.0, Some(op(fill)), None);
        let s = n.to_string();
        c.text(tx + 11.0 - c.width(&s, F::SmallBold) / 2.0, y + (h - 22.0) / 2.0, 22.0, &s, F::SmallBold, b.on_acc_b);
        tx += 32.0;
    }
    c.text(tx, y, h - 1.0, title, F::Bold, if dim { b.tx3 } else { b.tx });
    h
}

/// A keyboard card: its header and one row per action. Returns where it ends.
pub(super) fn key_card(c: &Canvas, x: f32, y: f32, title: &str, rows: &[KeyRow]) -> f32 {
    let b = c.t.b;
    let h = HEAD + rows.len() as f32 * KROW;
    c.boxed((x, y, LEFT_W, h), 8.0, b.card, Some(b.line));
    let mut ry = y + header(c, (x, y, LEFT_W), title, None, false);
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            c.rule(x + 1.0, ry, LEFT_W - 2.0, op(b.line));
        }
        let right = x + LEFT_W - 16.0;
        let kw = if r.keys.is_empty() { c.width("No key", F::Small) } else { keys_w(c, &r.keys) };
        if r.keys.is_empty() {
            c.text(right - kw, ry, KROW, "No key", F::Small, b.tx3);
        } else {
            draw_keys(c, right - kw, ry + KROW / 2.0, &r.keys, false);
        }
        c.text(x + 16.0, ry, KROW, &c.fit(&r.title, F::Body, LEFT_W - 48.0 - kw), F::Body, b.tx);
        ry += KROW;
    }
    ry
}

/// One gesture: its direction mark, then what it does and its twice line. Returns its height.
pub(super) fn gact(c: &Canvas, x: f32, y: f32, w: f32, g: &Gact, dim: bool) -> f32 {
    let (t, b) = (c.t, c.t.b);
    let glyph = match g.motion {
        Motion::Left => "\u{E72B}",
        Motion::Right => "\u{E72A}",
        Motion::Up => "\u{E74A}",
        Motion::Down => "\u{E74B}",
        Motion::Tap => "\u{E815}",
    };
    let mark = if dim { b.tx3 } else { b.acc };
    match () {
        _ if t.hc => c.stroke_rr((x + 0.5, y + 0.5, 19.0, 19.0), 4.0, 1.0, op(mark)),
        _ if dim => c.fill_rr((x, y, 20.0, 20.0), 4.0, op(b.track)),
        _ => c.fill_rr((x, y, 20.0, 20.0), 4.0, op(b.held)),
    }
    c.text(x + (20.0 - c.width(glyph, F::IconTiny)) / 2.0, y, 20.0, glyph, F::IconTiny, mark);
    let (lx, lw) = (x + 26.0, w - 26.0);
    let ink = if dim { b.tx3 } else { b.tx };
    let mut h = 20.0;
    match &g.once {
        Does::Name(n) => c.text(lx, y, 20.0, &c.fit(n, F::Body, lw), F::Body, ink),
        Does::Nothing => c.text(lx, y, 20.0, "Nothing", F::Body, b.tx3),
        Does::Types(k) => {
            c.text(lx, y - 1.0, CAP, "Types", F::Body, ink);
            draw_keys(c, lx + c.width("Types", F::Body) + 6.0, y + 10.0, std::slice::from_ref(k), false);
            h = CAP;
        }
    }
    if let Some(tw) = &g.twice {
        c.text(lx, y + h, 16.0, &c.fit(tw, F::Small, lw), F::Small, if dim { b.tx3 } else { b.tx2 });
        h += 16.0;
    }
    h
}

/// A gesture card: 3 or 4 fingers, its swipes in two columns, then its tap. Returns where it ends.
pub(super) fn gesture_card(c: &Canvas, x: f32, y: f32, fingers: u8, acts: &[Gact], dim: bool) -> f32 {
    let b = c.t.b;
    let col = (RIGHT_W - 32.0 - 60.0 - 12.0) / 2.0;
    let gx = x + 16.0 + 44.0 + 16.0;
    // Measured first: the card's edge goes under everything.
    let m = c.measuring();
    let row_h = |a: &Gact, b2: &Gact| gact(&m, 0.0, 0.0, col, a, dim).max(gact(&m, 0.0, 0.0, col, b2, dim));
    let (r1, r2) = (row_h(&acts[0], &acts[1]), row_h(&acts[2], &acts[3]));
    let swipe_h = 8.0 + r1 + 6.0 + r2 + 8.0;
    let tap_h = (8.0 + gact(&m, 0.0, 0.0, col, &acts[4], dim) + 8.0).max(40.0);
    let h = 39.0 + swipe_h + tap_h;
    c.boxed((x, y, RIGHT_W, h), 8.0, b.card, Some(b.line));
    let mut ry = y + header(c, (x, y, RIGHT_W), &format!("{fingers} fingers"), Some(fingers), dim);
    let label = if dim { b.tx3 } else { b.tx };
    c.text(x + 16.0, ry + 10.0, 20.0, "Swipe", F::Bold, label);
    gact(c, gx, ry + 8.0, col, &acts[0], dim);
    gact(c, gx + col + 12.0, ry + 8.0, col, &acts[1], dim);
    gact(c, gx, ry + 8.0 + r1 + 6.0, col, &acts[2], dim);
    gact(c, gx + col + 12.0, ry + 8.0 + r1 + 6.0, col, &acts[3], dim);
    ry += swipe_h;
    c.rule(x + 1.0, ry, RIGHT_W - 2.0, op(b.line));
    c.text(x + 16.0, ry, tap_h, "Tap", F::Bold, label);
    let th = gact(&m, 0.0, 0.0, col, &acts[4], dim);
    gact(c, gx, ry + (tap_h - th) / 2.0, col, &acts[4], dim);
    ry + tap_h
}

/// A button's pill width for its label.
pub(super) fn btn_w(c: &Canvas, label: &str) -> f32 {
    c.width(label, F::Body) + 30.0
}

/// A note: an icon, its title and line, and its buttons on the right. Returns its height.
#[allow(clippy::too_many_arguments)]
pub(super) fn note(c: &Canvas, places: &mut Vec<Place>, (x, y, w): (f32, f32, f32), (icon, color): (&str, u32), title: &str, line: &str, buttons: &[(i32, &'static str)], scrolls: bool) -> f32 {
    let b = c.t.b;
    let bw: f32 = buttons.iter().map(|(_, l)| btn_w(c, l) + 8.0).sum::<f32>();
    let lw = w - 24.0 - 28.0 - if bw > 0.0 { bw + 4.0 } else { 0.0 };
    let m = c.measuring();
    let text_h = m.para(0.0, 0.0, lw, 20.0, &[(title, F::Bold, 0)]) + m.para(0.0, 0.0, lw, 16.0, &[(line, F::Small, 0)]);
    let inner = text_h.max(if buttons.is_empty() { 0.0 } else { BTN_H });
    let h = 20.0 + inner;
    c.boxed((x, y, w, h), 6.0, b.card, Some(b.line));
    c.text(x + 12.0, y + 12.0, 16.0, icon, F::Icon, color);
    let ty = y + 10.0 + (inner - text_h) / 2.0;
    let th = c.para(x + 40.0, ty, lw, 20.0, &[(title, F::Bold, b.tx)]);
    c.para(x + 40.0, ty + th, lw, 16.0, &[(line, F::Small, b.tx2)]);
    let mut bx = x + w - 12.0;
    for &(id, label) in buttons.iter().rev() {
        let bw = btn_w(c, label);
        bx -= bw;
        places.push(Place { id, bx: (bx, y + (h - BTN_H) / 2.0, bw, BTN_H), back: b.card, label, scrolls });
        bx -= 8.0;
    }
    h
}

/// The scrolling middle, its top at `top`: the keyboard column and the touchpad column.
/// Returns its whole height, from `top` - 1 (its 1 dip inset) to its bottom padding.
pub(super) fn middle(c: &Canvas, places: &mut Vec<Place>, v: &View, top: f32) -> f32 {
    let b = c.t.b;
    // The keyboard.
    c.text(SIDE, top, 20.0, "Keyboard", F::Bold, b.tx);
    let mut y = key_card(c, SIDE, top + 28.0, "Move around", &v.around) + 12.0;
    y = key_card(c, SIDE, y, "Move windows", &v.windows);
    y += 10.0 + c.para(SIDE, y + 10.0, LEFT_W, 16.0, &[(v.rule, F::Small, b.tx2)]);
    y += 6.0 + c.para(SIDE, y + 6.0, LEFT_W, 16.0, &[(REMOTE, F::Small, b.tx2)]);
    if v.copilot {
        c.text(SIDE, y + 10.0, 16.0, COPILOT, F::Small, b.tx2);
        y += 26.0;
    }
    let left = y;
    // The touchpad.
    let (status, words, button) = pad_words(v.pad);
    let right = RIGHT_X + RIGHT_W;
    c.text(RIGHT_X, top, 20.0, "Touchpad", F::Bold, b.tx);
    let sw = c.width(status, F::Small);
    c.text(right - sw, top, 20.0, status, F::Small, b.tx2);
    if v.pad == Pad::On {
        c.text(right - sw - 16.0, top, 20.0, "\u{E73E}", F::IconSmall, b.good);
    }
    let mut y = top + 28.0;
    if let Some((icon, title, line, warn)) = &words {
        let ids: Vec<(i32, &'static str)> = button.map(|l| (ID_NOTE, l)).into_iter().collect();
        y += note(c, places, (RIGHT_X, y, RIGHT_W), (icon, if *warn { b.warn } else { b.tx2 }), title, line, &ids, true) + 12.0;
    }
    let dim = v.pad != Pad::On;
    for (i, (fingers, acts)) in v.cards.iter().enumerate() {
        if i > 0 {
            y += 12.0;
        }
        y = gesture_card(c, RIGHT_X, y, *fingers, acts, dim);
    }
    left.max(y) + 12.0 - (top - 1.0)
}

/// The footer's height: its line, padding, the recorder's note when one shows, the key row.
pub(super) fn foot_h(c: &Canvas, rec: &Rec, error: Option<&str>) -> f32 {
    let mut scratch = vec![];
    1.0 + 16.0 + foot_note(&c.measuring(), &mut scratch, rec, error, 0.0).map_or(0.0, |h| h + 12.0) + BTN_H + 20.0
}

/// The recorder's note or a save error, at `y`: its height, or None when neither shows.
pub(super) fn foot_note(c: &Canvas, places: &mut Vec<Place>, rec: &Rec, error: Option<&str>, y: f32) -> Option<f32> {
    let b = c.t.b;
    let at = (SIDE, y, W - 2.0 * SIDE);
    Some(match (rec, error) {
        (Rec::Recording(None), _) => note(c, places, at, ("\u{E765}", b.acc), "Press the new key", "Hold Win, Ctrl, Alt or Shift with a key. If Windows uses it, des-k takes it over. Esc alone cancels.", &[(ID_CANCEL, "Cancel")], false),
        (Rec::Recording(Some((t, l))), _) => note(c, places, at, ("\u{E7BA}", b.warn), t, l, &[(ID_CANCEL, "Cancel")], false),
        (Rec::Clash { title, sub, .. }, _) => note(c, places, at, ("\u{E7BA}", b.warn), title, sub, &[(ID_MOVE, "Move it here"), (ID_CANCEL, "Cancel")], false),
        (Rec::Idle, Some(why)) => note(c, places, at, ("\u{E7BA}", b.warn), "Not saved", why, &[], false),
        (Rec::Idle, None) => return None,
    })
}

/// The footer band at `top`: the recorder's note, then the hints key and Open settings.
pub(super) fn footer(c: &Canvas, places: &mut Vec<Place>, v: &View, rec: &Rec, error: Option<&str>, top: f32, height: f32) {
    let (t, b) = (c.t, c.t.b);
    c.fill_rr((0.0, top, W, height - top), 0.0, op(if t.hc { b.bg } else { b.band }));
    c.rule(0.0, top, W, op(b.line));
    let mut y = top + 1.0 + 16.0;
    if let Some(h) = foot_note(c, places, rec, error, y) {
        y += h + 12.0;
    }
    let back = if t.hc { b.bg } else { b.band };
    let cy = y + BTN_H / 2.0;
    let mut x = SIDE;
    let button = |x: f32, id: i32, label: &'static str, places: &mut Vec<Place>| {
        let w = btn_w(c, label);
        places.push(Place { id, bx: (x, y, w, BTN_H), back, label, scrolls: false });
        x + w
    };
    match rec {
        Rec::Idle if v.key.is_empty() => {
            c.text(x, y, BTN_H, "Opens with: no key", F::Body, b.tx2);
            x += c.width("Opens with: no key", F::Body) + 8.0;
            button(x, ID_SET, "Set key", places);
        }
        Rec::Idle => {
            c.text(x, y, BTN_H, "Opens with", F::Body, b.tx2);
            x += c.width("Opens with", F::Body) + 8.0;
            x = draw_keys(c, x, cy, &list(&v.key), false) + 8.0;
            x = button(x, ID_SET, "Change", places) + 8.0;
            button(x, ID_REMOVE, "Remove", places);
        }
        // The field waits for the key, or holds the one that clashed.
        Rec::Recording(_) | Rec::Clash { .. } => {
            c.text(x, y, BTN_H, "Opens with", F::Body, b.tx2);
            x += c.width("Opens with", F::Body) + 8.0;
            let inner = match rec {
                Rec::Clash { combo, .. } => keys_w(c, &[caps(combo)]),
                _ => c.width("key", F::Small) + 14.0,
            };
            let field = (x, y, inner + 10.0, BTN_H);
            c.fill_rr(field, 6.0, op(if t.hc { b.bg } else { b.card }));
            c.stroke_rr((field.0 + 1.0, field.1 + 1.0, field.2 - 2.0, field.3 - 2.0), 5.0, 2.0, op(b.acc));
            match rec {
                Rec::Clash { combo, .. } => {
                    draw_keys(c, x + 5.0, cy, &[caps(combo)], true);
                }
                _ => {
                    let g = (x + 5.0, cy - CAP / 2.0, inner, CAP);
                    c.stroke_dash((g.0 + 0.5, g.1 + 0.5, g.2 - 1.0, g.3 - 1.0), 4.0, 1.0, op(b.tx3), windows::Win32::Graphics::GdiPlus::DashStyleDash);
                    c.text(g.0 + 7.0, g.1, CAP, "key", F::Small, b.tx3);
                }
            }
        }
    }
    let w = btn_w(c, "Open settings");
    places.push(Place { id: ID_SETTINGS, bx: (W - SIDE - w, y, w, BTN_H), back, label: "Open settings", scrolls: false });
}

/// The whole window, back to front, `height` dip tall, the middle scrolled by `scroll`.
pub(super) fn compose(c: &Canvas, places: &mut Vec<Place>, v: &View, rec: &Rec, error: Option<&str>, height: f32, scroll: f32) -> Fit {
    let b = c.t.b;
    let foot = foot_h(c, rec, error);
    let shown = (height - foot - MID_TOP).max(0.0);
    let content = middle(&c.measuring(), &mut vec![], v, MID_TOP + 1.0);
    let scroll = scroll.clamp(0.0, (content - shown).max(0.0));
    c.fill_rr((0.0, 0.0, W, height), 0.0, op(b.bg));
    middle(c, places, v, MID_TOP + 1.0 - scroll);
    // The title area covers what scrolled above the middle; the footer what is below it.
    c.fill_rr((0.0, 0.0, W, MID_TOP), 0.0, op(b.bg));
    c.text(SIDE, 16.0, 32.0, TITLE, F::Heading, b.tx);
    c.text(SIDE, 52.0, 20.0, "Every des-k shortcut and gesture, as set now. Esc closes this.", F::Body, b.tx2);
    if scroll > 0.5 {
        c.rule(0.0, MID_TOP, W, op(b.line));
    }
    footer(c, places, v, rec, error, height - foot, height);
    if content > shown + 0.5 {
        let th = ((shown - 8.0) * shown / content).max(24.0);
        let ty = MID_TOP + 4.0 + scroll / (content - shown) * (shown - 8.0 - th);
        c.fill_rr((W - 5.0, ty, 2.0, th), 1.0, op(b.tx3));
    }
    Fit { height, content, shown }
}

/// One button, filling its window: the backdrop, the pill, the focus ring.
pub(super) fn paint_button(c: &Canvas, p: &Place, (x, y, w, h): Bx, pressed: bool, focus: bool) {
    let b = c.t.b;
    let fill = if pressed { mix(b.card, b.tx, 0.12) } else { b.card };
    c.boxed((x, y, w, h), 6.0, if c.t.hc { b.bg } else { fill }, Some(b.line));
    let tw = c.width(p.label, F::Body);
    c.text(x + (w - tw) / 2.0, y, h, p.label, F::Body, b.tx);
    if focus {
        c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), 9.0, 2.0, op(b.tx));
    }
}
