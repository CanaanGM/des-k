// The Names card: workspace names and screen renames, per screen or for all (spec 0023).
use super::*;
use crate::settings_page::rows::Card;

const NAME_W: f32 = 300.0; // a name box (spec 0023)

// ---- Names (spec 0023 AC-12, AC-13) ----

/// A line of the Names card's chooser: All screens (no id), then the screens plugged in, then
/// remembered ones, each by its shown name. Nothing comes from the indicator's settings.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct NameChoice {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) windows: String, // Windows' own name: the Screen name box's hint
    pub(crate) side: String,
}

pub(crate) fn name_choices(view: &crate::ws::View) -> Vec<NameChoice> {
    let mut out = vec![NameChoice { id: None, name: "All screens".into(), windows: String::new(), side: String::new() }];
    for p in &view.present {
        out.push(NameChoice { id: Some(p.id.clone()), name: p.name.clone(), windows: p.windows_name.clone(), side: String::new() });
    }
    for w in view.waiting.iter().filter(|w| !view.present.iter().any(|p| p.id == w.id)) {
        out.push(NameChoice { id: Some(w.id.clone()), name: w.name.clone(), windows: w.windows_name.clone(), side: "Not plugged in".into() });
    }
    // Two screens you gave the same name are told apart by Windows' own.
    let names: Vec<String> = out.iter().map(|c| c.name.clone()).collect();
    for c in out.iter_mut().skip(1) {
        if names.iter().skip(1).filter(|n| **n == c.name).count() > 1 {
            c.side = c.windows.clone();
        }
    }
    out
}

/// A row of the Names card: the chosen screen's own name, or workspace N (0 based).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Nm {
    Screen,
    Ws(usize),
}

/// How a name row reads, set by the file's keys alone (AC-12).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum NameState {
    All,    // an All screens row: [names] wsN
    Follow, // a screen's row with no key of its own: it shows the All screens name
    Own,    // a screen's own name, even one equal to All screens'
    NoName, // "" on this screen: only the number shows
    Rename, // the Screen name row
}

/// Row `row` for `screen` (None: All screens): its state and the text its box holds when saved.
pub(crate) fn name_held(s: &Settings, screen: Option<&str>, row: Nm) -> (NameState, String) {
    let own = screen.and_then(|id| s.names_screens.get(id));
    match (screen, row) {
        (_, Nm::Screen) => (NameState::Rename, own.and_then(|n| n.screen.clone()).unwrap_or_default()),
        (None, Nm::Ws(i)) => (NameState::All, s.names[i].clone()),
        (Some(_), Nm::Ws(i)) => match own.and_then(|n| n.ws[i].as_deref()) {
            None => (NameState::Follow, String::new()),
            Some("") => (NameState::NoName, String::new()),
            Some(t) => (NameState::Own, t.to_string()),
        },
    }
}

/// The file key a name row writes: `names.wsN`, or in the screen's own table.
pub(crate) fn name_path(screen: Option<&str>, row: Nm) -> Option<Vec<String>> {
    let key = match row {
        Nm::Screen => "screen".to_string(),
        Nm::Ws(i) => format!("ws{}", i + 1),
    };
    match screen {
        Some(id) => Some(vec!["names".into(), "screens".into(), id.into(), key]),
        None if row == Nm::Screen => None,
        None => Some(vec!["names".into(), key]),
    }
}

/// What saving `text` in a name box writes, if anything (AC-12, AC-13). Text is cleaned as the
/// file's reader cleans it, so a box never saves what reads back different. An emptied box
/// takes its key out (an own row then follows all screens again); Enter on a following row left
/// empty writes "" (no name on this screen), and only when All screens has a name to hide;
/// unchanged text writes nothing.
pub(crate) fn name_edit(s: &Settings, screen: Option<&str>, row: Nm, text: &str, enter: bool) -> Option<Edit> {
    let text = settings::parse::clean_name(text);
    let path = name_path(screen, row)?;
    let (state, saved) = name_held(s, screen, row);
    match (state, row) {
        (NameState::Follow, Nm::Ws(i)) if text.is_empty() => (enter && !s.names[i].is_empty()).then(|| Edit::At(path, "".into())),
        (NameState::NoName, _) if text.is_empty() => None,
        _ if text.is_empty() => (!saved.is_empty()).then_some(Edit::Remove(path)),
        _ => (text != saved).then(|| Edit::At(path, text.into())),
    }
}

/// The title a failed save's note names: "Couldn't save “Workspace 2 name”" (AC-13).
pub(in crate::settings_page) fn name_title(row: Nm) -> String {
    match row {
        Nm::Screen => "Screen name".into(),
        Nm::Ws(i) => format!("Workspace {} name", i + 1),
    }
}

impl Ctx<'_, '_> {
    // ---- Names (spec 0023 AC-12, AC-13) ----

    /// The Names card. With two or more known screens: the chooser, then for a chosen screen its
    /// Screen name row, then a row per workspace in use. With one: that screen's Screen name
    /// row, then All screens' rows.
    pub(super) fn names(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let (list, i) = names_chosen(self.w);
        let two = list.len() > 2;
        self.out.cards.push((Card::Names, y - self.top0));
        self.header(y, "Names", None, "");
        if two {
            let who = if i == 0 { "all screens".to_string() } else { list[i].name.clone() };
            let rw = self.cx.c.width("For ", F::Small) + self.cx.c.width(&who, F::SmallBold);
            self.cx.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[("For ", F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
        }
        let mut h = 38.0;
        if two {
            h += self.names_chooser_row(y + h, &list, i);
        }
        let mut rows = if (two && i > 0) || list.len() == 2 { vec![Nm::Screen] } else { vec![] };
        rows.extend((0..self.s.workspaces).map(Nm::Ws));
        for (k, row) in rows.into_iter().enumerate() {
            if k > 0 || two {
                self.sep(y + h);
            }
            h += self.name_row(y + h, row);
        }
        h
    }

    fn names_chooser_row(&mut self, y: f32, list: &[NameChoice], i: usize) -> f32 {
        let cap = match &list[i].id {
            None => "Names every screen shows, unless it has its own. Up to 32 characters each; most emoji count as two.".to_string(),
            Some(id) => {
                let own = self.s.names_screens.get(id);
                match (0..self.s.workspaces).filter(|&k| own.is_some_and(|o| o.ws[k].is_some())).count() {
                    0 => "Empty rows follow all screens.".to_string(),
                    1 => "1 workspace differs on this screen. Empty rows follow all screens.".to_string(),
                    n => format!("{n} workspaces differ on this screen. Empty rows follow all screens."),
                }
            }
        };
        let name = list[i].name.clone();
        self.cx.say(Ctl::NameScreens, UIA_ComboBoxControlTypeId, "Screen", &cap, false, (None, Some(name.clone())));
        self.row(y, "Screen", &[t(cap)], (240.0, 32.0), |c, x, y| c.screen_picker(Ctl::NameScreens, x, y, 240.0, &name), None)
    }

    /// One name row: its title and caption, the chip of a workspace row, the box, and the
    /// focused box's note.
    fn name_row(&mut self, y: f32, row: Nm) -> f32 {
        let look = name_look(self.w, self.s, row);
        let ctl = Ctl::Name(row);
        let focused = self.w.focus == Some(ctl) && !self.cx.ro;
        let text = if focused { self.w.typed.clone().unwrap_or_else(|| look.shown.clone()) } else { look.shown.clone() };
        // No link takes focus on a read only page.
        let same = |k: usize, ro: bool| (!ro).then(|| P::Link(" Same as all screens".into(), Ctl::NameSame(k)));
        let cap: Vec<P> = match (look.state, row) {
            _ if look.unsaved => {
                let back = if look.saved.is_empty() { "Esc empties it again.".to_string() } else { format!("Esc puts back {}.", look.saved) };
                vec![P::Why("Not saved.".into()), t(format!(" Enter in the box tries again; {back}"))]
            }
            (NameState::Rename, _) => vec![t("Shown everywhere des-k names this screen. Empty: Windows' name.")],
            (NameState::All, Nm::Ws(k)) => self.differ_pieces(k),
            (NameState::Follow, _) if look.all.is_empty() => vec![t("Follows all screens, which has no name for it.")],
            (NameState::Follow, _) => vec![t("Follows all screens.")],
            (NameState::Own, Nm::Ws(k)) => [Some(t("Its own name on this screen.")), same(k, self.cx.ro)].into_iter().flatten().collect(),
            (NameState::NoName, Nm::Ws(k)) => [Some(t("Only the number shows here.")), same(k, self.cx.ro)].into_iter().flatten().collect(),
            _ => vec![],
        };
        let note = focused.then(|| {
            let line = match (look.state, row) {
                (NameState::Follow, _) if !look.all.is_empty() && text.is_empty() => "Type a name for this screen, or press Enter with the box empty: no name on this screen.".to_string(),
                (NameState::Own, _) if text.is_empty() => {
                    let all = if look.all.is_empty() { String::new() } else { format!(" ({})", look.all) };
                    format!("Enter saves it empty: this row follows all screens again{all}. Esc puts back {}.", look.saved)
                }
                (NameState::All, Nm::Ws(k)) if !look.saved.is_empty() && text.is_empty() => {
                    format!("Enter saves it empty: no name for workspace {} on screens that follow all screens. Esc puts back {}.", k + 1, look.saved)
                }
                _ => format!("Enter saves it; Esc puts back the saved name. Leaving the box saves it too. {} of {}.", text.encode_utf16().count(), settings::NAME_MAX),
            };
            NoteSpec { tone: Tone::Info, icon: I_INFO, title: String::new(), lines: vec![t(line)], buttons: vec![], group: 1 }
        });
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str() }).collect();
        self.cx.say(ctl, UIA_EditControlTypeId, &name_title(row), help.trim(), self.cx.ro, (None, Some(text.clone())));
        let (title, chip) = match row {
            Nm::Screen => ("Screen name".to_string(), 0.0),
            Nm::Ws(k) => (format!("Workspace {}", k + 1), self.ws_chip_w(k) + 10.0),
        };
        self.row(y, &title, &cap, (chip + NAME_W, 32.0), |c, x, y| {
            if let Nm::Ws(k) = row {
                c.ws_chip(x, y + 7.0, k);
            }
            c.name_box(ctl, x + chip, y, &text, &look.hint, focused);
        }, note.as_ref())
    }

    /// An All screens row's caption: each screen whose own key differs (AC-12).
    fn differ_pieces(&self, k: usize) -> Vec<P> {
        let line: Vec<String> = self
            .s
            .names_screens
            .iter()
            .filter_map(|(id, o)| match o.ws[k].as_deref()? {
                "" => Some(format!("{}: no name there.", self.screen_called(id))),
                own => Some(format!("{} has its own: {own}.", self.screen_called(id))),
            })
            .collect();
        if line.is_empty() { vec![] } else { vec![t(line.join(" "))] }
    }

    /// A name box, drawn as the popup words' box: an empty one shows its hint. On a read only
    /// page names are greyed, empty boxes show no hint, and no box takes focus.
    fn name_box(&mut self, ctl: Ctl, x: f32, y: f32, text: &str, hint: &str, focused: bool) {
        let b = self.cx.c.t.b;
        let bx = (x, y, NAME_W, 32.0);
        if !self.cx.ro {
            self.cx.hit(ctl, bx, 1);
        }
        self.cx.field(bx, b.card, None);
        if focused {
            self.cx.c.fill_rr((x + 1.0, y + 29.0, NAME_W - 2.0, 2.0), 0.0, op(b.acc));
        }
        let room = NAME_W - 24.0;
        let shown = self.cx.c.fit(text, F::Body, room);
        if !text.is_empty() {
            self.cx.c.text(x + 12.0, y, 32.0, &shown, F::Body, if self.cx.ro { b.tx3 } else { b.tx });
        } else if !self.cx.ro {
            self.cx.c.text(x + 12.0, y, 32.0, &self.cx.c.fit(hint, F::Body, room), F::Body, b.hint);
        }
        if focused {
            let tw = if text.is_empty() { 0.0 } else { self.cx.c.width(&shown, F::Body) + 1.0 };
            self.cx.c.fill_rr((x + 12.0 + tw, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
    }
}

/// The Names card's choices and the chosen one: All screens when the card's screen is gone.
pub(in crate::settings_page) fn names_chosen(w: &Win) -> (Vec<NameChoice>, usize) {
    let list = name_choices(&w.pics.view);
    let i = w.names_screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// A name row as the Names card draws it and the text box takes it (spec 0023 AC-12, AC-13).
pub(in crate::settings_page) struct NameLook {
    pub(in crate::settings_page) screen: Option<String>, // the screen the row writes for; None: All screens
    state: NameState,
    saved: String, // what its box holds when saved
    pub(in crate::settings_page) shown: String, // what it shows without focus: your text that failed to save, else the saved
    unsaved: bool,
    pub(in crate::settings_page) hint: String, // what it shows empty
    all: String,  // All screens' name for its workspace
}

pub(in crate::settings_page) fn name_look(w: &Win, s: &Settings, row: Nm) -> NameLook {
    let (list, i) = names_chosen(w);
    // With one known screen there is no chooser: its Screen name row, then All screens' rows.
    let screen = match row {
        _ if list.len() > 2 => list[i].id.clone(),
        Nm::Screen => list.get(1).and_then(|c| c.id.clone()),
        Nm::Ws(_) => None,
    };
    let (state, saved) = name_held(s, screen.as_deref(), row);
    let kept = w.unsaved.iter().find(|u| u.0 == screen && u.1 == row).map(|u| u.2.clone());
    let all = match row {
        Nm::Ws(k) => s.names[k].clone(),
        Nm::Screen => String::new(),
    };
    let hint = match state {
        NameState::All => "Add a name".to_string(),
        NameState::Follow | NameState::Own if all.is_empty() => "No name · all screens".to_string(),
        NameState::Follow | NameState::Own => format!("{all} · all screens"),
        NameState::NoName => "No name on this screen".to_string(),
        NameState::Rename => list.iter().find(|c| c.id == screen).map(|c| c.windows.clone()).unwrap_or_default(),
    };
    NameLook { screen, state, unsaved: kept.is_some(), shown: kept.unwrap_or_else(|| saved.clone()), saved, hint, all }
}

/// Same as all screens on a Names row: the screen's key goes, so the row follows all screens
/// again, and any text of yours that failed to save there goes with it.
pub(in crate::settings_page) fn same_name(k: usize) {
    let Some(screen) = get(|w| name_look(w, &settings::get(), Nm::Ws(k)).screen).flatten() else { return };
    let _ = set(|w| w.unsaved.retain(|u| !(u.0.as_deref() == Some(screen.as_str()) && u.1 == Nm::Ws(k))));
    if let Some(path) = name_path(Some(&screen), Nm::Ws(k)) {
        commit(Edit::Remove(path), &name_title(Nm::Ws(k)));
    }
}

#[cfg(test)]
mod tests;
