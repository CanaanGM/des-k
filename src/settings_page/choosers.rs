// The screen choosers: the screens a card can show, the open screen menu, its keys and what a
// choice changes (spec 0020, spec 0023, spec 0026).
use super::*;
use crate::settings_page::cards::apps::{addable, app_choice, app_screens};
use crate::settings_page::cards::names::names_chosen;
use crate::settings_page::cards::wallpaper::{choose_wall, chooser_wall, tick_wall, wall_chosen, wall_dirs, wall_folder, wall_held, wall_screen, wall_show_line};
use crate::settings_page::cards::workspaces::ws_label;
use super::menu::close_menu;

/// A line of the screen chooser: All screens (no id), then the screens left to right, then
/// remembered ones not plugged in, then ids in the file des-k never saw.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ScreenChoice {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) side: &'static str,
    pub(crate) present: Option<usize>, // its place in View::present
}

pub(crate) fn screen_choices(view: &crate::ws::View, s: &Settings) -> Vec<ScreenChoice> {
    let own = |id: &str| if s.indicator_screens.contains_key(id) { "Own place" } else { "" };
    let mut out = vec![ScreenChoice { id: None, name: "All screens".into(), side: "", present: None }];
    for (i, p) in view.present.iter().enumerate() {
        out.push(ScreenChoice { id: Some(p.id.clone()), name: p.name.clone(), side: own(&p.id), present: Some(i) });
    }
    for w in view.waiting.iter().filter(|w| !view.present.iter().any(|p| p.id == w.id)) {
        out.push(ScreenChoice { id: Some(w.id.clone()), name: w.name.clone(), side: "Not plugged in", present: None });
    }
    let unknown: Vec<&String> = s.indicator_screens.keys().filter(|id| !out.iter().any(|c| c.id.as_deref() == Some(id.as_str()))).collect();
    for id in unknown {
        out.push(ScreenChoice { id: Some(id.clone()), name: id.clone(), side: "Unknown screen", present: None });
    }
    out
}

impl Ctx<'_, '_> {
    pub(super) fn chooser_row(&mut self, y: f32, choices: &[ScreenChoice], i: usize) -> f32 {
        let ch = &choices[i];
        let mut cap = vec![];
        match &ch.id {
            None => {
                cap.push(t("The place for every screen without its own."));
                let named: Vec<String> = settings::store::problems()
                    .iter()
                    .filter_map(|p| p.key.strip_prefix("indicator.screens.\"")?.split('"').next().map(String::from))
                    .filter_map(|id| choices.iter().find(|c| c.id.as_deref() == Some(id.as_str())).map(|c| c.name.clone()))
                    .collect();
                if let Some(n) = named.first() {
                    cap.push(P::Why(format!(" {n} has a value des-k can't use.")));
                }
            }
            Some(id) => {
                let has = self.s.indicator_screens.contains_key(id);
                cap.push(t(match (ch.side, has) {
                    ("Not plugged in", _) => "Not plugged in. Its place is kept for when it's back.",
                    ("Unknown screen", _) => "des-k hasn't seen this screen. Its place is kept.",
                    (_, true) => "This screen has its own place.",
                    _ => "This screen uses All screens' place.",
                }));
                if has {
                    cap.push(P::Link(" Same as all screens".into(), Ctl::SameAll));
                }
            }
        }
        let pw = 240.0;
        let name = ch.name.clone();
        self.cx.say(Ctl::Screens, UIA_ComboBoxControlTypeId, "Screen", "", false, (None, Some(name.clone())));
        self.row(y, "Screen", &cap, (pw, 32.0), |c, x, y| c.screen_picker(Ctl::Screens, x, y, pw, &name), None)
    }

    /// The chooser: the touchpad's picker, with a screen for its mark. Never greyed: it only
    /// picks which place you see (live with the indicator off and in read only).
    pub(super) fn screen_picker(&mut self, ctl: Ctl, x: f32, y: f32, w: f32, name: &str) {
        let b = self.cx.c.t.b;
        if matches!(self.w.chooser, Some((from, ..)) if from == ctl) {
            self.cx.field((x, y, w, 32.0), b.card, Some(b.acc));
        } else {
            let fill = if self.cx.hovered(ctl) { mix(b.track, b.tx, 0.05) } else { b.track };
            self.cx.field((x, y, w, 32.0), fill, None);
        }
        self.cx.icon(x + 10.0, y, 32.0, "\u{E7F4}", F::Icon, b.tx2);
        let shown = self.cx.c.fit(name, F::Body, w - 36.0 - 26.0);
        self.cx.c.text(x + 36.0, y, 32.0, &shown, F::Body, b.tx);
        let cw = self.cx.c.width(I_CHEV, F::IconTiny);
        self.cx.icon(x + w - 10.0 - cw, y, 32.0, I_CHEV, F::IconTiny, b.tx2);
        self.cx.ring((x, y, w, 32.0), 6.0, ctl);
        self.cx.hit(ctl, (x, y, w, 32.0), 1);
    }

    /// The open screen menu, placed like the touchpad's: the chosen line over the chooser.
    pub(super) fn screens_menu(&mut self) {
        let Some((from, at, focus)) = self.w.chooser else { return };
        if matches!(from, Ctl::WallPick(_)) {
            return self.wall_menu();
        }
        let t = self.cx.c.t;
        let b = t.b;
        let (choices, cur) = chooser_lines(self.w, self.s, from);
        let add = from == Ctl::AppAdd;
        // Lines that don't fit the window's height go on in another column, so every open app
        // can be picked however many there are.
        let fit = (((self.w.vh - 16.0 - 32.0) / 28.0).floor() as usize).max(1);
        let cols = choices.len().div_ceil(fit).max(1);
        let rows = choices.len().div_ceil(cols);
        let col_w = if add { 420.0f32.min((self.w.vw - NAV - 32.0) / cols as f32) } else { at.2.max(320.0) };
        let mw = col_w * cols as f32;
        let mh = 4.0 + 24.0 + rows as f32 * 28.0 + 4.0;
        // The chosen line over the chooser; Add, with none chosen, opens under its button.
        let my = cur.map_or(at.1 + at.3 + 4.0, |c| at.1 - 28.0 - c as f32 * 28.0).clamp(8.0, (self.w.vh - mh - 8.0).max(8.0));
        let left = if add { at.0 + at.2 - mw } else { at.0 };
        let mx = left.min(self.w.vw - mw - 16.0).max(NAV);
        let cur = cur.unwrap_or(usize::MAX);
        let title = match from {
            Ctl::AppWs(_) => "Workspace",
            Ctl::AppAdd => "Apps open now",
            _ => "Screen",
        };
        self.cx.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.cx.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.cx.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.cx.c.text(mx + 14.0, my + 4.0, 24.0, title, F::SmallBold, b.tx2);
        for (i, line) in choices.iter().enumerate() {
            let (name, side) = (&line.name, &line.side);
            let (x, y, w) = (mx + 4.0 + (i / rows) as f32 * col_w, my + 28.0 + (i % rows) as f32 * 28.0, col_w - 8.0);
            let ctl = Ctl::ScreenItem(i);
            let on = i == cur;
            let hot = self.cx.hovered(ctl) || (self.w.kb && focus == i);
            let (fill, ink, check) = match () {
                _ if on && t.hc => (Some(b.hi), b.on_acc, b.on_acc),
                _ if on => (Some(b.track), b.tx, b.acc),
                _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx, b.acc),
                _ => (None, b.tx, b.acc),
            };
            if let Some(f) = fill {
                self.cx.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
            }
            if on {
                self.cx.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, check);
            }
            // Add's lines: the app's icon first, and the chip of where it starts before the side words.
            let mut nx = x + 30.0;
            if let Some(icon) = line.icon {
                self.cx.c.icon(x + 8.0, y + 6.0, 16.0, icon);
                nx = x + 32.0;
            }
            let side_chip = line.chip.filter(|_| add);
            if let Some(k) = line.chip.filter(|_| !add) {
                self.ws_chip(nx, y + 5.0, k);
                nx += self.ws_chip_w(k) + 8.0;
            }
            let sw = if side.is_empty() { 0.0 } else { self.cx.c.width(side, F::Small) + 10.0 };
            let cw = side_chip.map_or(0.0, |k| self.ws_chip_w(k) + 6.0);
            let shown = self.cx.c.fit(name, F::Body, x + w - nx - 6.0 - sw - cw);
            let ink = if line.grey && !(on && t.hc) { if add { b.tx3 } else { b.tx2 } } else { ink };
            self.cx.c.text(nx, y, 28.0, &shown, F::Body, ink);
            if let Some(k) = side_chip {
                self.ws_chip(x + w - sw - cw, y + 5.0, k);
            }
            if !side.is_empty() {
                self.cx.c.text(x + w - sw, y, 28.0, side, F::Small, if on && t.hc { b.on_acc } else { b.tx2 });
            }
            if self.w.kb && focus == i {
                self.cx.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.cx.hit(ctl, (x, y, w, 28.0), 3);
        }
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.cx.hit(Ctl::ScreenItem(usize::MAX), (mx, my, mw, mh), 3);
    }

    /// A mark on the ring or square: a white disc with a dark halo, filled in the middle.
    pub(super) fn mark(&self, x: f32, y: f32, r: f32, fill: u32, focus: bool) {
        self.cx.c.circle(x, y, r + 1.5, Some(ui::alpha(0, 0.45)), None);
        self.cx.c.circle(x, y, r, Some(op(0xFFFFFF)), None);
        self.cx.c.circle(x, y, r - 3.0, Some(op(fill)), None);
        if focus {
            self.cx.c.circle(x, y, r + 4.5, None, Some((op(self.cx.c.t.b.tx), 2.0)));
        }
    }
}

/// The screen menu's choices and the chosen one: All screens when the page's screen is gone.
pub(super) fn chosen(w: &Win, s: &Settings) -> (Vec<ScreenChoice>, usize) {
    let list = screen_choices(&w.pics.view, s);
    let i = w.screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// What leads a Wallpaper menu line (spec 0028 AC-11): a check (Windows wallpaper), a tick box
/// (a picture), a folder mark and a check (a slideshow folder), or Open folder's marks.
#[derive(Clone, Copy)]
pub(super) enum Lead {
    Check(bool),
    Tick(bool),
    Folder(bool),
    Open,
}

/// What a menu button shows before its name.
#[derive(Clone, Copy)]
pub(super) enum Mark {
    Chip(usize),
    Glyph(&'static str),
    Stack, // a Wallpaper row's set of pictures (spec 0028)
}

/// A line of the open screen menu: its name and the words on its right, and for the Apps
/// section's menus an app's icon, a workspace chip, or a grey name.
#[derive(Clone, Default)]
pub(super) struct Choice {
    pub(super) name: String,
    pub(super) side: String,
    icon: Option<HICON>,
    chip: Option<usize>,
    grey: bool,
}

impl Choice {
    pub(super) fn new(name: &str, side: &str) -> Choice {
        Choice { name: name.into(), side: side.into(), ..Choice::default() }
    }
}

/// The open screen menu's lines and the chosen one, if any: Where it shows' chooser, the
/// Names card's, or an Apps menu (spec 0027 AC-13).
pub(super) fn chooser_lines(w: &Win, s: &Settings, from: Ctl) -> (Vec<Choice>, Option<usize>) {
    match from {
        Ctl::NameScreens => {
            let (list, i) = names_chosen(w);
            (list.into_iter().map(|c| Choice::new(&c.name, &c.side)).collect(), Some(i))
        }
        Ctl::WallScreens => {
            let (list, i) = wall_chosen(w);
            (list.into_iter().map(|c| Choice::new(&c.name, &c.side)).collect(), Some(i))
        }
        Ctl::WallPick(k) => {
            // Windows wallpaper, the folder's pictures, its slideshow folders, Open folder. The
            // check marks Windows wallpaper or a folder; pictures show ticks instead (spec 0028).
            let (_, pick) = wall_held(s, wall_screen(w, s).as_deref(), k);
            let n = w.wall_files.len();
            let mut lines = vec![Choice::new("Windows wallpaper", "")];
            lines.extend(w.wall_files.iter().map(|f| Choice::new(f, "")));
            lines.extend(w.wall_dirs.iter().map(|(f, _)| Choice::new(f, "")));
            lines.push(Choice::new("Open folder", ""));
            let cur = match &pick {
                Pick::None => Some(0),
                Pick::Picture(p) => w.wall_files.iter().position(|f| f == p).map(|i| i + 1),
                Pick::Folder(d) => w.wall_dirs.iter().position(|(f, _)| f == d).map(|i| n + i + 1),
                Pick::Set(_) => None,
            };
            (lines, cur)
        }
        Ctl::AppWs(r) => {
            let Some((_, rule)) = s.apps.get(r) else { return (vec![], None) };
            let lines = (0..s.workspaces)
                .map(|k| {
                    let (name, grey) = ws_label(s, rule.screen.as_deref(), k);
                    Choice { name, chip: Some(k), grey, ..Choice::default() }
                })
                .collect();
            (lines, (rule.workspace < s.workspaces).then_some(rule.workspace))
        }
        Ctl::AppScreen(r) => {
            let Some((_, rule)) = s.apps.get(r) else { return (vec![], None) };
            let list = app_screens(&w.pics.view, rule);
            let i = list.iter().position(|(id, _)| *id == rule.screen);
            (list.into_iter().map(|(_, c)| c).collect(), i)
        }
        Ctl::AppAdd => {
            let list = addable(w, s);
            if list.is_empty() {
                return (vec![Choice { name: "Every open app has a rule".into(), grey: true, ..Choice::default() }], None);
            }
            let lines = list
                .into_iter()
                .map(|a| {
                    let screen = w.pics.view.present.iter().find(|p| p.device == a.device);
                    let side = format!("{} · {}", ws_label(s, screen.map(|p| p.id.as_str()), a.ws).0, screen.map_or("", |p| p.name.as_str()));
                    Choice { name: a.name.clone(), side, icon: Some(a.icon), chip: Some(a.ws), grey: false }
                })
                .collect();
            (lines, None)
        }
        _ => {
            let (list, i) = chosen(w, s);
            (list.into_iter().map(|c| Choice::new(&c.name, c.side)).collect(), Some(i))
        }
    }
}

/// After a screen's table changed: a screen with no other reason to be listed goes, the
/// chooser to All screens with the focus on it, or on Place when the chooser itself went.
pub(super) fn after_table(keep_focus: bool) {
    let _ = set(|w| {
        let list = screen_choices(&w.pics.view, &settings::get());
        let gone = w.screen.as_ref().is_some_and(|id| !list.iter().any(|c| c.id.as_ref() == Some(id)));
        if gone {
            w.screen = None;
        }
        if gone || keep_focus {
            w.focus = Some(if list.len() > 2 { Ctl::Screens } else { Ctl::Picture });
        }
    });
    layout();
}

pub(super) fn open_chooser(from: Ctl) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == from).map(|h| h.at)).flatten() else { return };
    // A Wallpaper menu lists the folder as it is now (AC-14), and its folders (spec 0028 AC-13).
    let files = matches!(from, Ctl::WallPick(_)).then(|| (wall_folder(), wall_dirs()));
    let _ = set(|w| {
        if let Some((files, dirs)) = files {
            w.wall_files = files;
            w.wall_dirs = dirs;
        }
        let (_, i) = chooser_lines(w, &settings::get(), from);
        // A set checks no line: the focus starts on its first ticked picture.
        let i = i.or_else(|| match chooser_wall(w).map(|k| wall_held(&settings::get(), wall_screen(w, &settings::get()).as_deref(), k).1) {
            Some(Pick::Set(v)) => w.wall_files.iter().position(|f| v.contains(f)).map(|p| p + 1),
            _ => None,
        });
        let i = i.unwrap_or(0);
        w.chooser = Some((from, at, i));
        wall_show_line(w, i, true);
    });
    layout();
}

/// A line of the open screen menu chosen. A new screen on the Names card drops the text of
/// boxes that failed to save (spec 0023 AC-13).
pub(super) fn choose_screen(i: usize) {
    if let Some(Some((Ctl::WallPick(k), ..))) = get(|w| w.chooser) {
        return choose_wall(k, i);
    }
    let apps = |c: Ctl| matches!(c, Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_));
    if let Some((from, edit)) = get(|w| w.chooser.filter(|(from, ..)| apps(*from)).map(|(from, ..)| (from, app_choice(w, &settings::get(), from, i)))).flatten() {
        let _ = set(|w| {
            w.chooser = None;
            w.focus = Some(from);
        });
        layout();
        // After the menu closes: a write reloads, and the reload lays the page out again.
        if let Some(edit) = edit {
            commit(edit, "Apps");
        }
        return;
    }
    let _ = set(|w| {
        let Some((from, ..)) = w.chooser else { return };
        if from == Ctl::NameScreens {
            let (list, cur) = names_chosen(w);
            if let Some(c) = list.get(i).filter(|_| i != cur) {
                w.names_screen = c.id.clone();
                w.unsaved.clear();
            }
        } else if from == Ctl::WallScreens {
            if let Some(c) = wall_chosen(w).0.get(i) {
                w.wall_screen = c.id.clone();
            }
        } else if let Some(c) = chosen(w, &settings::get()).0.get(i) {
            w.screen = c.id.clone();
        }
        w.chooser = None;
        w.focus = Some(from);
    });
    layout();
}

/// The screen menu's keys: Up and Down move, Enter or Space picks, Esc closes.
pub(super) fn chooser_key(vk: VIRTUAL_KEY) {
    let Some((from, at, f, n)) = get(|w| w.chooser.map(|(from, at, f)| (from, at, f, chooser_lines(w, &settings::get(), from).0.len()))).flatten() else { return };
    match vk {
        VK_ESCAPE => close_menu(),
        VK_UP | VK_DOWN | VK_HOME | VK_END => {
            let f = match vk {
                VK_UP => f.saturating_sub(1),
                VK_DOWN => (f + 1).min(n - 1),
                VK_HOME => 0,
                _ => n - 1,
            };
            let _ = set(|w| {
                w.chooser = Some((from, at, f));
                wall_show_line(w, f, false);
            });
            layout();
        }
        // Space on a picture ticks it; Enter picks just that one (spec 0028 AC-11).
        VK_SPACE if matches!(from, Ctl::WallPick(_)) && get(|w| (1..=w.wall_files.len()).contains(&f)) == Some(true) => tick_wall(f),
        VK_RETURN | VK_SPACE => choose_screen(f),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
