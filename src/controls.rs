// The drawing context the settings page, the first run and What's new share (spec 0035 AC-7):
// a pass over one screen's canvas with its click areas, focus and hover, and what Narrator
// reads, over that screen's own control ids. The widgets drawn through it are in
// controls/widgets.rs. Each screen keeps its own context around this one, with its own content.
pub(crate) mod widgets;

use crate::ui::{Bx, Canvas};
use crate::uia::Node;
use windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID;

/// A screen's control id: one of its own `Ctl`s.
pub(crate) trait Id: Copy + PartialEq {
    /// Whether using it writes settings.toml, so a read only screen greys it.
    fn writes(self) -> bool;
}

/// A click area: the control, its box, and the screen's own grouping of it (the settings page's
/// and the first run's stop group, What's new's "inside the scrolled cards").
#[derive(Clone, Copy)]
pub(crate) struct Hit<C, E> {
    pub(crate) ctl: C,
    pub(crate) at: Bx,
    pub(crate) group: E,
}

/// One pass over a screen, drawing or only measuring.
pub(crate) struct Ctx<'a, 'f, C, E> {
    pub(crate) c: &'a mut Canvas<'f>,
    pub(crate) hits: Vec<Hit<C, E>>,
    pub(crate) said: Vec<(C, Node)>, // what Narrator reads for each stop, in order
    pub(crate) ro: bool,             // settings.toml can't be read: what writes it greys
    pub(crate) focus: Option<C>,
    pub(crate) hover: Option<C>,
    pub(crate) kb: bool, // the last input was a key, so the focus ring shows
    pub(crate) group: E, // the group the widgets' own hits go in
}

/// Where a measuring pass started, to undo what it added.
pub(crate) struct Mark {
    draw: bool,
    hits: usize,
    said: usize,
}

impl<C: Id, E: Copy> Ctx<'_, '_, C, E> {
    pub(crate) fn hit(&mut self, ctl: C, at: Bx, group: E) {
        self.hits.push(Hit { ctl, at, group });
    }

    /// What Narrator reads for a stop: kept once, the first time the stop is drawn.
    pub(crate) fn say(&mut self, ctl: C, role: UIA_CONTROLTYPE_ID, name: &str, help: &str, greyed: bool, state: (Option<bool>, Option<String>)) {
        if self.said.iter().any(|(c, _)| *c == ctl) {
            return;
        }
        let node = Node { role, name: name.to_string(), help: help.to_string(), greyed, focusable: true, toggled: state.0, value: state.1, ..Node::default() };
        self.said.push((ctl, node));
    }

    pub(crate) fn focused(&self, ctl: C) -> bool {
        self.kb && self.focus == Some(ctl)
    }

    pub(crate) fn hovered(&self, ctl: C) -> bool {
        self.hover == Some(ctl)
    }

    /// Stops drawing, for a measuring pass that `rewind` undoes.
    pub(crate) fn mark(&mut self) -> Mark {
        let draw = std::mem::replace(&mut self.c.draw, false);
        Mark { draw, hits: self.hits.len(), said: self.said.len() }
    }

    /// Drops the hits and Narrator entries added since `m`, and draws again if it did.
    pub(crate) fn rewind(&mut self, m: Mark) {
        self.hits.truncate(m.hits);
        self.said.truncate(m.said);
        self.c.draw = m.draw;
    }

    /// Lays `f` out without drawing it or keeping its controls, for a box that needs its
    /// height before its contents.
    pub(crate) fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32 {
        let m = self.mark();
        let h = f(self);
        self.rewind(m);
        h
    }
}

/// A piece of a caption: words, words in the text color (a reason), a link, monospaced words,
/// or a warning with its mark, in the warning color.
pub(crate) enum P<C> {
    T(String),
    Why(String),
    Link(String, C),
    Mono(String),
    Warn(String),
}

/// A note's look: its icon's color.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tone {
    Info,
    Warn,
    Good,
    Rec,
}

/// A note: its icon, title, words and buttons; `group` is its buttons' stop group.
pub(crate) struct NoteSpec<C, E> {
    pub(crate) tone: Tone,
    pub(crate) icon: &'static str,
    pub(crate) title: String,
    pub(crate) lines: Vec<P<C>>,
    pub(crate) buttons: Vec<(C, &'static str)>,
    pub(crate) group: E,
}

/// A screen's own drawing context: it holds a `Ctx` and lays its cards out in one column.
/// Rows and cards draw through it, since what goes in them is the screen's own.
pub(crate) trait Screen<'a, 'f: 'a>: Sized {
    type C: Id;
    type E: Copy;

    fn cx(&mut self) -> &mut Ctx<'a, 'f, Self::C, Self::E>;

    /// The column's left edge and width.
    fn column(&self) -> (f32, f32);

    /// Lays `f` out without drawing it or keeping anything it adds, the screen's own too.
    fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32;
}
