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
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings::{self, Action, Combo, Edit, Motion, Owner, Settings};
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
            Section::Shortcuts => "The combos pdesk listens for. Click a combo to record a new one in its place, or + Add for another. No combo means off.".into(),
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

/// The workspace count: drawn as the pictures' + Add and ×, not as a row of its own (AC-10).
const WORKSPACES: Row = row("workspaces", "Workspaces", "", step(2, 9, 1, "workspaces"));

fn find_row(key: &str) -> Option<&'static Row> {
    ROWS.iter().map(|(_, r)| r).chain([&WORKSPACES]).find(|r| r.key == key)
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
        "workspaces" => s.workspaces as i64,
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

/// A shortcut key's name in [shortcuts]: "shortcuts.finder" → "finder".
fn short(key: &str) -> &str {
    key.strip_prefix("shortcuts.").unwrap_or(key)
}

/// One of the three digit entries.
fn is_digits(key: &str) -> bool {
    settings::SETS.contains(&short(key))
}

/// A shortcut entry by its name in [shortcuts]: its page key and title.
fn entry(name: &str) -> (&'static str, &'static str) {
    SHORTCUTS.iter().find(|(k, _)| short(k) == name).copied().unwrap_or(("", ""))
}

/// A shortcut entry's combos as the file spells them; a digit entry as "Win + 1…9", with the
/// digits a list takes from it (AC-8).
fn combos(s: &Settings, key: &str) -> Vec<String> {
    let name = short(key);
    if !is_digits(key) {
        return s.list(name).iter().map(|c| c.to_string()).collect();
    }
    let Some(m) = s.set(name) else { return vec![] };
    let mut text = format!("{} + 1…9", settings::mods_text(Some(m)));
    let skips: Vec<String> = s.skips(name).iter().map(|(d, list)| format!("{d} ({})", entry(list).1)).collect();
    if !skips.is_empty() {
        text += &format!(", skips {}", skips.join(", "));
    }
    vec![text]
}

/// "Next workspace" in a sentence: "next workspace".
fn lower(title: &str) -> String {
    let mut c = title.chars();
    c.next().map_or(String::new(), |f| f.to_lowercase().chain(c).collect())
}

/// An action's label as a picker shows it: "Next workspace".
fn label(a: Action) -> String {
    let text = a.label();
    let mut c = text.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// What the recorder is recording for (spec 0012 AC-7, AC-12).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rec {
    List(&'static str, Option<usize>), // a shortcut list, and the combo it replaces (None: + Add)
    Digits(&'static str),
    Gesture(usize, u8), // a GESTURE_ROWS row and its times: Key combo…
}

/// A clash the recorder found, waiting for Move it here or Cancel (AC-9). Nothing is saved
/// until you choose.
struct Prompt {
    key: &'static str, // the entry it shows under
    title: String,
    sub: String,
    edit: Edit,
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
    Chip(&'static str, usize),   // a shortcut's combo: records a new one in its place
    Remove(&'static str, usize), // its ×
    Add(&'static str),           // + Add: records one more
    StopRec(&'static str),       // Cancel while recording
    Move(&'static str),          // the clash prompt's two buttons
    Cancel(&'static str),
    Once(usize), // a gesture row's pickers, by GESTURE_ROWS index
    Twice(usize),
    Play,        // the indicator's Play: the real dots on this screen (AC-19)
    DismissEdit, // the hand edit banner (AC-20)
}

impl Ctl {
    fn key(self) -> Option<&'static str> {
        match self {
            Ctl::Toggle(k) | Ctl::Minus(k) | Ctl::Plus(k) | Ctl::Stepper(k) | Ctl::Choice(k, _) | Ctl::Pick(k) | Ctl::Reset(k) => Some(k),
            Ctl::Chip(k, _) | Ctl::Remove(k, _) | Ctl::Add(k) | Ctl::StopRec(k) | Ctl::Move(k) | Ctl::Cancel(k) => Some(k),
            _ => None,
        }
    }

    /// Changes the file (or starts recording a change); a read only page still lets you focus it.
    fn writes(self) -> bool {
        !matches!(self, Ctl::StopRec(_) | Ctl::Cancel(_)) && (self.key().is_some() || matches!(self, Ctl::Once(_) | Ctl::Twice(_)))
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
    rec: Option<Rec>,
    refused: Option<(String, String)>, // the last key the recorder refused: title and line
    prompt: Option<Prompt>,
    pics: Pics,
    edited: Option<(String, Vec<String>)>, // the last hand edit's banner: its time and lines (AC-20)
}

/// What the workspace pictures draw, copied out of ws (AC-18), with what removing would move
/// (AC-6): to one fewer, and to the default.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::ScreenInfo>,
    view: crate::ws::View,
    pad: Option<crate::setup::State>, // the touchpad status line's source (AC-20)
}

/// The pictures' data now, or None while ws is busy (the next tick tries again).
fn pictures() -> Option<Pics> {
    let n = settings::get().workspaces;
    let view = crate::ws::view(&[n.saturating_sub(1).max(2), DEFAULTS.workspaces])?;
    Some(Pics { screens: crate::ws::screen_info(), view, pad: Some(crate::setup::state()) })
}

/// Takes a new copy of the pictures' data; lays the page out again only if it changed.
fn update_pictures(always: bool) {
    let changed = pictures().and_then(|new| {
        set(|w| {
            let changed = w.pics != new;
            w.pics = new;
            changed
        })
    });
    if changed == Some(true) || always {
        layout();
    }
}

/// Main loop, every second and on a display change: the pictures follow your windows while
/// the page is open and not minimized (AC-18).
pub fn tick() {
    if let Some(h) = hwnd()
        && !unsafe { IsIconic(h) }.as_bool()
    {
        update_pictures(false);
    }
}

/// A hand edit was put in effect: an open page shows what changed until Dismiss or the next
/// one (AC-20). Changes made on the page show none.
pub fn hand_edit(changes: &[settings::Change], moves: &[String]) {
    if hwnd().is_none() {
        return;
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let mut lines = Vec::new();
    for c in changes {
        lines.push(format!("{}: {} → {}", c.key, c.was, c.now));
        if c.key == "workspaces" {
            lines.extend(moves.iter().cloned());
        }
    }
    let _ = set(|w| w.edited = Some((format!("{:02}:{:02}", t.wHour, t.wMinute), lines)));
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

/// A sample window in the fixed drawings: its name, then left, top, width, height as parts of its tile.
type Sample = (&'static str, f32, f32, f32, f32);

/// The pictures' tile and window colors: tile, its edge, a window, its edge. High contrast
/// fills windows with the text color, so a maximized one never reads as an empty tile.
fn pic_colors(t: &Theme) -> (u32, u32, u32, u32) {
    if t.hc {
        (t.surface, t.text, t.text, t.surface)
    } else {
        (t.tile, mix(t.card_edge, t.dim, 0.5), mix(t.tile, t.dim, 0.45), mix(t.tile, t.text, 0.5))
    }
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
                let mut h = 14.0 + c.pictures(y + 14.0) + 4.0;
                h += c.c.para(x, y + h, CW - 32.0, 20.0, &[("Every screen has ", F::Body, t.text), (&n, F::Bold, t.text), (" workspaces ", F::Body, t.text), ("(2 to 9) ", F::Small, t.dim), ("workspaces", F::Mono, t.dim)]);
                let e = c.measured(|c| c.extras("workspaces", x, 0.0, CW - 32.0));
                if e > 0.0 {
                    c.extras("workspaces", x, y + h + 6.0, CW - 32.0);
                    h += 6.0 + e;
                }
                h += c.removal(y + h);
                h += 14.0;
                c.line(y + h);
                h + c.rows(Section::Workspaces, y + h)
            }),
            Section::Shortcuts => self.card(y + h, |c, y| {
                let mut h = c.scenes(y);
                for (key, title) in SHORTCUTS.iter() {
                    c.line(y + h);
                    h += c.shortcut(key, title, y + h);
                }
                h
            }),
            Section::Touchpad => {
                let mut g = self.card(y + h, |c, y| {
                    let h = c.touchpad_status(y);
                    c.line(y + h);
                    let h = h + c.rows(Section::Touchpad, y + h);
                    c.line(y + h);
                    h + c.setup_row(y + h)
                });
                g += 16.0;
                g + self.card(y + h + g, |c, y| c.gestures(y))
            }
            Section::Indicator => self.card(y + h, |c, y| {
                let h = c.rows(Section::Indicator, y);
                c.line(y + h);
                h + c.play_row(y + h)
            }),
            Section::Overview => self.card(y + h, |c, y| {
                let h = c.overview_art(y);
                c.line(y + h);
                h + c.rows(Section::Overview, y + h)
            }),
            _ => self.card(y + h, |c, y| c.rows(sec, y)),
        };
        h
    }

    /// One picture per present screen: its workspaces as tiles with their windows where they
    /// sit, the one in view marked "now", then + Add, and × on the first screen's last tile
    /// (AC-18, AC-10). The whole picture is one keyboard stop: Right adds, Left and Delete remove.
    fn pictures(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let w = self.w;
        let n = self.s.workspaces;
        let (x0, wmax) = (CX + 16.0, CW - 32.0);
        let th = if n <= 4 { 52.0 } else if n <= 6 { 40.0 } else { 24.0 };
        let small = n > 6;
        let gap = if small { 5.0 } else { 10.0 };
        let (tile, edge, win, win_edge) = pic_colors(&t);
        let (minus, plus) = (Ctl::Minus("workspaces"), Ctl::Plus("workspaces"));
        if w.pics.screens.is_empty() {
            return self.c.para(x0, y, wmax, 18.0, &[("Reading your screens…", F::Small, t.dim)]) + 8.0;
        }
        let going = !self.ro && n > 2 && (self.hovered(minus) || self.focused(Ctl::Stepper("workspaces")));
        let mut h = 0.0;
        for (si, sc) in w.pics.screens.iter().enumerate() {
            if si > 0 {
                h += 12.0;
            }
            let (rw, rh) = (sc.rect.right - sc.rect.left, sc.rect.bottom - sc.rect.top);
            let head = format!("Screen {}", sc.number);
            let hw = self.c.width(&head, F::SmallBold);
            self.c.text(x0, y + h, 18.0, &head, F::SmallBold, t.text);
            let rest = self.fit(&format!("{} · {rw} × {rh} · {}%", sc.name, sc.scale), F::Small, wmax - hw - 8.0);
            self.c.text(x0 + hw + 8.0, y + h, 18.0, &rest, F::Small, t.dim);
            h += 24.0;
            let (rw, rh) = (rw.max(1) as f32, rh.max(1) as f32);
            let tw = (th * rw / rh).round().clamp(th * 0.5, th * 3.0);
            let active = w.pics.view.active.iter().find(|(d, _)| *d == sc.device).map_or(0, |(_, a)| *a);
            let mut x = x0;
            for i in 0..n {
                if x + tw > x0 + wmax + 0.5 {
                    x = x0;
                    h += th + 26.0;
                }
                let (ty, last, cur) = (y + h, i + 1 == n, i == active);
                let (fill, ed) = if last && going { (mix(tile, t.card, 0.5), mix(edge, t.card, 0.5)) } else { (tile, edge) };
                self.c.boxed((x, ty, tw, th), 6.0, fill, Some(ed));
                let mut count = 0;
                for (d, ws, r) in &w.pics.view.windows {
                    if *d != sc.device || *ws != i {
                        continue;
                    }
                    count += 1;
                    // Cut at the tile's edge, like a window hanging off its screen.
                    let fx = |v: i32| ((v - sc.rect.left) as f32 / rw).clamp(0.0, 1.0) * (tw - 4.0) + x + 2.0;
                    let fy = |v: i32| ((v - sc.rect.top) as f32 / rh).clamp(0.0, 1.0) * (th - 4.0) + ty + 2.0;
                    let (l, top, r, b) = (fx(r.left), fy(r.top), fx(r.right), fy(r.bottom));
                    if r - l >= 1.0 && b - top >= 1.0 {
                        self.c.boxed((l, top, r - l, b - top), 2.0, win, Some(win_edge));
                    }
                }
                let num = (i + 1).to_string();
                let bh = if small { 14.0 } else { 16.0 };
                let nw = self.c.width(&num, F::SmallBold);
                let bw = (nw + 10.0).max(bh);
                let (bf, bfg, be) = if cur { (t.accent, t.on_accent, t.accent) } else { (t.surface, t.text, edge) };
                self.c.boxed((x + 3.0, ty + th - bh - 3.0, bw, bh), bh / 2.0, bf, Some(be));
                self.c.text(x + 3.0 + (bw - nw) / 2.0, ty + th - bh - 3.0, bh, &num, F::SmallBold, bfg);
                if cur {
                    self.c.stroke_rr((x - 4.0, ty - 4.0, tw + 8.0, th + 8.0), 9.0, 2.5, op(t.accent));
                }
                let plural = |k: usize| if k == 1 { "window" } else { "windows" };
                let cap = match (small, cur, count) {
                    (true, true, _) => "now".to_string(),
                    (true, false, 0) => "·".to_string(),
                    (true, false, k) => format!("{k} win"),
                    (false, true, 0) => "now · empty".to_string(),
                    (false, true, k) => format!("now · {k} {}", plural(k)),
                    (false, false, 0) => "empty".to_string(),
                    (false, false, k) => format!("{k} {}", plural(k)),
                };
                let cap = self.fit(&cap, F::Small, tw + gap);
                let cw = self.c.width(&cap, F::Small);
                self.c.text(x + (tw - cw) / 2.0, ty + th + 4.0, 16.0, &cap, F::Small, if cur { t.text } else { t.dim });
                if last && si == 0 {
                    let (cx, cy) = (x + tw - 2.0, ty + 2.0);
                    let on = !self.ro && n > 2;
                    let fill = if on && self.hovered(minus) { mix(t.btn, t.text, 0.08) } else { t.btn };
                    self.c.circle(cx, cy, 11.0, Some(op(fill)), Some((op(t.btn_edge), 1.0)));
                    let xw = self.c.width("×", F::Body);
                    self.c.text(cx - xw / 2.0, cy - 10.0, 20.0, "×", F::Body, if on { t.text } else { t.disabled });
                    if on {
                        self.hit(minus, (cx - 12.0, cy - 12.0, 24.0, 24.0), 1);
                    }
                }
                x += tw + gap;
            }
            let aw = if small { 40.0 } else { (tw * 0.6).max(56.0) };
            if x + aw > x0 + wmax + 0.5 {
                x = x0;
                h += th + 26.0;
            }
            let ty = y + h;
            let on = !self.ro && n < 9;
            if on && self.hovered(plus) {
                self.c.boxed((x, ty, aw, th), 6.0, mix(t.card, t.text, 0.06), None);
            }
            self.c.stroke_rr((x, ty, aw, th), 6.0, 1.0, op(if on { t.btn_edge } else { t.card_edge }));
            let label = if n >= 9 { "9 max" } else { "+ Add" };
            let lw = self.c.width(label, F::SmallBold);
            self.c.text(x + (aw - lw) / 2.0, ty, th, label, F::SmallBold, if on { t.text } else { t.disabled });
            if on {
                self.hit(plus, (x, ty, aw, th), 1);
            }
            h += th + 24.0;
        }
        self.ring((x0 - 8.0, y - 6.0, wmax + 16.0, h + 4.0), 8.0, Ctl::Stepper("workspaces"));
        h
    }

    /// Before a removal, what moves where on each screen, since there is no undo (AC-6):
    /// while × is hovered or the pictures have the keyboard, and while Reset would lower the count.
    fn removal(&mut self, y: f32) -> f32 {
        let n = self.s.workspaces;
        let removals = &self.w.pics.view.removals;
        let (to, title) = if n > 2 && (self.hovered(Ctl::Minus("workspaces")) || self.focused(Ctl::Stepper("workspaces"))) {
            (0, format!("× removes workspace {n} from every screen. There is no undo."))
        } else if n > DEFAULTS.workspaces && (self.hovered(Ctl::Reset("workspaces")) || self.focused(Ctl::Reset("workspaces"))) {
            (1, format!("Reset removes workspaces {} to {n} from every screen and goes back to {}. There is no undo.", DEFAULTS.workspaces + 1, DEFAULTS.workspaces))
        } else {
            return 0.0;
        };
        if self.ro {
            return 0.0;
        }
        let lines = removals.get(to).filter(|l| !l.is_empty()).map_or("nothing moves".to_string(), |l| l.join("\n"));
        let sub = format!("{lines}\nA screen showing a removed workspace switches to the new last one. From the keyboard, Delete removes one.");
        12.0 + self.note(CX + 16.0, y + 12.0, CW - 32.0, 1, &title, &sub, &[])
    }

    /// Play: the real dots on this screen with the current hold and fade; off while the dots
    /// are off (AC-19).
    fn play_row(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let x = CX + 16.0;
        let on = self.s.indicator_enabled;
        let bw = self.c.width("Play", F::Body) + 28.0;
        let help = if on { "Shows the dots on this screen, with the hold and fade above." } else { "Turn the dots on to try them." };
        self.c.text(x, y + 14.0, 20.0, "Try it", F::Bold, t.text);
        let h = 20.0 + self.c.para(x, y + 34.0, CW - 64.0 - bw, 17.0, &[(help, F::Small, t.dim)]);
        let bx = CX + CW - 16.0 - bw;
        if on {
            self.button(Ctl::Play, bx, y + 14.0, Some(bw), "Play", 1);
        } else {
            self.c.boxed((bx, y + 14.0, bw, 32.0), 16.0, t.card, Some(t.card_edge));
            let lw = self.c.width("Play", F::Body);
            self.c.text(bx + (bw - lw) / 2.0, y + 14.0, 32.0, "Play", F::Body, t.disabled);
        }
        14.0 + h.max(32.0) + 14.0
    }

    /// The overview as a fixed drawing that follows the labels and animation settings (AC-21).
    fn overview_art(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let (tile, edge, win, win_edge) = pic_colors(&t);
        let labels = word(settings::LABELS, self.s.overview_labels);
        let (tw, th, gap) = (96.0, 60.0, 12.0);
        let x0 = CX + (CW - (4.0 * tw + 3.0 * gap)) / 2.0;
        let ty = y + 16.0;
        let wins: [&[Sample]; 4] = [&[("Mail", 0.06, 0.08, 0.52, 0.42), ("Terminal", 0.44, 0.48, 0.5, 0.42)], &[], &[], &[("Docs", 0.1, 0.12, 0.7, 0.66)]];
        for (i, list) in wins.iter().enumerate() {
            let x = x0 + i as f32 * (tw + gap);
            self.c.boxed((x, ty, tw, th), 6.0, tile, Some(edge));
            if i == 1 {
                self.c.stroke_rr((x - 4.0, ty - 4.0, tw + 8.0, th + 8.0), 9.0, 2.5, op(t.accent));
            }
            for (j, (name, fx, fy, fw, fh)) in list.iter().enumerate() {
                let b = (x + fx * tw, ty + fy * th, fw * tw, fh * th);
                self.c.boxed(b, 2.0, win, Some(win_edge));
                if labels == "always" || (labels == "hover" && i == 0 && j == 0) {
                    let lw = self.c.width(name, F::Small) + 8.0;
                    self.c.boxed((b.0 + 2.0, b.1 + b.3 - 18.0, lw, 16.0), 3.0, t.surface, Some(edge));
                    self.c.text(b.0 + 6.0, b.1 + b.3 - 18.0, 16.0, name, F::Small, t.text);
                }
            }
        }
        let names = match labels {
            "always" => "every window's name shows",
            "hover" => "a window's name shows when you point at it",
            _ => "no names show",
        };
        let opening = if word(settings::ANIMATIONS, self.s.overview_animation) == "fade" { "it fades in" } else { "it opens at once" };
        let cap = format!("Win+Tab shows every workspace of every screen: {names}, and {opening}.");
        let ch = self.c.para(CX + 16.0, ty + th + 12.0, CW - 32.0, 17.0, &[(&cap, F::Small, t.dim)]);
        16.0 + th + 12.0 + ch + 14.0
    }

    /// The three moves as fixed drawings with your current combos under them (AC-21).
    fn scenes(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let (tile, edge, win, win_edge) = pic_colors(&t);
        let s = self.s;
        let first = |key: &str| combos(s, key).into_iter().next().unwrap_or_else(|| "not set".into());
        let list = [
            ("Switch", false, "you go", "shortcuts.previous", "shortcuts.next", "shortcuts.jump"),
            ("Send and go", true, "the window and you go", "shortcuts.send_previous", "shortcuts.send_next", "shortcuts.send_to"),
            ("Send and stay", true, "the window goes, you stay", "shortcuts.send_previous_stay", "shortcuts.send_next_stay", "shortcuts.send_to_stay"),
        ];
        let colw = (CW - 32.0 - 2.0 * 16.0) / 3.0;
        let (tw, th, gap) = (28.0, 18.0, 8.0);
        let mut most: f32 = 0.0;
        for (k, (title, sends, sub, prev, next, digits)) in list.iter().enumerate() {
            let x = CX + 16.0 + k as f32 * (colw + 16.0);
            let mut h = 14.0;
            self.c.text(x, y + h, 20.0, title, F::Bold, t.text);
            h += 20.0;
            self.c.text(x, y + h, 16.0, sub, F::Small, t.dim);
            h += 26.0;
            let ax = x + (colw - (4.0 * tw + 3.0 * gap)) / 2.0;
            let ty = y + h;
            let tx = |i: usize| ax + i as f32 * (tw + gap);
            for i in 0..4 {
                self.c.boxed((tx(i), ty, tw, th), 4.0, tile, Some(if i == 1 { t.accent } else { edge }));
                if *sends && i != 1 {
                    self.c.boxed((tx(i) + 6.0, ty + 4.0, tw - 12.0, th - 8.0), 2.0, win, Some(win_edge));
                }
            }
            if *sends {
                // Where the window was: an outline only.
                self.c.stroke_rr((tx(1) + 6.0, ty + 4.0, tw - 12.0, th - 8.0), 2.0, 1.0, op(win_edge));
            }
            self.c.circle(tx(1) + tw / 2.0, ty - 7.0, 3.5, Some(op(t.accent)), None);
            // An arrow from where you are to each side.
            for to in [0usize, 2] {
                let (a, b) = (tx(1) + tw / 2.0, tx(to) + tw / 2.0);
                let base = ty + th + 3.0;
                let pts: Vec<(f32, f32)> = (0..=8)
                    .map(|j| {
                        let u = j as f32 / 8.0;
                        (a + (b - a) * u, base + 8.0 * (1.0 - (2.0 * u - 1.0).powi(2)))
                    })
                    .collect();
                self.c.lines(&pts, 1.5, op(t.dim));
                let back = if b > a { -4.0 } else { 4.0 };
                self.c.lines(&[(b + back, base + 4.0), (b, base), (b + back * 1.25, base - 1.0)], 1.5, op(t.dim));
            }
            h += th + 20.0;
            for (label, key) in [("←  ", prev), ("→  ", next), ("", digits)] {
                let text = self.fit(&format!("{label}{}", first(key)), F::Small, colw);
                self.c.text(x, y + h, 17.0, &text, F::Small, t.text);
                h += 17.0;
            }
            most = most.max(h);
        }
        most + 14.0
    }

    /// Where reading gestures stands, in one of six states checked in AC-20's order.
    fn touchpad_status(&mut self, y: f32) -> f32 {
        use crate::setup::{ROWS as WIN_ROWS, Row as WinRow};
        let Some(st) = self.w.pics.pad else { return 0.0 };
        let (kind, title, line): (u8, String, &str) = if !st.pad {
            (0, "No precision touchpad found".into(), "Your gestures are kept and work when one is plugged in.")
        } else if !st.enabled && !st.off {
            (0, "Reading is off here, and Windows' own gestures are on".into(), "Your touchpad works the Windows way. Turn on Read touchpad gestures to use the gestures below.")
        } else if !st.enabled {
            (1, "No three or four finger gesture does anything now".into(), "Reading is off here, but Windows' own gestures are set to Nothing. Turn reading on, or give them back to Windows in Touchpad setup.")
        } else if st.rows.contains(&WinRow::CantRead) {
            (1, "pdesk can't read one of Windows' gesture settings, so it waits".into(), "Touchpad setup shows which row.")
        } else if let Some(i) = st.rows.iter().position(|r| *r == WinRow::StillOn) {
            (1, format!("pdesk is waiting: Windows' own {} are still on", WIN_ROWS[i].0), "pdesk acts only when all of Windows' three and four finger gestures are Nothing, so one gesture never does two things.")
        } else {
            (0, "pdesk reads your three and four finger gestures".into(), "Windows' own three and four finger gestures are all set to Nothing.")
        };
        14.0 + self.note(CX + 16.0, y + 14.0, CW - 32.0, kind, &title, line, &[]) + 14.0
    }

    /// `text` cut with "…" to fit `w`.
    fn fit(&self, text: &str, f: F, w: f32) -> String {
        if self.c.width(text, f) <= w {
            return text.into();
        }
        let mut s: String = text.into();
        while !s.is_empty() && self.c.width(&format!("{s}…"), f) > w {
            s.pop();
        }
        format!("{}…", s.trim_end())
    }

    /// A note inside a card, under the row it is about: a glyph, a bold title, a line, and
    /// buttons under them. `kind`: 0 information, 1 a question (the clash prompt), 2 refused.
    #[allow(clippy::too_many_arguments)]
    fn note(&mut self, x: f32, y: f32, w: f32, kind: u8, title: &str, sub: &str, buttons: &[(Ctl, &str)]) -> f32 {
        let t = self.c.t;
        let (tx, tw) = (x + 40.0, w - 52.0);
        let text = |c: &mut Self| {
            let mut h = c.c.para(tx, y + 10.0, tw, 20.0, &[(title, F::Bold, t.text)]);
            for line in sub.split('\n') {
                h += 2.0 + c.c.para(tx, y + 12.0 + h, tw, 17.0, &[(line, F::Small, t.dim)]);
            }
            h
        };
        let th = self.measured(|c| text(c));
        let h = 10.0 + th + if buttons.is_empty() { 10.0 } else { 10.0 + 32.0 + 12.0 };
        let (fill, edge, fg, glyph) = match kind {
            0 => (t.tile, t.card_edge, t.accent, "i"),
            1 => (t.warn, t.warn_edge, t.text, "!"),
            _ => (t.chip_err, mix(t.chip_err, t.chip_err_fg, 0.4), t.chip_err_fg, "×"),
        };
        self.c.boxed((x, y, w, h), 8.0, fill, Some(edge));
        self.c.circle(x + 20.0, y + 20.0, 10.0, None, Some((op(fg), 1.5)));
        let gw = self.c.width(glyph, F::Glyph);
        self.c.text(x + 20.0 - gw / 2.0, y + 10.0, 20.0, glyph, F::Glyph, fg);
        text(self);
        let mut bx = tx;
        for (ctl, label) in buttons {
            bx += self.button(*ctl, bx, y + 10.0 + th + 10.0, None, label, 1) + 8.0;
        }
        h
    }

    /// A combo chip with its × on the right. Returns its width.
    fn chip(&mut self, key: &'static str, i: usize, x: f32, y: f32, text: &str) -> f32 {
        let t = self.c.t;
        let (body, cross) = (Ctl::Chip(key, i), Ctl::Remove(key, i));
        let tw = self.c.width(text, F::Small);
        let w = tw + 20.0 + 24.0;
        let fill = if self.hovered(body) && !self.ro { mix(t.tile, t.text, 0.08) } else { t.tile };
        self.c.boxed((x, y, w, 28.0), 8.0, fill, Some(t.card_edge));
        self.c.text(x + 10.0, y, 28.0, text, F::Small, t.text);
        let (cx, cy) = (x + w - 15.0, y + 14.0);
        if self.hovered(cross) && !self.ro {
            self.c.circle(cx, cy, 9.0, Some(op(mix(t.tile, t.text, 0.14))), None);
        }
        let col = op(if self.ro { t.disabled } else { t.dim });
        self.c.lines(&[(cx - 3.5, cy - 3.5), (cx + 3.5, cy + 3.5)], 1.4, col);
        self.c.lines(&[(cx + 3.5, cy - 3.5), (cx - 3.5, cy + 3.5)], 1.4, col);
        self.ring((x, y, w - 26.0, 28.0), 8.0, body);
        self.ring((cx - 10.0, y + 4.0, 20.0, 20.0), 10.0, cross);
        self.hit(body, (x, y, w - 26.0, 28.0), 1);
        self.hit(cross, (x + w - 26.0, y, 26.0, 28.0), 1);
        w
    }

    /// The chip that waits for a combo while recording. Returns its width.
    fn waiting(&mut self, x: f32, y: f32, text: &str) -> f32 {
        let t = self.c.t;
        let w = self.c.width(text, F::SmallBold) + 24.0;
        self.c.boxed((x, y, w, 28.0), 8.0, mix(t.card, t.accent, 0.12), Some(t.accent));
        self.c.text(x + 12.0, y, 28.0, text, F::SmallBold, t.accent);
        w
    }

    /// The recording note or the clash prompt for `key`, if either is open. Returns its height.
    fn row_note(&mut self, key: &'static str, x: f32, y: f32, w: f32) -> f32 {
        if let Some(p) = self.w.prompt.as_ref().filter(|p| p.key == key) {
            let (title, sub) = (p.title.clone(), p.sub.clone());
            return 8.0 + self.note(x, y + 8.0, w, 1, &title, &sub, &[(Ctl::Move(key), "Move it here"), (Ctl::Cancel(key), "Cancel")]);
        }
        let Some(rec) = self.w.rec.filter(|r| match r {
            Rec::List(k, _) | Rec::Digits(k) => *k == key,
            Rec::Gesture(..) => key == "gesture",
        }) else {
            return 0.0;
        };
        let stop = [(Ctl::StopRec(key), "Cancel")];
        if let Some((title, sub)) = self.w.refused.clone() {
            return 8.0 + self.note(x, y + 8.0, w, 2, &title, &sub, &stop);
        }
        let (title, sub) = match rec {
            Rec::Digits(_) => ("Hold the keys you want, then press any digit 1 to 9.", "All nine digits get the same keys: with Win+Alt, Win+Alt+1 goes to workspace 1, and so on. Esc alone cancels."),
            Rec::List(..) => ("Press the new shortcut.", "pdesk's own shortcuts are paused, and the keys reach no other program, until you finish. Esc alone or a click elsewhere cancels; Esc with Win, Ctrl, Alt or Shift held is recorded."),
            Rec::Gesture(..) => ("Press the key combo this gesture should type.", "No clash check here: a gesture may type any combo, one of pdesk's own too. Esc alone or a click elsewhere cancels."),
        };
        8.0 + self.note(x, y + 8.0, w, 0, title, sub, &stop)
    }

    /// A shortcut entry: its combos as chips, each with ×, and + Add (AC-8). While it records,
    /// a waiting chip and a note; after a clash, the prompt (AC-9).
    fn shortcut(&mut self, key: &'static str, title: &str, y: f32) -> f32 {
        let t = self.c.t;
        let (x, w) = (CX + 16.0, CW - 32.0);
        let name = format!(" {}", short(key));
        let mut h = 12.0;
        h += self.c.para(x, y + h, w, 20.0, &[(title, F::Bold, t.text), (&name, F::Mono, t.dim)]);
        h += 6.0;
        let list = combos(self.s, key);
        let digits = is_digits(key);
        let rec = self.w.rec;
        let (mut cx, mut cy) = (x, y + h);
        // Wraps to a new line when the next piece does not fit.
        let room = |cx: &mut f32, cy: &mut f32, pw: f32| {
            if *cx > x && *cx + pw > x + w {
                *cx = x;
                *cy += 36.0;
            }
        };
        for (i, text) in list.iter().enumerate() {
            if rec == Some(Rec::List(key, Some(i))) || (digits && rec == Some(Rec::Digits(key))) {
                let label = if digits { "Hold keys, press a digit" } else { "Press a combo" };
                room(&mut cx, &mut cy, self.c.width(label, F::SmallBold) + 24.0);
                cx += self.waiting(cx, cy, label) + 8.0;
                continue;
            }
            room(&mut cx, &mut cy, self.c.width(text, F::Small) + 44.0);
            cx += self.chip(key, i, cx, cy, text) + 8.0;
        }
        if rec == Some(Rec::List(key, None)) || (digits && list.is_empty() && rec == Some(Rec::Digits(key))) {
            let label = if digits { "Hold keys, press a digit" } else { "Press a combo" };
            room(&mut cx, &mut cy, self.c.width(label, F::SmallBold) + 24.0);
            cx += self.waiting(cx, cy, label) + 8.0;
        } else if list.is_empty() {
            self.c.text(cx, cy, 28.0, "Off", F::Small, t.dim);
            cx += self.c.width("Off", F::Small) + 12.0;
        }
        // A digit entry holds one set of keys, so it records anew from its chip instead.
        if !digits || list.is_empty() {
            let label = "+ Add";
            room(&mut cx, &mut cy, self.c.width(label, F::Body) + 28.0);
            let ctl = Ctl::Add(key);
            let bw = self.c.width(label, F::Small) + 24.0;
            let fill = if self.hovered(ctl) && !self.ro { mix(t.card, t.text, 0.08) } else { t.card };
            self.c.boxed((cx, cy, bw, 28.0), 8.0, fill, Some(t.stroke));
            self.c.text(cx + 12.0, cy, 28.0, label, F::Small, if self.ro { t.disabled } else { t.text });
            self.ring((cx, cy, bw, 28.0), 8.0, ctl);
            self.hit(ctl, (cx, cy, bw, 28.0), 1);
        }
        h = cy - y + 28.0;
        let e = self.measured(|c| c.extras(key, x, 0.0, w));
        if e > 0.0 {
            self.extras(key, x, y + h + 6.0, w);
            h += 6.0 + e;
        }
        h += self.row_note(key, x, y + h, w);
        h + 12.0
    }

    /// A gesture picker: what the row does now, and a ▾. While it records, the waiting chip.
    fn picker(&mut self, ctl: Ctl, x: f32, y: f32, w: f32, text: &str, waiting: bool) {
        let t = self.c.t;
        if waiting {
            self.c.boxed((x, y, w, 30.0), 8.0, mix(t.card, t.accent, 0.12), Some(t.accent));
            self.c.text(x + 10.0, y, 30.0, "Press a combo", F::SmallBold, t.accent);
        } else {
            let fill = if self.hovered(ctl) && !self.ro { mix(t.btn, t.text, 0.08) } else { t.btn };
            self.c.boxed((x, y, w, 30.0), 8.0, fill, Some(t.btn_edge));
            let shown = self.fit(text, F::Small, w - 34.0);
            self.c.text(x + 10.0, y, 30.0, &shown, F::Small, if self.ro { t.disabled } else { t.text });
            let (ax, ay) = (x + w - 16.0, y + 13.0);
            self.c.lines(&[(ax - 4.0, ay), (ax, ay + 4.0), (ax + 4.0, ay)], 1.4, op(t.dim));
        }
        self.ring((x, y, w, 30.0), 8.0, ctl);
        self.hit(ctl, (x, y, w, 30.0), 1);
    }

    /// The 10 gesture rows, each with its once and twice picker (AC-12).
    fn gestures(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let (x, w) = (CX + 16.0, CW - 32.0);
        let (px, pw) = (x + 150.0, (w - 150.0 - 10.0) / 2.0);
        let mut h = 14.0;
        h += self.c.para(x, y + h, w, 20.0, &[("Gestures", F::Bold, t.text), (" gesture", F::Mono, t.dim)]);
        h += self.c.para(x, y + h, w, 17.0, &[("What each gesture does. A twice action waits for a second go (Wait for a second go, above). Not set: the once action runs at once. Nothing: the gesture still waits, and a second go does nothing.", F::Small, t.dim)]);
        h += 8.0;
        self.c.text(px, y + h, 18.0, "Once", F::SmallBold, t.dim);
        self.c.text(px + pw + 10.0, y + h, 18.0, "Twice", F::SmallBold, t.dim);
        h += 22.0;
        let motions = [(Motion::Left, "swipe left"), (Motion::Right, "swipe right"), (Motion::Up, "swipe up"), (Motion::Down, "swipe down"), (Motion::Tap, "tap")];
        for fingers in [3u8, 4] {
            for (m, word) in motions {
                let Some(i) = settings::GESTURE_ROWS.iter().position(|r| *r == (fingers, m)) else { continue };
                let (once, twice) = (self.s.rule(fingers, m, 1), self.s.rule(fingers, m, 2));
                let mut rh = 30.0;
                self.c.text(x, y + h, 30.0, &format!("{fingers} fingers, {word}"), F::Small, t.text);
                if once.is_none() {
                    self.c.text(x, y + h + 24.0, 16.0, "not in the file", F::Small, t.chip_on_fg);
                    rh = 42.0;
                }
                let rec = self.w.rec;
                self.picker(Ctl::Once(i), px, y + h, pw, &label(once.unwrap_or(Action::Nothing)), rec == Some(Rec::Gesture(i, 1)));
                self.picker(Ctl::Twice(i), px + pw + 10.0, y + h, pw, &twice.map_or("Not set".into(), label), rec == Some(Rec::Gesture(i, 2)));
                h += rh + 8.0;
                if matches!(rec, Some(Rec::Gesture(r, _)) if r == i) {
                    h += self.row_note("gesture", x, y + h - 8.0, w);
                }
            }
        }
        let e = self.measured(|c| c.extras("gesture", x, 0.0, w));
        if e > 0.0 {
            self.extras("gesture", x, y + h + 6.0, w);
            h += 6.0 + e;
        }
        h + 6.0
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
    if let Some((time, lines)) = &w.edited {
        let lines: Vec<(String, F)> = lines.iter().map(|l| (l.clone(), F::Mono)).collect();
        y += x.banner(y, false, &format!("settings.toml changed at {time} and is in use:"), &lines, Some((Ctl::DismissEdit, "Dismiss"))) + 16.0;
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
        update_pictures(true);
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
        None => layout(), // the focus can show the removal preview (AC-6)
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

/// Writes an edit that is not one row's value; a failure shows like a failed save.
fn commit(edit: Edit, title: &str) {
    let failed = settings::write(edit).err();
    let _ = set(|w| w.error = failed.map(|why| (format!("Not saved. {title} stays as it was."), why)));
    refresh();
}

/// Starts recording; the hook swallows every key until it stops (AC-7).
fn start_rec(rec: Rec) {
    let _ = set(|w| {
        w.rec = Some(rec);
        w.refused = None;
        w.prompt = None;
    });
    crate::keys::record(true);
    refresh();
}

/// Ends recording, after a combo, Esc, Cancel, a click, focus loss or close.
fn stop_rec() {
    crate::keys::record(false);
    let _ = set(|w| {
        w.rec = None;
        w.refused = None;
    });
    refresh();
}

/// The hook's WM_RECORDED: the first key down that is not a modifier, and the modifiers held
/// with it. Main thread.
pub fn recorded(vk: u16, mods: u8) {
    let Some(rec) = get(|w| w.rec).flatten() else { return };
    if vk == VK_ESCAPE.0 && mods == 0 {
        return stop_rec();
    }
    let refuse = |title: &str, sub: &str| {
        let _ = set(|w| w.refused = Some((title.into(), sub.into())));
        refresh();
    };
    if let Rec::Digits(key) = rec {
        let digit = (b'1' as u16..=b'9' as u16).contains(&vk) || (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&vk);
        if !digit {
            return refuse("Press a digit 1 to 9 while you hold the keys.", "Still recording. Esc alone cancels.");
        }
        if mods & (settings::WIN | settings::CTRL | settings::ALT) == 0 {
            return refuse("Hold Win, Ctrl or Alt with the digit.", "Shift alone would take every typed ! @ #. Shift can join the others. Still recording.");
        }
        stop_rec();
        return digits_recorded(key, mods);
    }
    if !settings::savable(vk) {
        return refuse("This key can't be saved; try another", "Use a letter, a number row digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Numpad, punctuation and media keys can't be saved. Still recording.");
    }
    if mods == 0 {
        return refuse("Hold Win, Ctrl, Alt or Shift with the key.", "A combo needs at least one of them. Still recording; Esc alone cancels.");
    }
    stop_rec();
    let combo = Combo { mods, vk };
    match rec {
        Rec::List(key, replace) => combo_recorded(key, replace, combo),
        Rec::Gesture(row, times) => set_gesture(row, times, Some(Action::Combo(combo))),
        Rec::Digits(_) => {}
    }
}

/// A combo for a list: saved, or the clash prompt when another entry has it (AC-9).
fn combo_recorded(key: &'static str, replace: Option<usize>, combo: Combo) {
    let (s, name) = (settings::get(), short(key));
    let mut mine = s.list(name).to_vec();
    match replace {
        Some(i) if i < mine.len() => mine[i] = combo,
        _ => mine.push(combo),
    }
    let mut seen = vec![];
    mine.retain(|c| !seen.contains(c) && {
        seen.push(*c);
        true
    });
    if mine == s.list(name) {
        return;
    }
    let title = entry(name).1;
    let edit = Edit::Combos(key, mine);
    let prompt = match s.combo_owner(name, combo) {
        None => return commit(edit, title),
        Some(Owner::List(other)) => {
            let (other_key, other_title) = entry(other);
            let rest: Vec<Combo> = s.list(other).iter().filter(|c| **c != combo).copied().collect();
            let after = if rest.is_empty() {
                format!("{other_title} has no shortcut left (it turns off)")
            } else {
                format!("{other_title} keeps {}", rest.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "))
            };
            Prompt {
                key,
                title: format!("{combo} already belongs to {other_title}."),
                sub: format!("Move it here, and {after}. Cancel keeps both as they are. Nothing is saved until you choose."),
                edit: Edit::Pair(Box::new(edit), Box::new(Edit::Combos(other_key, rest))),
            }
        }
        // The hook tries the lists first, so the set only loses this one digit (AC-8).
        Some(Owner::Set(set)) => {
            let set_title = entry(set).1;
            let d = combo.vk as u8 as char;
            Prompt {
                key,
                title: format!("{combo} is part of {set_title} ({} + 1…9).", settings::mods_text(Some(combo.mods))),
                sub: format!("Move it here, and {} skips {d}; its other eight digits keep working. Cancel keeps it. Nothing is saved until you choose.", lower(set_title)),
                edit,
            }
        }
    };
    let _ = set(|w| {
        w.prompt = Some(prompt);
        w.focus = Some(Ctl::Cancel(key));
    });
    reveal_after_layout();
}

/// Modifiers for a digit set: saved, or the clash prompt when another set has them (AC-9). A
/// list on one of their digits is no clash: the set skips that digit.
fn digits_recorded(key: &'static str, mods: u8) {
    let (s, name) = (settings::get(), short(key));
    if s.set(name) == Some(mods) {
        return;
    }
    let edit = Edit::Digits(key, Some(mods));
    let Some(other) = s.set_owner(name, mods) else { return commit(edit, entry(name).1) };
    let (other_key, other_title) = entry(other);
    let prompt = Prompt {
        key,
        title: format!("{} + 1…9 already belongs to {other_title}.", settings::mods_text(Some(mods))),
        sub: format!("Move it here, and {} turns off. Cancel keeps both as they are. Nothing is saved until you choose.", lower(other_title)),
        edit: Edit::Pair(Box::new(edit), Box::new(Edit::Digits(other_key, None))),
    };
    let _ = set(|w| {
        w.prompt = Some(prompt);
        w.focus = Some(Ctl::Cancel(key));
    });
    reveal_after_layout();
}

fn reveal_after_layout() {
    layout();
    reveal();
}

/// × on a combo: that list loses it, a digit set turns off. No combo left means off (AC-8).
fn remove(key: &'static str, i: usize) {
    let (s, name) = (settings::get(), short(key));
    if is_digits(key) {
        return commit(Edit::Digits(key, None), entry(name).1);
    }
    let mut mine = s.list(name).to_vec();
    if i < mine.len() {
        mine.remove(i);
        commit(Edit::Combos(key, mine), entry(name).1);
    }
}

/// One gesture row's once or twice action; None: no twice rule (Not set). The whole map is
/// written again, every row's once rule included (AC-12).
fn set_gesture(row: usize, times: u8, action: Option<Action>) {
    let s = settings::get();
    let (fingers, motion) = settings::GESTURE_ROWS[row];
    let now = s.rule(fingers, motion, times);
    // A row with no once rule in the file shows Nothing; picking Nothing writes it down.
    if now == action && (times == 2 || now.is_some()) {
        return;
    }
    let map = settings::full_map(|f, m, t| if (f, m, t) == (fingers, motion, times) { action } else { s.rule(f, m, t) });
    commit(Edit::Gestures(map), "Gestures");
}

/// The once or twice picker as a menu under its button: Not set (twice only), Nothing, every
/// action by its label, the three digit families, and Key combo… (AC-12).
fn pick(row: usize, times: u8) {
    const NOT_SET: usize = 1;
    const COMBO: usize = 2;
    const BASE: usize = 100;
    let Some((h, at, scale)) = get(|w| w.hits.iter().find(|x| x.ctl == if times == 1 { Ctl::Once(row) } else { Ctl::Twice(row) }).map(|x| (w.hwnd, x.at, w.scale))).flatten() else {
        return;
    };
    let (fingers, motion) = settings::GESTURE_ROWS[row];
    let now = settings::get().rule(fingers, motion, times);
    let mut list: Vec<Action> = vec![Action::Nothing];
    list.extend(settings::actions().filter(|a| *a != Action::Nothing));
    type Family = fn(usize) -> Action;
    let families: [(&str, Family); 3] = [
        ("Go to workspace", Action::Jump),
        ("Send window to, and go", |i| Action::SendTo(i, true)),
        ("Send window to, stay", |i| Action::SendTo(i, false)),
    ];
    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let add = |menu: HMENU, id: usize, text: &str, on: bool| {
            let flags = if on { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(menu, flags, id, PCWSTR(HSTRING::from(text).as_ptr()));
        };
        if times == 2 {
            add(menu, NOT_SET, "Not set", now.is_none());
        }
        let shown = now.or((times == 1).then_some(Action::Nothing));
        for (i, a) in list.iter().enumerate() {
            add(menu, BASE + i, &label(*a), shown == Some(*a));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        for (name, make) in families {
            let Ok(sub) = CreatePopupMenu() else { continue };
            let mut any = false;
            for n in 0..9 {
                let a = make(n);
                add(sub, BASE + list.len(), &(n + 1).to_string(), now == Some(a));
                any |= now == Some(a);
                list.push(a);
            }
            let flags = if any { MF_POPUP | MF_CHECKED } else { MF_POPUP };
            let _ = AppendMenuW(menu, flags, sub.0 as usize, PCWSTR(HSTRING::from(name).as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let combo = match now {
            Some(Action::Combo(c)) => format!("Key combo… ({c})"),
            _ => "Key combo…".into(),
        };
        add(menu, COMBO, &combo, matches!(now, Some(Action::Combo(_))));
        let mut p = windows::Win32::Foundation::POINT { x: (at.0 * scale) as i32, y: ((at.1 + at.3) * scale) as i32 };
        let _ = ClientToScreen(h, &mut p);
        let id = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN).0, p.x, p.y, h, None).0 as usize;
        let _ = DestroyMenu(menu);
        id
    };
    match chosen {
        0 => {}
        NOT_SET => set_gesture(row, times, None),
        COMBO => start_rec(Rec::Gesture(row, times)),
        n => {
            if let Some(a) = list.get(n - BASE) {
                set_gesture(row, times, Some(*a));
            }
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
        Ctl::Chip(key, i) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, Some(i)) }),
        Ctl::Add(key) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, None) }),
        Ctl::Remove(key, i) => remove(key, i),
        Ctl::StopRec(_) => stop_rec(),
        Ctl::Move(_) => {
            if let Some(p) = set(|w| w.prompt.take()).flatten() {
                commit(p.edit, entry(short(p.key)).1);
            }
        }
        Ctl::Cancel(_) => {
            let _ = set(|w| w.prompt = None);
            refresh();
        }
        Ctl::Once(row) => pick(row, 1),
        Ctl::Twice(row) => pick(row, 2),
        Ctl::Play => play(),
        Ctl::DismissEdit => {
            let _ = set(|w| w.edited = None);
            refresh();
        }
    }
}

/// The real dots on the screen the page is on, with the current hold and fade (AC-19).
fn play() {
    let Some(h) = hwnd() else { return };
    let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
    let active = crate::ws::active(&crate::ws::device_name(mon));
    crate::indicator::show(mon, active, settings::get().workspaces, None);
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
        // On the pictures: removes the last workspace, as × does (AC-10).
        VK_DELETE if focus == Some(Ctl::Stepper("workspaces")) => {
            change(Ctl::Stepper("workspaces"), false);
        }
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
            rec: None,
            refused: None,
            prompt: None,
            pics: Pics::default(),
            edited: None,
        }));
        let (ww, wh) = size(h);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        update_pictures(true);
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
            // A held Delete removes one workspace only, not one per repeat.
            WM_KEYDOWN if wp.0 as u16 == VK_DELETE.0 && lp.0 >> 30 & 1 == 1 => LRESULT(0),
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
                // A click while recording only cancels it: the note goes, and the page moves
                // under the mouse, so the click must not land on whatever is there now.
                if get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                    return LRESULT(0);
                }
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
                    layout(); // hover on × or Reset shows what a removal moves (AC-6)
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let _ = set(|w| w.hover = None);
                layout();
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
                // Recording ends when the page loses focus (AC-7).
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                }
                refresh();
                LRESULT(0)
            }
            WM_DESTROY => {
                crate::keys::record(false);
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

    // Not a check that can fail: draws the whole page, as tall as its content, in the
    // recorder's states into target/settings/*.png, so they can be compared with the approved
    // design without opening the window. Run with `cargo test render_settings -- --ignored`.
    #[test]
    #[ignore]
    fn render_settings() {
        assert!(crate::welcome::gdiplus());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("settings");
        std::fs::create_dir_all(&dir).unwrap();
        let next = "shortcuts.next";
        let clash = || Prompt { key: next, title: "Win+F already belongs to Finder.".into(), sub: "Move it here, and Finder has no shortcut left (it turns off). Cancel keeps both as they are. Nothing is saved until you choose.".into(), edit: Edit::Gestures(vec![]) };
        // Two screens as a laptop plus an external one, a hovered ×, a hand edit and a waiting
        // touchpad: the step 4 parts (spec 0012 AC-6, AC-18, AC-20).
        let r = |l, t, rr, b| RECT { left: l, top: t, right: rr, bottom: b };
        let fake = || Pics {
            screens: vec![
                crate::ws::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 0, 2880, 1800), scale: 200 },
                crate::ws::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(2880, 0, 6720, 2160), scale: 150 },
            ],
            view: crate::ws::View {
                active: vec![("DISPLAY1".into(), 1), ("DISPLAY2".into(), 3)],
                windows: vec![
                    ("DISPLAY1".into(), 1, r(100, 100, 1500, 1200)),
                    ("DISPLAY1".into(), 1, r(1200, 600, 3000, 1700)),
                    ("DISPLAY1".into(), 0, r(0, 0, 2880, 1740)),
                    ("DISPLAY2".into(), 3, r(3000, 100, 5000, 1500)),
                    ("DISPLAY2".into(), 2, r(4000, 200, 6700, 2100)),
                ],
                removals: vec![vec!["Screen 2: 1 window from 4 moves to 3".into()], vec![]],
            },
            pad: Some(crate::setup::State { pad: true, enabled: true, rows: [crate::setup::Row::Nothing, crate::setup::Row::StillOn, crate::setup::Row::Nothing, crate::setup::Row::Nothing], off: false, saved: false }),
        };
        let states: [(&str, Option<Rec>, Option<(String, String)>, bool); 6] = [
            ("rest", None, None, false),
            ("rec", Some(Rec::List(next, None)), None, false),
            ("refused", Some(Rec::List("shortcuts.finder", Some(0))), Some(("This key can't be saved; try another".into(), "Still recording.".into())), false),
            ("digits", Some(Rec::Digits("shortcuts.jump")), None, false),
            ("clash", None, None, true),
            ("pictures", None, None, false),
        ];
        for (tname, theme) in [("light", crate::welcome::LIGHT), ("dark", crate::welcome::DARK)] {
            for (sname, rec, refused, prompt) in &states {
                let scale = 1.0;
                let win = Win {
                    hwnd: HWND::default(), scale, theme, fonts: Fonts::new(scale), scroll: 0.0, height: 0.0, tops: vec![], hits: vec![],
                    focus: prompt.then_some(Ctl::Cancel(next)), kb: *prompt, hover: None, press: None, drag: None, error: None,
                    rec: *rec, refused: refused.clone(), prompt: prompt.then(clash), pics: fake(), edited: None,
                };
                let mut win = win;
                if *sname == "pictures" {
                    win.hover = Some(Ctl::Minus("workspaces"));
                    win.edited = Some(("14:32".into(), vec!["indicator.hold_ms: 800 → 550".into(), "workspaces: 5 → 4".into(), "Screen 1: 2 windows from 5 move to 4".into()]));
                }
                let height = unsafe {
                    let dc = GetDC(None);
                    let out = compose(&mut Canvas::new(dc, false, scale, theme, &win.fonts), &win);
                    ReleaseDC(None, dc);
                    out.height
                };
                let ph = (BAND + height).ceil() as i32;
                crate::welcome::tests::save_png(W as i32, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                    let mut c = Canvas::new(dc, true, scale, theme, &win.fonts);
                    c.boxed((0.0, 0.0, W + 1.0, ph as f32 + 1.0), 0.0, theme.surface, None);
                    compose(&mut c, &win);
                });
                win.fonts.delete();
            }
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
