// Where it shows: the indicator's spot or custom place per screen, its picture and what each
// change writes (spec 0020).
use super::*;
use crate::settings_page::choosers::{ScreenChoice, after_table, chosen};
use crate::settings_page::rows::stepped;
use super::colors::path_of;
use super::indicator::show;

// The place rows' keys; MODE is the page's Spots or Custom, never a key in the file.
pub(crate) const SPOT: &str = "indicator.spot";

pub(crate) const GAP: &str = "indicator.gap";
pub(crate) const ACROSS: &str = "indicator.x";
pub(crate) const DOWN: &str = "indicator.y";
pub(in crate::settings_page) const MODE: &str = "indicator.mode";
const SPOT_NAMES: [&str; 10] = ["Top left", "Top", "Top right", "Left", "Center", "Right", "Bottom left", "Bottom", "Bottom right", "Custom"];

pub(in crate::settings_page) fn is_place(key: &str) -> bool {
    matches!(key, SPOT | GAP | ACROSS | DOWN | MODE)
}

fn spot_index(spot: settings::Spot) -> usize {
    settings::SPOTS.iter().position(|(_, s)| *s == spot).unwrap_or(4)
}

/// A place key's name inside a table: "spot", "gap", "x" or "y".
fn leaf(key: &str) -> &str {
    key.rsplit('.').next().unwrap_or(key)
}

/// A place key's value as a number: a spot by its place in SPOTS.
fn pos_get(p: settings::Pos, key: &str) -> f64 {
    match leaf(key) {
        "spot" => spot_index(p.spot) as f64,
        "gap" => p.gap as f64,
        "x" => p.x,
        _ => p.y,
    }
}

fn ov_get(o: &settings::PosOverride, key: &str) -> Option<f64> {
    match leaf(key) {
        "spot" => o.spot.map(|s| spot_index(s) as f64),
        "gap" => o.gap.map(f64::from),
        "x" => o.x,
        _ => o.y,
    }
}

fn place_value(key: &str, v: f64) -> toml_edit::Value {
    match leaf(key) {
        "spot" => settings::SPOTS[v as usize].0.into(),
        _ if v.fract() == 0.0 => (v as i64).into(),
        _ => v.into(),
    }
}

/// The place a screen uses: its own keys over All screens' (spec 0020 AC-9).
fn pos_of(s: &Settings, screen: Option<&str>) -> settings::Pos {
    let d = s.indicator_pos;
    screen.and_then(|id| s.indicator_screens.get(id)).map_or(d, |o| o.over(d))
}

/// What setting a place key to `v` writes, None when nothing changes. All screens writes
/// [indicator]. A screen writes its own table, but a value equal to All screens' takes that
/// key out instead, so the screen follows All screens again; never the table's last key, so
/// walking the spots or stepping a number can't drop a kept place.
pub(crate) fn place_edit(s: &Settings, screen: Option<&str>, key: &str, v: f64) -> Option<Edit> {
    if pos_get(pos_of(s, screen), key) == v && screen.is_none_or(|id| s.indicator_screens.get(id).is_none_or(|o| ov_get(o, key).is_some())) {
        return None;
    }
    let Some(id) = screen else { return Some(Edit::At(path_of(key), place_value(key, v))) };
    let path = vec!["indicator".to_string(), "screens".into(), id.to_string(), leaf(key).to_string()];
    let all = pos_get(s.indicator_pos, key);
    let o = s.indicator_screens.get(id);
    let own = o.and_then(|o| ov_get(o, key));
    let keys = o.map_or(0, |o| [o.spot.is_some(), o.gap.is_some(), o.x.is_some(), o.y.is_some()].iter().filter(|b| **b).count());
    match () {
        _ if v != all => Some(Edit::At(path, place_value(key, v))),
        _ if own.is_none() => None,  // it follows All screens already
        _ if keys > 1 => Some(Edit::Remove(path)),
        _ if own == Some(v) => None, // its last key, kept
        _ => Some(Edit::At(path, place_value(key, v))),
    }
}

impl Ctx<'_, '_> {
    // ---- Where it shows (spec 0020 AC-13) ----

    /// The indicator's place, for All screens or one screen: the chooser, the picture with
    /// Spots or Custom, the distance, and Across and Down for Custom.
    pub(super) fn place(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let (choices, i) = chosen(self.w, self.s);
        let ch = choices[i].clone();
        self.header(y, "Where it shows", None, "");
        let (lead, who) = match &ch.id {
            None => ("For ", "all screens".to_string()),
            Some(_) => ("For ", ch.name.clone()),
        };
        let rw = self.cx.c.width(lead, F::Small) + self.cx.c.width(&who, F::SmallBold);
        self.cx.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[(lead, F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
        let mut h = 38.0;
        if choices.len() > 2 {
            h += self.chooser_row(y + h, &choices, i);
            self.sep(y + h);
        }
        let id = ch.id.as_deref();
        h += self.place_row(y + h, &ch);
        self.sep(y + h);
        h += self.place_num(GAP, y + h, &ch);
        if pos_of(self.s, id).spot == settings::Spot::Custom {
            for key in [ACROSS, DOWN] {
                self.sep(y + h);
                h += self.place_num(key, y + h, &ch);
            }
        }
        h
    }

    /// The words a place value is shown in: "Top right", "24 px", "50 %".
    fn place_shown(key: &str, v: f64) -> String {
        match leaf(key) {
            "spot" => SPOT_NAMES.get(v as usize).copied().unwrap_or("Center").to_string(),
            "gap" => format!("{} px", v.round()),
            _ => format!("{} %", v.round()),
        }
    }

    /// The problem key a place row shows: [indicator]'s, or the chosen screen table's.
    fn place_problem(key: &str, id: Option<&str>) -> String {
        id.map_or(key.to_string(), |id| format!("indicator.screens.\"{id}\".{}", leaf(key)))
    }

    /// The end of a place row's caption: All screens says its default and Reset; a screen says
    /// All screens' value and Reset for a key it sets, From All screens for one it doesn't.
    fn place_pieces(&self, key: &'static str, ch: &ScreenChoice) -> Vec<P> {
        let bad = settings::store::problems().iter().any(|p| p.key == Self::place_problem(key, ch.id.as_deref()));
        match &ch.id {
            None => {
                let (v, d) = (pos_get(self.s.indicator_pos, key), pos_get(DEFAULTS.indicator_pos, key));
                if v == d && !bad {
                    return vec![];
                }
                vec![t(format!(" Default: {}.", Self::place_shown(key, d))), P::Link(" Reset".into(), Ctl::Reset(key))]
            }
            Some(id) => {
                let own = self.s.indicator_screens.get(id).and_then(|o| ov_get(o, key));
                if own.is_none() && !bad {
                    return vec![t(" From All screens.")];
                }
                vec![t(format!(" All screens: {}.", Self::place_shown(key, pos_get(self.s.indicator_pos, key)))), P::Link(" Reset".into(), Ctl::Reset(key))]
            }
        }
    }

    fn place_bad(&self, key: &str, ch: &ScreenChoice) -> Option<NoteSpec> {
        let problems = settings::store::problems();
        let p = problems.iter().find(|p| p.key == Self::place_problem(key, ch.id.as_deref()))?;
        let (title, fix) = match &ch.id {
            None => {
                let d = Self::place_shown(key, pos_get(DEFAULTS.indicator_pos, key));
                (format!("des-k can't use this value, so it uses {d}"), format!(" Change it here, or Reset puts {d} back in the file."))
            }
            Some(_) => (format!("des-k can't use this value, so {} uses All screens' {}", ch.name, Self::place_shown(key, pos_get(self.s.indicator_pos, key))), " Change it here, or Reset removes the line.".into()),
        };
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title, lines: vec![P::Mono(p.message.clone()), t(fix)], buttons: vec![], group: 1 })
    }

    /// Place: what it is in words, Spots or Custom, then the picture in the chosen screen's
    /// shape with the name, size and Show it there under it. The picture is drawn first so it
    /// is the stop before Spots or Custom (Reset after both).
    fn place_row(&mut self, y: f32, ch: &ScreenChoice) -> f32 {
        let b = self.cx.c.t.b;
        let id = ch.id.as_deref();
        let na = !self.s.indicator_enabled;
        let mut pos = pos_of(self.s, id);
        if let (Some((x, yy)), settings::Spot::Custom) = (self.w.dragging, pos.spot) {
            (pos.x, pos.y) = (x, yy);
        }
        let custom = pos.spot == settings::Spot::Custom;
        let mut cap = vec![t(match pos.spot {
            settings::Spot::Custom => format!("Its center {} % across and {} % down. Click or drag on the screen to move it; Spots goes to the nearest spot.", pos.x.round(), pos.y.round()),
            settings::Spot::Center => "Center. Click a spot on the screen.".to_string(),
            s => format!("{}, {} px from the edges. Click a spot on the screen.", SPOT_NAMES[spot_index(s)], pos.gap),
        })];
        if custom && self.kept_inside(ch, pos) {
            cap.push(t(" des-k keeps the card inside the screen, so it sits a little further in."));
        }
        cap.extend(self.place_pieces(SPOT, ch));
        let note = self.place_bad(SPOT, ch);
        let words = ["Spots", "Custom"];
        let size = (self.cx.seg_w(&words), 28.0);
        let rh = self.measured(|c| c.row(y, "Place", &cap, size, |_, _, _| {}, note.as_ref()));
        let (full, work, scale, label) = self.shape(ch);
        let pw = 352.0;
        let ph = (pw * (full.bottom - full.top) as f32 / (full.right - full.left).max(1) as f32).clamp(120.0, 300.0);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + rh;
        let well_h = 16.0 + ph + 12.0 + 32.0 + 12.0;
        self.cx.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        self.picture((wx + (ww - pw) / 2.0, wy + 16.0, pw, ph), (full, work, scale), pos, na);
        let help = format!("{} {}", SPOT_NAMES[spot_index(pos.spot)], "Left and Right move it.");
        self.cx.say(Ctl::Picture, UIA_SpinnerControlTypeId, "Place", &help, na || self.cx.ro, (None, Some(SPOT_NAMES[spot_index(pos.spot)].into())));
        self.cx.say(Ctl::Pick(MODE), UIA_ComboBoxControlTypeId, "Spots or Custom", "", na || self.cx.ro, (None, Some(words[custom as usize].into())));
        self.row(y, "Place", &cap, size, |c, x, y| c.cx.seg(x, y, &words, Some(custom as usize), na, |i| Ctl::Choice(MODE, i), Ctl::Pick(MODE), None), note.as_ref());
        let by = wy + 16.0 + ph + 12.0;
        let sw = self.cx.btn_w("Show it there", None);
        let line = self.cx.c.fit(&label, F::Small, ww - 24.0 - sw - 12.0);
        self.cx.c.text(wx + 12.0, by, 32.0, &line, F::Small, b.tx2);
        let off = na || (ch.id.is_some() && ch.present.is_none());
        self.cx.button(Ctl::ShowThere, wx + ww - 12.0 - sw, by, "Show it there", None, off, 1);
        rh + well_h + 16.0
    }

    /// The chosen screen's whole rect, work area, scale and the line under the picture. All
    /// screens and a screen not plugged in: 16:9 with the taskbar at the bottom.
    fn shape(&self, ch: &ScreenChoice) -> (RECT, RECT, f32, String) {
        let full = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let plain = (full, RECT { bottom: 1032, ..full }, 1.0);
        let present = ch.present.and_then(|i| self.w.pics.view.present.get(i));
        let (full, work, scale) = present.map_or(plain, |p| {
            let sc = self.w.pics.screens.iter().find(|s| s.device == p.device).map_or(1.0, |s| s.scale as f32 / 100.0);
            (p.full, p.work, sc)
        });
        let label = match (&ch.id, present) {
            (None, _) => "Any screen: des-k fits the place to each one. Show it there uses the screen this window is on.".to_string(),
            (Some(_), Some(p)) => format!("{} · {} × {} · {}%", ch.name, p.full.right - p.full.left, p.full.bottom - p.full.top, (scale * 100.0).round()),
            (Some(_), None) if ch.side == "Unknown screen" => format!("{} · des-k hasn't seen this screen", ch.name),
            (Some(_), None) => format!("{} · not plugged in", ch.name),
        };
        (full, work, scale, label)
    }

    /// Whether des-k moves a Custom card inward on this screen: indicator::place with the plain
    /// card's size at the screen's scale, against the unclamped center. All screens asks for
    /// the screen this window is on.
    fn kept_inside(&self, ch: &ScreenChoice, pos: settings::Pos) -> bool {
        let view = &self.w.pics.view;
        let p = match &ch.id {
            Some(_) => ch.present.and_then(|i| view.present.get(i)),
            None => self.w.pics.here.as_ref().and_then(|d| view.present.iter().find(|p| p.device.trim_start_matches(r"\\.\") == d.as_str())),
        };
        let Some(p) = p else { return false };
        let scale = self.w.pics.screens.iter().find(|s| s.device == p.device).map_or(1.0, |s| s.scale as f32 / 100.0);
        let card = crate::indicator::Card::new(0, self.s.workspaces, None);
        let (cw, chh) = crate::indicator::draw_preview(&self.cx.c.measuring(), 0.0, 0.0, self.s.indicator_style, &card);
        let (w, h) = ((cw * scale).round() as i32, (chh * scale).round() as i32);
        let (x, y) = crate::indicator::place(p.work, w, h, pos, scale);
        let r = p.work;
        let ux = r.left as f64 + pos.x / 100.0 * (r.right - r.left) as f64;
        let uy = r.top as f64 + pos.y / 100.0 * (r.bottom - r.top) as f64;
        ((x + w / 2) as f64 - ux).abs() > 1.5 || ((y + h / 2) as f64 - uy).abs() > 1.5
    }

    /// The place picture: a screen in the chosen shape, the taskbar on its edge, and the card
    /// where des-k puts it, at a fixed 52 × 16 so it reads; the distance drawn to scale. Spots:
    /// the other eight dashed, each third across and down picks one. Custom: the card's center
    /// on two dotted lines, and the whole screen takes clicks and drags.
    fn picture(&mut self, (x, y, pw, ph): Bx, (full, work, scale): (RECT, RECT, f32), pos: settings::Pos, na: bool) {
        let b = self.cx.c.t.b;
        let hc = self.cx.c.t.hc;
        self.cx.c.fill_rr((x, y, pw, ph), 6.0, op(b.bez));
        let (sx, sy, sw, sh) = (x + 4.0, y + 4.0, pw - 8.0, ph - 8.0);
        let screen = if hc { b.card } else { mix(b.card, b.tx, 0.12) };
        self.cx.c.fill_rr((sx, sy, sw, sh), 2.0, op(screen));
        let k = sw / (full.right - full.left).max(1) as f32;
        let kv = sh / (full.bottom - full.top).max(1) as f32;
        let wl = sx + (work.left - full.left) as f32 * k;
        let wt = sy + (work.top - full.top) as f32 * kv;
        let wr = sx + (work.right - full.left) as f32 * k;
        let wb = sy + (work.bottom - full.top) as f32 * kv;
        let bar = if hc { b.tx } else { mix(b.card, b.tx, 0.40) };
        for r in [(sx, sy, sw, wt - sy), (sx, wb, sw, sy + sh - wb), (sx, sy, wl - sx, sh), (wr, sy, sx + sw - wr, sh)] {
            if r.2 > 0.5 && r.3 > 0.5 {
                self.cx.c.fill_rr(r, 0.0, op(bar));
            }
        }
        let (cw, chh) = (52.0, 16.0);
        let gap = (pos.gap as f32 * scale * k).max(2.0);
        let at = |spot: usize| -> (f32, f32) {
            let (x0, xc, x1) = (wl + gap, (wl + wr - cw) / 2.0, wr - gap - cw);
            let (y0, yc, y1) = (wt + gap, (wt + wb - chh) / 2.0, wb - gap - chh);
            let (px, py) = match spot {
                9 => (wl + pos.x as f32 / 100.0 * (wr - wl) - cw / 2.0, wt + pos.y as f32 / 100.0 * (wb - wt) - chh / 2.0),
                4 => (xc, yc),
                s => ([x0, xc, x1][s % 3], [y0, yc, y1][s / 3]),
            };
            (px.clamp(wl, (wr - cw).max(wl)), py.clamp(wt, (wb - chh).max(wt)))
        };
        let chosen = spot_index(pos.spot);
        let custom = chosen == 9;
        if custom {
            let (cx, cy) = at(9);
            let (mx, my) = (cx + cw / 2.0, cy + chh / 2.0);
            let line = op(if na { b.tx3 } else { b.tx2 });
            self.cx.c.dash_line((mx, wt), (mx, wb), 1.0, line);
            self.cx.c.dash_line((wl, my), (wr, my), 1.0, line);
            self.cx.c.text(mx + 4.0, wt + 1.0, 16.0, &format!("{} %", pos.x.round()), F::Small, b.tx);
            self.cx.c.text(wl + 4.0, my - 17.0, 16.0, &format!("{} %", pos.y.round()), F::Small, b.tx);
            self.cx.hit(Ctl::Picture, (wl, wt, wr - wl, wb - wt), 1);
        } else {
            for s in 0..9 {
                if s != chosen {
                    let (px, py) = at(s);
                    self.cx.c.stroke_dash((px, py, cw, chh), 2.0, 1.0, op(if na { b.tx3 } else { b.tx2 }), DashStyleDash);
                }
                let (tw, th) = ((wr - wl) / 3.0, (wb - wt) / 3.0);
                self.cx.hit(Ctl::Spot(s), (wl + (s % 3) as f32 * tw, wt + (s / 3) as f32 * th, tw, th), 1);
            }
        }
        let (px, py) = at(chosen);
        let n = self.s.workspaces;
        let a = self.active_on(&self.w.pics.here.clone().unwrap_or_default()).min(8);
        let (lit, off) = if na || hc { (b.tx3, b.tx3) } else { (b.ws[a], b.bar_off) };
        self.cx.c.boxed((px, py, cw, chh), 3.0, b.card, Some(if hc { b.tx } else { b.line }));
        self.cx.c.fill_rr((px + 3.0, py + 3.0, 10.0, 10.0), 2.0, op(lit));
        for i in 0..n.min(3) {
            self.cx.c.fill_rr((px + 17.0 + i as f32 * 10.0, py + 6.5, 8.0, 3.0), 1.5, op(if i == 0 { lit } else { off }));
        }
        self.cx.ring((px, py, cw, chh), 3.0, Ctl::Picture);
    }

    /// Distance from the edges, Across or Down: a number for the chosen screen, greyed with
    /// the indicator off, and the distance with its reason for Center and Custom.
    fn place_num(&mut self, key: &'static str, y: f32, ch: &ScreenChoice) -> f32 {
        let pos = pos_of(self.s, ch.id.as_deref());
        let (title, base, lo, hi, unit) = match key {
            GAP => ("Distance from the edges", "From the screen's edges, above the taskbar, at 100% scale; des-k scales it with each screen. 0 to 400 px.", 0, 400, "px"),
            ACROSS => ("Across", "Where the card's center sits, from the left edge. 0 to 100 %.", 0, 100, "%"),
            _ => ("Down", "Where the card's center sits, from the top edge. 0 to 100 %.", 0, 100, "%"),
        };
        let v = pos_get(pos, key).round() as i64;
        let mut cap = vec![t(base)];
        let idle = key == GAP && matches!(pos.spot, settings::Spot::Center | settings::Spot::Custom);
        if idle {
            let why = if pos.spot == settings::Spot::Center { "Center sits in the middle" } else { "Custom places it by percent" };
            cap.push(P::Why(format!(" Not in use: {why}.")));
        }
        cap.extend(self.place_pieces(key, ch));
        let note = self.place_bad(key, ch);
        let na = !self.s.indicator_enabled || idle;
        let text = format!("{v} {unit}");
        let size = (self.cx.stepper_w(&text), 32.0);
        self.cx.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, title, base, self.cx.ro || na, (None, Some(text)));
        self.row(y, title, &cap, size, |c, x, y| c.cx.stepper(Ctl::steps(key), x, y, step_words(v, unit), (v > lo, v < hi), na, false, None), note.as_ref())
    }
}

/// The screen Where it shows changes; None: All screens.
fn screen_id() -> Option<String> {
    get(|w| {
        let (list, i) = chosen(w, &settings::get());
        list[i].id.clone()
    })
    .flatten()
}

/// A place key changed from the page; nothing while the indicator is off.
pub(in crate::settings_page) fn set_place(key: &'static str, v: f64) {
    let s = settings::get();
    if !s.indicator_enabled {
        return;
    }
    if let Some(e) = place_edit(&s, screen_id().as_deref(), key, v) {
        commit(e, "Where it shows");
    }
}

/// − and + on the distance, Across or Down; a decimal in the file lands on a whole number.
pub(in crate::settings_page) fn step_place(key: &'static str, up: bool) {
    let s = settings::get();
    let pos = pos_of(&s, screen_id().as_deref());
    if key == GAP && matches!(pos.spot, settings::Spot::Center | settings::Spot::Custom) {
        return;
    }
    let (hi, by) = if key == GAP { (400, 4) } else { (100, 1) };
    let v = pos_get(pos, key);
    let n = if v.fract() != 0.0 { if up { v.ceil() } else { v.floor() } } else { stepped(v as i64, 0, hi, by, up) as f64 };
    if n != v {
        set_place(key, n);
    }
}

/// The spot nearest a Custom card: the third it is in across and down.
fn nearest(x: f64, y: f64) -> usize {
    let third = |v: f64| if v < 100.0 / 3.0 { 0 } else if v < 200.0 / 3.0 { 1 } else { 2 };
    third(y) * 3 + third(x)
}

/// Spots or Custom. Spots picks the spot nearest the card and keeps x and y in the file, so
/// Custom comes back where it was.
pub(in crate::settings_page) fn set_mode(custom: bool) {
    let pos = pos_of(&settings::get(), screen_id().as_deref());
    if custom != (pos.spot == settings::Spot::Custom) {
        set_place(SPOT, if custom { 9.0 } else { nearest(pos.x, pos.y) as f64 });
    }
}

/// Left and Right on the picture: the nine spots in reading order, stopping at the ends; with
/// Custom, the card 1 % across.
pub(in crate::settings_page) fn walk_picture(right: bool) {
    let pos = pos_of(&settings::get(), screen_id().as_deref());
    if pos.spot == settings::Spot::Custom {
        let n = if right { (pos.x.floor() + 1.0).min(100.0) } else { (pos.x.ceil() - 1.0).max(0.0) };
        set_place(ACROSS, n);
    } else {
        let i = spot_index(pos.spot);
        let n = if right { (i + 1).min(8) } else { i.saturating_sub(1) };
        if n != i {
            set_place(SPOT, n as f64);
        }
    }
}

/// Reset: All screens writes the default back, as every [indicator] row does; a screen's
/// Reset takes its key out (its last key takes the table).
pub(in crate::settings_page) fn reset_place(key: &'static str) {
    match screen_id() {
        None => commit(Edit::At(path_of(key), place_value(key, pos_get(DEFAULTS.indicator_pos, key))), "Where it shows"),
        Some(id) => {
            commit(Edit::Remove(vec!["indicator".into(), "screens".into(), id, leaf(key).into()]), "Where it shows");
            after_table(false);
        }
    }
}

/// Same as all screens: the chosen screen's whole table goes.
pub(in crate::settings_page) fn same_as_all() {
    let Some(id) = screen_id() else { return };
    commit(Edit::Remove(vec!["indicator".into(), "screens".into(), id]), "Where it shows");
    after_table(true);
}

/// Show it there: the real indicator at its place on the chosen screen; All screens shows it
/// on the screen this window is on. Off with the indicator off or a screen not plugged in.
pub(in crate::settings_page) fn show_there() {
    let s = settings::get();
    if !s.indicator_enabled {
        return;
    }
    let target = get(|w| {
        let (list, i) = chosen(w, &s);
        match (&list[i].id, list[i].present) {
            (None, _) => Some(None),
            (Some(_), Some(p)) => w.pics.view.present.get(p).map(|p| Some(p.full)),
            _ => None,
        }
    })
    .flatten();
    match target {
        Some(None) => show(),
        Some(Some(full)) => crate::ws::switch::show_indicator(unsafe { MonitorFromRect(&full, MONITOR_DEFAULTTONEAREST) }),
        None => {}
    }
}

/// A point on the place picture as whole percent of its work area, kept 0 to 100.
pub(in crate::settings_page) fn percent(at: Bx, (x, y): (f32, f32)) -> (f64, f64) {
    let p = |v: f32, o: f32, len: f32| (((v - o) / len.max(1.0)) as f64 * 100.0).round().clamp(0.0, 100.0);
    (p(x, at.0, at.2), p(y, at.1, at.3))
}

#[cfg(test)]
mod tests;
