// The Workspaces card: the count per screen and its tiles (spec 0012).
use super::*;

impl Ctx<'_, '_> {
    // ---- Workspaces ----

    pub(super) fn workspaces(&mut self, y: f32) -> f32 {
        let mut h = self.screens_art(y);
        let w = self.w;
        let n = self.s.workspaces;
        // Pointing at the last tile only offers the picker; the removal preview waits for the ×.
        let removing = !self.ro && n > 2 && (self.hovered(Ctl::TileRemove) || self.focused(Ctl::Tiles));
        let resetting = !self.ro && n > DEFAULTS.workspaces && (self.hovered(Ctl::Reset("workspaces")) || self.focused(Ctl::Reset("workspaces")));
        let gone: Vec<usize> = if removing { vec![n - 1] } else if resetting { (DEFAULTS.workspaces..n).collect() } else { vec![] };
        for (i, sc) in w.pics.screens.clone().iter().enumerate() {
            if i > 0 || h > 0.0 {
                self.sep(y + h);
            }
            h += self.screen_row(y + h, i, sc, &gone);
        }
        for (i, away) in w.pics.view.waiting.clone().iter().enumerate() {
            self.sep(y + h);
            h += self.away_row(y + h, i, away);
        }
        self.sep(y + h);
        let note = if removing || resetting { Some(self.removal(removing)) } else { None };
        let mut cap = vec![t(format!("{n} on every screen, 2 to 9. + adds one to every screen; point at the last tile, and its × removes it from every screen."))];
        cap.extend(self.default_pieces("workspaces", false));
        let hh = self.measured(|c| c.row(0.0, "Workspaces per screen", &cap, (0.0, 0.0), |_, _, _| {}, note.as_ref()));
        if note.is_some() {
            let b = self.c.t.b;
            let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.06) };
            self.c.fill_rr((X0 + 1.0, y + h + 1.0, self.cw - 2.0, hh - 1.0), 0.0, op(tint));
        }
        h += self.row(y + h, "Workspaces per screen", &cap, (0.0, 0.0), |_, _, _| {}, note.as_ref());
        self.sep(y + h);
        h += self.setting("follow_activation", y + h);
        self.sep(y + h);
        h + self.setting("jump_back", y + h)
    }

    pub(in crate::settings_page) fn ws_chip_w(&self, k: usize) -> f32 {
        (self.c.width(&(k + 1).to_string(), F::SmallBold) + 10.0).max(20.0)
    }

    /// Workspace `k`'s chip, as the overview draws a shown workspace's: its color, its number.
    pub(in crate::settings_page) fn ws_chip(&self, x: f32, y: f32, k: usize) {
        let b = self.c.t.b;
        let (w, color) = (self.ws_chip_w(k), b.ws[k.min(8)]);
        self.c.boxed((x, y, w, 18.0), 4.0, color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)));
        let label = (k + 1).to_string();
        let tw = self.c.width(&label, F::SmallBold);
        self.c.text(x + (w - tw) / 2.0, y, 18.0, &label, F::SmallBold, b.on_ws[k.min(8)]);
    }
}

/// Workspace `k` as a rule for `screen` (None: any) shows it: that screen's name for it, else
/// every screen's, else "Workspace N" in grey (true).
pub(in crate::settings_page) fn ws_label(s: &Settings, screen: Option<&str>, k: usize) -> (String, bool) {
    let name = match screen {
        Some(id) => s.ws_name(id, k).map(String::from),
        None => s.names.get(k).filter(|n| !n.is_empty() && k < s.workspaces).cloned(),
    };
    name.map_or_else(|| (format!("Workspace {}", k + 1), true), |n| (n, false))
}
