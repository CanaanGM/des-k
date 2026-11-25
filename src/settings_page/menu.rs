// The gesture picker's menu: its lines, drawn over the page, and what a line writes (spec 0012).
use super::*;
use super::recording::{Rec, start_rec};
use super::rows::spelled;

/// The gesture picker's menu, drawn over the page (as the design places it, inside the window).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Menu {
    pub(crate) row: usize,
    pub(crate) times: u8,
    pub(crate) family: Option<usize>, // a digit family's own list: Go to workspace and the two Send window to
    pub(crate) at: Bx,                // the picker it opened from
    pub(crate) focus: usize,
}

/// What a menu line does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Does {
    Set(Option<Action>), // None: no twice action
    Combo,               // Key combo…: records one
    Family(usize),
    Twice, // opens the done twice menu
}

pub(crate) struct Line {
    pub(crate) text: String,
    pub(crate) col: u8, // 0 des-k, 1 Windows and media, 2 the footer, 3 the done twice line
    pub(crate) does: Does,
    pub(crate) on: bool,
    pub(crate) default: bool,
}

type Family = fn(usize) -> Action;
pub(crate) const FAMILIES: [(&str, Family); 3] = [("Go to workspace", Action::Jump), ("Send window to", |i| Action::SendTo(i, true)), ("Send window to … and stay", |i| Action::SendTo(i, false))];

/// No twice rule and a twice rule that does nothing behave apart (the second still waits), so
/// their words must not look alike (spec 0012 AC-12).
const NO_TWICE: &str = "No twice action";
const TWICE_NOTHING: &str = "Nothing (still waits)";

pub(crate) fn once_label(a: Action) -> String {
    match a {
        Action::Combo(c) => format!("Types {}", spelled(&crate::hints::caps(&c))),
        a => crate::hints::name(a),
    }
}

pub(crate) fn twice_label(a: Option<Action>) -> String {
    match a {
        None => NO_TWICE.into(),
        Some(Action::Nothing) => TWICE_NOTHING.into(),
        Some(a) => once_label(a),
    }
}

/// A des-k action's name in the menu: shorter than the picker's, as the design lists them.
fn menu_name(a: Action) -> String {
    match a {
        Action::Send(d, follow) => format!("Send window to {}{}", if d < 0 { "previous" } else { "next" }, if follow { "" } else { " and stay" }),
        a => once_label(a),
    }
}

fn is_desk(a: Action) -> bool {
    matches!(a, Action::Switch(_) | Action::Back | Action::Send(..) | Action::Jump(_) | Action::SendTo(..) | Action::Finder | Action::Overview | Action::Pin)
}

/// The open menu's lines, in keyboard order.
pub(crate) fn menu_lines(m: &Menu, s: &Settings) -> Vec<Line> {
    let (fingers, motion) = settings::GESTURE_ROWS[m.row];
    let now = s.rule(fingers, motion, m.times);
    let default = DEFAULTS.rule(fingers, motion, m.times);
    let line = |text: String, col: u8, does: Does, a: Option<Action>| Line { text, col, does, on: now == a, default: default == a && a.is_some() };
    let mut out = vec![];
    if let Some(f) = m.family {
        let (_, make) = FAMILIES[f];
        for i in 0..9 {
            let a = make(i);
            out.push(line(once_label(a), 0, Does::Set(Some(a)), Some(a)));
        }
        return out;
    }
    let named: Vec<Action> = settings::actions().filter(|a| *a != Action::Nothing).collect();
    // The design's order: moving first, then the finder and overview, then sending.
    let desk = [Action::Switch(1), Action::Switch(-1), Action::Back, Action::Overview, Action::Finder, Action::Send(1, true), Action::Send(-1, true), Action::Send(1, false), Action::Send(-1, false), Action::Pin];
    for a in desk {
        out.push(line(menu_name(a), 0, Does::Set(Some(a)), Some(a)));
    }
    for (i, (name, make)) in FAMILIES.iter().enumerate() {
        let on = (0..9).any(|k| now == Some(make(k)));
        out.push(Line { text: name.to_string(), col: 0, does: Does::Family(i), on, default: (0..9).any(|k| default == Some(make(k))) });
    }
    for a in named.iter().filter(|a| !is_desk(**a)) {
        out.push(line(menu_name(*a), 1, Does::Set(Some(*a)), Some(*a)));
    }
    let combo = match now {
        Some(Action::Combo(c)) => format!("Key combo… ({})", spelled(&crate::hints::caps(&c))),
        _ => "Key combo…".into(),
    };
    out.push(Line { text: combo, col: 2, does: Does::Combo, on: matches!(now, Some(Action::Combo(_))), default: false });
    if m.times == 1 {
        // A row with no once rule in the file shows Nothing.
        out.push(Line { text: "Nothing".into(), col: 2, does: Does::Set(Some(Action::Nothing)), on: now.is_none() || now == Some(Action::Nothing), default: default == Some(Action::Nothing) });
        let twice = twice_label(s.rule(fingers, motion, 2));
        out.push(Line { text: format!("Done twice: {twice}"), col: 3, does: Does::Twice, on: false, default: false });
    } else {
        out.push(line(NO_TWICE.into(), 2, Does::Set(None), None));
        out.push(line(TWICE_NOTHING.into(), 2, Does::Set(Some(Action::Nothing)), Some(Action::Nothing)));
    }
    out
}

impl Ctx<'_, '_> {
    /// The open menu, over everything: its lines in two columns and a footer, the chosen one
    /// checked, the default marked, kept inside the window.
    pub(super) fn menu(&mut self) {
        let Some(m) = self.w.menu else { return };
        let t = self.cx.c.t;
        let b = t.b;
        let lines = menu_lines(&m, self.s);
        let (fingers, motion) = settings::GESTURE_ROWS[m.row];
        let dir = if motion == Motion::Tap { "tap".to_string() } else { format!("swipe {}", motion.word()) };
        let head = match m.family {
            Some(f) => FAMILIES[f].0.to_string(),
            None if m.times == 2 => format!("{fingers} fingers, {dir}, done twice"),
            None => format!("{fingers} fingers, {dir}"),
        };
        let small = m.family.is_some();
        let mw = if small { 272.0 } else { 480.0 };
        let colw = if small { mw - 8.0 } else { (mw - 8.0 - 4.0) / 2.0 };
        let n0 = lines.iter().filter(|l| l.col == 0).count();
        let n1 = lines.iter().filter(|l| l.col == 1).count();
        let rows = n0.max(n1) as f32;
        let feet = lines.iter().any(|l| l.col == 2) as u8 as f32 + lines.iter().any(|l| l.col == 3) as u8 as f32;
        let mh = 4.0 + 24.0 + if small { 0.0 } else { 24.0 } + rows * 28.0 + feet * (9.0 + 28.0) + 4.0;
        // Like a Windows combo box: the chosen line over the picker, inside the window.
        let chosen = lines.iter().position(|l| l.on && l.col < 2);
        let off = chosen.map_or(0.0, |i| {
            let l = &lines[i];
            let k = lines.iter().take(i).filter(|x| x.col == l.col).count();
            4.0 + 24.0 + if small { 0.0 } else { 24.0 } + k as f32 * 28.0
        });
        let my = (m.at.1 - off).clamp(8.0, (self.w.vh - mh - 8.0).max(8.0));
        let mx = m.at.0.min(self.w.vw - mw - 16.0).max(NAV);
        self.cx.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.cx.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.cx.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.cx.c.text(mx + 14.0, my + 4.0, 24.0, &head, F::SmallBold, b.tx2);
        let mut ys = [my + 28.0; 2];
        if !small {
            self.cx.c.text(mx + 14.0, my + 28.0, 24.0, "des-k", F::SmallBold, b.tx2);
            self.cx.c.text(mx + 4.0 + colw + 4.0 + 10.0, my + 28.0, 24.0, "Windows and media", F::SmallBold, b.tx2);
            ys = [my + 52.0; 2];
        }
        let foot_y = my + 4.0 + 24.0 + if small { 0.0 } else { 24.0 } + rows * 28.0;
        let mut fx = 0.0;
        let mut drew_rule = [false; 2];
        let focus_line = m.focus;
        for (i, l) in lines.iter().enumerate() {
            let (x, y, w) = match l.col {
                0 | 1 => {
                    let c = l.col as usize;
                    let y = ys[c];
                    ys[c] += 28.0;
                    (mx + 4.0 + c as f32 * (colw + 4.0), y, colw)
                }
                c => {
                    let row = (c - 2) as usize;
                    let y = foot_y + row as f32 * 37.0 + 9.0;
                    if !drew_rule[row] {
                        self.cx.c.rule(mx + 4.0, y - 5.0, mw - 8.0, op(b.line));
                        drew_rule[row] = true;
                        fx = 0.0;
                    }
                    let per = lines.iter().filter(|x| x.col == c).count() as f32;
                    let w = (mw - 8.0 - 4.0 * (per - 1.0)) / per;
                    let x = mx + 4.0 + fx;
                    fx += w + 4.0;
                    (x, y, w)
                }
            };
            let ctl = Ctl::Item(i);
            let hot = self.cx.hovered(ctl) || (self.w.kb && focus_line == i);
            let (fill, ink, check) = match () {
                _ if l.on && t.hc => (Some(b.hi), b.on_acc, b.on_acc),
                _ if l.on => (Some(b.track), b.tx, b.acc),
                _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx, b.acc),
                _ => (None, b.tx, b.acc),
            };
            if let Some(f) = fill {
                self.cx.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
            }
            if l.on {
                self.cx.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, check);
            }
            let tx = x + 30.0;
            let more = matches!(l.does, Does::Family(_) | Does::Twice);
            let room = w - 30.0 - 6.0 - if l.default { 52.0 } else { 0.0 } - if more { 16.0 } else { 0.0 };
            let shown = self.cx.c.fit(&l.text, F::Body, room);
            self.cx.c.text(tx, y, 28.0, &shown, F::Body, ink);
            let mut rx = x + w - 10.0;
            if more {
                let cw = self.cx.c.width(I_MORE, F::IconTiny);
                rx -= cw;
                self.cx.icon(rx, y, 28.0, I_MORE, F::IconTiny, b.tx2);
                rx -= 6.0;
            }
            if l.default {
                let dw = self.cx.c.width("Default", F::Small);
                self.cx.c.text(rx - dw, y, 28.0, "Default", F::Small, b.tx2);
            }
            if self.w.kb && focus_line == i {
                self.cx.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.cx.hit(ctl, (x, y, w, 28.0), 3);
        }
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.cx.hit(Ctl::Item(usize::MAX), (mx, my, mw, mh), 3);
    }
}

/// One gesture row's once or twice action; None: no twice rule. The whole map is written again,
/// every row's once rule included.
pub(super) fn set_gesture(row: usize, times: u8, action: Option<Action>) {
    if let Some(edit) = gesture_edit(row, times, action) {
        commit(edit, "Gestures");
    }
}

pub(crate) fn gesture_edit(row: usize, times: u8, action: Option<Action>) -> Option<Edit> {
    let s = settings::get();
    let (fingers, motion) = settings::GESTURE_ROWS[row];
    let now = s.rule(fingers, motion, times);
    // A row with no once rule in the file shows Nothing; picking Nothing writes it down.
    if now == action && (times == 2 || now.is_some()) {
        return None;
    }
    Some(Edit::Gestures(settings::full_map(|f, m, t| if (f, m, t) == (fingers, motion, times) { action } else { s.rule(f, m, t) })))
}

/// Opens a picker's menu, its chosen line with the keyboard focus.
pub(super) fn open_menu(row: usize, times: u8, family: Option<usize>) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == Ctl::Picker(row)).map(|h| h.at)).flatten() else { return };
    let m = Menu { row, times, family, at, focus: 0 };
    let focus = menu_lines(&m, &settings::get()).iter().position(|l| l.on).unwrap_or(0);
    let _ = set(|w| w.menu = Some(Menu { focus, ..m }));
    layout();
}

pub(super) fn close_menu() {
    let _ = set(|w| {
        w.menu = None;
        w.chooser = None;
    });
    layout();
}

/// The gesture menu, the screen menu or the color picker: while one is open, only it takes clicks.
pub(super) fn menu_open(w: &Win) -> bool {
    w.menu.is_some() || w.chooser.is_some() || w.pick.is_some()
}

/// A menu line chosen, by click or Enter.
pub(super) fn choose(i: usize) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let lines = menu_lines(&m, &settings::get());
    let Some(l) = lines.get(i) else { return };
    match l.does {
        Does::Set(a) => {
            close_menu();
            set_gesture(m.row, m.times, a);
        }
        Does::Combo => start_rec(Rec::Gesture(m.row, m.times)),
        Does::Family(f) => open_menu(m.row, m.times, Some(f)),
        Does::Twice => open_menu(m.row, 2, None),
    }
}

#[cfg(test)]
mod tests;
