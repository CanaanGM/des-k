// The settings page (spec 0012): every setting in settings.toml, changed by clicking, saved at
// once and in effect at once. Built to the approved design (pdesk-design/settings/
// micro-suite-r4.html): a band, a side list of sections, and one scrolling column of cards.
// One drawn window with its own focus list and no child controls, so everything scrolls
// together; it reuses the welcome screen's theme, fonts and shapes. Lives on the main thread:
// its keys reach wndproc through main's loop. Sizes are in dip, scaled to the screen's dpi
// when drawn. The page never keeps its own copy of a value: it draws what settings::get()
// holds, and every change goes through settings::write.
use std::cell::RefCell;
use std::sync::LazyLock;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::settings::{self, Edit, Motion, Settings};
use crate::welcome::{Bx, Canvas, F, Fonts, Theme, alpha, buffered, dark_title, mix, op, theme};

const CLASS: PCWSTR = w!("pdesk-settings");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);

// The window's client area and its parts, in dip.
const W: f32 = 760.0;
const H: f32 = 620.0;
const BAND: f32 = 56.0;
const SIDE: f32 = 184.0;
const CX: f32 = SIDE + 24.0; // the content column
const CW: f32 = W - CX - 30.0;
const VIEW: f32 = H - BAND; // the visible height of the content

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Workspaces,
    Shortcuts,
    Indicator,
    Restore,
    Overview,
    Touchpad,
}

const SECTIONS: [Section; 6] = [Section::Workspaces, Section::Shortcuts, Section::Indicator, Section::Restore, Section::Overview, Section::Touchpad];

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Workspaces => "Workspaces",
            Section::Shortcuts => "Shortcuts",
            Section::Indicator => "Indicator",
            Section::Restore => "Restore",
            Section::Overview => "Overview",
            Section::Touchpad => "Touchpad",
        }
    }

    /// [restore] acts only when pdesk starts; everything else applies at once (AC-4).
    fn now(self) -> bool {
        self != Section::Restore
    }

    fn intro(self, s: &Settings) -> String {
        match self {
            Section::Workspaces => "Every screen has its own row of workspaces, all the same length.".into(),
            Section::Shortcuts => "The combos pdesk listens for. No combo means off. Recording new ones comes with the shortcut recorder; until then, change them in settings.toml.".into(),
            Section::Indicator => "The dots that show which workspace you are on after a switch, a jump or a send.".into(),
            Section::Restore => "pdesk reads these once, when it starts.".into(),
            Section::Overview => match s.overview.first() {
                Some(c) => format!("What {c} shows: every workspace of every screen."),
                None => "Every workspace of every screen at once. It has no combo now (Shortcuts).".into(),
            },
            Section::Touchpad => "Three and four finger gestures on a precision touchpad.".into(),
        }
    }
}

/// Which section a problem belongs to, by its key; None: no one setting (the Notes card, AC-2).
fn home(key: &str) -> Option<Section> {
    Some(match key.split('.').next().unwrap_or("") {
        "workspaces" | "copilot_as_right_ctrl" | "follow_activation" => Section::Workspaces,
        "shortcuts" => Section::Shortcuts,
        "indicator" => Section::Indicator,
        "restore" => Section::Restore,
        "overview" => Section::Overview,
        "touchpad" | "gesture" => Section::Touchpad,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Kind {
    Toggle,
    /// A number in spec 0001's range, in fixed steps (AC-10).
    Step { lo: i64, hi: i64, by: i64, unit: &'static str },
    /// The words the file takes, each with its label.
    Choice(&'static [(&'static str, &'static str)]),
}

struct Row {
    key: &'static str,
    title: &'static str,
    desc: &'static str,
    kind: Kind,
}

const LABEL_WORDS: &[(&str, &str)] = &[("hover", "On hover"), ("always", "Always"), ("none", "None")];
const ANIMATION_WORDS: &[(&str, &str)] = &[("off", "At once"), ("fade", "Fade in")];

const fn row(key: &'static str, title: &'static str, desc: &'static str, kind: Kind) -> Row {
    Row { key, title, desc, kind }
}

const fn step(lo: i64, hi: i64, by: i64, unit: &'static str) -> Kind {
    Kind::Step { lo, hi, by, unit }
}

/// Every plain setting, in page order.
const ROWS: &[(Section, Row)] = &[
    (Section::Workspaces, row("follow_activation", "Follow a window", "When a hidden window comes to the front, go to its workspace.", Kind::Toggle)),
    (Section::Workspaces, row("copilot_as_right_ctrl", "Copilot key works as Right Ctrl", "So Win+Ctrl shortcuts work with it.", Kind::Toggle)),
    (Section::Indicator, row("indicator.enabled", "Show the workspace dots", "", Kind::Toggle)),
    (Section::Indicator, row("indicator.hold_ms", "Hold", "how long the dots stay", step(0, 5000, 50, "ms"))),
    (Section::Indicator, row("indicator.fade_ms", "Fade", "how long they take to go", step(0, 2000, 50, "ms"))),
    (Section::Restore, row("restore.enabled", "Put windows back after a restart", "After a reboot or sign in, reopened windows go back to their workspaces, hidden.", Kind::Toggle)),
    (Section::Restore, row("restore.minutes", "Keep matching for", "after that, windows open where Windows puts them", step(1, 30, 1, "min"))),
    (Section::Overview, row("overview.labels", "Window names", "", Kind::Choice(LABEL_WORDS))),
    (Section::Overview, row("overview.animation", "Opening", "", Kind::Choice(ANIMATION_WORDS))),
    (Section::Overview, row("overview.live", "Live window list", "Off: the overview shows the windows as they were when it opened.", Kind::Toggle)),
    (Section::Touchpad, row("touchpad.enabled", "Read touchpad gestures", "Three and four finger gestures, also over admin windows.", Kind::Toggle)),
    (Section::Touchpad, row("touchpad.swipe_mm", "Swipe distance", "how far your fingers travel before a swipe counts", step(5, 50, 1, "mm"))),
    (Section::Touchpad, row("touchpad.twice_ms", "Wait for a second go", "only gestures with a twice action wait; their once action runs after it", step(100, 1000, 50, "ms"))),
];

/// The 11 [shortcuts] entries: key and title.
const SHORTCUTS: [(&str, &str); 11] = [
    ("shortcuts.previous", "Previous workspace"),
    ("shortcuts.next", "Next workspace"),
    ("shortcuts.send_previous", "Send window to previous, and go"),
    ("shortcuts.send_next", "Send window to next, and go"),
    ("shortcuts.send_previous_stay", "Send window to previous, stay"),
    ("shortcuts.send_next_stay", "Send window to next, stay"),
    ("shortcuts.jump", "Go to workspace 1 to 9"),
    ("shortcuts.send_to", "Send window to 1 to 9, and go"),
    ("shortcuts.send_to_stay", "Send window to 1 to 9, stay"),
    ("shortcuts.finder", "Finder"),
    ("shortcuts.overview", "Overview"),
];

static DEFAULTS: LazyLock<Settings> = LazyLock::new(Settings::default);

fn find_row(key: &str) -> Option<&'static Row> {
    ROWS.iter().map(|(_, r)| r).find(|r| r.key == key)
}

/// A setting with a place of its own on the page; its problems show under it.
fn on_row(key: &str) -> bool {
    key == "workspaces" || find_row(key).is_some() || SHORTCUTS.iter().any(|(k, _)| *k == key)
}

fn word<T: Copy + PartialEq>(list: &[(&'static str, T)], v: T) -> &'static str {
    list.iter().find(|(_, x)| *x == v).map_or("", |(w, _)| w)
}

/// A row's value as a number: on and off as 1 and 0, a choice as its index.
fn value(s: &Settings, key: &str) -> i64 {
    let index = |w: &str, words: &[(&str, &str)]| words.iter().position(|(x, _)| *x == w).unwrap_or(0) as i64;
    match key {
        "follow_activation" => s.follow_activation as i64,
        "copilot_as_right_ctrl" => s.copilot_as_right_ctrl as i64,
        "indicator.enabled" => s.indicator_enabled as i64,
        "indicator.hold_ms" => s.hold_ms as i64,
        "indicator.fade_ms" => s.fade_ms as i64,
        "restore.enabled" => s.restore_enabled as i64,
        "restore.minutes" => s.restore_minutes as i64,
        "overview.labels" => index(word(settings::LABELS, s.overview_labels), LABEL_WORDS),
        "overview.animation" => index(word(settings::ANIMATIONS, s.overview_animation), ANIMATION_WORDS),
        "overview.live" => s.overview_live as i64,
        "touchpad.enabled" => s.touchpad_enabled as i64,
        "touchpad.swipe_mm" => s.swipe_mm as i64,
        "touchpad.twice_ms" => s.twice_ms as i64,
        _ => 0,
    }
}

fn file_value(row: &Row, v: i64) -> toml_edit::Value {
    match row.kind {
        Kind::Toggle => (v != 0).into(),
        Kind::Step { .. } => v.into(),
        Kind::Choice(words) => words[v as usize].0.into(),
    }
}

fn shown(row: &Row, v: i64) -> String {
    match row.kind {
        Kind::Toggle => if v != 0 { "On" } else { "Off" }.into(),
        Kind::Step { unit, .. } => format!("{v} {unit}"),
        Kind::Choice(words) => words.get(v as usize).map_or("", |(_, l)| l).into(),
    }
}

/// The next value on the step grid. A hand written value off the grid steps onto it (AC-10).
fn stepped(v: i64, lo: i64, hi: i64, by: i64, up: bool) -> i64 {
    let off = v.rem_euclid(by);
    let n = if up { v - off + by } else if off != 0 { v - off } else { v - by };
    n.clamp(lo, hi)
}

/// A shortcut entry's combos as the file spells them; a digit entry as "Win + 1…9".
fn combos(s: &Settings, key: &str) -> Vec<String> {
    let list = |l: &[settings::Combo]| l.iter().map(|c| c.to_string()).collect();
    let digits = |m: Option<u8>| m.map(|m| format!("{} + 1…9", settings::mods_text(Some(m)))).into_iter().collect();
    match key {
        "shortcuts.previous" => list(&s.previous),
        "shortcuts.next" => list(&s.next),
        "shortcuts.send_previous" => list(&s.send_previous),
        "shortcuts.send_next" => list(&s.send_next),
        "shortcuts.send_previous_stay" => list(&s.send_previous_stay),
        "shortcuts.send_next_stay" => list(&s.send_next_stay),
        "shortcuts.jump" => digits(s.jump),
        "shortcuts.send_to" => digits(s.send_to),
        "shortcuts.send_to_stay" => digits(s.send_to_stay),
        "shortcuts.finder" => list(&s.finder),
        "shortcuts.overview" => list(&s.overview),
        _ => vec![],
    }
}

/// Something you can click, focus from the keyboard, or both.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Side(Option<Section>), // None: the Notes item
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Stepper(&'static str), // focus only: a number's − and + are one stop, Left and Right step
    Choice(&'static str, usize),
    Pick(&'static str), // focus only: a choice's buttons are one stop, Left and Right move
    Reset(&'static str),
    Open,    // Open settings.toml
    Dismiss, // the save error banner
    Setup,   // Touchpad setup: the welcome screen
}

impl Ctl {
    fn key(self) -> Option<&'static str> {
        match self {
            Ctl::Toggle(k) | Ctl::Minus(k) | Ctl::Plus(k) | Ctl::Stepper(k) | Ctl::Choice(k, _) | Ctl::Pick(k) | Ctl::Reset(k) => Some(k),
            _ => None,
        }
    }

    /// Changes the file; a read only page still lets you focus it.
    fn writes(self) -> bool {
        self.key().is_some()
    }

    /// The keyboard stop this control belongs to: one per row.
    fn stop(self) -> Ctl {
        match self {
            Ctl::Minus(k) | Ctl::Plus(k) => Ctl::Stepper(k),
            Ctl::Choice(k, _) => Ctl::Pick(k),
            c => c,
        }
    }

    /// In the side area (the side list and its file button), else in the settings.
    fn in_side(self) -> bool {
        matches!(self, Ctl::Side(_) | Ctl::Open)
    }
}

/// Where a control sits, in window dip. Group: 0 the side list, 1 the content, 2 the footer.
#[derive(Clone, Copy)]
struct Hit {
    ctl: Ctl,
    at: Bx,
    group: u8,
}

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    scroll: f32,
    height: f32, // the content's full height
    tops: Vec<(Option<Section>, f32)>, // where each section starts in the content; None: the notes
    hits: Vec<Hit>,
    focus: Option<Ctl>,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    drag: Option<f32>, // dragging the scroll thumb: where in the thumb it was grabbed
    error: Option<(String, String)>, // the last save that failed: what stays, and why
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

// Never hold the borrow across a call that comes back here: settings::write reloads, and the
// reload calls refresh().
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    WIN.with(|w| w.try_borrow().ok().and_then(|w| w.as_ref().map(f)))
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WIN.with(|w| w.try_borrow_mut().ok().and_then(|mut w| w.as_mut().map(f)))
}

/// What one pass of compose found: the controls, the content's height, the section tops.
struct Out {
    hits: Vec<Hit>,
    height: f32,
    tops: Vec<(Option<Section>, f32)>,
}

/// One pass over the page, drawing or only measuring.
struct Ctx<'a, 'f> {
    c: &'a mut Canvas<'f>,
    w: &'a Win,
    s: &'static Settings,
    ro: bool,
    top0: f32, // where the content starts in the window, after scrolling
    out: Out,
}

impl Ctx<'_, '_> {
    fn hit(&mut self, ctl: Ctl, at: Bx, group: u8) {
        self.out.hits.push(Hit { ctl, at, group });
    }

    /// Lays `f` out without drawing it or keeping its controls, for a box that needs its
    /// height before its contents.
    fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32 {
        let draw = std::mem::replace(&mut self.c.draw, false);
        let (hits, tops) = (self.out.hits.len(), self.out.tops.len());
        let h = f(self);
        self.out.hits.truncate(hits);
        self.out.tops.truncate(tops);
        self.c.draw = draw;
        h
    }

    fn focused(&self, ctl: Ctl) -> bool {
        self.w.kb && self.w.focus == Some(ctl)
    }

    fn hovered(&self, ctl: Ctl) -> bool {
        self.w.hover == Some(ctl)
    }

    fn ring(&self, (x, y, w, h): Bx, r: f32, ctl: Ctl) {
        if self.focused(ctl) {
            self.c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), r + 3.0, 2.0, op(self.c.t.accent));
        }
    }

    /// A pill button. Returns its width.
    fn button(&mut self, ctl: Ctl, x: f32, y: f32, w: Option<f32>, label: &str, group: u8) -> f32 {
        let t = self.c.t;
        let w = w.unwrap_or_else(|| self.c.width(label, F::Body) + 28.0);
        let fill = if self.hovered(ctl) { mix(t.btn, t.text, 0.08) } else { t.btn };
        self.c.boxed((x, y, w, 32.0), 16.0, fill, Some(t.btn_edge));
        let lw = self.c.width(label, F::Body);
        self.c.text(x + (w - lw) / 2.0, y, 32.0, label, F::Body, t.text);
        self.ring((x, y, w, 32.0), 16.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), group);
        w
    }

    /// The on and off switch, with its word on the left; `x` is the switch's left edge.
    fn switch(&mut self, key: &'static str, x: f32, y: f32, on: bool) {
        let t = self.c.t;
        let ctl = Ctl::Toggle(key);
        let label = if on { "On" } else { "Off" };
        let lw = self.c.width(label, F::Body);
        self.c.text(x - 10.0 - lw, y, 20.0, label, F::Body, t.text);
        let track = (x, y, 40.0, 20.0);
        if on {
            self.c.boxed(track, 10.0, if self.ro { t.disabled } else { t.accent }, None);
            self.c.circle(x + 30.0, y + 10.0, 6.0, Some(op(t.on_accent)), None);
        } else {
            let fill = if self.hovered(ctl) { mix(t.card, t.text, 0.08) } else { t.card };
            self.c.boxed(track, 10.0, fill, Some(t.stroke));
            self.c.circle(x + 10.0, y + 10.0, 6.0, Some(op(t.dim)), None);
        }
        self.ring(track, 10.0, ctl);
        self.hit(ctl, (x - 10.0 - lw, y - 4.0, 50.0 + lw, 28.0), 1);
    }

    /// − value +. `x`, `y`: the pill's top left.
    fn stepper(&mut self, key: &'static str, x: f32, y: f32, text: &str, down: bool, up: bool) {
        let t = self.c.t;
        self.c.boxed((x, y, 124.0, 30.0), 15.0, t.btn, Some(t.btn_edge));
        let tw = self.c.width(text, F::Body);
        self.c.text(x + (124.0 - tw) / 2.0, y, 30.0, text, F::Body, t.text);
        for (ctl, cx, live) in [(Ctl::Minus(key), x + 16.0, down), (Ctl::Plus(key), x + 108.0, up)] {
            let cy = y + 15.0;
            if self.hovered(ctl) && live && !self.ro {
                self.c.circle(cx, cy, 11.0, Some(op(mix(t.btn, t.text, 0.12))), None);
            }
            let col = op(if live && !self.ro { t.text } else { t.disabled });
            self.c.lines(&[(cx - 4.5, cy), (cx + 4.5, cy)], 1.5, col);
            if matches!(ctl, Ctl::Plus(_)) {
                self.c.lines(&[(cx, cy - 4.5), (cx, cy + 4.5)], 1.5, col);
            }
            self.hit(ctl, (cx - 16.0, y, 32.0, 30.0), 1);
        }
        self.ring((x, y, 124.0, 30.0), 15.0, Ctl::Stepper(key));
    }

    /// Choice buttons in one pill. Returns its width.
    fn choice(&mut self, key: &'static str, x: f32, y: f32, words: &[(&str, &str)], sel: usize) -> f32 {
        let t = self.c.t;
        let widths: Vec<f32> = words.iter().map(|(_, l)| self.c.width(l, F::Body) + 28.0).collect();
        let total = widths.iter().sum::<f32>() + 8.0;
        self.c.boxed((x, y, total, 34.0), 17.0, t.card, Some(t.stroke));
        let mut ox = x + 4.0;
        for (i, ((_, label), w)) in words.iter().zip(&widths).enumerate() {
            let ctl = Ctl::Choice(key, i);
            let bx = (ox, y + 4.0, *w, 26.0);
            let fg = if i == sel {
                self.c.boxed(bx, 13.0, if self.ro { t.disabled } else { t.accent }, None);
                t.on_accent
            } else {
                if self.hovered(ctl) && !self.ro {
                    self.c.boxed(bx, 13.0, mix(t.card, t.text, 0.08), None);
                }
                t.text
            };
            self.c.text(ox + 14.0, y + 4.0, 26.0, label, F::Body, fg);
            self.hit(ctl, bx, 1);
            ox += w;
        }
        self.ring((x, y, total, 34.0), 17.0, Ctl::Pick(key));
        total
    }

    /// What a row adds under itself: its default and Reset when it differs (AC-11), and its
    /// problems in the log's words (AC-14). Returns their height.
    fn extras(&mut self, key: &'static str, x: f32, y: f32, w: f32) -> f32 {
        let t = self.c.t;
        let mut h = 0.0;
        let problems: Vec<&settings::Problem> = settings::problems().iter().filter(|p| p.key == key).collect();
        if let Some(row) = find_row(key) {
            let (v, d) = (value(self.s, key), value(&DEFAULTS, key));
            // With a problem the value in use is already the default; the note says so.
            if v != d && problems.is_empty() {
                let text = format!("default: {}", shown(row, d));
                let tw = self.c.width(&text, F::Small);
                self.c.text(x, y, 22.0, &text, F::Small, t.dim);
                let ctl = Ctl::Reset(key);
                let lw = self.c.width("Reset", F::SmallBold);
                let lx = x + tw + 14.0;
                let col = if self.ro { t.disabled } else { t.accent };
                self.c.text(lx, y, 22.0, "Reset", F::SmallBold, col);
                if self.hovered(ctl) && !self.ro {
                    self.c.lines(&[(lx, y + 17.0), (lx + lw, y + 17.0)], 1.0, op(col));
                }
                self.ring((lx - 4.0, y + 1.0, lw + 8.0, 20.0), 4.0, ctl);
                self.hit(ctl, (lx - 4.0, y, lw + 8.0, 22.0), 1);
                h += 22.0;
            }
        }
        for p in problems {
            h += 2.0 + self.c.para(x, y + h + 2.0, w, 17.0, &[("!", F::SmallBold, t.chip_on_fg), (&format!(" {}", p.message), F::Small, t.chip_on_fg)]);
        }
        h
    }

    fn row(&mut self, row: &'static Row, y: f32) -> f32 {
        let t = self.c.t;
        let (x, w) = (CX + 16.0, CW - 32.0);
        let name = format!(" {}", row.key.rsplit('.').next().unwrap_or(row.key));
        let v = value(self.s, row.key);
        let mut h = 14.0;
        match row.kind {
            Kind::Toggle => {
                let tw = w - 120.0;
                h += self.c.para(x, y + h, tw, 20.0, &[(row.title, F::Bold, t.text), (&name, F::Mono, t.dim)]);
                if !row.desc.is_empty() {
                    h += self.c.para(x, y + h, tw, 17.0, &[(row.desc, F::Small, t.dim)]);
                }
                self.switch(row.key, x + w - 40.0, y + 14.0 + ((h - 14.0 - 20.0) / 2.0).max(0.0), v != 0);
            }
            Kind::Step { lo, hi, by, unit } => {
                // The picture: the range as a bar, filled up to the value.
                let f = ((v - lo) as f32 / (hi - lo) as f32).clamp(0.0, 1.0);
                self.c.boxed((x, y + h, w, 8.0), 4.0, t.tile, Some(t.card_edge));
                if f > 0.0 {
                    self.c.fill_rr((x, y + h, (w * f).max(8.0), 8.0), 4.0, op(t.accent));
                }
                h += 10.0;
                let hi_text = format!("{hi} {unit}");
                self.c.text(x, y + h, 18.0, &lo.to_string(), F::Small, t.dim);
                self.c.text(x + w - self.c.width(&hi_text, F::Small), y + h, 18.0, &hi_text, F::Small, t.dim);
                h += 24.0;
                let desc = format!(" {}", row.desc);
                h += self.c.para(x, y + h, w, 20.0, &[(row.title, F::Bold, t.text), (&name, F::Mono, t.dim), (&desc, F::Small, t.dim)]);
                h += 6.0;
                self.stepper(row.key, x, y + h, &format!("{v} {unit}"), v > lo, v < hi);
                h += 30.0;
                let _ = by;
            }
            Kind::Choice(words) => {
                h += self.c.para(x, y + h, w, 20.0, &[(row.title, F::Bold, t.text), (&name, F::Mono, t.dim)]);
                h += 8.0;
                self.choice(row.key, x, y + h, words, v as usize);
                h += 34.0;
            }
        }
        let e = self.measured(|c| c.extras(row.key, x, 0.0, w));
        if e > 0.0 {
            self.extras(row.key, x, y + h + 6.0, w);
            h += 6.0 + e;
        }
        h + 14.0
    }

    fn line(&self, y: f32) {
        self.c.lines(&[(CX + 1.0, y), (CX + CW - 1.0, y)], 1.0, op(self.c.t.card_edge));
    }

    /// A card around `body`, which draws from its top and returns its height.
    fn card(&mut self, y: f32, body: impl Fn(&mut Self, f32) -> f32) -> f32 {
        let h = self.measured(|c| body(c, y));
        let t = self.c.t;
        self.c.boxed((CX, y, CW, h), 10.0, t.card, Some(t.card_edge));
        body(self, y);
        h
    }

    /// The section's plain rows, with lines between.
    fn rows(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = 0.0;
        for (i, (_, row)) in ROWS.iter().filter(|(s, _)| *s == sec).enumerate() {
            if i > 0 {
                self.line(y + h);
            }
            h += self.row(row, y + h);
        }
        h
    }

    /// A banner: a glyph, a bold title, lines under it, and an optional button on the right.
    fn banner(&mut self, y: f32, err: bool, title: &str, lines: &[(String, F)], button: Option<(Ctl, &str)>) -> f32 {
        let t = self.c.t;
        let bw = button.map_or(0.0, |(_, l)| self.c.width(l, F::Body) + 28.0 + 12.0);
        let (tx, tw) = (CX + 44.0, CW - 60.0 - bw);
        let body = |c: &mut Self| {
            let mut h = c.c.para(tx, y + 12.0, tw, 20.0, &[(title, F::Bold, t.text)]);
            for (text, f) in lines {
                h += 2.0 + c.c.para(tx, y + 12.0 + h + 2.0, tw, 17.0, &[(text, *f, t.dim)]);
            }
            h + 24.0
        };
        let h = self.measured(body).max(56.0);
        let (fill, fg) = if err { (t.chip_err, t.chip_err_fg) } else { (t.warn, t.text) };
        let edge = if err { mix(t.chip_err, t.chip_err_fg, 0.4) } else { t.warn_edge };
        self.c.boxed((CX, y, CW, h), 10.0, fill, Some(edge));
        let glyph = if err { "×" } else { "!" };
        self.c.circle(CX + 24.0, y + 22.0, 11.0, None, Some((op(fg), 1.5)));
        let gw = self.c.width(glyph, F::Glyph);
        self.c.text(CX + 24.0 - gw / 2.0, y + 12.0, 20.0, glyph, F::Glyph, fg);
        body(self);
        if let Some((ctl, label)) = button {
            self.button(ctl, CX + CW - bw, y + 12.0, None, label, 1);
        }
        h
    }

    /// The heading with its "applies" chip (AC-4), the intro, and the section's own notes.
    fn heading(&mut self, sec: Section, y: f32) -> f32 {
        let t = self.c.t;
        self.out.tops.push((Some(sec), y - self.top0));
        self.c.text(CX, y, 32.0, sec.name(), F::Title, t.text);
        let (label, fill, fg) = if sec.now() { ("Applies now", t.chip_off, t.chip_off_fg) } else { ("Applies from pdesk's next start", t.chip_on, t.chip_on_fg) };
        let x = CX + self.c.width(sec.name(), F::Title) + 12.0;
        let check = if sec.now() { 15.0 } else { 0.0 };
        let cw = self.c.width(label, F::SmallBold) + 20.0 + check;
        self.c.boxed((x, y + 5.0, cw, 22.0), 11.0, fill, if t.hc { Some(t.stroke) } else { None });
        if sec.now() {
            self.c.check(x + 9.0, y + 11.0, 9.0, 1.6, op(fg));
        }
        self.c.text(x + 10.0 + check, y + 5.0, 22.0, label, F::SmallBold, fg);
        let mut h = 36.0;
        h += self.c.para(CX, y + h, CW, 18.0, &[(&sec.intro(self.s), F::Small, t.dim)]);
        h += 12.0;
        // Problems of this section that no row on the page owns, like a clash pdesk fixed.
        let notes: Vec<(String, F)> = settings::problems().iter().filter(|p| home(&p.key) == Some(sec) && !on_row(&p.key)).map(|p| (p.message.clone(), F::Small)).collect();
        if !notes.is_empty() {
            let title = if notes.len() == 1 { "A note about this section in settings.toml".to_string() } else { format!("{} notes about this section in settings.toml", notes.len()) };
            h += self.banner(y + h, false, &title, &notes, None) + 12.0;
        }
        h
    }

    fn section(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = self.heading(sec, y);
        h += match sec {
            Section::Workspaces => self.card(y + h, |c, y| {
                let t = c.c.t;
                let x = CX + 16.0;
                let n = c.s.workspaces.to_string();
                let mut h = 14.0;
                h += c.c.para(x, y + h, CW - 32.0, 20.0, &[("Every screen has ", F::Body, t.text), (&n, F::Bold, t.text), (" workspaces ", F::Body, t.text), ("(2 to 9) ", F::Small, t.dim), ("workspaces", F::Mono, t.dim)]);
                let e = c.measured(|c| c.extras("workspaces", x, 0.0, CW - 32.0));
                if e > 0.0 {
                    c.extras("workspaces", x, y + h + 6.0, CW - 32.0);
                    h += 6.0 + e;
                }
                h += 14.0;
                c.line(y + h);
                h + c.rows(Section::Workspaces, y + h)
            }),
            Section::Shortcuts => self.card(y + h, |c, y| {
                let mut h = 0.0;
                for (i, (key, title)) in SHORTCUTS.iter().enumerate() {
                    if i > 0 {
                        c.line(y + h);
                    }
                    h += c.shortcut(key, title, y + h);
                }
                h
            }),
            Section::Touchpad => {
                let mut g = self.card(y + h, |c, y| {
                    let h = c.rows(Section::Touchpad, y);
                    c.line(y + h);
                    h + c.setup_row(y + h)
                });
                g += 16.0;
                g + self.card(y + h + g, |c, y| c.gestures(y))
            }
            _ => self.card(y + h, |c, y| c.rows(sec, y)),
        };
        h
    }

    /// A shortcut entry and its combos, read only until the shortcut recorder (step 5).
    fn shortcut(&mut self, key: &'static str, title: &str, y: f32) -> f32 {
        let t = self.c.t;
        let (x, w) = (CX + 16.0, CW - 32.0);
        let name = format!(" {}", key.rsplit('.').next().unwrap_or(key));
        let mut h = 12.0;
        h += self.c.para(x, y + h, w, 20.0, &[(title, F::Bold, t.text), (&name, F::Mono, t.dim)]);
        h += 6.0;
        let list = combos(self.s, key);
        let mut cx = x;
        if list.is_empty() {
            self.c.text(cx, y + h, 24.0, "Off", F::Small, t.dim);
        }
        for combo in &list {
            let cw = self.c.width(combo, F::Small) + 20.0;
            self.c.boxed((cx, y + h, cw, 24.0), 6.0, t.tile, Some(t.card_edge));
            self.c.text(cx + 10.0, y + h, 24.0, combo, F::Small, t.text);
            cx += cw + 8.0;
        }
        h += 24.0;
        let e = self.measured(|c| c.extras(key, x, 0.0, w));
        if e > 0.0 {
            self.extras(key, x, y + h + 6.0, w);
            h += 6.0 + e;
        }
        h + 12.0
    }

    fn setup_row(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let (x, w) = (CX + 16.0, CW - 32.0);
        let label = "Touchpad setup";
        let bw = self.c.width(label, F::Body) + 28.0;
        let mut h = 14.0;
        h += self.c.para(x, y + h, w - bw - 16.0, 20.0, &[("Touchpad setup", F::Bold, t.text)]);
        h += self.c.para(x, y + h, w - bw - 16.0, 17.0, &[("Windows' own gestures, and giving them back.", F::Small, t.dim)]);
        self.button(Ctl::Setup, x + w - bw, y + 14.0 + ((h - 14.0 - 32.0) / 2.0).max(0.0), Some(bw), label, 1);
        h.max(14.0 + 32.0) + 14.0
    }

    /// The 10 gesture rows and what each does, read only until the gesture editor (step 6).
    fn gestures(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let x = CX + 16.0;
        let mut h = 14.0;
        h += self.c.para(x, y + h, CW - 32.0, 20.0, &[("Gestures", F::Bold, t.text), (" gesture", F::Mono, t.dim)]);
        h += self.c.para(x, y + h, CW - 32.0, 17.0, &[("Choosing them here comes with the gesture editor; until then, change them in settings.toml.", F::Small, t.dim)]);
        h += 6.0;
        let motions = [(Motion::Left, "swipe left"), (Motion::Right, "swipe right"), (Motion::Up, "swipe up"), (Motion::Down, "swipe down"), (Motion::Tap, "tap")];
        for fingers in [3, 4] {
            for (m, word) in motions {
                let name = format!("{fingers} fingers, {word}");
                self.c.text(x, y + h, 22.0, &name, F::Small, t.dim);
                h += self.c.para(x + 150.0, y + h + 2.5, CW - 32.0 - 150.0, 17.0, &[(&crate::welcome::does(fingers, m), F::Small, t.text)]).max(22.0);
            }
        }
        let e = self.measured(|c| c.extras("gesture", x, 0.0, CW - 32.0));
        if e > 0.0 {
            self.extras("gesture", x, y + h + 6.0, CW - 32.0);
            h += 6.0 + e;
        }
        h + 12.0
    }

    /// The side list item the view is in: the last section whose top has scrolled past.
    fn current(&self) -> Option<Option<Section>> {
        current(self.w.scroll, self.out.height, &self.out.tops)
    }

    fn icon(&self, sec: Option<Section>, x: f32, y: f32, col: u32) {
        let c = &self.c;
        let col = op(col);
        match sec {
            None => {
                c.circle(x + 8.0, y + 8.0, 7.0, None, Some((col, 1.4)));
                c.lines(&[(x + 8.0, y + 4.5), (x + 8.0, y + 9.0)], 1.6, col);
                c.circle(x + 8.0, y + 11.5, 0.9, Some(col), None);
            }
            Some(Section::Workspaces) => {
                for (dx, dy) in [(1.0, 1.0), (9.0, 1.0), (1.0, 9.0), (9.0, 9.0)] {
                    c.stroke_rr((x + dx, y + dy, 6.0, 6.0), 1.5, 1.3, col);
                }
            }
            Some(Section::Shortcuts) => {
                c.stroke_rr((x, y + 3.0, 16.0, 10.0), 2.0, 1.3, col);
                for dx in [4.0, 8.0, 12.0] {
                    c.circle(x + dx, y + 6.5, 0.8, Some(col), None);
                }
                c.lines(&[(x + 5.0, y + 10.0), (x + 11.0, y + 10.0)], 1.3, col);
            }
            Some(Section::Indicator) => {
                c.circle(x + 2.5, y + 8.0, 1.8, Some(col), None);
                c.fill_rr((x + 5.5, y + 6.2, 5.0, 3.6), 1.8, col);
                c.circle(x + 13.5, y + 8.0, 1.8, Some(col), None);
            }
            Some(Section::Restore) => {
                c.circle(x + 8.0, y + 8.0, 6.5, None, Some((col, 1.3)));
                c.lines(&[(x + 8.0, y + 4.5), (x + 8.0, y + 8.0), (x + 10.5, y + 9.5)], 1.3, col);
            }
            Some(Section::Overview) => {
                c.stroke_rr((x + 1.0, y + 1.0, 14.0, 14.0), 2.0, 1.3, col);
                c.lines(&[(x + 8.0, y + 1.0), (x + 8.0, y + 15.0)], 1.3, col);
                c.lines(&[(x + 1.0, y + 8.0), (x + 15.0, y + 8.0)], 1.3, col);
            }
            Some(Section::Touchpad) => {
                c.stroke_rr((x, y + 2.0, 16.0, 12.0), 2.0, 1.3, col);
                c.lines(&[(x + 3.0, y + 11.0), (x + 13.0, y + 11.0)], 1.3, col);
            }
        }
    }

    /// The side list, over the content's left edge, and the file at its foot.
    fn side(&mut self) {
        let t = self.c.t;
        self.c.boxed((0.0, BAND, SIDE, VIEW + 1.0), 0.0, t.foot, None);
        self.c.lines(&[(SIDE - 0.5, BAND), (SIDE - 0.5, H)], 1.0, op(t.card_edge));
        let problems = settings::problems();
        let count = |sec: Option<Section>| problems.iter().filter(|p| home(&p.key) == sec).count();
        let mut items: Vec<(Option<Section>, &str)> = vec![];
        if count(None) > 0 {
            items.push((None, "Notes"));
        }
        items.extend(SECTIONS.iter().map(|s| (Some(*s), s.name())));
        let current = self.current();
        let mut y = BAND + 12.0;
        for (sec, name) in items {
            let ctl = Ctl::Side(sec);
            let bx = (8.0, y, SIDE - 16.0, 36.0);
            let here = current == Some(sec);
            if here {
                self.c.boxed(bx, 8.0, mix(t.foot, t.accent, 0.18), if t.hc { Some(t.accent) } else { None });
                self.c.fill_rr((8.0, y + 9.0, 3.0, 18.0), 1.5, op(t.accent));
            } else if self.hovered(ctl) {
                self.c.boxed(bx, 8.0, mix(t.foot, t.text, 0.07), None);
            }
            self.icon(sec, 22.0, y + 10.0, if here { t.text } else { t.dim });
            self.c.text(48.0, y, 36.0, name, if here { F::Bold } else { F::Body }, t.text);
            let n = count(sec);
            if n > 0 {
                let label = n.to_string();
                let bw = (self.c.width(&label, F::SmallBold) + 12.0).max(20.0);
                self.c.boxed((SIDE - 20.0 - bw, y + 8.0, bw, 20.0), 10.0, t.chip_on, if t.hc { Some(t.stroke) } else { None });
                self.c.text(SIDE - 20.0 - bw + (bw - self.c.width(&label, F::SmallBold)) / 2.0, y + 8.0, 20.0, &label, F::SmallBold, t.chip_on_fg);
            }
            self.ring(bx, 8.0, ctl);
            self.hit(ctl, bx, 0);
            y += 40.0;
        }
        self.c.lines(&[(16.0, H - 104.0), (SIDE - 16.0, H - 104.0)], 1.0, op(t.card_edge));
        self.c.text(16.0, H - 96.0, 18.0, "%LOCALAPPDATA%\\pdesk\\", F::Mono, t.dim);
        self.c.text(16.0, H - 78.0, 18.0, "settings.toml", F::Mono, t.dim);
        self.button(Ctl::Open, 16.0, H - 50.0, Some(SIDE - 32.0), "Open settings.toml", 2);
    }

    /// The band: what this window is, and the one rule that matters (AC-4).
    fn band(&mut self) {
        let t = self.c.t;
        if t.hc {
            self.c.boxed((0.0, 0.0, W, BAND), 0.0, t.surface, None);
            self.c.lines(&[(0.0, BAND - 0.5), (W, BAND - 0.5)], 1.0, op(t.stroke));
        } else {
            self.c.gradient(0.0, 0.0, W, BAND, t.band_a, t.band_b);
        }
        self.c.fill_rr((16.0, 12.0, 32.0, 32.0), 8.0, alpha(t.on_band, 0.18));
        for (dx, dy) in [(23.0, 19.0), (32.0, 19.0), (23.0, 28.0), (32.0, 28.0)] {
            self.c.stroke_rr((dx, dy, 7.0, 7.0), 1.5, 1.4, op(t.on_band));
        }
        self.c.text(60.0, 5.0, 28.0, "Settings", F::Title, t.on_band);
        self.c.text(60.0, 32.0, 16.0, "pdesk · every change saves to settings.toml at once; Restore applies from the next start", F::Small, t.on_band);
    }

    fn scrollbar(&self) {
        if let Some((y, h)) = thumb(self.w.scroll, self.out.height) {
            self.c.fill_rr((W - 11.0, y, 5.0, h), 2.5, alpha(self.c.t.dim, if self.w.drag.is_some() { 0.8 } else { 0.5 }));
        }
    }
}

/// The side list item the view is in, for a scroll, content height and section tops.
fn current(scroll: f32, height: f32, tops: &[(Option<Section>, f32)]) -> Option<Option<Section>> {
    if scroll >= height - VIEW - 1.0 {
        return tops.last().map(|(s, _)| *s);
    }
    let pos = scroll + 60.0;
    tops.iter().take_while(|(_, top)| *top <= pos).last().or(tops.first()).map(|(s, _)| *s)
}

/// The scroll thumb's top and height, when the content is taller than the view.
fn thumb(scroll: f32, height: f32) -> Option<(f32, f32)> {
    (height > VIEW + 0.5).then(|| {
        let h = (VIEW * VIEW / height).max(32.0);
        (BAND + scroll / (height - VIEW) * (VIEW - h), h)
    })
}

/// The whole page, back to front: content first, then the band and side list over its edges.
fn compose(c: &mut Canvas, w: &Win) -> Out {
    let s = settings::get();
    let parse_error = settings::parse_error();
    let top0 = BAND - w.scroll;
    let mut x = Ctx { c, w, s, ro: parse_error.is_some(), top0, out: Out { hits: vec![], height: 0.0, tops: vec![] } };
    let mut y = top0 + 20.0;
    if let Some(why) = parse_error {
        let lines = [
            ("pdesk uses the settings shown below: the ones from before the file broke, or all the defaults if it was already broken when pdesk started. Fix the file and this page updates by itself.".to_string(), F::Small),
            (why, F::Mono),
        ];
        y += x.banner(y, true, "pdesk can't read settings.toml, so nothing here can change.", &lines, None) + 16.0;
    }
    if let Some((what, why)) = &w.error {
        y += x.banner(y, true, what, &[(why.clone(), F::Mono)], Some((Ctl::Dismiss, "Dismiss"))) + 16.0;
    }
    let notes: Vec<(String, F)> = settings::problems().iter().filter(|p| home(&p.key).is_none()).map(|p| (p.message.clone(), F::Small)).collect();
    if !notes.is_empty() {
        x.out.tops.push((None, y - top0));
        let title = if notes.len() == 1 { "1 note about settings.toml. It belongs to no one setting.".to_string() } else { format!("{} notes about settings.toml. They belong to no one setting.", notes.len()) };
        y += x.banner(y, false, &title, &notes, None) + 16.0;
    }
    for sec in SECTIONS {
        y += x.section(sec, y) + 32.0;
    }
    x.out.height = y - top0;
    x.band();
    x.side();
    x.scrollbar();
    x.out.hits.sort_by_key(|h| h.group);
    x.out
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the page out again for its scroll, and keeps the focus on a control that is still there.
fn layout() {
    for _ in 0..2 {
        let Some(out) = get(|w| unsafe {
            let dc = GetDC(Some(w.hwnd));
            let out = {
                let mut c = Canvas::new(dc, false, w.scale, w.theme, &w.fonts);
                compose(&mut c, w)
            };
            ReleaseDC(Some(w.hwnd), dc);
            out
        }) else {
            return;
        };
        let moved = set(|w| {
            let max = (out.height - VIEW).max(0.0);
            let scroll = w.scroll.clamp(0.0, max);
            let moved = scroll != w.scroll;
            w.scroll = scroll;
            w.height = out.height;
            w.tops = out.tops;
            w.hits = out.hits;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                // A Reset that just reset itself away: the focus moves to its row's control.
                w.focus = f.key().and_then(|k| w.hits.iter().find(|h| h.ctl.key() == Some(k)).map(|h| h.ctl.stop()));
            }
            moved
        });
        if moved != Some(true) {
            break;
        }
    }
    repaint();
}

/// Shows what pdesk read back from the file. Called after every reload.
pub fn refresh() {
    if hwnd().is_some() {
        layout();
    }
}

fn scroll_to(y: f32) {
    let _ = set(|w| w.scroll = y);
    layout();
}

fn scroll_by(dy: f32) {
    if let Some(y) = get(|w| w.scroll + dy) {
        scroll_to(y);
    }
}

/// Scrolls the focused control into view.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().find(|h| Some(h.ctl.stop()) == w.focus && h.group == 1)?;
        let (top, bottom) = (h.at.1 - 12.0, h.at.1 + h.at.3 + 12.0);
        if top < BAND {
            Some(top - BAND)
        } else if bottom > H {
            Some(bottom - H)
        } else {
            None
        }
    })
    .flatten();
    match dy {
        Some(dy) => scroll_by(dy),
        None => repaint(),
    }
}

fn jump(sec: Option<Section>) {
    if let Some(top) = get(|w| w.tops.iter().find(|(s, _)| *s == sec).map(|(_, t)| *t)).flatten() {
        scroll_to(top - 12.0);
    }
}

/// Writes one value; a failure shows what stays and why until the next good save.
fn save(key: &'static str, v: i64) {
    let Some(row) = find_row(key) else { return };
    let old = value(settings::get(), key);
    if v == old {
        return;
    }
    match settings::write(Edit::Value(key, file_value(row, v))) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Err(why) => {
            let what = format!("Not saved. {} stays {}.", row.title, shown(row, old).to_lowercase());
            let _ = set(|w| w.error = Some((what, why)));
            refresh();
        }
    }
}

fn step_key(key: &'static str, up: bool) {
    if let Some(Row { kind: Kind::Step { lo, hi, by, .. }, .. }) = find_row(key) {
        save(key, stepped(value(settings::get(), key), *lo, *hi, *by, up));
    }
}

fn act(ctl: Ctl) {
    if ctl.writes() && settings::parse_error().is_some() {
        return; // read only (AC-14)
    }
    match ctl {
        Ctl::Side(sec) => jump(sec),
        Ctl::Toggle(key) => save(key, 1 - value(settings::get(), key)),
        Ctl::Minus(key) => step_key(key, false),
        Ctl::Plus(key) => step_key(key, true),
        Ctl::Stepper(_) | Ctl::Pick(_) => {} // focus only: Left and Right change them
        Ctl::Choice(key, i) => save(key, i as i64),
        Ctl::Reset(key) => save(key, value(&DEFAULTS, key)),
        Ctl::Open => crate::tray::open(&settings::path()),
        Ctl::Dismiss => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Ctl::Setup => crate::welcome::show(false),
    }
}

/// The keyboard stops in one area, in order, each with its top: the side list and its file
/// button, or the settings.
fn stops(w: &Win, side: bool) -> Vec<(Ctl, f32)> {
    let mut out: Vec<(Ctl, f32)> = vec![];
    for h in w.hits.iter().filter(|h| h.ctl.in_side() == side) {
        let c = h.ctl.stop();
        if out.last().map(|l| l.0) != Some(c) {
            out.push((c, h.at.1));
        }
    }
    out
}

/// Focuses `c` and shows it; a side list item also jumps to its section.
fn focus_on(c: Ctl) {
    let _ = set(|w| w.focus = Some(c));
    match c {
        Ctl::Side(sec) => jump(sec),
        _ => reveal(),
    }
}

/// Where the keyboard goes next, as in Windows Settings: Tab between the side list and the
/// settings, Up and Down from row to row, Left and Right change the focused row's value.
fn next_stop(w: &Win, vk: VIRTUAL_KEY) -> Option<Ctl> {
    let side = w.focus.is_some_and(Ctl::in_side);
    // The settings row at the top of the view: where the keyboard lands in the settings.
    let first_seen = || {
        let list = stops(w, false);
        list.iter().find(|(_, y)| *y >= BAND).or(list.first()).map(|(c, _)| *c)
    };
    match vk {
        VK_TAB if side => first_seen(),
        VK_TAB => current(w.scroll, w.height, &w.tops).map(Ctl::Side),
        _ if w.focus.is_none() => first_seen(),
        _ => {
            let list = stops(w, side);
            let i = list.iter().position(|(c, _)| Some(*c) == w.focus)?;
            let j = match vk {
                VK_UP => i.checked_sub(1)?,
                VK_DOWN => i + 1,
                VK_HOME => 0,
                _ => list.len().checked_sub(1)?, // VK_END
            };
            list.get(j).map(|(c, _)| *c)
        }
    }
}

/// Left and Right on the focused row: a switch goes off or on, a number steps, a choice moves.
fn change(focus: Ctl, right: bool) {
    if settings::parse_error().is_some() {
        return; // read only (AC-14)
    }
    match focus {
        Ctl::Toggle(k) => save(k, right as i64),
        Ctl::Stepper(k) => step_key(k, right),
        Ctl::Pick(k) => {
            if let Some(Row { kind: Kind::Choice(words), .. }) = find_row(k) {
                let i = value(settings::get(), k) as usize;
                save(k, if right { (i + 1).min(words.len() - 1) } else { i.saturating_sub(1) } as i64);
            }
        }
        _ => return,
    }
    reveal();
}

fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    let focus = get(|w| w.focus).flatten();
    match vk {
        VK_ESCAPE => unsafe { let _ = DestroyWindow(h); },
        VK_TAB | VK_UP | VK_DOWN | VK_HOME | VK_END => match get(|w| next_stop(w, vk)).flatten() {
            Some(c) => focus_on(c),
            None => repaint(), // at the end already: just show the ring
        },
        VK_LEFT | VK_RIGHT => {
            if let Some(f) = focus {
                change(f, vk == VK_RIGHT);
            }
        }
        VK_SPACE | VK_RETURN => match focus {
            // A number or a choice changes with Left and Right only, so a stray Space never does.
            Some(Ctl::Stepper(_) | Ctl::Pick(_)) | None => {}
            Some(f) => {
                act(f);
                reveal();
            }
        },
        VK_PRIOR => scroll_by(-(VIEW - 40.0)),
        VK_NEXT => scroll_by(VIEW - 40.0),
        _ => {}
    }
}

fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. Content controls count only inside the content's view.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let shown = |h: &Hit| h.group != 1 || (x > SIDE && y > BAND);
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the page on the screen under the mouse, or brings the open one to the front (AC-1).
/// `section`: scroll to it.
pub fn open(section: Option<Section>) {
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        if section.is_some() {
            jump(section);
        }
        return;
    }
    crate::welcome::gdiplus();
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when the page opens a second time
        let (mon, _) = crate::ws::under_mouse();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("pdesk settings"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("settings page: no window: {e}"),
        };
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        WIN.set(Some(Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            scroll: 0.0,
            height: 0.0,
            tops: vec![],
            hits: vec![],
            focus: None,
            kb: false,
            hover: None,
            press: None,
            drag: None,
            error: None,
        }));
        let (ww, wh) = size(h);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        layout();
        if section.is_some() {
            jump(section);
        }
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
}

/// The window's outer size for its client area at its dpi.
fn size(h: HWND) -> (i32, i32) {
    unsafe {
        let dpi = GetDpiForWindow(h);
        let s = dpi as f32 / 96.0;
        let mut r = RECT { left: 0, top: 0, right: (W * s).round() as i32, bottom: (H * s).round() as i32 };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        (r.right - r.left, r.bottom - r.top)
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(h, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let _ = get(|w| {
                    buffered(dc, 0, 0, r.right, r.bottom, |mem| {
                        let mut c = Canvas::new(mem, true, w.scale, w.theme, &w.fonts);
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, w.theme.surface, None);
                        compose(&mut c, w);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_KEYDOWN => {
                on_key(h, VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetCapture(h);
                let _ = set(|w| {
                    let (x, y) = point(lp, w.scale);
                    w.kb = false;
                    match thumb(w.scroll, w.height) {
                        Some((ty, th)) if x >= W - 16.0 && y > BAND => {
                            // On the thumb: drag it; beside it: jump there, then drag.
                            let grab = if (ty..ty + th).contains(&y) { y - ty } else { th / 2.0 };
                            w.drag = Some(grab);
                        }
                        _ => {
                            w.press = hit_at(w, (x, y));
                            if w.press.is_some() {
                                w.focus = w.press.map(Ctl::stop);
                            }
                        }
                    }
                });
                // A click beside the thumb moves it there at once.
                let _ = PostMessageW(Some(h), WM_MOUSEMOVE, wp, lp);
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let drag = get(|w| {
                    let (_, y) = point(lp, w.scale);
                    let (_, th) = thumb(w.scroll, w.height)?;
                    let grab = w.drag?;
                    Some((y - grab - BAND) / (VIEW - th) * (w.height - VIEW))
                })
                .flatten();
                if let Some(to) = drag {
                    scroll_to(to);
                } else if set(|w| {
                    let hover = hit_at(w, point(lp, w.scale));
                    std::mem::replace(&mut w.hover, hover) != hover
                }) == Some(true)
                {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    repaint();
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let _ = set(|w| w.hover = None);
                repaint();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let click = set(|w| {
                    let was = w.press.take();
                    let dragged = w.drag.take().is_some();
                    let now = hit_at(w, point(lp, w.scale));
                    (!dragged && was.is_some() && was == now).then_some(was).flatten()
                })
                .flatten();
                if let Some(c) = click {
                    act(c);
                }
                repaint();
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                let _ = set(|w| w.drag = None);
                LRESULT(0)
            }
            WM_ACTIVATE => {
                refresh();
                LRESULT(0)
            }
            WM_DESTROY => {
                if let Some(w) = WIN.with_borrow_mut(Option::take) {
                    w.fonts.delete();
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let to = *(lp.0 as *const RECT);
                let scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                let _ = set(|w| {
                    w.fonts.delete();
                    w.scale = scale;
                    w.fonts = Fonts::new(scale);
                });
                let (ww, wh) = size(h);
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
                layout();
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                let t = theme();
                if set(|w| std::mem::replace(&mut w.theme, t) != t) == Some(true) {
                    dark_title(h, &t);
                    layout();
                }
                DefWindowProcW(h, m, wp, lp)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC-10: a hand written value off the grid is kept as is, and the next step lands on the
    // grid, not one step away from the odd value.
    #[test]
    fn steps_land_on_the_grid() {
        assert_eq!(stepped(555, 0, 5000, 50, false), 550);
        assert_eq!(stepped(555, 0, 5000, 50, true), 600);
        assert_eq!(stepped(550, 0, 5000, 50, true), 600);
        assert_eq!(stepped(550, 0, 5000, 50, false), 500);
    }

    // A stepper never writes a value the parser would refuse and reset.
    #[test]
    fn steps_stay_in_range() {
        assert_eq!(stepped(0, 0, 5000, 50, false), 0);
        assert_eq!(stepped(5000, 0, 5000, 50, true), 5000);
        assert_eq!(stepped(5, 5, 50, 1, false), 5);
    }

    // Every row writes a value the parser reads back as the same setting: a typo in a key or
    // a word here would save something pdesk ignores.
    #[test]
    fn every_row_round_trips_through_the_file() {
        for (_, row) in ROWS {
            let d = value(&DEFAULTS, row.key);
            let v = match row.kind {
                Kind::Toggle => 1 - d,
                Kind::Step { lo, hi, .. } => if d == hi { lo } else { hi },
                Kind::Choice(words) => (d + 1) % words.len() as i64,
            };
            let text = settings::edit_for_test(row.key, file_value(row, v));
            let (s, warn) = settings::parse_for_test(&text);
            assert!(warn.is_empty(), "{}: {warn:?}", row.key);
            assert_eq!(value(&s, row.key), v, "{}", row.key);
        }
    }

    // A problem is shown under its own setting, or in its section, or in the notes; never lost.
    #[test]
    fn problems_have_a_home() {
        assert_eq!(home("indicator.hold_ms"), Some(Section::Indicator));
        assert_eq!(home("shortcuts"), Some(Section::Shortcuts));
        assert_eq!(home("gesture"), Some(Section::Touchpad));
        assert_eq!(home(""), None); // unknown keys and gesture rule N
        assert!(on_row("workspaces") && on_row("shortcuts.next") && on_row("touchpad.twice_ms"));
        assert!(!on_row("shortcuts"));
    }
}
