// The Apps section: a rule per app, its workspace and screen menus, and what each writes
// (spec 0027).
use super::*;
use crate::settings_page::choosers::{Choice, Mark};
use super::workspaces::ws_label;

const APP_WS_W: f32 = 184.0; // an Apps rule's workspace menu (spec 0027)
const APP_SCREEN_W: f32 = 204.0; // and its screen menu

impl Ctx<'_, '_> {
    // ---- Apps (spec 0027 AC-13): an Add card, then the rules, as approved in
    // pdesk-design/apps/apps-design.html r1 ----

    pub(super) fn app_add(&mut self, y: f32) -> f32 {
        let bw = self.cx.btn_w("Add app", Some(I_ADD));
        self.row(y, "Add an app", &[t("Pick an app that's open now. Its rule starts where its window is now.")], (bw, 32.0), |c, x, y| {
            c.cx.button(Ctl::AppAdd, x, y, "Add app", Some(I_ADD), false, 1);
        }, None)
    }

    /// Where a rule row's parts sit: the icon at `x`, then the name, the two menus and remove.
    fn app_cols(&self) -> (f32, f32, f32, f32, f32) {
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let remove = x + w - 32.0;
        let screen = remove - 12.0 - APP_SCREEN_W;
        let ws = screen - 12.0 - APP_WS_W;
        (x, x + 40.0, (ws - 16.0 - x - 40.0).max(40.0), ws, screen)
    }

    pub(super) fn app_rules(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let n = self.s.apps.len();
        self.header(y, "Rules", Some(n.min(99) as u8), "");
        let (plain, bold) = ("New windows only · ", "your screens stay");
        let rw = self.cx.c.width(plain, F::Small) + self.cx.c.width(bold, F::SmallBold);
        self.cx.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[(plain, F::Small, b.tx2), (bold, F::SmallBold, b.tx)]);
        let mut h = 38.0;
        if n == 0 {
            return h + self.apps_empty(y + h);
        }
        let (_, name, _, ws, screen) = self.app_cols();
        for (text, x) in [("App", name), ("Workspace", ws), ("Screen", screen)] {
            self.cx.c.text(x, y + h, 33.0, text, F::Small, b.tx2);
        }
        h += 33.0;
        for i in 0..n {
            self.sep(y + h);
            h += self.app_row(y + h, i);
        }
        h
    }

    /// One rule: the app's icon and name while it runs, else its key; under the name its note,
    /// or its key, or Not running; then its workspace and screen menus and remove.
    fn app_row(&mut self, y: f32, i: usize) -> f32 {
        let b = self.cx.c.t.b;
        let (key, rule) = &self.s.apps[i];
        let (x, nx, nw, wx, sx) = self.app_cols();
        let seen = self.w.pics.apps.iter().find(|a| a.key == *key).cloned();
        let name = seen.as_ref().map_or_else(|| key.clone(), |a| a.name.clone());
        let note = app_note(&self.w.pics.view, self.s, rule);
        let note_ink = |warn: bool| if warn { b.warn } else { b.tx2 };
        let sub_h = match &note {
            Some((text, warn)) => self.measured(|c| c.cx.c.para(nx + 18.0, 0.0, nw - 18.0, 16.0, &[(text, F::Small, note_ink(*warn))])),
            None => 16.0,
        };
        let h = (10.0 + 20.0 + 2.0 + sub_h + 10.0).max(60.0);
        let ty = y + (h - 22.0 - sub_h) / 2.0;
        let iy = y + (h - 24.0) / 2.0;
        match &seen {
            Some(a) => self.cx.c.icon(x, iy, 24.0, a.icon),
            None => {
                self.cx.c.stroke_rr((x + 0.5, iy + 0.5, 23.0, 23.0), 6.0, 1.0, op(b.tx3));
                let gw = self.cx.c.width(I_APP, F::IconSmall);
                self.cx.icon(x + (24.0 - gw) / 2.0, iy, 24.0, I_APP, F::IconSmall, b.tx3);
            }
        }
        let shown = self.cx.c.fit(&name, F::Bold, nw);
        self.cx.c.text(nx, ty, 20.0, &shown, F::Bold, b.tx);
        match &note {
            Some((text, warn)) => {
                self.cx.icon(nx, ty + 22.0, 16.0, if *warn { I_WARN } else { I_INFO }, F::IconTiny, note_ink(*warn));
                self.cx.c.para(nx + 18.0, ty + 22.0, nw - 18.0, 16.0, &[(text, F::Small, note_ink(*warn))]);
            }
            None if seen.is_none() => self.cx.c.text(nx, ty + 22.0, 16.0, "Not running", F::Small, b.tx2),
            None => {
                // A Store app's id is long and spelled as the file holds it.
                let f = if key.contains('!') { F::Mono } else { F::Small };
                let k = self.cx.c.fit(key, f, nw);
                self.cx.c.text(nx, ty + 22.0, 16.0, &k, f, b.tx2);
            }
        }
        let my = y + (h - 32.0) / 2.0;
        let (ws, grey) = ws_label(self.s, rule.screen.as_deref(), rule.workspace);
        let screen = rule.screen.as_deref().map_or_else(|| "Any screen".to_string(), |id| self.screen_called(id));
        let help = note.as_ref().map_or(String::new(), |(text, _)| text.clone());
        self.cx.say(Ctl::AppWs(i), UIA_ComboBoxControlTypeId, &format!("Workspace for {name}"), &help, self.cx.ro, (None, Some(ws.clone())));
        self.app_picker(Ctl::AppWs(i), (wx, my, APP_WS_W), Mark::Chip(rule.workspace), &ws, grey);
        self.cx.say(Ctl::AppScreen(i), UIA_ComboBoxControlTypeId, &format!("Screen for {name}"), "", self.cx.ro, (None, Some(screen.clone())));
        self.app_picker(Ctl::AppScreen(i), (sx, my, APP_SCREEN_W), Mark::Glyph(I_SCREEN), &screen, false);
        self.app_x(i, sx + APP_SCREEN_W + 12.0, my, &name);
        h
    }

    /// A menu button, as the screen chooser draws one: a workspace's chip and name, or a mark
    /// (a screen, a picture) and a name.
    pub(super) fn app_picker(&mut self, ctl: Ctl, (x, y, w): (f32, f32, f32), mark: Mark, label: &str, grey: bool) {
        let b = self.cx.c.t.b;
        if matches!(self.w.chooser, Some((from, ..)) if from == ctl) {
            self.cx.field((x, y, w, 32.0), b.card, Some(b.acc));
        } else {
            let fill = if self.cx.hovered(ctl) && !self.cx.ro { mix(b.track, b.tx, 0.05) } else { b.track };
            self.cx.field((x, y, w, 32.0), fill, None);
        }
        let mut tx = x + 10.0;
        match mark {
            Mark::Chip(k) => {
                self.ws_chip(tx, y + 7.0, k);
                tx += self.ws_chip_w(k) + 8.0;
            }
            Mark::Glyph(g) => {
                self.cx.icon(tx, y, 32.0, g, F::Icon, b.tx2);
                tx += 26.0;
            }
            Mark::Stack => {
                // Two pictures, the front one masked by the button's own fill (spec 0028 AC-11).
                let fill = if matches!(self.w.chooser, Some((from, ..)) if from == ctl) { b.card } else if self.cx.hovered(ctl) && !self.cx.ro { mix(b.track, b.tx, 0.05) } else { b.track };
                self.cx.icon(tx + 4.0, y - 3.0, 32.0, I_PIC, F::IconSmall, mix(fill, b.tx2, 0.6));
                let gw = self.cx.c.width(I_PIC, F::IconSmall);
                self.cx.c.fill_rr((tx - 1.0, y + 11.0, gw + 2.0, 14.0), 2.0, op(fill));
                self.cx.icon(tx, y + 2.0, 32.0, I_PIC, F::IconSmall, b.tx2);
                tx += 26.0;
            }
        }
        let cw = self.cx.c.width(I_CHEV, F::IconTiny);
        let shown = self.cx.c.fit(label, F::Body, x + w - 18.0 - cw - tx);
        self.cx.c.text(tx, y, 32.0, &shown, F::Body, if grey || self.cx.ro { b.tx2 } else { b.tx });
        self.cx.icon(x + w - 10.0 - cw, y, 32.0, I_CHEV, F::IconTiny, b.tx2);
        self.cx.ring((x, y, w, 32.0), 6.0, ctl);
        self.cx.hit(ctl, (x, y, w, 32.0), 1);
    }

    /// A rule's remove button: an × that shows its square under the pointer.
    fn app_x(&mut self, i: usize, x: f32, y: f32, name: &str) {
        let b = self.cx.c.t.b;
        let ctl = Ctl::AppX(i);
        let hot = self.cx.hovered(ctl) && !self.cx.ro;
        if hot {
            self.cx.c.boxed((x, y, 32.0, 32.0), 4.0, mix(b.card, b.tx, 0.06), Some(b.line));
        }
        let iw = self.cx.c.width(I_X, F::IconTiny);
        let ink = match () {
            _ if self.cx.ro => b.tx3,
            _ if hot => b.tx,
            _ => b.tx2,
        };
        self.cx.icon(x + (32.0 - iw) / 2.0, y, 32.0, I_X, F::IconTiny, ink);
        self.cx.ring((x, y, 32.0, 32.0), 6.0, ctl);
        self.cx.hit(ctl, (x, y, 32.0, 32.0), 1);
        self.cx.say(ctl, UIA_ButtonControlTypeId, &format!("Remove {name}"), "Takes this app's rule out of settings.toml.", self.cx.ro, (None, None));
    }

    /// No rules yet: what a rule does, and where else you can write one.
    fn apps_empty(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let hc = self.cx.c.t.hc;
        let cx = X0 + self.cw / 2.0;
        let tile = if hc { b.card } else { mix(b.card, b.acc_a, 0.09) };
        self.cx.c.boxed((cx - 24.0, y + 28.0, 48.0, 48.0), 12.0, tile, hc.then_some(b.line));
        let gw = self.cx.c.width(I_APPS, F::Icon);
        self.cx.icon(cx - gw / 2.0, y + 28.0, 48.0, I_APPS, F::Icon, if hc { b.tx } else { b.acc });
        let mut ly = y + 86.0;
        for (text, f, ink, lh) in [
            ("No rules yet", F::Bold, b.tx, 20.0),
            ("Add an app and its new windows open on the workspace you pick, wherever you are.", F::Small, b.tx2, 16.0),
            ("You can also write rules under [apps] in settings.toml.", F::Small, b.tx2, 16.0),
        ] {
            let tw = self.cx.c.width(text, f);
            self.cx.c.text(cx - tw / 2.0, ly, lh, text, f, ink);
            ly += lh + 2.0;
        }
        ly - y + 26.0
    }
}

/// A rule's screen menu, each line with the id it writes: Any screen, the screens plugged in,
/// remembered ones, then the rule's own id if des-k never saw that screen.
pub(in crate::settings_page) fn app_screens(v: &crate::ws::views::View, rule: &settings::Rule) -> Vec<(Option<String>, Choice)> {
    let mut out = vec![(None, Choice::new("Any screen", "Where it opens"))];
    for p in &v.present {
        out.push((Some(p.id.clone()), Choice::new(&p.name, "")));
    }
    for w in v.waiting.iter().filter(|w| !v.present.iter().any(|p| p.id == w.id)) {
        out.push((Some(w.id.clone()), Choice::new(&w.name, "Not plugged in")));
    }
    if let Some(id) = rule.screen.as_ref().filter(|id| !out.iter().any(|(o, _)| o.as_ref() == Some(*id))) {
        out.push((Some(id.clone()), Choice::new(id, "Not seen yet")));
    }
    out
}

/// The Add menu's apps: open now, and with no rule yet.
pub(in crate::settings_page) fn addable<'a>(w: &'a Win, s: &Settings) -> Vec<&'a crate::apps::Seen> {
    w.pics.apps.iter().filter(|a| !s.apps.iter().any(|(k, _)| *k == a.key)).collect()
}

/// A rule row's note, if one applies, and whether it is a warning: a rule past your count or
/// for an unplugged screen still works; one for a screen des-k never saw is skipped (AC-7, AC-9).
fn app_note(v: &crate::ws::views::View, s: &Settings, rule: &settings::Rule) -> Option<(String, bool)> {
    if rule.workspace >= s.workspaces {
        return Some((format!("Waits: you have {} workspaces", s.workspaces), false));
    }
    let id = rule.screen.as_deref()?;
    match () {
        _ if v.present.iter().any(|p| p.id == id) => None,
        _ if v.waiting.iter().any(|w| w.id == id) => Some(("Screen not plugged in, windows wait on the main display".into(), false)),
        _ => Some(("des-k hasn't seen this screen yet".into(), true)),
    }
}

/// What a change to an app's rule writes, its key as the file spells it (AC-14): Add one
/// inline table, a menu that rule's one key, Any screen takes `screen` out, remove the rule.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum AppChange {
    Add(usize, Option<String>), // the workspace (0 based) and screen id where its window is
    Ws(usize),
    Screen(Option<String>), // None: Any screen
    Remove,
}

pub(crate) fn app_edit(key: &str, change: AppChange) -> Edit {
    let path = |leaf: Option<&str>| [Some("apps"), Some(key), leaf].into_iter().flatten().map(String::from).collect::<Vec<_>>();
    match change {
        AppChange::Add(ws, screen) => {
            let mut t = toml_edit::InlineTable::new();
            t.insert("workspace", (ws as i64 + 1).into());
            if let Some(id) = screen {
                t.insert("screen", id.into());
            }
            t.fmt();
            Edit::At(path(None), t.into())
        }
        AppChange::Ws(ws) => Edit::At(path(Some("workspace")), (ws as i64 + 1).into()),
        AppChange::Screen(Some(id)) => Edit::At(path(Some("screen")), id.into()),
        AppChange::Screen(None) => Edit::Remove(path(Some("screen"))),
        AppChange::Remove => Edit::Remove(path(None)),
    }
}

/// What choosing line `i` of an open Apps menu writes, if it changes anything.
pub(in crate::settings_page) fn app_choice(w: &Win, s: &Settings, from: Ctl, i: usize) -> Option<Edit> {
    match from {
        Ctl::AppAdd => {
            let a = *addable(w, s).get(i)?;
            let screen = w.pics.view.present.iter().find(|p| p.device == a.device).map(|p| p.id.clone());
            Some(app_edit(&a.key, AppChange::Add(a.ws, screen)))
        }
        Ctl::AppWs(r) => {
            let (key, rule) = s.apps.get(r)?;
            (i != rule.workspace && i < s.workspaces).then(|| app_edit(&settings::edit::app_key_in_file(key), AppChange::Ws(i)))
        }
        Ctl::AppScreen(r) => {
            let (key, rule) = s.apps.get(r)?;
            let id = app_screens(&w.pics.view, rule).into_iter().nth(i)?.0;
            (id != rule.screen).then(|| app_edit(&settings::edit::app_key_in_file(key), AppChange::Screen(id)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
