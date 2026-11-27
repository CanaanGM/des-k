// The screen rows on the Workspaces card: each screen's tiles, the screens not plugged in, and
// the picture of your screens (spec 0012, spec 0016, spec 0021).
use super::*;
use crate::ws::screens::Moved;

impl Ctx<'_, '_> {
    /// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
    /// rename, else the id.
    pub(super) fn screen_called(&self, id: &str) -> String {
        screen_called(&self.w.pics.view, self.s, id)
    }

    /// Whether a screen can differ from the shared count: two or more rows, or a line that
    /// already does (spec 0040 AC-10).
    pub(super) fn split(&self) -> bool {
        tile_rows(self.w) > 1 || self.s.count_by_screen.values().any(|&n| n != self.s.shared_count)
    }

    /// Row `r`'s own count.
    pub(super) fn row_count(&self, r: usize) -> usize {
        row_id(self.w, r).map_or(self.s.shared_count, |id| self.s.count_for(&id))
    }

    /// What a removal moves, per screen, unplugged ones included (AC-SET-3), since there is no
    /// undo: one row's screen for its × or Left, the screens that follow the shared count for its
    /// − or Reset (spec 0040 AC-8, AC-10).
    pub(super) fn removal(&self, what: Removal) -> NoteSpec {
        let (shared, d) = (self.s.shared_count, DEFAULTS.shared_count);
        let every = if self.split() { "every screen without its own count" } else { "every screen" };
        let removals = &self.w.pics.view.removals;
        let (title, list, to, one) = match what {
            Removal::Row(r) => {
                let id = row_id(self.w, r).unwrap_or_default();
                let (n, name) = (self.s.count_for(&id), self.screen_called(&id));
                let title = if self.cx.focused(Ctl::Tiles(r)) { format!("Delete or Left removes workspace {n} from {name}") } else { format!("× removes workspace {n} from {name}") };
                let list: Vec<Moved> = removals.get(2).map(|l| l.iter().filter(|m| m.id == id).cloned().collect()).unwrap_or_default();
                (title, list, n - 1, true)
            }
            Removal::Lower => (format!("− removes workspace {shared} from {every}"), removals.first().cloned().unwrap_or_default(), shared - 1, true),
            Removal::Reset => (format!("Reset goes back to {d} and removes workspaces {} to {shared} from {every}", d + 1), removals.get(1).cloned().unwrap_or_default(), d, false),
        };
        let mut parts = vec![if one { format!("Its windows move to workspace {to}.") } else { format!("Their windows move to workspace {to}.") }];
        let win = |k: usize| if k == 1 { "1 window".to_string() } else { format!("{k} windows") };
        for m in list.iter().filter(|m| m.windows > 0 && !m.unplugged) {
            parts.push(format!("{}: {}.", m.name, win(m.windows)));
        }
        for m in list.iter().filter(|m| m.windows > 0 && m.unplugged) {
            parts.push(format!("{} (not plugged in): {}.", m.name, win(m.windows)));
        }
        for m in list.iter().filter(|m| m.windows == 0 && !m.unplugged) {
            parts.push(format!("{}: nothing moves.", m.name));
        }
        parts.push(format!("A screen showing a removed workspace switches to {to}. There is no undo."));
        NoteSpec { tone: Tone::Warn, icon: I_WARN, title, lines: vec![t(parts.join(" "))], buttons: vec![], group: 1 }
    }

    pub(super) fn name_of(&self, sc: &crate::ws::views::ScreenInfo) -> String {
        self.w.pics.view.names.iter().find(|(d, _)| *d == sc.device).map_or_else(|| sc.name.clone(), |(_, n)| n.clone())
    }

    /// Screen `device`'s workspace names, as its indicator shows them (spec 0023 build
    /// conditions): the same lookup as `ws::views::names`, by the id the page's view already holds,
    /// since the page can lay out from inside a switch and the ws lock is not reentrant.
    pub(super) fn names_here(&self, device: &str) -> Vec<String> {
        let Some(p) = self.w.pics.view.present.iter().find(|p| p.device == device) else { return vec![] };
        (0..self.s.count_for(&p.id)).map(|i| self.s.ws_name(&p.id, i).unwrap_or_default().to_string()).collect()
    }

    pub(super) fn active_on(&self, device: &str) -> usize {
        self.w.pics.view.active.iter().find(|(d, _)| d == device).map_or(0, |(_, a)| *a)
    }

    fn tiles_w(&self, n: usize, add: bool) -> f32 {
        let tw = if n > 6 { 40.0 } else { 48.0 };
        n as f32 * tw + (n as f32 - 1.0) * 6.0 + if add { 6.0 + tw } else { 0.0 }
    }

    /// A screen's name on its row, and "its own count" when its line differs from the shared
    /// count (spec 0040 AC-10).
    fn row_title(&self, id: &str, name: String) -> String {
        if self.s.follows_shared(id) { name } else { format!("{name} · its own count") }
    }

    /// One present screen: its name and size, and its own workspaces as tiles (AC-SET-3).
    pub(super) fn screen_row(&mut self, y: f32, row: usize, sc: &crate::ws::views::ScreenInfo, gone: &[usize]) -> f32 {
        let id = row_id(self.w, row).unwrap_or_default();
        let n = self.s.count_for(&id);
        let name = self.row_title(&id, self.name_of(sc));
        let a = self.active_on(&sc.device);
        let (rw, rh) = (sc.rect.right - sc.rect.left, sc.rect.bottom - sc.rect.top);
        let cap = vec![t(format!("{rw} × {rh} · {}% · on workspace {}", sc.scale, a + 1))];
        let counts: Vec<usize> = (0..n).map(|ws| self.w.pics.view.windows.iter().filter(|(d, k, _)| *d == sc.device && *k == ws).count()).collect();
        let add = n < 9;
        let tw = self.tiles_w(n, add);
        let label = self.name_of(sc);
        let mut h = self.row(y, &name, &cap, (tw, 34.0), |c, x, y| c.tiles(x, y, &counts, (row, &label), Some(a), gone, add), None);
        h = h.max(60.0);
        h
    }

    /// The tiles of card row `row` (named `label`): a workspace each in its color, with up to
    /// three window marks; the one in view ringed; × on the last one while you point at it, +
    /// after them. `now`: a plugged in screen's workspace in view; its tiles open the color picker.
    /// A screen not plugged in has grey tiles, and its own + and × too (spec 0040 AC-8).
    #[allow(clippy::too_many_arguments)]
    fn tiles(&mut self, x: f32, y: f32, counts: &[usize], (row, label): (usize, &str), now: Option<usize>, gone: &[usize], add: bool) {
        let t = self.cx.c.t;
        let b = t.b;
        let n = counts.len();
        let tw = if n > 6 { 40.0 } else { 48.0 };
        let away = now.is_none();
        let device = if away { String::new() } else { self.w.pics.screens.get(row).map(|s| s.device.clone()).unwrap_or_default() };
        for (i, &k) in counts.iter().enumerate() {
            let tx = x + i as f32 * (tw + 6.0);
            let bx = (tx, y, tw, 34.0);
            let cur = now == Some(i);
            let (fill, ink) = match () {
                _ if away => (b.track, b.tx2),
                _ if t.hc && cur => (b.hi, b.on_acc),
                _ if t.hc => (b.card, b.tx),
                _ => (b.ws[i], b.on_ws[i]),
            };
            if cur && !t.hc {
                self.cx.c.fill_rr((tx - 4.0, y - 4.0, tw + 8.0, 42.0), 8.0, op(b.tx));
                self.cx.c.fill_rr((tx - 2.0, y - 2.0, tw + 4.0, 38.0), 6.0, op(b.card));
            }
            if away || t.hc {
                self.cx.c.boxed(bx, 4.0, fill, Some(if away { b.line } else { b.tx }));
            } else {
                self.cx.c.fill_rr(bx, 4.0, op(fill));
            }
            self.cx.c.text(tx + 6.0, y + 34.0 - 19.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
            let marks = k.min(3);
            for m in 0..marks {
                let mx = tx + tw - 4.0 - (marks - m) as f32 * 10.0 + 2.0;
                let col = if away { b.tx3 } else if t.hc { if cur { b.on_acc } else { b.tx } } else { b.mk_ws[i] };
                self.cx.c.fill_rr((mx, y + 4.0, 8.0, 10.0), 2.0, ui::color::alpha(col, if away || t.hc { 1.0 } else { 0.9 }));
            }
            if gone.contains(&i) {
                self.cx.c.stroke_dash((tx + 1.0, y + 1.0, tw - 2.0, 32.0), 3.0, 2.0, op(b.tx), DashStyleDash);
            }
            // A grey tile opens no picker: pointing at the last one focuses its row, for the ×.
            let ctl = if away { Ctl::Tiles(row) } else { Ctl::WsTile(row, i) };
            if !away {
                if self.cx.hovered(ctl) && self.picks() && !cur {
                    self.cx.cue(bx, ink);
                }
                // Before the ×'s hit, so a click inside the tile opens the picker and never removes
                // a workspace (spec 0021 AC-1).
                self.cx.hit(ctl, bx, 1);
                self.out.geo.tiles.push((device.clone(), i, bx));
            } else if i + 1 == n {
                self.cx.hit(ctl, bx, 1);
            }
            if i + 1 == n && n > 2 && !self.cx.ro {
                let show = self.cx.hovered(ctl) || self.cx.hovered(Ctl::TileRemove(row)) || self.cx.focused(Ctl::Tiles(row));
                if show {
                    let (cx, cy) = (tx + tw + 2.0, y - 2.0);
                    let hov = self.cx.hovered(Ctl::TileRemove(row));
                    let (bg, fg) = if hov { (b.tx, b.card) } else { (b.card, b.tx) };
                    self.cx.c.circle(cx, cy, 9.0, Some(op(bg)), Some((op(b.tx3), 1.0)));
                    self.cx.c.lines(&[(cx - 3.0, cy - 3.0), (cx + 3.0, cy + 3.0)], 1.2, op(fg));
                    self.cx.c.lines(&[(cx + 3.0, cy - 3.0), (cx - 3.0, cy + 3.0)], 1.2, op(fg));
                    self.cx.hit(Ctl::TileRemove(row), (cx - 10.0, cy - 10.0, 20.0, 20.0), 1);
                }
            }
        }
        if !away {
            self.out.geo.rows.push((device, (x, y, self.tiles_w(n, add), 34.0)));
        }
        if add {
            let ax = x + n as f32 * (tw + 6.0);
            let off = self.cx.ro;
            let fill = if self.cx.hovered(Ctl::TileAdd(row)) && !off { mix(b.card, b.tx, 0.06) } else { b.card };
            self.cx.c.boxed((ax, y, tw, 34.0), 4.0, fill, Some(b.tx3));
            let iw = self.cx.c.width(I_ADD, F::IconSmall);
            self.cx.icon(ax + (tw - iw) / 2.0, y, 34.0, I_ADD, F::IconSmall, if off { b.tx3 } else { b.tx2 });
            self.cx.hit(Ctl::TileAdd(row), (ax, y, tw, 34.0), 1);
        }
        let w = self.tiles_w(n, add);
        self.cx.ring((x - 3.0, y - 3.0, w + 6.0, 40.0), 6.0, Ctl::Tiles(row));
        if !self.cx.said.iter().any(|(c, _)| *c == Ctl::Tiles(row)) {
            let name = format!("{label} workspaces, {n}");
            self.cx.say(Ctl::Tiles(row), UIA_SpinnerControlTypeId, &name, "Right adds one to this screen; Left or Delete removes its last.", self.cx.ro, (None, Some(n.to_string())));
        }
    }

    /// A screen not plugged in that windows wait for (spec 0016 AC-9), with its own count's tiles
    /// (spec 0040 AC-8) and Forget.
    pub(super) fn away_row(&mut self, y: f32, i: usize, a: &crate::ws::views::Waiting, gone: &[usize]) -> f32 {
        let row = self.w.pics.screens.len() + i;
        let n = self.s.count_for(&a.id);
        let total: usize = a.windows.iter().sum();
        let wait = if total == 1 { "1 window waits".to_string() } else { format!("{total} windows wait") };
        let cap = vec![t(format!("Not plugged in · {wait} for it and go back when you plug it in. Forget leaves them where they are now."))];
        let counts: Vec<usize> = (0..n).map(|ws| a.windows.get(ws).copied().unwrap_or(0)).collect();
        let add = n < 9;
        let tw = self.tiles_w(n, add);
        let fw = self.cx.btn_w("Forget", None);
        let title = self.row_title(&a.id, a.name.clone());
        self.row(y, &title, &cap, (tw + 16.0 + fw, 34.0), |c, x, y| {
            c.tiles(x, y, &counts, (row, &a.name), None, gone, add);
            c.cx.button(Ctl::Forget(i), x + tw + 16.0, y + 1.0, "Forget", None, false, 1);
        }, None)
        .max(60.0)
    }

    /// Your screens as Windows arranges them, each showing its workspace's color and number.
    pub(super) fn screens_art(&mut self, y: f32) -> f32 {
        let list = self.w.pics.screens.clone();
        if list.is_empty() {
            return 0.0;
        }
        let b = self.cx.c.t.b;
        let (wd, ht) = (420.0, 150.0);
        let bx = list.iter().map(|s| s.rect.left).min().unwrap_or(0) as f32;
        let by = list.iter().map(|s| s.rect.top).min().unwrap_or(0) as f32;
        let bw = list.iter().map(|s| s.rect.right).max().unwrap_or(1) as f32 - bx;
        let bh = list.iter().map(|s| s.rect.bottom).max().unwrap_or(1) as f32 - by;
        let sc = ((wd - 4.0) / bw).min((ht - 18.0) / bh).min(0.09);
        let ox = X0 + (self.cw - wd) / 2.0 + (wd - bw * sc) / 2.0;
        let oy = y + 12.0 + (ht - (bh * sc + 16.0)) / 2.0;
        for s in &list {
            let x = ox + (s.rect.left as f32 - bx) * sc + 3.0;
            let yy = oy + (s.rect.top as f32 - by) * sc + 3.0;
            let (w, h) = ((s.rect.right - s.rect.left) as f32 * sc - 6.0, (s.rect.bottom - s.rect.top) as f32 * sc - 6.0);
            let below = list.iter().any(|o| o.device != s.device && (o.rect.top - s.rect.bottom).abs() < 2 && o.rect.left < s.rect.right && o.rect.right > s.rect.left);
            let laptop = s.name == "Built-in display";
            if !below {
                if laptop {
                    let pts = [(x - 6.0, yy + h), (x + w + 6.0, yy + h), (x + w + 3.0, yy + h + 4.0), (x - 3.0, yy + h + 4.0)];
                    self.cx.c.fill_rr((pts[0].0, pts[0].1, w + 12.0, 4.0), 2.0, op(b.stand));
                } else {
                    self.cx.c.fill_rr((x + w / 2.0 - 8.0, yy + h, 16.0, 9.0), 0.0, op(b.stand));
                    self.cx.c.fill_rr((x + w / 2.0 - 22.0, yy + h + 9.0, 44.0, 3.0), 1.5, op(b.stand));
                }
            }
            let a = self.active_on(&s.device);
            self.cx.c.fill_rr((x, yy, w, h), 6.0, op(b.bez));
            let (fill, ink) = if self.cx.c.t.hc { (b.card, b.tx) } else { (b.ws[a.min(8)], b.on_ws[a.min(8)]) };
            self.cx.c.fill_rr((x + 2.0, yy + 2.0, w - 4.0, h - 4.0), 4.0, op(fill));
            let name = self.name_of(s);
            if h >= 45.0 && self.cx.c.width(&name, F::Small) <= w - 16.0 {
                self.cx.c.text(x + 9.0, yy + 7.0, 16.0, &name, F::Small, ink);
            }
            self.cx.c.text(x + 9.0, yy + h - 32.0, 28.0, &(a + 1).to_string(), F::Title, ink);
        }
        12.0 + ht + 4.0
    }
}

// ---- The rows' writes (spec 0040 AC-9) ----

/// What a Workspaces card removal preview is about.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::settings_page) enum Removal {
    /// Card row `r`'s last workspace: its × or Left.
    Row(usize),
    /// The shared count's −.
    Lower,
    /// The shared count's Reset.
    Reset,
}

/// How many tile rows the Workspaces card has: plugged in screens, then those not plugged in.
pub(in crate::settings_page) fn tile_rows(w: &Win) -> usize {
    w.pics.screens.len() + w.pics.view.waiting.len()
}

/// Card row `r`'s screen id.
pub(in crate::settings_page) fn row_id(w: &Win, r: usize) -> Option<String> {
    match w.pics.screens.get(r) {
        Some(sc) => w.pics.view.present.iter().find(|p| p.device == sc.device).map(|p| p.id.clone()),
        None => w.pics.view.waiting.get(r - w.pics.screens.len()).map(|a| a.id.clone()),
    }
}

/// What a row's + or × writes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::settings_page) enum Write {
    /// `workspaces`: the card's only row, whose screen follows it.
    Shared(usize),
    /// The screen's own line.
    Line(usize),
    /// The screen's line comes out: its new count is the shared one (the spec 0020 rule).
    Remove,
}

/// `rows` on the card, whether this screen `has_line` that differs from `shared`, and its `new`
/// count.
pub(in crate::settings_page) fn row_write(rows: usize, has_line: bool, new: usize, shared: usize) -> Write {
    match () {
        _ if rows == 1 && !has_line => Write::Shared(new),
        _ if new == shared => Write::Remove,
        _ => Write::Line(new),
    }
}

/// The shared count's new value, and out go the lines equal to the old one: they followed it,
/// so they keep following (spec 0040 AC-1, your call during the build). Lines that differ stay.
pub(in crate::settings_page) fn shared_edit(s: &Settings, new: usize) -> Edit {
    let set = Edit::Value("workspaces", toml_edit::Value::from(new as i64));
    s.count_by_screen.iter().filter(|&(_, &n)| n == s.shared_count).fold(set, |e, (id, _)| Edit::Pair(Box::new(e), Box::new(line_out(id))))
}

fn line_out(id: &str) -> Edit {
    Edit::Remove(vec!["workspaces_per_screen".into(), id.into()])
}

/// The edit for screen `id`'s row going to `new`, with `rows` rows on the card.
pub(in crate::settings_page) fn row_edit(s: &Settings, rows: usize, id: &str, new: usize) -> Edit {
    match row_write(rows, !s.follows_shared(id), new, s.shared_count) {
        Write::Shared(n) => shared_edit(s, n),
        Write::Remove => line_out(id),
        Write::Line(n) => Edit::At(vec!["workspaces_per_screen".into(), id.into()], toml_edit::Value::from(n as i64)),
    }
}

// ---- Apps (spec 0027 AC-13, AC-14) ----

/// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
/// rename, else the id.
pub(in crate::settings_page) fn screen_called(v: &crate::ws::views::View, s: &Settings, id: &str) -> String {
    v.present
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .or_else(|| v.waiting.iter().find(|w| w.id == id).map(|w| w.name.clone()))
        .or_else(|| s.screen_rename(id).map(String::from))
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests;
