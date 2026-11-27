// The Workspaces card: the count per screen and its tiles (spec 0012).
use super::*;
use super::screens::{Removal, row_id, tile_rows};

impl Ctx<'_, '_> {
    // ---- Workspaces ----

    /// One tile row per screen, plugged in then not, each with its own + and × (spec 0040 AC-8),
    /// and the shared count's stepper once a screen can differ from it (AC-10).
    pub(super) fn workspaces(&mut self, y: f32) -> f32 {
        let mut h = self.screens_art(y);
        let w = self.w;
        let (shared, d, ro) = (self.s.shared_count, DEFAULTS.shared_count, self.cx.ro);
        let split = self.split();
        let rows = tile_rows(w);
        // Pointing at the last tile only offers the picker; the removal preview waits for the ×.
        let row_rm = (0..rows).find(|&r| self.cx.hovered(Ctl::TileRemove(r)) || self.cx.focused(Ctl::Tiles(r))).filter(|&r| !ro && self.row_count(r) > 2);
        let stepper = [Ctl::Minus("workspaces"), Ctl::Stepper("workspaces")];
        let lowering = !ro && split && shared > 2 && stepper.iter().any(|&c| self.cx.hovered(c) || self.cx.focused(c));
        let resetting = !ro && shared > d && (self.cx.hovered(Ctl::Reset("workspaces")) || self.cx.focused(Ctl::Reset("workspaces")));
        let what = row_rm.map(Removal::Row).or(lowering.then_some(Removal::Lower)).or(resetting.then_some(Removal::Reset));
        let gone = |c: &Self, r: usize| -> Vec<usize> {
            let Some(id) = row_id(w, r) else { return vec![] };
            match what {
                Some(Removal::Row(x)) if x == r => vec![c.s.count_for(&id) - 1],
                Some(Removal::Lower) if c.s.follows_shared(&id) => vec![shared - 1],
                Some(Removal::Reset) if c.s.follows_shared(&id) => (d..shared).collect(),
                _ => vec![],
            }
        };
        for (i, sc) in w.pics.screens.clone().iter().enumerate() {
            if i > 0 || h > 0.0 {
                self.sep(y + h);
            }
            let g = gone(self, i);
            h += self.screen_row(y + h, i, sc, &g);
        }
        for (i, away) in w.pics.view.waiting.clone().iter().enumerate() {
            self.sep(y + h);
            let g = gone(self, w.pics.screens.len() + i);
            h += self.away_row(y + h, i, away, &g);
        }
        self.sep(y + h);
        let note = what.map(|k| self.removal(k));
        let mut cap = if split {
            vec![t(format!("{shared} on every screen without its own count, 2 to 9."))]
        } else {
            vec![t(format!("{shared} workspaces, 2 to 9. + adds one; point at the last tile, and its × removes it."))]
        };
        cap.extend(self.default_pieces("workspaces", false));
        let words = step_words(shared as i64, "");
        let size = if split { (self.cx.stepper_w(&shared.to_string()), 32.0) } else { (0.0, 0.0) };
        if split {
            let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(), P::Icon(..) => "" }).collect::<String>().trim().to_string();
            self.cx.say(Ctl::Stepper("workspaces"), UIA_SpinnerControlTypeId, "Workspaces per screen", &help, ro, (None, Some(shared.to_string())));
        }
        let hh = self.measured(|c| c.row(0.0, "Workspaces per screen", &cap, size, |_, _, _| {}, note.as_ref()));
        if note.is_some() {
            let b = self.cx.c.t.b;
            let tint = if self.cx.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.06) };
            self.cx.c.fill_rr((X0 + 1.0, y + h + 1.0, self.cw - 2.0, hh - 1.0), 0.0, op(tint));
        }
        h += self.row(y + h, "Workspaces per screen", &cap, size, |c, x, y| {
            if split {
                c.cx.stepper(Ctl::steps("workspaces"), x, y, words.clone(), (shared > 2, shared < 9), false, false, None);
            }
        }, note.as_ref());
        self.sep(y + h);
        h += self.setting("follow_activation", y + h);
        self.sep(y + h);
        h += self.setting("act_on", y + h);
        self.sep(y + h);
        h + self.setting("jump_back", y + h)
    }

    pub(in crate::settings_page) fn ws_chip_w(&self, k: usize) -> f32 {
        (self.cx.c.width(&(k + 1).to_string(), F::SmallBold) + 10.0).max(20.0)
    }

    /// Workspace `k`'s chip, as the overview draws a shown workspace's: its color, its number.
    pub(in crate::settings_page) fn ws_chip(&self, x: f32, y: f32, k: usize) {
        let b = self.cx.c.t.b;
        let (w, color) = (self.ws_chip_w(k), b.ws[k.min(8)]);
        self.cx.c.boxed((x, y, w, 18.0), 4.0, color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)));
        let label = (k + 1).to_string();
        let tw = self.cx.c.width(&label, F::SmallBold);
        self.cx.c.text(x + (w - tw) / 2.0, y, 18.0, &label, F::SmallBold, b.on_ws[k.min(8)]);
    }
}

/// Workspace `k` as a rule for `screen` (None: any) shows it: that screen's name for it, else
/// every screen's, else "Workspace N" in grey (true).
pub(in crate::settings_page) fn ws_label(s: &Settings, screen: Option<&str>, k: usize) -> (String, bool) {
    let name = match screen {
        Some(id) => s.ws_name(id, k).map(String::from),
        None => s.names.get(k).filter(|n| !n.is_empty() && k < s.most()).cloned(),
    };
    name.map_or_else(|| (format!("Workspace {}", k + 1), true), |n| (n, false))
}
