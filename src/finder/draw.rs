// The finder's drawing: each row's columns, its place and chip, the notes and the key line.
use super::*;
use super::rank::split_marks;

/// One column of a row: `text` with the letters at `marks` in 600 and `mark` color, cut with …
/// to `w` (DT_END_ELLIPSIS works on one font only).
#[allow(clippy::too_many_arguments)]
fn column(c: &Canvas, x: f32, y: f32, w: f32, text: &str, marks: &[usize], color: u32, mark: u32) {
    let mut runs: Vec<(String, bool)> = vec![];
    for (i, ch) in text.chars().enumerate() {
        let on = marks.binary_search(&i).is_ok();
        match runs.last_mut() {
            Some((t, b)) if *b == on => t.push(ch),
            _ => runs.push((ch.to_string(), on)),
        }
    }
    let font = |on: bool| if on { F::Bold } else { F::Body };
    let total: f32 = runs.iter().map(|(t, on)| c.width(t, font(*on))).sum();
    let room = if total <= w { w } else { w - c.width("…", F::Body) };
    let mut cx = x;
    for (t, on) in &runs {
        let (f, ink) = (font(*on), if *on { mark } else { color });
        let tw = c.width(t, f);
        if cx + tw <= x + room {
            c.text(cx, y, ROW, t, f, ink);
            cx += tw;
            continue;
        }
        // The cut falls in this run: as many letters as fit, then the ….
        let chars: Vec<char> = t.chars().collect();
        let part = |n: usize| chars[..n].iter().collect::<String>();
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if cx + c.width(&part(mid), f) <= x + room { lo = mid } else { hi = mid - 1 }
        }
        let kept = part(lo);
        let kept = kept.trim_end();
        c.text(cx, y, ROW, kept, f, ink);
        c.text(cx + c.width(kept, f), y, ROW, "…", F::Body, color);
        return;
    }
}

/// A screen's name and `then` cut to `w`: a twin's " #n" stays (AC-FND-3).
fn fit_place(c: &Canvas, name: &str, then: &str, w: f32) -> String {
    let whole = format!("{name}{then}");
    if c.width(&whole, F::Small) <= w {
        return whole;
    }
    let (base, end) = match name.rsplit_once(" #") {
        Some((base, n)) if !n.is_empty() && n.chars().all(|d| d.is_ascii_digit()) => (base, format!(" #{n}{then}")),
        _ => (name, then.to_string()),
    };
    c.fit(base, F::Small, w - c.width(&end, F::Small)) + &end
}

/// A screen's name cut as `fit_place` cuts it, or nothing when fewer than 4 of its letters
/// would show: a stub names no screen (spec 0023 handoff fitScreen).
fn fit_screen(c: &Canvas, screen: &str, w: f32) -> String {
    let cut = fit_place(c, screen, "", w);
    match cut.find('…') {
        Some(i) if cut != screen && cut[..i].chars().count() < 4 => String::new(),
        _ => cut,
    }
}

/// A named row's place column (spec 0023 AC-8): its workspace's name, then the rest,
/// " · <screen>" when a screen is shown and " · unplugged" for one that left. Too wide: the
/// screen is cut, then dropped, then the name is cut; " · unplugged" always stays.
fn named_place(c: &Canvas, name: &str, screen: Option<&str>, unplugged: bool, w: f32) -> (String, String) {
    let tail = if unplugged { " · unplugged" } else { "" };
    if let Some(screen) = screen {
        let rest = format!(" · {screen}{tail}");
        if c.width(name, F::Small) + c.width(&rest, F::Small) <= w {
            return (name.to_string(), rest);
        }
        let screen = fit_screen(c, screen, w - c.width(&format!("{name} · "), F::Small) - c.width(tail, F::Small));
        if !screen.is_empty() {
            return (name.to_string(), format!(" · {screen}{tail}"));
        }
    }
    (c.fit_name(name, F::Small, w - c.width(tail, F::Small)), tail.to_string())
}

/// A row's workspace chip, its right edge at CHIP_RIGHT: full when the window is not hidden,
/// soft (40%) when des-k hid it, a pin when pinned, a grey outline when its screen just left.
fn chip(c: &Canvas, r: &Row, y: f32, chosen: bool, bg: u32) {
    let (b, hc) = (c.t.b, c.t.hc);
    let wi = (r.workspace.max(1) - 1).min(8);
    let color = b.ws[wi];
    let (label, f) = if r.pinned { ("\u{E718}".to_string(), F::IconSmall) } else { (r.workspace.to_string(), F::SmallBold) };
    let tw = c.width(&label, f);
    let w = (tw + 10.0).max(20.0);
    let at = (CHIP_RIGHT - w, y + (ROW - 18.0) / 2.0, w, 18.0);
    let full = r.on_screen && !r.unplugged;
    let soft = !r.on_screen && !r.unplugged;
    let (fill, edge, ink) = match (hc, chosen) {
        // High contrast: a full chip in Highlight, a soft one outlined; on the chosen row both
        // turn to HighlightText, and an unplugged one is filled like a full one.
        (true, true) if soft => (bg, Some(b.on_acc), b.on_acc),
        (true, true) => (b.on_acc, None, b.acc),
        (true, false) if full => (color, None, b.on_ws[wi]),
        (true, false) if soft => (bg, Some(b.tx), b.tx),
        (true, false) => (bg, Some(b.tx3), b.tx2),
        _ if full => (color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)), b.on_ws[wi]),
        _ if soft => (mix(b.card, color, 0.4), Some(mix(b.card, color, 0.7)), b.tx),
        _ => (bg, Some(b.tx3), b.tx2),
    };
    c.boxed(at, 4.0, fill, edge);
    c.text(at.0 + (w - tw) / 2.0, at.1, 18.0, &label, f, ink);
}

fn row(c: &Canvas, s: &Session, r: &Row, y: f32, chosen: bool, hover: bool) {
    let (b, hc) = (c.t.b, c.t.hc);
    let closing = s.closing.contains(&r.hwnd);
    let at = (ROW_X, y, ROW_W, ROW);
    let bg = match () {
        _ if chosen && hc => b.acc,
        _ if chosen => mix(b.card, b.acc, 0.10),
        _ if hover && !hc => b.track,
        _ => b.card,
    };
    if hover && hc && !chosen {
        c.boxed(at, 6.0, bg, Some(b.acc));
    } else if bg != b.card {
        c.fill_rr(at, 6.0, op(bg));
    }
    if chosen {
        // The settings side list's bar.
        c.fill_rr((ROW_X, y + 10.0, 3.0, 16.0), 1.5, op(if hc { b.on_acc } else { b.acc }));
    }
    let ink = |color: u32| if hc && chosen { b.on_acc } else { color };
    // A window Ctrl+W was sent to fades until it goes.
    let fade = |color: u32| if closing { mix(bg, color, 0.45) } else { color };
    let icon_y = y + (ROW - ICON) / 2.0;
    c.icon(ROW_X + ROW_PAD, icon_y, ICON, r.icon);
    if closing {
        c.fill_rr((ROW_X + ROW_PAD, icon_y, ICON, ICON), 0.0, alpha(bg, 0.55));
    }
    let (pm, tm) = split_marks(r);
    column(c, PROGRAM_X, y, PROGRAM_W, &r.program, &pm, fade(ink(b.tx2)), fade(ink(b.tx)));
    column(c, TITLE_X, y, TITLE_W, &r.title, &tm, fade(ink(b.tx)), fade(ink(b.tx)));
    // The workspace's name in the text color, the rest grey, right aligned together.
    let (name, place) = match () {
        _ if closing => (String::new(), "Closing…".to_string()),
        _ if !r.ws_name.is_empty() => named_place(c, &r.ws_name, (r.unplugged || s.screens > 1).then_some(r.place.as_str()), r.unplugged, SCREEN_W),
        _ if r.unplugged => (String::new(), fit_place(c, &r.place, " · unplugged", SCREEN_W)),
        _ if s.screens > 1 => (String::new(), fit_place(c, &r.place, "", SCREEN_W)),
        _ => (String::new(), String::new()),
    };
    let place_x = SCREEN_RIGHT - c.width(&place, F::Small);
    c.text(place_x - c.width(&name, F::Small), y, ROW, &name, F::Small, ink(b.tx));
    c.text(place_x, y, ROW, &place, F::Small, ink(b.tx2));
    if !closing {
        chip(c, r, y, chosen, bg);
    }
}

/// A key line item: its key caps, then what it does. Returns where it ends.
fn key_item(c: &Canvas, x: f32, y: f32, keys: &[&str], label: &str, grey: bool) -> f32 {
    let b = c.t.b;
    let fade = |color: u32| if grey { mix(b.band, color, 0.45) } else { color };
    let ky = y + (FOOT - 20.0) / 2.0;
    let mut x = x;
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            x += 4.0;
        }
        let tw = c.width(k, F::Small);
        let w = (tw + 12.0).max(20.0);
        c.fill_rr((x, ky + 1.0, w, 20.0), 4.0, op(fade(b.key_edge))); // its 1 px shadow below
        c.boxed((x, ky, w, 20.0), 4.0, fade(b.key), Some(fade(b.key_edge)));
        c.text(x + (w - tw) / 2.0, ky, 20.0, k, F::Small, fade(b.tx));
        x += w;
    }
    x += 8.0;
    c.text(x, y, FOOT, label, F::Small, fade(b.tx2));
    x + c.width(label, F::Small)
}

/// The whole finder at 0, 0. `typed` paints the search text and caret as the EDIT would, for
/// the render tests; on screen the EDIT draws them itself.
pub(super) fn draw(c: &Canvas, s: &Session, typed: Option<&str>) {
    let (b, hc) = (c.t.b, c.t.hc);
    let height = s.height();
    c.fill_rr((0.0, 0.0, WIDTH, height), 0.0, op(b.card));
    // The search box.
    c.text(SIDE, 0.0, BOX - 1.0, "\u{E721}", F::Icon, b.tx2);
    if let Some(n) = s.count() {
        c.text(WIDTH - SIDE - c.width(&n, F::Small), 0.0, BOX - 1.0, &n, F::Small, b.tx2);
    }
    if let Some(q) = typed {
        let y = (BOX - 1.0 - TEXT_H) / 2.0;
        let caret = if q.is_empty() {
            c.text(TEXT_X + 1.0, y, TEXT_H, PLACEHOLDER, F::Body, b.tx2);
            TEXT_X
        } else {
            c.text(TEXT_X, y, TEXT_H, q, F::Body, b.tx);
            TEXT_X + c.width(q, F::Body) + 1.0
        };
        c.fill_rr((caret, y, 1.0, TEXT_H), 0.0, op(b.tx));
    }
    c.rule(0.0, BOX - 1.0, WIDTH, op(b.line));
    // The list.
    let top = BOX + LIST_PAD;
    match s.empty_text() {
        Some("No match") => {
            c.text(ROW_X + 40.0, top + 8.0, 20.0, "No match", F::Body, b.tx2);
            c.text(ROW_X + 40.0, top + 28.0, 16.0, "Windows on Windows' own desktops and paused Store apps aren't listed.", F::Small, b.tx2);
        }
        Some(e) => c.text(ROW_X + 40.0, top, ROW, e, F::Body, b.tx2),
        None => {}
    }
    for (k, &(i, _)) in s.shown.iter().enumerate().skip(s.scroll).take(ROWS) {
        row(c, s, &s.rows[i], top + (k - s.scroll) as f32 * ROW, k == s.selected, s.hover == Some(k));
    }
    let n = s.shown.len();
    if n > ROWS {
        let track = ROWS as f32 * ROW - 12.0;
        let h = track * ROWS as f32 / n as f32;
        let y = top + (track - h) * s.scroll as f32 / (n - ROWS) as f32;
        c.fill_rr((WIDTH - 6.0, y, 3.0, h), 1.5, op(b.tx3));
    }
    // The note.
    if let Some(note) = s.note {
        let (icon, line, under, warn) = note.words(&s.main);
        let y = BOX + s.list_h();
        c.boxed((8.0, y, WIDTH - 16.0, s.note_h() - 8.0), 6.0, b.bg, Some(b.line));
        c.text(20.0, y + 8.0, 20.0, icon, F::Icon, if warn { b.warn } else { b.tx2 });
        c.text(48.0, y + 8.0, 20.0, line, F::Bold, b.tx);
        if let Some(u) = under {
            c.text(48.0, y + 28.0, 16.0, &c.fit(&u, F::Small, WIDTH - 16.0 - 52.0), F::Small, b.tx2);
        }
    }
    // The key line; with no rows only Esc does anything.
    let y = height - FOOT;
    c.fill_rr((0.0, y, WIDTH, FOOT), 0.0, op(if hc { b.card } else { b.band }));
    c.rule(0.0, y, WIDTH, op(b.line));
    let none = s.shown.is_empty();
    let mut x = SIDE;
    for (keys, label) in [(&["Enter"][..], "Go to"), (&["Shift", "Enter"][..], "Bring here"), (&["Ctrl", "W"][..], "Close window")] {
        x = key_item(c, x, y, keys, label, none) + 20.0;
    }
    let esc = key_item(&c.measuring(), 0.0, y, &["Esc"], "Cancel", false);
    key_item(c, WIDTH - SIDE - esc, y, &["Esc"], "Cancel", false);
}

pub(super) fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // Drawn off screen first: the list repaints on every key, and would flicker otherwise.
        ui::canvas::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
            SESSION.with_borrow(|s| {
                if let Some(s) = s {
                    let fonts = ui::fonts::fonts(s.scale);
                    draw(&Canvas::new(mem, true, s.scale, ui::theme(), &fonts), s, None);
                }
            });
        });
        let _ = EndPaint(hwnd, &ps);
    }
}


#[cfg(test)]
mod tests;
