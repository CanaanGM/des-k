// The screen rows on the Workspaces card: each screen's tiles, the screens not plugged in, and
// the picture of your screens (spec 0012, spec 0016, spec 0021).
use super::*;

impl Ctx<'_, '_> {
    /// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
    /// rename, else the id.
    pub(super) fn screen_called(&self, id: &str) -> String {
        screen_called(&self.w.pics.view, self.s, id)
    }

    /// What a removal moves, per screen, unplugged ones included (AC-SET-3), since there is no undo.
    pub(super) fn removal(&self, one: bool) -> NoteSpec {
        let n = self.s.workspaces;
        let d = DEFAULTS.workspaces;
        let (list, to) = if one { (self.w.pics.view.removals.first(), n - 1) } else { (self.w.pics.view.removals.get(1), d) };
        let title = if !one {
            format!("Reset goes back to {d} and removes workspaces {} to {n} from every screen", d + 1)
        } else if self.focused(Ctl::Tiles) {
            format!("Delete or Left removes workspace {n} from every screen")
        } else {
            format!("× removes workspace {n} from every screen")
        };
        let mut parts = vec![if one { format!("Its windows move to workspace {to}.") } else { format!("Their windows move to workspace {to}.") }];
        let list = list.cloned().unwrap_or_default();
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

    pub(super) fn name_of(&self, sc: &crate::ws::ScreenInfo) -> String {
        self.w.pics.view.names.iter().find(|(d, _)| *d == sc.device).map_or_else(|| sc.name.clone(), |(_, n)| n.clone())
    }

    /// Screen `device`'s workspace names, as its indicator shows them (spec 0023 build
    /// conditions): the same lookup as `ws::names`, by the id the page's view already holds,
    /// since the page can lay out from inside a switch and the ws lock is not reentrant.
    pub(super) fn names_here(&self, device: &str) -> Vec<String> {
        let Some(p) = self.w.pics.view.present.iter().find(|p| p.device == device) else { return vec![] };
        (0..self.s.workspaces).map(|i| self.s.ws_name(&p.id, i).unwrap_or_default().to_string()).collect()
    }

    pub(super) fn active_on(&self, device: &str) -> usize {
        self.w.pics.view.active.iter().find(|(d, _)| d == device).map_or(0, |(_, a)| *a)
    }

    fn tiles_w(&self, n: usize, add: bool) -> f32 {
        let tw = if n > 6 { 40.0 } else { 48.0 };
        n as f32 * tw + (n as f32 - 1.0) * 6.0 + if add { 6.0 + tw } else { 0.0 }
    }

    /// One present screen: its name and size, and its workspaces as tiles (AC-SET-3).
    pub(super) fn screen_row(&mut self, y: f32, row: usize, sc: &crate::ws::ScreenInfo, gone: &[usize]) -> f32 {
        let n = self.s.workspaces;
        let name = self.name_of(sc);
        let a = self.active_on(&sc.device);
        let (rw, rh) = (sc.rect.right - sc.rect.left, sc.rect.bottom - sc.rect.top);
        let cap = vec![t(format!("{rw} × {rh} · {}% · on workspace {}", sc.scale, a + 1))];
        let counts: Vec<usize> = (0..n).map(|ws| self.w.pics.view.windows.iter().filter(|(d, k, _)| *d == sc.device && *k == ws).count()).collect();
        let add = n < 9;
        let tw = self.tiles_w(n, add);
        let mut h = self.row(y, &name, &cap, (tw, 34.0), |c, x, y| c.tiles(x, y, &counts, Some((a, row)), gone, add), None);
        h = h.max(60.0);
        h
    }

    /// The tiles: a workspace each in its color, with up to three window marks; the one in
    /// view ringed; × on the last one while you point at it, + after them. `now`: a plugged in
    /// screen's workspace in view and its row in Pics::screens; its tiles open the color picker.
    #[allow(clippy::too_many_arguments)]
    fn tiles(&mut self, x: f32, y: f32, counts: &[usize], now: Option<(usize, usize)>, gone: &[usize], add: bool) {
        let t = self.c.t;
        let b = t.b;
        let n = counts.len();
        let tw = if n > 6 { 40.0 } else { 48.0 };
        let away = now.is_none();
        let (now, row) = (now.map(|(a, _)| a), now.map_or(0, |(_, r)| r));
        let device = self.w.pics.screens.get(row).map(|s| s.device.clone()).unwrap_or_default();
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
                self.c.fill_rr((tx - 4.0, y - 4.0, tw + 8.0, 42.0), 8.0, op(b.tx));
                self.c.fill_rr((tx - 2.0, y - 2.0, tw + 4.0, 38.0), 6.0, op(b.card));
            }
            if away || t.hc {
                self.c.boxed(bx, 4.0, fill, Some(if away { b.line } else { b.tx }));
            } else {
                self.c.fill_rr(bx, 4.0, op(fill));
            }
            self.c.text(tx + 6.0, y + 34.0 - 19.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
            let marks = k.min(3);
            for m in 0..marks {
                let mx = tx + tw - 4.0 - (marks - m) as f32 * 10.0 + 2.0;
                let col = if away { b.tx3 } else if t.hc { if cur { b.on_acc } else { b.tx } } else { b.mk_ws[i] };
                self.c.fill_rr((mx, y + 4.0, 8.0, 10.0), 2.0, ui::alpha(col, if away || t.hc { 1.0 } else { 0.9 }));
            }
            if gone.contains(&i) && !away {
                self.c.stroke_dash((tx + 1.0, y + 1.0, tw - 2.0, 32.0), 3.0, 2.0, op(b.tx), DashStyleDash);
            }
            if away {
                continue;
            }
            let ctl = Ctl::WsTile(row, i);
            if self.hovered(ctl) && self.picks() && !cur {
                self.cue(bx, ink);
            }
            // Before the ×'s hit, so a click inside the tile opens the picker and never removes
            // a workspace (spec 0021 AC-1).
            self.hit(ctl, bx, 1);
            self.out.geo.tiles.push((device.clone(), i, bx));
            if i + 1 == n && n > 2 && !self.ro {
                let show = self.hovered(ctl) || self.hovered(Ctl::TileRemove) || self.focused(Ctl::Tiles);
                if show {
                    let (cx, cy) = (tx + tw + 2.0, y - 2.0);
                    let hov = self.hovered(Ctl::TileRemove);
                    let (bg, fg) = if hov { (b.tx, b.card) } else { (b.card, b.tx) };
                    self.c.circle(cx, cy, 9.0, Some(op(bg)), Some((op(b.tx3), 1.0)));
                    self.c.lines(&[(cx - 3.0, cy - 3.0), (cx + 3.0, cy + 3.0)], 1.2, op(fg));
                    self.c.lines(&[(cx + 3.0, cy - 3.0), (cx - 3.0, cy + 3.0)], 1.2, op(fg));
                    self.hit(Ctl::TileRemove, (cx - 10.0, cy - 10.0, 20.0, 20.0), 1);
                }
            }
        }
        if !away {
            self.out.geo.rows.push((device, (x, y, self.tiles_w(n, add), 34.0)));
        }
        if !away && add {
            let ax = x + n as f32 * (tw + 6.0);
            let off = self.ro;
            let fill = if self.hovered(Ctl::TileAdd) && !off { mix(b.card, b.tx, 0.06) } else { b.card };
            self.c.boxed((ax, y, tw, 34.0), 4.0, fill, Some(b.tx3));
            let iw = self.c.width(I_ADD, F::IconSmall);
            self.icon(ax + (tw - iw) / 2.0, y, 34.0, I_ADD, F::IconSmall, if off { b.tx3 } else { b.tx2 });
            self.hit(Ctl::TileAdd, (ax, y, tw, 34.0), 1);
        }
        if !away {
            let w = self.tiles_w(n, add);
            self.ring((x - 3.0, y - 3.0, w + 6.0, 40.0), 6.0, Ctl::Tiles);
            if !self.out.said.iter().any(|(c, _)| *c == Ctl::Tiles) {
                let name = format!("Workspaces per screen, {n}");
                self.say(Ctl::Tiles, UIA_SpinnerControlTypeId, &name, "Right adds one to every screen; Left or Delete removes the last.", self.ro, (None, Some(n.to_string())));
            }
        }
    }

    /// A screen not plugged in that windows wait for (spec 0016 AC-9), with Forget.
    pub(super) fn away_row(&mut self, y: f32, i: usize, a: &crate::ws::Waiting) -> f32 {
        let n = self.s.workspaces;
        let total: usize = a.windows.iter().sum();
        let wait = if total == 1 { "1 window waits".to_string() } else { format!("{total} windows wait") };
        let cap = vec![t(format!("Not plugged in · {wait} for it and go back when you plug it in. Forget leaves them where they are now."))];
        let counts: Vec<usize> = (0..n).map(|ws| a.windows.get(ws).copied().unwrap_or(0)).collect();
        let tw = self.tiles_w(n, false);
        let fw = self.btn_w("Forget", None);
        self.row(y, &a.name, &cap, (tw + 16.0 + fw, 34.0), |c, x, y| {
            c.tiles(x, y, &counts, None, &[], false);
            c.button(Ctl::Forget(i), x + tw + 16.0, y + 1.0, "Forget", None, false, 1);
        }, None)
        .max(60.0)
    }

    /// Your screens as Windows arranges them, each showing its workspace's color and number.
    pub(super) fn screens_art(&mut self, y: f32) -> f32 {
        let list = self.w.pics.screens.clone();
        if list.is_empty() {
            return 0.0;
        }
        let b = self.c.t.b;
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
                    self.c.fill_rr((pts[0].0, pts[0].1, w + 12.0, 4.0), 2.0, op(b.stand));
                } else {
                    self.c.fill_rr((x + w / 2.0 - 8.0, yy + h, 16.0, 9.0), 0.0, op(b.stand));
                    self.c.fill_rr((x + w / 2.0 - 22.0, yy + h + 9.0, 44.0, 3.0), 1.5, op(b.stand));
                }
            }
            let a = self.active_on(&s.device);
            self.c.fill_rr((x, yy, w, h), 6.0, op(b.bez));
            let (fill, ink) = if self.c.t.hc { (b.card, b.tx) } else { (b.ws[a.min(8)], b.on_ws[a.min(8)]) };
            self.c.fill_rr((x + 2.0, yy + 2.0, w - 4.0, h - 4.0), 4.0, op(fill));
            let name = self.name_of(s);
            if h >= 45.0 && self.c.width(&name, F::Small) <= w - 16.0 {
                self.c.text(x + 9.0, yy + 7.0, 16.0, &name, F::Small, ink);
            }
            self.c.text(x + 9.0, yy + h - 32.0, 28.0, &(a + 1).to_string(), F::Title, ink);
        }
        12.0 + ht + 4.0
    }
}

// ---- Apps (spec 0027 AC-13, AC-14) ----

/// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
/// rename, else the id.
fn screen_called(v: &crate::ws::View, s: &Settings, id: &str) -> String {
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
