// The settings page (spec 0012, in look B by spec 0017's 0017-settings-page.md): every setting in
// settings.toml, changed by clicking or from the keyboard, saved at once and in effect at once.
// Drawn to the approved design (pdesk-design restart/settings-r3.html, with the Style row and
// preview of the indicator approval and the motion rows of settings-r7.html): Windows' title
// bar, a side list, and one scrolling column of cards. One drawn window with its own focus list
// and no child controls, so everything scrolls together; the gesture picker's menu is drawn in
// it too. Lives on the main thread: its keys reach wndproc through main's loop. Sizes are in
// dip, scaled to the screen's dpi when drawn. The page never keeps its own copy of a value: it
// draws what settings::get() holds, and every change goes through settings::write.
use std::cell::RefCell;
use std::sync::{Arc, LazyLock, Mutex};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{Gdi::*, GdiPlus::DashStyleDash},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::*,
        Controls::WM_MOUSELEAVE,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::settings::{self, Action, Combo, Edit, Motion, Owner, Problem, Settings};
use crate::setup::{self, GaveBack, Row as PadRow, State as PadState, Wrote};
use crate::ui::{self, Bx, Canvas, F, Fonts, Theme, buffered, dark_title, mix, op, theme};
use crate::uia::{self, Node, Tree};

const CLASS: PCWSTR = w!("des-k-settings");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_THICKFRAME.0);
const WHO: &str = "des-k settings";

// The client area and its parts, in dip (AC-SET-1): 1040 × 680 with Windows' title bar, never
// narrower, as tall as a 1366 × 768 screen at 125% allows.
const W: f32 = 1040.0;
const H: f32 = 648.0;
const MIN_H: f32 = 528.0;
const NAV: f32 = 240.0; // the side list
const X0: f32 = 248.0; // the column's left edge
const PAD_R: f32 = 32.0;
const COL_MAX: f32 = 1000.0; // the column stops growing here
const ROW: f32 = 52.0; // a row's least height
const KT_KEYS: f32 = 326.0; // a shortcut row's key column
const KT_SIDE: f32 = 112.0; // and its des-k or Windows column

// Segoe Fluent Icons.
const I_NOTES: &str = "\u{E7BA}";
const I_WARN: &str = "\u{E7BA}";
const I_INFO: &str = "\u{E946}";
const I_OK: &str = "\u{E73E}";
const I_KEY: &str = "\u{E765}";
const I_ADD: &str = "\u{E710}";
const I_MINUS: &str = "\u{E738}";
const I_X: &str = "\u{E711}";
const I_OUT: &str = "\u{E8A7}";
const I_CHEV: &str = "\u{E70D}";
const I_MORE: &str = "\u{E76C}";
const I_SETUP: &str = "\u{E713}";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Workspaces,
    Shortcuts,
    Indicator,
    Restore,
    Overview,
    Motion,
    Touchpad,
}

const SECTIONS: [Section; 7] = [Section::Workspaces, Section::Shortcuts, Section::Indicator, Section::Restore, Section::Overview, Section::Motion, Section::Touchpad];

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Workspaces => "Workspaces",
            Section::Shortcuts => "Shortcuts",
            Section::Indicator => "Indicator",
            Section::Restore => "Restore",
            Section::Overview => "Overview",
            Section::Motion => "Motion",
            Section::Touchpad => "Touchpad",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Workspaces => "\u{E8A9}",
            Section::Shortcuts => "\u{E765}",
            Section::Indicator => "\u{E712}",
            Section::Restore => "\u{E777}",
            Section::Overview => "\u{E7C4}",
            Section::Motion => "\u{EC4A}",
            Section::Touchpad => "\u{EFA5}",
        }
    }

    fn line(self) -> &'static str {
        match self {
            Section::Workspaces => "Every screen has its own workspaces, all the same count.",
            Section::Shortcuts => "Click a key to change it, × to remove it, + to add another. An action with no key is off. Choosing Windows gives that Windows key back.",
            Section::Indicator => "Shows the workspace for a moment after a switch, a send or a pin, and when a screen comes or goes.",
            Section::Restore => "Puts windows back on their workspaces after a reboot or sign-in.",
            Section::Overview => "Every workspace of every screen at once.",
            Section::Motion => "How des-k's fades and moves play. Windows' own Animation effects setting doesn't change them.",
            Section::Touchpad => "What 3- and 4-finger gestures do on a precision touchpad.",
        }
    }
}

/// Which section a problem belongs to, by its key; None: no one setting (the Notes section).
fn home(key: &str) -> Option<Section> {
    Some(match key.split('.').next().unwrap_or("") {
        "workspaces" | "follow_activation" => Section::Workspaces,
        "shortcuts" | "copilot_as_right_ctrl" => Section::Shortcuts,
        "indicator" => Section::Indicator,
        "restore" => Section::Restore,
        "overview" => Section::Overview,
        "fastest" => Section::Motion,
        "touchpad" | "gesture" => Section::Touchpad,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Toggle,
    /// A switch drawn as two choices: the word for on, then for off.
    Pair(&'static str, &'static str),
    /// A number in spec 0001's range, in fixed steps.
    Step { lo: i64, hi: i64, by: i64, unit: &'static str },
    /// The words the file takes, each with its label.
    Choice(&'static [(&'static str, &'static str)]),
}

struct Row {
    key: &'static str,
    title: &'static str,
    cap: &'static str,
    kind: Kind,
}

// The first run's words (AC-SET-6): Never, None and Fade; the file's words stay.
const LABEL_WORDS: &[(&str, &str)] = &[("hover", "On hover"), ("always", "Always"), ("none", "Never")];
const ANIMATION_WORDS: &[(&str, &str)] = &[("off", "None"), ("fade", "Fade")];
const STYLE_WORDS: &[(&str, &str)] = &[("card", "Card"), ("dots", "Dots")];

const fn row(key: &'static str, title: &'static str, cap: &'static str, kind: Kind) -> Row {
    Row { key, title, cap, kind }
}

const fn step(lo: i64, hi: i64, by: i64, unit: &'static str) -> Kind {
    Kind::Step { lo, hi, by, unit }
}

/// Every plain setting.
const ROWS: &[Row] = &[
    row("follow_activation", "Follow a window", "When a hidden window comes to the front (an app reuses it, a dialog pops up), go to its workspace.", Kind::Toggle),
    row("copilot_as_right_ctrl", "Copilot key as Right Ctrl", "", Kind::Pair("des-k", "Windows")),
    row("indicator.enabled", "Workspace indicator", "", Kind::Toggle),
    row("indicator.style", "Style", "", Kind::Choice(STYLE_WORDS)),
    row("indicator.hold_ms", "Indicator stays", "How long it shows. 0 to 5000 ms. With a line under it, at least 1.5 s.", step(0, 5000, 50, "ms")),
    row("indicator.fade_ms", "Indicator fades out", "How long it takes to go. 0 to 2000 ms.", step(0, 2000, 50, "ms")),
    row("restore.enabled", "Restore after restart", "Reopened windows go back to their workspaces, hidden.", Kind::Toggle),
    row("restore.minutes", "Restore runs for", "How long after des-k starts it puts reopened windows back. 1 to 30 min.", step(1, 30, 1, "min")),
    row("overview.labels", "Window names in overview", "", Kind::Choice(LABEL_WORDS)),
    row("overview.animation", "Overview animation", "", Kind::Choice(ANIMATION_WORDS)),
    row("overview.fade_ms", "Overview fades for", "How long the fade in and the fade out each take. 0 to 1000 ms.", step(0, 1000, 50, "ms")),
    row("overview.live", "Live overview", "Off: shows windows as they were when the overview opened.", Kind::Toggle),
    row("fastest", "Skip animations", "Every des-k fade and move happens at once, and the overview leaves out its window shadows. Windows stops animating the settings, welcome and hints windows as they open and close.", Kind::Toggle),
    row("touchpad.enabled", "Read touchpad gestures", "Off: des-k runs no gestures. Windows' own work only if they're on in Windows Settings.", Kind::Toggle),
    row("touchpad.swipe_mm", "Swipe distance", "How far fingers move before a swipe counts. 5 to 50 mm.", step(5, 50, 1, "mm")),
    row("touchpad.twice_ms", "Wait for a second gesture", "Only for gestures with a twice action; their single action runs after the wait. 100 to 1000 ms.", step(100, 1000, 50, "ms")),
];

/// The workspace count: drawn as the tiles, not as a row of its own (AC-SET-3).
const WORKSPACES: Row = row("workspaces", "Workspaces per screen", "", step(2, 9, 1, ""));

static DEFAULTS: LazyLock<Settings> = LazyLock::new(Settings::default);

fn find_row(key: &str) -> Option<&'static Row> {
    ROWS.iter().chain([&WORKSPACES]).find(|r| r.key == key)
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
        "fastest" => s.fastest as i64,
        "indicator.enabled" => s.indicator_enabled as i64,
        "indicator.style" => index(word(settings::STYLES, s.indicator_style), STYLE_WORDS),
        "indicator.hold_ms" => s.hold_ms as i64,
        "indicator.fade_ms" => s.fade_ms as i64,
        "restore.enabled" => s.restore_enabled as i64,
        "restore.minutes" => s.restore_minutes as i64,
        "overview.labels" => index(word(settings::LABELS, s.overview_labels), LABEL_WORDS),
        "overview.animation" => index(word(settings::ANIMATIONS, s.overview_animation), ANIMATION_WORDS),
        "overview.fade_ms" => s.overview_fade_ms as i64,
        "overview.live" => s.overview_live as i64,
        "touchpad.enabled" => s.touchpad_enabled as i64,
        "touchpad.swipe_mm" => s.swipe_mm as i64,
        "touchpad.twice_ms" => s.twice_ms as i64,
        _ => 0,
    }
}

fn file_value(row: &Row, v: i64) -> toml_edit::Value {
    match row.kind {
        Kind::Toggle | Kind::Pair(..) => (v != 0).into(),
        Kind::Step { .. } => v.into(),
        Kind::Choice(words) => words[v as usize].0.into(),
    }
}

/// A value in the page's words: "550 ms", "on", "des-k", "On hover".
fn shown(row: &Row, v: i64) -> String {
    match row.kind {
        Kind::Toggle => if v != 0 { "on" } else { "off" }.into(),
        Kind::Pair(on, off) => if v != 0 { on } else { off }.into(),
        Kind::Step { unit: "", .. } => v.to_string(),
        Kind::Step { unit, .. } => format!("{v} {unit}"),
        Kind::Choice(words) => words.get(v as usize).map_or("", |(_, l)| l).into(),
    }
}

/// The next value on the step grid. A hand written value off the grid steps onto it.
fn stepped(v: i64, lo: i64, hi: i64, by: i64, up: bool) -> i64 {
    let off = v.rem_euclid(by);
    let n = if up { v - off + by } else if off != 0 { v - off } else { v - by };
    n.clamp(lo, hi)
}

/// Why a kept value is not in use now (AC-SET-8): every reason, in the caption's order. The
/// Style row greys with the indicator and needs no words: the switch is right above it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Why {
    AnimationNone,
    Skip,
    IndicatorOff,
}

fn unused(s: &Settings, key: &str) -> Vec<Why> {
    match key {
        "indicator.style" if !s.indicator_enabled => vec![Why::IndicatorOff],
        "indicator.fade_ms" if s.fastest => vec![Why::Skip],
        "overview.fade_ms" => {
            let mut why = vec![];
            if s.overview_animation == settings::Animation::Off {
                why.push(Why::AnimationNone);
            }
            if s.fastest {
                why.push(Why::Skip);
            }
            why
        }
        _ => vec![],
    }
}

// ---- Shortcuts ----

/// Windows' own key for an action: a combo, or a digit action's modifiers.
#[derive(Clone, Copy, PartialEq, Debug)]
enum WinKey {
    Combo(Combo),
    Digits(u8),
}

struct Short {
    key: &'static str,
    title: &'static str,
    win: Option<(WinKey, &'static str)>, // and what it does in Windows
}

const fn combo(mods: u8, vk: u16) -> WinKey {
    WinKey::Combo(Combo { mods, vk })
}

const fn short(key: &'static str, title: &'static str, win: Option<(WinKey, &'static str)>) -> Short {
    Short { key, title, win }
}

use settings::{ALT, CTRL, SHIFT, WIN};

const MOVE: &[Short] = &[
    short("shortcuts.previous", "Previous workspace", Some((combo(WIN | CTRL, 0x25), "switches desktops"))),
    short("shortcuts.next", "Next workspace", Some((combo(WIN | CTRL, 0x27), "switches desktops"))),
    short("shortcuts.jump", "Go to workspace 1–9", Some((WinKey::Digits(WIN), "opens taskbar app 1–9"))),
    short("shortcuts.overview", "Open overview", Some((combo(WIN, 0x09), "opens Task View"))),
    short("shortcuts.finder", "Find a window", Some((combo(WIN, 0x46), "opens Feedback Hub"))),
    short("shortcuts.hints", "Open shortcut hints", None),
];

const SEND: &[Short] = &[
    short("shortcuts.send_previous", "Send window to previous workspace", None),
    short("shortcuts.send_next", "Send window to next workspace", None),
    short("shortcuts.send_previous_stay", "Send window to previous workspace and stay", None),
    short("shortcuts.send_next_stay", "Send window to next workspace and stay", None),
    short("shortcuts.send_to", "Send window to 1–9", Some((WinKey::Digits(WIN | SHIFT), "opens a new window of taskbar app 1–9"))),
    short("shortcuts.send_to_stay", "Send window to 1–9 and stay", Some((WinKey::Digits(WIN | CTRL), "goes to the last window of taskbar app 1–9"))),
    short("shortcuts.pin", "Pin window to all workspaces", None),
];

/// A [shortcuts] key's name: "shortcuts.finder" → "finder".
fn name_of(key: &str) -> &str {
    key.strip_prefix("shortcuts.").unwrap_or(key)
}

fn is_digits(key: &str) -> bool {
    settings::SETS.contains(&name_of(key))
}

fn shortcut(key: &str) -> Option<&'static Short> {
    MOVE.iter().chain(SEND).find(|s| s.key == key || name_of(s.key) == key)
}

/// An action by its name in [shortcuts]: its page key and title.
fn entry(name: &str) -> (&'static str, &'static str) {
    shortcut(name).map_or(("", ""), |s| (s.key, s.title))
}

/// A key in words, as its caps read: "Win Ctrl ←".
fn spelled(caps: &[String]) -> String {
    caps.iter()
        .map(|k| match k.as_str() {
            "Left" => "←".to_string(),
            "Right" => "→".to_string(),
            "Up" => "↑".to_string(),
            "Down" => "↓".to_string(),
            k => k.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn win_caps(k: WinKey) -> Vec<String> {
    match k {
        WinKey::Combo(c) => crate::hints::caps(&c),
        WinKey::Digits(m) => digit_caps(m),
    }
}

fn digit_caps(mods: u8) -> Vec<String> {
    crate::hints::mods_caps(mods).into_iter().chain(["1–9".to_string()]).collect()
}

/// An action's keys as their caps, in the file's order, each with its place in the file.
fn keys_of(s: &Settings, key: &str) -> Vec<Vec<String>> {
    let name = name_of(key);
    if is_digits(key) {
        return s.set(name).map(digit_caps).into_iter().collect();
    }
    s.list(name).iter().map(crate::hints::caps).collect()
}

/// The order the row draws its keys: Windows' own key first (AC-SET-4), then the file's order.
/// Each item is its index in the file, so × and a recording act on the right one.
fn drawn_order(s: &Settings, sh: &Short) -> Vec<usize> {
    let n = keys_of(s, sh.key).len();
    let mut order: Vec<usize> = (0..n).collect();
    if let Some((WinKey::Combo(c), _)) = sh.win
        && let Some(i) = s.list(name_of(sh.key)).iter().position(|x| *x == c)
    {
        order.retain(|&x| x != i);
        order.insert(0, i);
    }
    order
}

/// The action holds Windows' own key now.
fn has_win(s: &Settings, sh: &Short) -> bool {
    match sh.win {
        Some((WinKey::Combo(c), _)) => s.list(name_of(sh.key)).contains(&c),
        Some((WinKey::Digits(m), _)) => s.set(name_of(sh.key)) == Some(m),
        None => false,
    }
}

/// Another action holds this one's Windows key: its title.
fn took(s: &Settings, sh: &Short) -> Option<&'static str> {
    let me = name_of(sh.key);
    match sh.win?.0 {
        WinKey::Combo(c) => match s.combo_owner(me, c)? {
            Owner::List(other) | Owner::Set(other) => Some(entry(other).1),
        },
        WinKey::Digits(m) => s.set_owner(me, m).map(|o| entry(o).1),
    }
}

/// An action's keys as words for a caption: "Win Ctrl ← or Ctrl Alt ←", or "no key".
fn said(s: &Settings, key: &str) -> String {
    let all: Vec<String> = keys_of(s, key).iter().map(|k| spelled(k)).collect();
    if all.is_empty() { "no key".into() } else { all.join(" or ") }
}

fn same_keys(a: &Settings, b: &Settings, key: &str) -> bool {
    let name = name_of(key);
    if is_digits(key) {
        return a.set(name) == b.set(name);
    }
    let (mut x, mut y) = (a.list(name).to_vec(), b.list(name).to_vec());
    x.sort_by_key(|c| (c.mods, c.vk));
    y.sort_by_key(|c| (c.mods, c.vk));
    x == y
}

// ---- Problems (AC-SET-5) ----

/// A page key's title, for a problem's words.
fn title_of(key: &str) -> Option<&'static str> {
    find_row(key).map(|r| r.title).or_else(|| shortcut(key).map(|s| s.title))
}

/// A log line's combo in words: "Win+F" → "Win F", "Win+Alt" (a digit set) → "Win Alt 1–9".
fn combo_words(text: &str, set: bool) -> String {
    let caps: Vec<String> = text.split('+').map(String::from).collect();
    let mut w = spelled(&caps);
    if set {
        w += " 1–9";
    }
    w
}

/// A plain title made from a problem's log line (AC-SET-5, amends spec 0001); the line itself
/// shows under it.
fn title(p: &Problem, s: &Settings) -> String {
    let m = p.message.as_str();
    if let Some(rest) = m.strip_prefix("unknown key ") {
        return format!("“{}” isn't a setting, so des-k skips it", rest.trim_end_matches(", ignored"));
    }
    if let Some(rest) = m.strip_prefix("shortcuts: ") {
        // "{c} is in both {a} and {b}, {b} turned off" or "…, using defaults for both"
        if let Some((c, rest)) = rest.split_once(" is in both ")
            && let Some((a, rest)) = rest.split_once(" and ")
        {
            let b = rest.split(',').next().unwrap_or("");
            let set = settings::SETS.contains(&a);
            let (ta, tb) = (entry(a).1, entry(b).1);
            let c = combo_words(c, set);
            return if rest.contains("defaults for both") {
                format!("{c} was set for both {ta} and {tb}, so both use their defaults")
            } else {
                format!("{c} was set for both {ta} and {tb}, so {tb} has no key now")
            };
        }
    }
    if let Some(n) = m.strip_prefix("gesture rule ").and_then(|r| r.split(':').next()) {
        return if m.contains("already set by an earlier rule") {
            format!("Gesture rule {n} repeats an earlier one, so des-k skips it")
        } else if m.contains("Windows never lets a program type") {
            format!("Gesture rule {n} types a key Windows never lets a program type")
        } else {
            format!("Gesture rule {n} can't be used, so des-k skips it")
        };
    }
    if m.starts_with("gesture: ") {
        return "The gestures can't be read, so des-k uses the default gestures".into();
    }
    if m.contains(": expected a [") && m.ends_with("using defaults for it") {
        return format!("[{}] isn't a section, so des-k uses its defaults", p.key);
    }
    if let Some(t) = shortcut(&p.key).map(|sh| sh.title) {
        return if m.ends_with(", dropped") {
            format!("One key for “{t}” can't be read, so des-k skips it")
        } else {
            format!("des-k can't use this value, so “{t}” uses {}", said(s, &p.key))
        };
    }
    if let Some(r) = find_row(&p.key) {
        let v = value(s, r.key);
        return match r.kind {
            Kind::Toggle => format!("des-k can't use this value, so {} is {}", r.title, shown(r, v)),
            _ => format!("des-k can't use this value, so it uses {}", shown(r, v)),
        };
    }
    "des-k skips a line in settings.toml".into()
}

/// What the note under a bad value says to do (the r7 rule: Reset writes the default even
/// though des-k already uses it).
fn fix_line(key: &str, s: &Settings) -> String {
    let Some(r) = find_row(key) else { return "Change it here to fix the file.".into() };
    let d = shown(r, value(&DEFAULTS, key));
    match r.kind {
        Kind::Toggle | Kind::Pair(..) => format!("Turn it on or off here, or Reset writes {} to the file.", if value(&DEFAULTS, key) != 0 { "true" } else { "false" }),
        _ if !unused(s, key).is_empty() => format!("Reset puts {d} back in the file."),
        _ => format!("Change it here, or Reset puts {d} back in the file."),
    }
}

// ---- What the page shows of des-k and Windows ----

/// What the recorder is recording for (spec 0012 AC-7, AC-12).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rec {
    List(&'static str, Option<usize>), // a shortcut list, and the key it replaces (None: +)
    Digits(&'static str),
    Gesture(usize, u8), // a GESTURE_ROWS row and its times: Key combo…
}

/// A clash the recorder found, waiting for Move it here or Cancel. Nothing is saved until you choose.
struct Prompt {
    key: &'static str, // the row it shows under
    held: Vec<String>, // the caps of the key it found
    title: String,
    sub: String,
    edit: Edit,
}

/// Where a caption link goes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Link {
    Skip, // the Skip animations switch
    Indicator,
    Overview,
}

/// What a touchpad note's button does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pad {
    Windows,  // Set up in, or Open, Windows Settings: setup::use_gestures (AC-SET-6)
    ReadOn,   // Turn reading on
    GiveBack, // Give back, or Try again
}

/// Something you can click, focus from the keyboard, or both.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Side(Option<Section>), // None: Notes
    Welcome,
    Open, // Open settings.toml
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Stepper(&'static str), // focus only: a number's − and + are one stop, Left and Right step
    Choice(&'static str, usize),
    Pick(&'static str), // focus only: a choice is one stop, Left and Right move
    Reset(&'static str),
    Link(Link),
    Tiles, // focus only: the workspace tiles, one stop for every screen
    LastTile,
    TileAdd,
    TileRemove,
    Forget(usize), // a screen not plugged in, by its place in View::waiting
    KeyRow(&'static str), // focus only: a shortcut row is one stop
    Key(&'static str, usize), // a key field, by its index in the file: records one in its place
    KeyX(&'static str, usize),
    AddKey(&'static str), // + or Set key
    WinSide(&'static str, bool), // des-k (true) or Windows
    StopRec(&'static str),
    Move(&'static str),
    Cancel(&'static str),
    Picker(usize), // a gesture row's picker, by GESTURE_ROWS index
    Item(usize),   // a line of the open menu
    Show,          // the indicator preview's Show: the real indicator on this screen
    Chart,         // the Motion chart: never clicked or focused, one image for Narrator
    Pad(Pad),
    Dismiss,     // the save error banner
    DismissEdit, // the hand edit banner
}

impl Ctl {
    fn key(self) -> Option<&'static str> {
        match self {
            Ctl::Toggle(k) | Ctl::Minus(k) | Ctl::Plus(k) | Ctl::Stepper(k) | Ctl::Choice(k, _) | Ctl::Pick(k) | Ctl::Reset(k) => Some(k),
            Ctl::KeyRow(k) | Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) | Ctl::StopRec(k) | Ctl::Move(k) | Ctl::Cancel(k) => Some(k),
            _ => None,
        }
    }

    /// Changes the file (or starts recording a change); a read only page still lets you focus
    /// it and follow links.
    fn writes(self) -> bool {
        matches!(
            self,
            Ctl::Toggle(_) | Ctl::Minus(_) | Ctl::Plus(_) | Ctl::Choice(..) | Ctl::Reset(_) | Ctl::LastTile | Ctl::TileAdd | Ctl::TileRemove | Ctl::Forget(_)
                | Ctl::KeyRow(_) | Ctl::Key(..) | Ctl::KeyX(..) | Ctl::AddKey(_) | Ctl::WinSide(..) | Ctl::Move(_) | Ctl::Picker(_) | Ctl::Item(_) | Ctl::Pad(Pad::ReadOn | Pad::GiveBack)
        )
    }

    /// The keyboard stop this control belongs to: one per row (spec 0012 AC-16).
    fn stop(self) -> Ctl {
        match self {
            Ctl::Minus(k) | Ctl::Plus(k) => Ctl::Stepper(k),
            Ctl::Choice(k, _) => Ctl::Pick(k),
            Ctl::LastTile | Ctl::TileAdd | Ctl::TileRemove => Ctl::Tiles,
            Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) => Ctl::KeyRow(k),
            c => c,
        }
    }

    fn in_side(self) -> bool {
        matches!(self, Ctl::Side(_) | Ctl::Welcome | Ctl::Open)
    }
}

/// Where a control sits, in window dip. Group: 0 the side list, 1 the column (clipped under the
/// banners), 2 the banners, 3 the open menu.
#[derive(Clone, Copy)]
struct Hit {
    ctl: Ctl,
    at: Bx,
    group: u8,
}

/// The gesture picker's menu, drawn over the page (as the design places it, inside the window).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Menu {
    row: usize,
    times: u8,
    family: Option<usize>, // a digit family's own list: Go to workspace and the two Send window to
    at: Bx,                // the picker it opened from
    focus: usize,
}

/// What a menu line does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Does {
    Set(Option<Action>), // None: no twice action
    Combo,               // Key combo…: records one
    Family(usize),
    Twice, // opens the done twice menu
}

struct Line {
    text: String,
    col: u8, // 0 des-k, 1 Windows and media, 2 the footer, 3 the done twice line
    does: Does,
    on: bool,
    default: bool,
}

type Family = fn(usize) -> Action;
const FAMILIES: [(&str, Family); 3] = [("Go to workspace", Action::Jump), ("Send window to", |i| Action::SendTo(i, true)), ("Send window to … and stay", |i| Action::SendTo(i, false))];

/// No twice rule and a twice rule that does nothing behave apart (the second still waits), so
/// their words must not look alike (spec 0012 AC-12).
const NO_TWICE: &str = "No twice action";
const TWICE_NOTHING: &str = "Nothing (still waits)";

fn once_label(a: Action) -> String {
    match a {
        Action::Combo(c) => format!("Types {}", spelled(&crate::hints::caps(&c))),
        a => crate::hints::name(a),
    }
}

fn twice_label(a: Option<Action>) -> String {
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
    matches!(a, Action::Switch(_) | Action::Send(..) | Action::Jump(_) | Action::SendTo(..) | Action::Finder | Action::Overview | Action::Pin)
}

/// The open menu's lines, in keyboard order.
fn menu_lines(m: &Menu, s: &Settings) -> Vec<Line> {
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
    let desk = [Action::Switch(1), Action::Switch(-1), Action::Overview, Action::Finder, Action::Send(1, true), Action::Send(-1, true), Action::Send(1, false), Action::Send(-1, false), Action::Pin];
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

/// What the tiles, the preview and the touchpad draw, copied out of ws and Windows.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::ScreenInfo>,
    view: crate::ws::View,
    pad: Option<PadState>,
    back: Option<GaveBack>,
    here: Option<String>, // the device the page is on, for the indicator preview
}

/// The pictures' data now, or None while ws is busy (the next tick tries again).
fn pictures(h: Option<HWND>) -> Option<Pics> {
    let n = settings::get().workspaces;
    let view = crate::ws::view(&[n.saturating_sub(1).max(2), DEFAULTS.workspaces])?;
    let screens = crate::ws::screen_info();
    let here = h.and_then(|h| {
        let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
        let mut mi = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
        unsafe { GetMonitorInfoW(mon, &mut mi.monitorInfo).as_bool() }.then(|| String::from_utf16_lossy(&mi.szDevice).trim_end_matches('\0').trim_start_matches(r"\\.\").to_string())
    });
    Some(Pics { screens, view, pad: Some(setup::state()), back: crate::welcome::gave_back(), here })
}

/// Takes a new copy of the pictures' data; lays the page out again only if it changed.
fn update_pictures(always: bool) {
    let changed = pictures(hwnd()).and_then(|new| {
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

/// Main loop, every second and on a display change: the tiles follow your windows while the
/// page is open and not minimized.
pub fn tick() {
    if let Some(h) = hwnd()
        && !unsafe { IsIconic(h) }.as_bool()
    {
        update_pictures(false);
    }
}

/// A hand edit was put in effect: an open page shows what changed until Dismiss or the next
/// one. Changes made on the page show none.
pub fn hand_edit(changes: &[settings::Change], _moves: &[String]) {
    if hwnd().is_none() {
        return;
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let lines: Vec<String> = changes.iter().map(|c| changed(c.key, &c.was, &c.now)).collect();
    let _ = set(|w| w.edited = Some((format!("{:02}:{:02}", t.wHour, t.wMinute), lines)));
}

/// One hand edit line in the page's words: "Indicator stays: 550 → 800 ms", "Open overview:
/// Win Tab → no key".
fn changed(key: &str, was: &str, now: &str) -> String {
    if key == "gestures" {
        return "Gestures: changed".into();
    }
    let name = title_of(key).unwrap_or(key);
    let words = |v: &str| -> String {
        if let Some(r) = find_row(key) {
            return match r.kind {
                Kind::Toggle | Kind::Pair(..) => shown(r, (v == "true") as i64),
                Kind::Choice(list) => list.iter().find(|(w, _)| *w == v).map_or(v.to_string(), |(_, l)| l.to_string()),
                Kind::Step { .. } => v.parse().map_or(v.to_string(), |n| shown(r, n)),
            };
        }
        if is_digits(key) {
            return if v.contains('+') || ["Win", "Ctrl", "Alt", "Shift"].contains(&v) { combo_words(v, true) } else { "no key".into() };
        }
        let list = v.trim_start_matches('[').trim_end_matches(']');
        let keys: Vec<String> = list.split(", ").filter(|k| !k.is_empty()).map(|k| combo_words(k, false)).collect();
        if keys.is_empty() { "no key".into() } else { keys.join(" or ") }
    };
    let (a, b) = (words(was), words(now));
    // "550 → 800 ms": the unit once, after the new value.
    match find_row(key).map(|r| r.kind) {
        Some(Kind::Step { unit, .. }) if !unit.is_empty() => format!("{name}: {} → {b}", a.trim_end_matches(unit).trim_end()),
        _ => format!("{name}: {a} → {b}"),
    }
}

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    scroll: f32,
    height: f32,  // the column's full height
    fixed: f32,   // the banners' height above it
    tops: Vec<(Option<Section>, f32)>, // where each section starts in the column; None: Notes
    hits: Vec<Hit>,
    focus: Option<Ctl>,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    drag: Option<f32>, // dragging the scroll thumb: where in the thumb it was grabbed
    vw: f32,           // the client area, in dip
    vh: f32,
    error: Option<(String, String)>, // the last save that failed: what, and Windows' words
    rec: Option<Rec>,
    refused: Option<(String, String)>, // the last key the recorder refused: title and line
    prompt: Option<Prompt>,
    menu: Option<Menu>,
    pics: Pics,
    edited: Option<(String, Vec<String>)>, // the last hand edit's banner: its time and lines
    tree: Tree,
}

thread_local! {
    static PAGE: RefCell<Option<Win>> = const { RefCell::new(None) };
}

// Never hold the borrow across a call that comes back here: settings::write reloads, and the
// reload calls refresh().
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&PAGE, "settings page get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&PAGE, "settings page set", f)
}

/// What one pass of compose found.
struct Out {
    hits: Vec<Hit>,
    height: f32,
    fixed: f32,
    tops: Vec<(Option<Section>, f32)>,
    said: Vec<(Ctl, Node)>, // what Narrator reads for each stop, in page order
}

/// A piece of a caption: words, words in the text color (a reason), or a link.
enum P {
    T(String),
    Why(String),
    Link(String, Ctl),
    Mono(String),
}

fn t(s: impl Into<String>) -> P {
    P::T(s.into())
}

/// A note's look: its icon's color, and whether it sits on its own with an edge.
#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Info,
    Warn,
    Good,
    Rec,
}

/// One pass over the page, drawing or only measuring.
struct Ctx<'a, 'f> {
    c: &'a mut Canvas<'f>,
    w: &'a Win,
    s: &'static Settings,
    ro: bool,
    cw: f32,  // the column's width
    top0: f32, // where the column starts in the window, after scrolling
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
        let (hits, tops, said) = (self.out.hits.len(), self.out.tops.len(), self.out.said.len());
        let h = f(self);
        self.out.hits.truncate(hits);
        self.out.tops.truncate(tops);
        self.out.said.truncate(said);
        self.c.draw = draw;
        h
    }

    /// What Narrator reads for a stop: kept once, the first time the stop is drawn.
    fn say(&mut self, ctl: Ctl, role: UIA_CONTROLTYPE_ID, name: &str, help: &str, greyed: bool, state: (Option<bool>, Option<String>)) {
        if self.out.said.iter().any(|(c, _)| *c == ctl) {
            return;
        }
        let node = Node { role, name: name.to_string(), help: help.to_string(), greyed, focusable: true, toggled: state.0, value: state.1, ..Node::default() };
        self.out.said.push((ctl, node));
    }

    fn focused(&self, ctl: Ctl) -> bool {
        self.w.kb && self.w.focus == Some(ctl)
    }

    fn hovered(&self, ctl: Ctl) -> bool {
        self.w.hover == Some(ctl)
    }

    /// The focus ring: 2 dip in the text color, 2 dip out (AC-SET-9).
    fn ring(&self, (x, y, w, h): Bx, r: f32, ctl: Ctl) {
        if self.focused(ctl) {
            self.c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), r + 3.0, 2.0, op(self.c.t.b.tx));
        }
    }

    fn icon(&self, x: f32, y: f32, lh: f32, icon: &str, f: F, color: u32) {
        self.c.text(x, y, lh, icon, f, color);
    }

    /// A field's fill with its edge: 1 dip all round and the darker bottom edge you chose
    /// (AC-SET-9); high contrast has the edge alone.
    fn field(&self, (x, y, w, h): Bx, fill: u32, ring: Option<u32>) {
        let b = self.c.t.b;
        match ring {
            Some(col) => {
                self.c.fill_rr((x, y, w, h), 6.0, op(col));
                self.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(fill));
            }
            None if self.c.t.hc => self.c.boxed((x, y, w, h), 6.0, fill, Some(b.line)),
            None => {
                self.c.fill_rr((x, y, w, h), 6.0, op(b.tx3));
                self.c.fill_rr((x, y, w, h - 1.0), 6.0, op(b.line));
                self.c.fill_rr((x + 1.0, y + 1.0, w - 2.0, h - 3.0), 5.0, op(fill));
            }
        }
    }

    fn btn_w(&self, label: &str, icon: Option<&str>) -> f32 {
        self.c.width(label, F::Body) + 28.0 + icon.map_or(0.0, |_| 18.0)
    }

    /// A button: 32 dip, its label, an icon before it if any. `na`: greyed. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, ctl: Ctl, x: f32, y: f32, label: &str, icon: Option<&str>, na: bool, group: u8) -> f32 {
        let b = self.c.t.b;
        let w = self.btn_w(label, icon);
        let off = na || (self.ro && ctl.writes());
        let fill = if self.hovered(ctl) && !off { mix(b.card, b.tx, 0.05) } else { b.card };
        self.field((x, y, w, 32.0), fill, None);
        let ink = if off { b.tx3 } else { b.tx };
        let mut tx = x + 14.0;
        if let Some(i) = icon {
            self.icon(tx, y, 32.0, i, F::IconSmall, ink);
            tx += 18.0;
        }
        self.c.text(tx, y, 32.0, label, F::Body, ink);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), group);
        if group != 0 {
            self.say(ctl, UIA_ButtonControlTypeId, label, "", off, (None, None));
        }
        w
    }

    /// The word and the switch: 40 × 20, with On or Off before it. `x`: the word's left edge.
    fn switch_w(&self, on: bool) -> f32 {
        self.c.width(if on { "On" } else { "Off" }, F::Body) + 10.0 + 40.0
    }

    fn switch(&mut self, key: &'static str, x: f32, y: f32, on: bool) {
        let b = self.c.t.b;
        let ctl = Ctl::Toggle(key);
        let label = if on { "On" } else { "Off" };
        let lw = self.c.width(label, F::Body);
        self.c.text(x, y, 20.0, label, F::Body, b.tx2);
        let (sx, ro) = (x + lw + 10.0, self.ro);
        let track = (sx, y, 40.0, 20.0);
        if on {
            self.c.fill_rr(track, 10.0, op(if ro { b.tx3 } else { b.acc }));
            self.c.circle(sx + 30.0, y + 10.0, 6.0, Some(op(b.on_acc)), None);
        } else {
            let fill = if self.hovered(ctl) && !ro { mix(b.card, b.tx, 0.06) } else { b.card };
            self.c.boxed(track, 10.0, fill, Some(if ro { b.tx3 } else { b.tx2 }));
            self.c.circle(sx + 10.0, y + 10.0, 6.0, Some(op(if ro { b.tx3 } else { b.tx2 })), None);
        }
        self.ring(track, 10.0, ctl);
        self.hit(ctl, (x, y - 6.0, lw + 50.0, 32.0), 1);
    }

    fn stepper_w(&self, text: &str) -> f32 {
        64.0 + (self.c.width(text, F::Body) + 16.0).max(56.0)
    }

    /// − value +, one keyboard stop (AC-SET-3: steppers only). `na`: greyed, kept but not in use;
    /// its − and + do nothing.
    fn stepper(&mut self, key: &'static str, x: f32, y: f32, (v, unit): (i64, &str), (down, up): (bool, bool), na: bool) {
        let b = self.c.t.b;
        let num = v.to_string();
        let unit = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        let (nw, uw) = (self.c.width(&num, F::Body), self.c.width(&unit, F::Small));
        let w = 64.0 + (nw + uw + 16.0).max(56.0);
        self.field((x, y, w, 32.0), b.card, None);
        let mid = w - 64.0;
        let tx = x + 32.0 + (mid - nw - uw) / 2.0;
        let off = na || self.ro;
        self.c.text(tx, y, 32.0, &num, F::Body, if off { b.tx3 } else { b.tx });
        self.c.text(tx + nw, y, 32.0, &unit, F::Small, if off { b.tx3 } else { b.tx2 });
        for (ctl, ix, icon, live) in [(Ctl::Minus(key), x, I_MINUS, down), (Ctl::Plus(key), x + w - 32.0, I_ADD, up)] {
            let live = live && !off;
            if self.hovered(ctl) && live {
                self.c.fill_rr((ix + 4.0, y + 4.0, 24.0, 24.0), 4.0, op(mix(b.card, b.tx, 0.08)));
            }
            let iw = self.c.width(icon, F::IconTiny);
            self.icon(ix + (32.0 - iw) / 2.0, y, 32.0, icon, F::IconTiny, if live { b.tx } else { b.tx3 });
            self.hit(ctl, (ix, y, 32.0, 32.0), 1);
        }
        self.ring((x, y, w, 32.0), 6.0, Ctl::Stepper(key));
    }

    fn seg_w(&self, words: &[&str]) -> f32 {
        4.0 + words.iter().map(|l| self.c.width(l, F::Small) + 20.0).sum::<f32>() + 2.0 * (words.len() as f32 - 1.0)
    }

    /// Choice buttons in one track, 28 dip; the chosen one raised with the ring (AC-SET-9).
    /// `na`: greyed. Each word is a Choice(key, i), or for a shortcut row a WinSide.
    #[allow(clippy::too_many_arguments)]
    fn seg(&mut self, x: f32, y: f32, words: &[&str], sel: Option<usize>, na: bool, ctl: impl Fn(usize) -> Ctl, stop: Ctl) {
        let b = self.c.t.b;
        let w = self.seg_w(words);
        self.c.boxed((x, y, w, 28.0), 6.0, b.track, Some(b.line));
        let off = na || self.ro;
        let mut ox = x + 2.0;
        for (i, label) in words.iter().enumerate() {
            let iw = self.c.width(label, F::Small) + 20.0;
            let bx = (ox, y + 2.0, iw, 24.0);
            let chosen = sel == Some(i) && !na;
            let fg = if chosen {
                if self.c.t.hc {
                    self.c.fill_rr(bx, 4.0, op(if self.ro { b.tx3 } else { b.hi }));
                    b.on_acc
                } else {
                    self.c.fill_rr((bx.0 - 1.0, bx.1 - 1.0, bx.2 + 2.0, bx.3 + 2.0), 5.0, op(b.tx3));
                    self.c.fill_rr(bx, 4.0, op(b.hi));
                    if self.ro { b.tx3 } else { b.tx }
                }
            } else {
                if self.hovered(ctl(i)) && !off {
                    self.c.fill_rr(bx, 4.0, op(mix(b.track, b.tx, 0.06)));
                }
                if off { b.tx3 } else { b.tx2 }
            };
            self.c.text(ox + 10.0, y + 2.0, 24.0, label, if chosen { F::SmallBold } else { F::Small }, fg);
            self.hit(ctl(i), bx, 1);
            ox += iw + 2.0;
        }
        self.ring((x, y, w, 28.0), 6.0, stop);
    }

    /// A caption's pieces as one wrapped text, 12/16; links get their hits, hover and ring.
    /// Returns its height.
    fn caption(&mut self, x: f32, y: f32, w: f32, pieces: &[P]) -> f32 {
        let b = self.c.t.b;
        let runs: Vec<(&str, F, u32)> = pieces
            .iter()
            .map(|p| match p {
                P::T(s) => (s.as_str(), F::Small, b.tx2),
                P::Why(s) => (s.as_str(), F::Small, b.tx),
                P::Link(s, _) => (s.as_str(), F::SmallBold, b.link),
                P::Mono(s) => (s.as_str(), F::Mono, b.tx2),
            })
            .collect();
        let (h, boxes) = self.c.para_boxes(x, y, w, 16.0, &runs);
        for (p, bxs) in pieces.iter().zip(&boxes) {
            let P::Link(text, ctl) = p else { continue };
            for &(bx, by, bw, bh) in bxs {
                if self.hovered(*ctl) || self.c.t.hc {
                    self.c.rule(bx, by + bh - 2.0, bw, op(b.link));
                }
                self.hit(*ctl, (bx - 2.0, by, bw + 4.0, bh), 1);
            }
            if let Some(&first) = bxs.first() {
                self.ring((first.0 - 1.0, first.1, first.2 + 2.0, first.3), 2.0, *ctl);
            }
            self.say(*ctl, UIA_HyperlinkControlTypeId, text.trim(), "", false, (None, None));
        }
        h
    }

    fn label_h(&mut self, w: f32, title: &str, cap: &[P]) -> f32 {
        self.measured(|c| c.label(0.0, 0.0, w, title, cap))
    }

    /// A row's name, 600 14/20, and its caption under it.
    fn label(&mut self, x: f32, y: f32, w: f32, title: &str, cap: &[P]) -> f32 {
        let b = self.c.t.b;
        let mut h = 0.0;
        if !title.is_empty() {
            h += self.c.para(x, y, w, 20.0, &[(title, F::Bold, b.tx)]);
        }
        if !cap.is_empty() {
            h += self.caption(x, y + h, w, cap);
        }
        h
    }

    /// One row in a card: name and caption on the left, the control on the right, both
    /// centred; under them, a note if one is given. The control is drawn first, so a link in
    /// the caption is its own stop after it (Reset under its row). Returns the row's height.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, y: f32, title: &str, cap: &[P], (cw, ch): (f32, f32), ctl: impl FnOnce(&mut Self, f32, f32), note: Option<&NoteSpec>) -> f32 {
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let lw = if cw > 0.0 { w - cw - 16.0 } else { w };
        let lh = self.label_h(lw, title, cap);
        let top = lh.max(ch).max(ROW - 16.0);
        ctl(self, x + w - cw, y + 8.0 + (top - ch) / 2.0);
        self.label(x, y + 8.0 + (top - lh) / 2.0, lw, title, cap);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.in_note(x, y + h + 6.0, w, n);
        }
        h + 8.0
    }

    /// A card's line between two rows.
    fn sep(&self, y: f32) {
        self.c.rule(X0 + 1.0, y, self.cw - 2.0, op(self.c.t.b.line));
    }

    /// A card around `body`, which draws from its top and returns its height.
    fn card(&mut self, y: f32, body: impl Fn(&mut Self, f32) -> f32) -> f32 {
        let h = self.measured(|c| body(c, y));
        let b = self.c.t.b;
        self.c.boxed((X0, y, self.cw, h), 8.0, b.card, Some(b.line));
        body(self, y);
        h
    }

    /// A card's tinted header: its title, and a caption on the right. `n`: the fingers' circle.
    fn header(&mut self, y: f32, title: &str, n: Option<u8>, cap: &str) -> f32 {
        let b = self.c.t.b;
        let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc, 0.09) };
        self.c.fill_rr((X0 + 1.0, y + 1.0, self.cw - 2.0, 36.0), 7.0, op(tint));
        self.c.fill_rr((X0 + 1.0, y + 20.0, self.cw - 2.0, 17.0), 0.0, op(tint));
        self.c.rule(X0 + 1.0, y + 37.0, self.cw - 2.0, op(b.line));
        let mut x = X0 + 16.0;
        if let Some(n) = n {
            self.c.circle(x + 11.0, y + 19.0, 11.0, Some(op(b.acc)), None);
            let d = n.to_string();
            let dw = self.c.width(&d, F::SmallBold);
            self.c.text(x + 11.0 - dw / 2.0, y + 8.0, 22.0, &d, F::SmallBold, b.on_acc);
            x += 32.0;
        }
        self.c.text(x, y + 8.0, 22.0, title, F::Bold, b.tx);
        if !cap.is_empty() {
            let cw = self.c.width(cap, F::Small);
            self.c.text(X0 + self.cw - 16.0 - cw, y + 8.0, 22.0, cap, F::Small, b.tx2);
        }
        38.0
    }

    fn tone_color(&self, tone: Tone) -> u32 {
        let b = self.c.t.b;
        match tone {
            Tone::Info => b.tx2,
            Tone::Warn => b.warn,
            Tone::Good => b.good,
            Tone::Rec => b.acc,
        }
    }

    /// A note's text and buttons, from `x`; returns its height. Buttons sit on the right when
    /// they leave the text room, else under it.
    fn note_body(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec) -> f32 {
        let col = self.tone_color(n.tone);
        let b = self.c.t.b;
        self.icon(x, y + 2.0, 16.0, n.icon, F::Icon, col);
        let bw: f32 = n.buttons.iter().map(|(_, l)| self.btn_w(l, None) + 8.0).sum();
        let beside = bw > 0.0 && bw < w * 0.4;
        let tx = x + 28.0;
        let tw = if beside { w - 28.0 - bw - 4.0 } else { w - 28.0 };
        let mut h = self.c.para(tx, y, tw, 20.0, &[(&n.title, F::Bold, b.tx)]);
        if !n.lines.is_empty() {
            h += self.caption(tx, y + h, tw, &n.lines);
        }
        if bw > 0.0 {
            let by = if beside { y + ((h - 32.0) / 2.0).max(0.0) } else { y + h + 8.0 };
            let mut bx = if beside { x + w - bw + 8.0 } else { tx };
            for (ctl, label) in &n.buttons {
                bx += self.button(*ctl, bx, by, label, None, false, n.group) + 8.0;
            }
            h = if beside { h.max(32.0) } else { h + 8.0 + 32.0 };
        }
        h
    }

    /// A note on its own: card colored, with its edge (above a section, a banner, Notes).
    fn note(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec) -> f32 {
        let h = self.measured(|c| c.note_body(x + 12.0, y + 10.0, w - 24.0, n)) + 20.0;
        let b = self.c.t.b;
        self.c.boxed((x, y, w, h), 6.0, b.card, Some(b.line));
        self.note_body(x + 12.0, y + 10.0, w - 24.0, n);
        h
    }

    /// A note inside a row: a dashed line over it, on the row's own background.
    fn in_note(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec) -> f32 {
        let b = self.c.t.b;
        if self.c.draw {
            self.c.stroke_dash((x, y, w, 0.0), 0.0, 1.0, op(b.line), DashStyleDash);
        }
        10.0 + self.note_body(x, y + 10.0, w, n) + 2.0
    }

    /// The default and Reset at the end of a caption, when the value differs, or is bad.
    fn default_pieces(&self, key: &'static str, bad: bool) -> Vec<P> {
        let Some(r) = find_row(key) else { return vec![] };
        let (v, d) = (value(self.s, key), value(&DEFAULTS, key));
        if v == d && !bad {
            return vec![];
        }
        vec![t(format!(" Default: {}.", shown(r, d))), P::Link(" Reset".into(), Ctl::Reset(key))]
    }

    /// Problems whose key is this row's, as its in-row note (AC-SET-5).
    fn bad(&self, key: &str) -> Option<NoteSpec> {
        let p = settings::problems().iter().find(|p| p.key == key)?;
        let mut lines = vec![P::Mono(p.message.clone())];
        if find_row(key).is_some() {
            lines.push(t(format!(" {}", fix_line(key, self.s))));
        }
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, self.s), lines, buttons: vec![], group: 1 })
    }

    /// A plain setting's row, with its control, default, Reset, greyed reasons and problem.
    fn setting(&mut self, key: &'static str, y: f32) -> f32 {
        let Some(r) = find_row(key) else { return 0.0 };
        let v = value(self.s, key);
        let why = unused(self.s, key);
        let na = !why.is_empty();
        let note = self.bad(key);
        let mut cap: Vec<P> = vec![];
        let base = match key {
            "indicator.style" if v == 0 => "The number and a bar per workspace, with the screen's name when you have two or more.",
            "indicator.style" => "A dot per workspace, the current one wide.",
            "copilot_as_right_ctrl" if v != 0 => "So Win Ctrl keys work with it. Taken from Windows, where it opens Copilot, or what you set.",
            "copilot_as_right_ctrl" => "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl.",
            _ => r.cap,
        };
        if !base.is_empty() {
            cap.push(t(base));
        }
        let reasons: Vec<&Why> = why.iter().filter(|w| **w != Why::IndicatorOff).collect();
        if !reasons.is_empty() {
            cap.push(P::Why(" Not in use: ".into()));
            for (i, w) in reasons.iter().enumerate() {
                if i > 0 {
                    cap.push(P::Why(", and ".into()));
                }
                match w {
                    Why::AnimationNone => cap.push(P::Why("Overview animation is None".into())),
                    Why::Skip => {
                        cap.push(P::Link("Skip animations".into(), Ctl::Link(Link::Skip)));
                        cap.push(P::Why(" is on".into()));
                    }
                    Why::IndicatorOff => {}
                }
            }
            cap.push(P::Why(".".into()));
        }
        cap.extend(self.default_pieces(key, note.is_some()));
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) => s.as_str() }).collect::<String>().trim().to_string();
        let ro = self.ro;
        match r.kind {
            Kind::Toggle => {
                let size = (self.switch_w(v != 0), 20.0);
                self.say(Ctl::Toggle(key), UIA_ButtonControlTypeId, r.title, &help, ro, (Some(v != 0), None));
                self.row(y, r.title, &cap, size, |c, x, y| c.switch(key, x, y, v != 0), note.as_ref())
            }
            Kind::Pair(on, off) => {
                let size = (self.seg_w(&[on, off]), 28.0);
                self.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, r.title, &help, ro, (None, Some(shown(r, v))));
                let sel = Some(if v != 0 { 0 } else { 1 });
                self.row(y, r.title, &cap, size, |c, x, y| c.seg(x, y, &[on, off], sel, false, |i| Ctl::Choice(key, i), Ctl::Pick(key)), note.as_ref())
            }
            Kind::Choice(words) => {
                let labels: Vec<&str> = words.iter().map(|(_, l)| *l).collect();
                let size = (self.seg_w(&labels), 28.0);
                self.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, r.title, &help, ro || na, (None, Some(shown(r, v))));
                self.row(y, r.title, &cap, size, |c, x, y| c.seg(x, y, &labels, Some(v as usize), na, |i| Ctl::Choice(key, i), Ctl::Pick(key)), note.as_ref())
            }
            Kind::Step { lo, hi, unit, .. } => {
                // A bad value shows what des-k uses: the default, already v.
                let text = format!("{v} {unit}");
                let size = (self.stepper_w(&text), 32.0);
                let spoken = format!("{v} {}", if unit == "ms" { "milliseconds" } else if unit == "min" { "minutes" } else if unit == "mm" { "millimetres" } else { "" }).trim().to_string();
                self.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, r.title, &help, ro || na, (None, Some(spoken)));
                self.row(y, r.title, &cap, size, |c, x, y| c.stepper(key, x, y, (v, unit), (v > lo, v < hi), na), note.as_ref())
            }
        }
    }

    // ---- Sections ----

    fn heading(&mut self, sec: Option<Section>, y: f32) -> f32 {
        let b = self.c.t.b;
        self.out.tops.push((sec, y - self.top0));
        let name = sec.map_or("Notes", Section::name);
        self.c.text(X0, y, 28.0, name, F::Title, b.tx);
        if sec == Some(Section::Restore) {
            self.chip(X0 + self.c.width(name, F::Title) + 12.0, y + 2.0, "\u{E777}", "Applies from des-k's next start");
        }
        let line = sec.map_or("Lines in settings.toml that des-k skips. Fix them in the file; each note goes once its line is fixed.", Section::line);
        let h = 30.0 + self.c.para(X0, y + 30.0, self.cw, 20.0, &[(line, F::Body, b.tx2)]);
        h + 12.0
    }

    /// The purple chip after a heading.
    fn chip(&mut self, x: f32, y: f32, icon: &str, text: &str) {
        let b = self.c.t.b;
        let w = 10.0 + 12.0 + 6.0 + self.c.width(text, F::Small) + 10.0;
        let (fill, edge, ic) = if self.c.t.hc { (b.card, b.tx, b.tx) } else { (mix(b.card, b.acc, 0.10), mix(b.card, b.acc, 0.30), b.acc) };
        self.c.boxed((x, y, w, 24.0), 12.0, fill, Some(edge));
        self.icon(x + 10.0, y, 24.0, icon, F::IconSmall, ic);
        self.c.text(x + 28.0, y, 24.0, text, F::Small, b.tx);
    }

    /// This section's problems no row owns (a key des-k turned off), above its cards.
    fn section_notes(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = 0.0;
        let owned = |k: &str| find_row(k).is_some() || shortcut(k).is_some() || k == "workspaces";
        let list: Vec<&Problem> = settings::problems().iter().filter(|p| home(&p.key) == Some(sec) && !owned(&p.key)).collect();
        for p in list {
            let fix = match sec {
                Section::Shortcuts => "Set a key for it below to fix the file. ",
                _ => "",
            };
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, self.s), lines: vec![t(fix), P::Mono(p.message.clone())], buttons: vec![], group: 1 };
            h += self.note(X0, y + h, self.cw, &n) + 12.0;
        }
        h
    }

    fn section(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = self.heading(Some(sec), y);
        h += self.section_notes(sec, y + h);
        h += match sec {
            Section::Workspaces => self.card(y + h, |c, y| c.workspaces(y)),
            Section::Shortcuts => self.shortcuts(y + h),
            Section::Indicator => self.card(y + h, |c, y| c.indicator(y)),
            Section::Restore => self.card(y + h, |c, y| c.restore(y)),
            Section::Overview => self.card(y + h, |c, y| {
                let mut h = c.overview_art(y);
                for (i, key) in ["overview.labels", "overview.animation", "overview.fade_ms", "overview.live"].into_iter().enumerate() {
                    if i > 0 {
                        c.sep(y + h);
                    }
                    h += c.setting(key, y + h);
                }
                h
            }),
            Section::Motion => self.card(y + h, |c, y| {
                let h = c.motion(y);
                c.sep(y + h);
                h + c.setting("fastest", y + h)
            }),
            Section::Touchpad => self.touchpad(y + h),
        };
        h
    }

    // ---- Workspaces ----

    fn workspaces(&mut self, y: f32) -> f32 {
        let mut h = self.screens_art(y);
        let w = self.w;
        let n = self.s.workspaces;
        let removing = !self.ro && n > 2 && (self.hovered(Ctl::TileRemove) || self.hovered(Ctl::LastTile) || self.focused(Ctl::Tiles));
        let resetting = !self.ro && n > DEFAULTS.workspaces && (self.hovered(Ctl::Reset("workspaces")) || self.focused(Ctl::Reset("workspaces")));
        let gone: Vec<usize> = if removing { vec![n - 1] } else if resetting { (DEFAULTS.workspaces..n).collect() } else { vec![] };
        for (i, sc) in w.pics.screens.clone().iter().enumerate() {
            if i > 0 || h > 0.0 {
                self.sep(y + h);
            }
            h += self.screen_row(y + h, sc, &gone);
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
            let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc, 0.06) };
            self.c.fill_rr((X0 + 1.0, y + h + 1.0, self.cw - 2.0, hh - 1.0), 0.0, op(tint));
        }
        h += self.row(y + h, "Workspaces per screen", &cap, (0.0, 0.0), |_, _, _| {}, note.as_ref());
        self.sep(y + h);
        h + self.setting("follow_activation", y + h)
    }

    /// What a removal moves, per screen, unplugged ones included (AC-SET-3), since there is no undo.
    fn removal(&self, one: bool) -> NoteSpec {
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

    fn name_of(&self, sc: &crate::ws::ScreenInfo) -> String {
        self.w.pics.view.names.iter().find(|(d, _)| *d == sc.device).map_or_else(|| sc.name.clone(), |(_, n)| n.clone())
    }

    fn active_on(&self, device: &str) -> usize {
        self.w.pics.view.active.iter().find(|(d, _)| d == device).map_or(0, |(_, a)| *a)
    }

    fn tiles_w(&self, n: usize, add: bool) -> f32 {
        let tw = if n > 6 { 40.0 } else { 48.0 };
        n as f32 * tw + (n as f32 - 1.0) * 6.0 + if add { 6.0 + tw } else { 0.0 }
    }

    /// One present screen: its name and size, and its workspaces as tiles (AC-SET-3).
    fn screen_row(&mut self, y: f32, sc: &crate::ws::ScreenInfo, gone: &[usize]) -> f32 {
        let n = self.s.workspaces;
        let name = self.name_of(sc);
        let a = self.active_on(&sc.device);
        let (rw, rh) = (sc.rect.right - sc.rect.left, sc.rect.bottom - sc.rect.top);
        let cap = vec![t(format!("{rw} × {rh} · {}% · on workspace {}", sc.scale, a + 1))];
        let counts: Vec<usize> = (0..n).map(|ws| self.w.pics.view.windows.iter().filter(|(d, k, _)| *d == sc.device && *k == ws).count()).collect();
        let add = n < 9;
        let tw = self.tiles_w(n, add);
        let mut h = self.row(y, &name, &cap, (tw, 34.0), |c, x, y| c.tiles(x, y, &counts, Some(a), gone, add), None);
        h = h.max(60.0);
        h
    }

    /// The tiles: a workspace each in its color, with up to three window marks; the one in
    /// view ringed; × on the last one while you point at it, + after them.
    #[allow(clippy::too_many_arguments)]
    fn tiles(&mut self, x: f32, y: f32, counts: &[usize], now: Option<usize>, gone: &[usize], add: bool) {
        let t = self.c.t;
        let b = t.b;
        let n = counts.len();
        let tw = if n > 6 { 40.0 } else { 48.0 };
        let away = now.is_none();
        let mark = ui::LIGHT.b.card;
        for (i, &k) in counts.iter().enumerate() {
            let tx = x + i as f32 * (tw + 6.0);
            let bx = (tx, y, tw, 34.0);
            let cur = now == Some(i);
            let (fill, ink) = match () {
                _ if away => (b.track, b.tx2),
                _ if t.hc && cur => (b.hi, b.on_acc),
                _ if t.hc => (b.card, b.tx),
                _ => (b.ws[i], b.on_ws),
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
                let col = if away { b.tx3 } else if t.hc { if cur { b.on_acc } else { b.tx } } else { mark };
                self.c.fill_rr((mx, y + 4.0, 8.0, 10.0), 2.0, ui::alpha(col, if away || t.hc { 1.0 } else { 0.9 }));
            }
            if gone.contains(&i) && !away {
                self.c.stroke_dash((tx + 1.0, y + 1.0, tw - 2.0, 32.0), 3.0, 2.0, op(b.tx), DashStyleDash);
            }
            if !away && i + 1 == n && n > 2 && !self.ro {
                let show = self.hovered(Ctl::LastTile) || self.hovered(Ctl::TileRemove) || self.focused(Ctl::Tiles);
                if show {
                    let (cx, cy) = (tx + tw + 2.0, y - 2.0);
                    let hov = self.hovered(Ctl::TileRemove);
                    let (bg, fg) = if hov { (b.tx, b.card) } else { (b.card, b.tx) };
                    self.c.circle(cx, cy, 9.0, Some(op(bg)), Some((op(b.tx3), 1.0)));
                    self.c.lines(&[(cx - 3.0, cy - 3.0), (cx + 3.0, cy + 3.0)], 1.2, op(fg));
                    self.c.lines(&[(cx + 3.0, cy - 3.0), (cx - 3.0, cy + 3.0)], 1.2, op(fg));
                    self.hit(Ctl::TileRemove, (cx - 10.0, cy - 10.0, 20.0, 20.0), 1);
                }
                self.hit(Ctl::LastTile, bx, 1);
            }
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
    fn away_row(&mut self, y: f32, i: usize, a: &crate::ws::Waiting) -> f32 {
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
    fn screens_art(&mut self, y: f32) -> f32 {
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
            let (fill, ink) = if self.c.t.hc { (b.card, b.tx) } else { (b.ws[a.min(8)], b.on_ws) };
            self.c.fill_rr((x + 2.0, yy + 2.0, w - 4.0, h - 4.0), 4.0, op(fill));
            let name = self.name_of(s);
            if h >= 45.0 && self.c.width(&name, F::Small) <= w - 16.0 {
                self.c.text(x + 9.0, yy + 7.0, 16.0, &name, F::Small, ink);
            }
            self.c.text(x + 9.0, yy + h - 32.0, 28.0, &(a + 1).to_string(), F::Title, ink);
        }
        12.0 + ht + 4.0
    }

    // ---- Shortcuts ----

    fn shortcuts(&mut self, y: f32) -> f32 {
        let mut h = self.card(y, |c, y| c.scenes(y)) + 12.0;
        h += self.card(y + h, |c, y| {
            let b = c.c.t.b;
            let mut h = c.header(y, "Move around", None, "");
            let (kx, sx) = (X0 + c.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS, X0 + c.cw - 16.0);
            c.c.text(X0 + 16.0, y + h, 32.0, "Action", F::SmallBold, b.tx2);
            c.c.text(kx, y + h, 32.0, "Key · click to change", F::SmallBold, b.tx2);
            let tail = "Windows' key goes to";
            c.c.text(sx - c.c.width(tail, F::SmallBold), y + h, 32.0, tail, F::SmallBold, b.tx2);
            h += 32.0;
            for sh in MOVE {
                c.sep(y + h);
                h += c.key_row(y + h, sh);
            }
            h
        }) + 12.0;
        h += self.card(y + h, |c, y| {
            let mut h = c.header(y, "Move windows", None, "");
            for (i, sh) in SEND.iter().enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                h += c.key_row(y + h, sh);
            }
            h
        }) + 12.0;
        h += self.card(y + h, |c, y| {
            let h = c.header(y, "Copilot key", None, "");
            h + c.copilot(y + h)
        });
        h
    }

    /// The Copilot key's row, in the shortcut table's columns.
    fn copilot(&mut self, y: f32) -> f32 {
        let key = "copilot_as_right_ctrl";
        let b = self.c.t.b;
        let v = value(self.s, key) != 0;
        let mut cap = vec![t(if v { "So Win Ctrl keys work with it. Taken from Windows, where it opens Copilot, or what you set." } else { "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl." })];
        let note = self.bad(key);
        cap.extend(self.default_pieces(key, note.is_some()));
        let lw = self.cw - 32.0 - KT_KEYS - KT_SIDE - 32.0;
        let help: String = cap.iter().filter_map(|p| if let P::T(s) = p { Some(s.as_str()) } else { None }).collect();
        self.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, "Copilot key as Right Ctrl", &help, self.ro, (None, Some(if v { "des-k" } else { "Windows" }.into())));
        let words = ["des-k", "Windows"];
        let sw = self.seg_w(&words);
        let lh = self.label_h(lw, "Copilot key as Right Ctrl", &cap);
        let top = lh.max(28.0).max(ROW - 16.0);
        let kx = X0 + self.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS;
        self.c.text(kx, y + 8.0, top, "The Copilot key", F::Small, b.tx2);
        self.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &words, Some(if v { 0 } else { 1 }), false, |i| Ctl::Choice(key, i), Ctl::Pick(key));
        self.label(X0 + 16.0, y + 8.0 + (top - lh) / 2.0, lw, "Copilot key as Right Ctrl", &cap);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.in_note(X0 + 16.0, y + h + 6.0, self.cw - 32.0, &n);
        }
        h + 8.0
    }

    /// The three kinds of shortcut, drawn with the workspace colors, with your keys under them.
    fn scenes(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let s = self.s;
        let colw = (self.cw - 32.0 - 32.0) / 3.0;
        let list = [
            ("Switch", "You go to the next workspace.", 0u8, "shortcuts.previous", "shortcuts.next", "shortcuts.jump"),
            ("Send window", "The window goes, and you go with it.", 1, "shortcuts.send_previous", "shortcuts.send_next", "shortcuts.send_to"),
            ("Send window and stay", "The window goes; you stay.", 2, "shortcuts.send_previous_stay", "shortcuts.send_next_stay", "shortcuts.send_to_stay"),
        ];
        let mut most: f32 = 0.0;
        for (k, (title, cap, kind, prev, next, digits)) in list.into_iter().enumerate() {
            let x = X0 + 16.0 + k as f32 * (colw + 16.0);
            let mut h = 14.0;
            self.c.text(x, y + h, 20.0, title, F::Bold, b.tx);
            h += 20.0;
            h += self.c.para(x, y + h, colw, 16.0, &[(cap, F::Small, b.tx2)]) + 8.0;
            self.scene(x, y + h, kind);
            h += 58.0 + 8.0;
            let (p, n) = (keys_of(s, prev), keys_of(s, next));
            // One ← → pair when both hold the same modifiers.
            let pair = match (p.first(), n.first()) {
                (Some(a), Some(z)) if a.last().map(String::as_str) == Some("Left") && z.last().map(String::as_str) == Some("Right") && a[..a.len() - 1] == z[..z.len() - 1] => {
                    let mut caps = a[..a.len() - 1].to_vec();
                    caps.extend(["Left".to_string(), "Right".to_string()]);
                    Some(vec![caps])
                }
                _ => None,
            };
            let rows: Vec<Vec<String>> = pair.unwrap_or_else(|| p.first().into_iter().chain(n.first()).cloned().collect());
            if rows.is_empty() {
                self.c.text(x, y + h, 22.0, "No key", F::Small, b.tx2);
                h += 26.0;
            }
            for caps in rows {
                self.caps(x, y + h, &caps, false);
                h += 26.0;
            }
            self.c.text(x, y + h, 16.0, "or", F::Small, b.tx2);
            h += 20.0;
            match keys_of(s, digits).first() {
                Some(caps) => {
                    self.caps(x, y + h, caps, false);
                }
                None => self.c.text(x, y + h, 22.0, "No key", F::Small, b.tx2),
            }
            h += 22.0;
            most = most.max(h);
        }
        most + 14.0
    }

    /// A scene: four workspaces, where you are after the press (ring and dot) and where the window is.
    fn scene(&mut self, x: f32, y: f32, kind: u8) {
        let t = self.c.t;
        let b = t.b;
        let (tw, th, gap, x0, y0) = (38.0, 26.0, 8.0, x + 2.0, y + 14.0);
        let tx = |i: usize| x0 + i as f32 * (tw + gap);
        let you = if kind == 2 { 1 } else { 2 };
        for i in 0..4 {
            if t.hc {
                self.c.boxed((tx(i), y0, tw, th), 4.0, if i == you { b.hi } else { b.card }, Some(b.tx));
            } else {
                self.c.fill_rr((tx(i), y0, tw, th), 4.0, op(b.ws[i]));
            }
        }
        self.c.stroke_rr((tx(you) - 2.0, y0 - 2.0, tw + 4.0, th + 4.0), 5.0, 2.0, op(b.tx));
        self.c.circle(tx(you) + tw / 2.0, y + 5.0, 3.5, Some(op(b.tx)), None);
        if kind > 0 {
            self.c.stroke_dash((tx(1) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, 1.0, op(b.on_ws), DashStyleDash);
            self.c.boxed((tx(2) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, ui::LIGHT.b.card, Some(b.on_ws));
        }
        let (a, z, base) = (tx(1) + tw / 2.0, tx(2) + tw / 2.0, y0 + th + 6.0);
        let pts: Vec<(f32, f32)> = (0..=10).map(|j| {
            let u = j as f32 / 10.0;
            (a + (z - a) * u, base + 10.0 * 2.0 * u * (1.0 - u))
        }).collect();
        self.c.lines(&pts, 1.5, op(b.tx2));
        self.c.lines(&[(z - 5.0, base + 1.0), (z, base), (z - 2.0, base + 5.0)], 1.5, op(b.tx2));
    }

    /// Key caps for one key, 22 dip, from `x`. Returns their width.
    fn caps(&self, x: f32, y: f32, caps: &[String], held: bool) -> f32 {
        let mut cx = x;
        for (i, k) in caps.iter().enumerate() {
            if i > 0 {
                cx += 4.0;
            }
            cx += self.c.key_cap(cx, y, 22.0, k, held);
        }
        cx - x
    }

    fn caps_w(&self, caps: &[String]) -> f32 {
        caps.iter().enumerate().map(|(i, k)| {
            let label = match k.as_str() {
                "Left" | "Right" | "Up" | "Down" => ("\u{E72B}", F::IconTiny),
                k => (k, F::Small),
            };
            let tw = self.c.width(label.0, label.1);
            (tw + 2.0 * (22.0 / 2.0 - 5.0) + 2.0).max(22.0) + if i > 0 { 4.0 } else { 0.0 }
        }).sum()
    }

    /// The key field the recorder fills: a ghost cap, or the key it found.
    fn rec_field(&mut self, x: f32, y: f32, held: &[String], ghost: &str) -> f32 {
        let b = self.c.t.b;
        let w = if held.is_empty() { self.c.width(ghost, F::Small) + 12.0 } else { self.caps_w(held) } + 10.0;
        self.field((x, y, w, 32.0), b.card, Some(b.acc));
        if held.is_empty() {
            let gw = self.c.width(ghost, F::Small) + 12.0;
            self.c.stroke_dash((x + 5.0, y + 5.0, gw, 22.0), 4.0, 1.0, op(b.tx2), DashStyleDash);
            self.c.text(x + 11.0, y + 5.0, 22.0, ghost, F::Small, b.tx2);
        } else {
            self.caps(x + 5.0, y + 5.0, held, true);
        }
        w
    }

    /// One shortcut: its keys as fields with × and +, and des-k or Windows (AC-SET-4). One stop
    /// for the row; recording, a clash or a refusal shows its note under it.
    fn key_row(&mut self, y: f32, sh: &'static Short) -> f32 {
        let b = self.c.t.b;
        let s = self.s;
        let key = sh.key;
        let digits = is_digits(key);
        let all = keys_of(s, key);
        let order = drawn_order(s, sh);
        let has = has_win(s, sh);
        let took = if has { None } else { took(s, sh) };
        let rec = self.w.rec;
        let prompt = self.w.prompt.as_ref().filter(|p| p.key == key);
        let recording = matches!(rec, Some(Rec::List(k, _)) | Some(Rec::Digits(k)) if k == key);
        let active = recording || prompt.is_some();
        // The caption.
        let mut cap: Vec<P> = vec![];
        if key == "shortcuts.hints" {
            cap.push(t(if all.is_empty() { "No key by default. Shows every key and gesture, on the screen under your mouse." } else { "Shows every key and gesture, on the screen under your mouse." }));
        }
        if let Some((wk, does)) = sh.win {
            let wk = spelled(&win_caps(wk));
            let line = if has {
                if all.len() > 1 { format!("{wk} is taken from Windows, where it {does}.") } else { format!("Taken from Windows, where it {does}.") }
            } else if let Some(other) = took {
                format!("{}{wk} goes to “{other}” now. Remove it there to get it back.", if all.is_empty() { "No key. " } else { "" })
            } else {
                format!("{}{wk} is Windows' again: it {does}.", if all.is_empty() { "No key. " } else { "Your key. " })
            };
            cap.push(t(line));
        }
        if digits && let Some(m) = s.set(name_of(key)) {
            for (d, list) in s.skips(name_of(key)) {
                cap.push(t(format!(" Skips {d}: “{}” uses {} {d}.", entry(list).1, spelled(&crate::hints::mods_caps(m)))));
            }
        }
        let bad = self.bad(key);
        if !same_keys(s, &DEFAULTS, key) || bad.is_some() {
            cap.push(t(format!(" Default: {}.", said(&DEFAULTS, key))));
            cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
        }
        let help: String = cap.iter().filter_map(|p| if let P::T(s) = p { Some(s.as_str()) } else { None }).collect::<String>().trim().to_string();
        let keys_said = if all.is_empty() { "no key".to_string() } else { all.iter().map(|k| spelled(k)).collect::<Vec<_>>().join(" or ") };
        self.say(Ctl::KeyRow(key), UIA_GroupControlTypeId, sh.title, &help, self.ro, (None, Some(keys_said)));
        // The note under it.
        let note = if let Some(p) = prompt {
            Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: p.title.clone(), lines: vec![t(p.sub.clone())], buttons: vec![(Ctl::Move(key), "Move it here"), (Ctl::Cancel(key), "Cancel")], group: 1 })
        } else if recording {
            Some(match self.w.refused.clone() {
                Some((title, sub)) => NoteSpec { tone: Tone::Warn, icon: I_WARN, title, lines: vec![t(sub)], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
                None if digits => NoteSpec { tone: Tone::Rec, icon: I_KEY, title: "Hold the new keys and press a digit".into(), lines: vec![t("Hold Win, Ctrl or Alt (Shift can join them), then press any digit 1 to 9; all nine get the same keys. Esc alone or a click elsewhere cancels.")], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
                None => NoteSpec { tone: Tone::Rec, icon: I_KEY, title: "Press the new key".into(), lines: vec![t("Hold Win, Ctrl, Alt or Shift with a key. If Windows uses it, des-k takes it over. Esc alone or a click elsewhere cancels.")], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
            })
        } else {
            bad
        };
        let lw = self.cw - 32.0 - KT_KEYS - KT_SIDE - 32.0;
        let lh = self.label_h(lw, sh.title, &cap);
        let kx = X0 + self.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS;
        // The key column's height: fields wrap after the column's width.
        let wells = |c: &mut Self, y: f32| -> f32 {
            let (mut cx, mut cy) = (kx, y);
            let room = |cx: &mut f32, cy: &mut f32, w: f32| {
                if *cx > kx && *cx + w > kx + KT_KEYS {
                    *cx = kx;
                    *cy += 38.0;
                }
            };
            let replacing = match rec {
                Some(Rec::List(k, Some(i))) if k == key => Some(i),
                Some(Rec::Digits(k)) if k == key && !all.is_empty() => Some(0),
                _ => None,
            };
            let held: Vec<String> = prompt.map(|p| p.held.clone()).unwrap_or_default();
            for &i in &order {
                if replacing == Some(i) {
                    let fw = c.measured(|c| c.rec_field(0.0, 0.0, &held, if digits { "1–9" } else { "key" }));
                    room(&mut cx, &mut cy, fw);
                    cx += c.rec_field(cx, cy, &held, if digits { "1–9" } else { "key" }) + 6.0;
                    continue;
                }
                let caps = &all[i];
                let fw = c.caps_w(caps) + 10.0 + 24.0;
                room(&mut cx, &mut cy, fw);
                let (body, cross) = (Ctl::Key(key, i), Ctl::KeyX(key, i));
                let hov = (c.hovered(body) || c.hovered(cross)) && !c.ro;
                c.field((cx, cy, fw, 32.0), if hov { b.card } else { b.track }, None);
                c.caps(cx + 5.0, cy + 5.0, caps, false);
                let (xx, xy) = (cx + fw - 25.0, cy + 6.0);
                if c.hovered(cross) && !c.ro {
                    c.c.boxed((xx, xy, 20.0, 20.0), 4.0, b.track, Some(b.tx3));
                }
                let iw = c.c.width(I_X, F::IconTiny);
                c.icon(xx + (20.0 - iw) / 2.0, xy, 20.0, I_X, F::IconTiny, if c.ro { b.tx3 } else if c.hovered(cross) { b.tx } else { b.tx2 });
                c.hit(cross, (xx - 2.0, cy, 24.0, 32.0), 1);
                c.hit(body, (cx, cy, fw - 26.0, 32.0), 1);
                cx += fw + 6.0;
            }
            let adding = match rec {
                Some(Rec::List(k, None)) if k == key => true,
                Some(Rec::Digits(k)) if k == key && all.is_empty() => true,
                _ => prompt.is_some(),
            };
            if adding {
                let ghost = if digits { "1–9" } else { "key" };
                let fw = c.measured(|c| c.rec_field(0.0, 0.0, &held, ghost));
                room(&mut cx, &mut cy, fw);
                c.rec_field(cx, cy, &held, ghost);
            } else if all.is_empty() {
                let fw = c.btn_w("Set key", Some(I_ADD));
                room(&mut cx, &mut cy, fw);
                c.button(Ctl::AddKey(key), cx, cy, "Set key", Some(I_ADD), false, 1);
            } else if !digits && !active {
                room(&mut cx, &mut cy, 32.0);
                let off = c.ro;
                let fill = if c.hovered(Ctl::AddKey(key)) && !off { mix(b.card, b.tx, 0.05) } else { b.card };
                c.field((cx, cy, 32.0, 32.0), fill, None);
                let iw = c.c.width(I_ADD, F::IconSmall);
                c.icon(cx + (32.0 - iw) / 2.0, cy, 32.0, I_ADD, F::IconSmall, if off { b.tx3 } else { b.tx });
                c.hit(Ctl::AddKey(key), (cx, cy, 32.0, 32.0), 1);
            }
            cy + 32.0 - y
        };
        let kh = self.measured(|c| wells(c, 0.0));
        let top = lh.max(kh).max(ROW - 16.0);
        let body_h = 8.0 + top + note.as_ref().map_or(0.0, |n| 6.0 + self.measured(|c| c.in_note(0.0, 0.0, c.cw - 32.0, n))) + 8.0;
        if active {
            let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc, 0.06) };
            self.c.fill_rr((X0 + 1.0, y + 0.5, self.cw - 2.0, body_h - 1.0), 0.0, op(tint));
        }
        wells(self, y + 8.0 + (top - kh) / 2.0);
        // des-k or Windows: greyed while another action holds the key.
        if sh.win.is_some() {
            let words = ["des-k", "Windows"];
            let sw = self.seg_w(&words);
            let sel = if took.is_some() { None } else { Some(if has { 0 } else { 1 }) };
            self.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &words, sel, took.is_some(), |i| Ctl::WinSide(key, i == 0), Ctl::KeyRow(key));
        }
        self.label(X0 + 16.0, y + 8.0 + (top - lh) / 2.0, lw, sh.title, &cap);
        if self.focused(Ctl::KeyRow(key)) {
            self.c.stroke_rr((X0 + 2.0, y + 2.0, self.cw - 4.0, 8.0 + top + 8.0 - 4.0), 6.0, 2.0, op(b.tx));
        }
        let mut h = 8.0 + top;
        if let Some(n) = &note {
            h += 6.0 + self.in_note(X0 + 16.0, y + h + 6.0, self.cw - 32.0, n);
        }
        h + 8.0
    }

    // ---- Indicator and Restore ----

    fn indicator(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let mut h = self.setting("indicator.enabled", y);
        self.sep(y + h);
        h += self.setting("indicator.style", y + h);
        // The preview well: the chosen style for this screen, through the indicator's own drawing (AC-SET-7).
        let n = self.s.workspaces;
        let here = self.w.pics.here.clone().unwrap_or_default();
        let a = self.active_on(&here);
        let named = self.w.pics.screens.len() > 1;
        let label = self.w.pics.view.names.iter().find(|(d, _)| *d == here).map(|(_, l)| l.clone());
        let card = crate::indicator::Card::new(a, n, if named { label } else { None });
        let style = self.s.indicator_style;
        let (pw, ph) = crate::indicator::draw_preview(&self.c.measuring(), 0.0, 0.0, style, &card);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let stage = ph.max(76.0);
        let well_h = 16.0 + stage + 12.0 + 32.0 + 12.0;
        let wy = y + h + 4.0;
        self.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        crate::indicator::draw_preview(self.c, wx + (ww - pw) / 2.0, wy + 16.0 + (stage - ph) / 2.0, style, &card);
        let on = self.s.indicator_enabled;
        let sw = self.btn_w("Show", None);
        let py = wy + 16.0 + stage + 12.0;
        let line = format!("On this screen, which is on workspace {} of {n}.", a + 1);
        self.c.text(wx + 12.0, py, 32.0, &line, F::Small, b.tx2);
        self.button(Ctl::Show, wx + ww - 12.0 - sw, py, "Show", None, !on, 1);
        h += 4.0 + well_h + 14.0;
        h += self.fade_line(y + h);
        self.sep(y + h);
        h += self.setting("indicator.hold_ms", y + h);
        self.sep(y + h);
        h + self.setting("indicator.fade_ms", y + h)
    }

    /// The indicator's time line: how long it shows, then its fade (none with Skip animations).
    fn fade_line(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let (hold, fade) = (self.s.hold_ms as f32, if self.s.fastest { 0.0 } else { self.s.fade_ms as f32 });
        let total = (hold + fade).max(1.0);
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let by = y + 2.0;
        self.c.boxed((x, by, w, 8.0), 4.0, b.track, Some(b.line));
        let hw = w * hold / total;
        self.c.fill_rr((x, by, hw, 8.0), 4.0, op(b.acc));
        if fade > 0.0 {
            if self.c.t.hc {
                self.c.stroke_rr((x + hw, by + 0.5, w - hw - 0.5, 7.0), 3.5, 1.0, op(b.acc));
            } else {
                self.c.gradient(x + hw, by, w - hw, 8.0, b.acc, mix(b.card, b.acc, 0.15));
            }
        }
        let ly = by + 8.0 + 6.0;
        self.c.text(x, ly, 16.0, "Shows", F::Small, b.tx2);
        let right = |c: &mut Self, runs: &[(&str, F, u32)]| {
            let rw: f32 = runs.iter().map(|(t, f, _)| c.c.width(t, *f)).sum();
            c.c.para(x + w - rw, ly, rw + 1.0, 16.0, runs);
        };
        if self.s.fastest {
            let ms = format!("{} ms", self.s.hold_ms);
            right(self, &[("gone at ", F::Small, b.tx2), (&ms, F::SmallBold, b.tx), (", no fade", F::Small, b.tx2)]);
        } else {
            let (from, gone) = (format!("{} ms", self.s.hold_ms), format!("{} ms", self.s.hold_ms + self.s.fade_ms));
            let fw = self.c.width("fades from ", F::Small) + self.c.width(&from, F::SmallBold);
            let fx = (x + hw - fw / 2.0).clamp(x + 60.0, x + w - fw - 120.0);
            self.c.para(fx, ly, fw + 1.0, 16.0, &[("fades from ", F::Small, b.tx2), (&from, F::SmallBold, b.tx)]);
            right(self, &[("gone at ", F::Small, b.tx2), (&gone, F::SmallBold, b.tx)]);
        }
        2.0 + 8.0 + 6.0 + 16.0 + 14.0
    }

    fn restore(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let mut h = self.setting("restore.enabled", y);
        let m = self.s.restore_minutes as f32;
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let ty = y + h + 4.0;
        let lead = format!("For {} min:", self.s.restore_minutes);
        self.c.para(x, ty, w, 16.0, &[(&lead, F::SmallBold, b.tx), (" reopened windows go back", F::Small, b.tx2)]);
        let tail = "After that, windows open where you are";
        self.c.text(x + w - self.c.width(tail, F::Small), ty, 16.0, tail, F::Small, b.tx2);
        let by = ty + 22.0;
        self.c.boxed((x, by, w, 8.0), 4.0, b.track, Some(b.line));
        let mw = w * (m / 12.0).min(1.0);
        self.c.fill_rr((x, by, mw, 8.0), 4.0, op(b.acc));
        let ly = by + 14.0;
        self.c.text(x, ly, 16.0, "des-k starts", F::Small, b.tx2);
        let at = format!("{} min", self.s.restore_minutes);
        let aw = self.c.width(&at, F::Small);
        self.c.text((x + mw - aw / 2.0).clamp(x + 90.0, x + w - aw), ly, 16.0, &at, F::Small, b.tx2);
        h += 4.0 + 22.0 + 8.0 + 6.0 + 16.0 + 14.0;
        self.sep(y + h);
        h + self.setting("restore.minutes", y + h)
    }

    // ---- Overview and Motion ----

    /// The overview, drawn: up to two screens' first four workspaces with their windows; names
    /// follow Window names in overview.
    fn overview_art(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let b = t.b;
        let (tw, th, gap) = (80.0, 46.0, 10.0);
        let screens: Vec<crate::ws::ScreenInfo> = self.w.pics.screens.iter().take(2).cloned().collect();
        let rows = screens.len().max(1);
        let (w2, h2) = (4.0 * (tw + gap) + 30.0, rows as f32 * (th + 22.0) + 14.0 - if rows == 1 { 0.0 } else { 2.0 });
        let x0 = X0 + (self.cw - w2) / 2.0;
        let y0 = y + 14.0;
        self.c.fill_rr((x0, y0, w2, h2), 6.0, op(b.bez));
        let labels = word(settings::LABELS, self.s.overview_labels);
        let white = ui::LIGHT.b.card;
        let (chip, chip_fg) = (ui::DARK.b.bg, ui::DARK.b.tx);
        let samples = ["Mail", "Code", "Notes"];
        for (r, sc) in screens.iter().enumerate() {
            let ry = y0 + 8.0 + r as f32 * (th + 22.0);
            let name = self.name_of(sc);
            self.c.text(x0 + 16.0, ry, 14.0, &name, F::Small, if t.hc { b.card } else { white });
            let a = self.active_on(&sc.device);
            for i in 0..4 {
                let (x, yy) = (x0 + 16.0 + i as f32 * (tw + gap), ry + 16.0);
                let fill = if t.hc { if i == a { b.hi } else { b.card } } else { b.ws[i] };
                self.c.fill_rr((x, yy, tw, th), 4.0, op(fill));
                if i == a {
                    self.c.stroke_rr((x, yy, tw, th), 4.0, 2.0, op(if t.hc { b.card } else { white }));
                }
                let k = self.w.pics.view.windows.iter().filter(|(d, ws, _)| *d == sc.device && *ws == i).count().min(3);
                for (j, sample) in samples.iter().enumerate().take(k) {
                    let (wx, wy) = (x + 6.0 + j as f32 * 20.0, yy + 6.0 + j as f32 * 4.0);
                    self.c.fill_rr((wx, wy, 34.0, 22.0), 2.0, ui::alpha(if t.hc { b.tx } else { white }, if t.hc { 1.0 } else { 0.92 }));
                    let show = labels == "always" || (labels == "hover" && r == 0 && i == a && j + 1 == k);
                    if show {
                        self.c.fill_rr((wx - 4.0, wy + 23.0, 42.0, 16.0), 3.0, op(chip));
                        let sw = self.c.width(sample, F::Small);
                        self.c.text(wx + 17.0 - sw / 2.0, wy + 23.0, 16.0, sample, F::Small, chip_fg);
                    }
                }
            }
        }
        14.0 + h2 + 6.0
    }

    /// Every des-k motion on one time axis, in the grey well, or "At once" (spec 0017 AC-11).
    fn motion(&mut self, y: f32) -> f32 {
        let th = self.c.t;
        let b = th.b;
        let s = self.s;
        let max = if (s.fade_ms.max(s.overview_fade_ms)) > 1000 { 2000.0 } else { 1000.0 };
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + 16.0;
        let bar = if th.hc { b.acc } else if th.dark { mix(b.acc, 0xFFFFFF, 0.28) } else { b.acc };
        let fade = s.overview_animation == settings::Animation::Fade;
        let lanes: [(&str, u32, Option<&str>, &str); 2] = [
            ("Indicator fades out", s.fade_ms, (!s.indicator_enabled).then_some("Indicator off"), ""),
            ("Overview fades in and out", s.overview_fade_ms, (!fade).then_some("No fade"), " each"),
        ];
        let body = |c: &mut Self, top: f32| -> f32 {
            let (nx, tx, vx) = (wx + 16.0, wx + 16.0 + 196.0 + 16.0, wx + ww - 16.0);
            let trw = vx - 100.0 - 16.0 - tx;
            let mut h = 14.0;
            for (name, ms, none, each) in lanes {
                let ly = top + h;
                c.c.text(nx, ly, 24.0, name, F::Body, b.tx);
                for q in 0..4 {
                    c.c.fill_rr((tx + q as f32 * trw / 4.0, ly, 1.0, 24.0), 0.0, op(b.grid));
                }
                c.c.fill_rr((tx + trw - 1.0, ly, 1.0, 24.0), 0.0, op(b.grid));
                if s.fastest {
                    c.c.fill_rr((tx, ly + 6.0, 3.0, 12.0), 1.5, op(bar));
                } else if none.is_none() && ms > 0 {
                    let bw = trw * (ms as f32 / max).min(1.0);
                    c.c.fill_rr((tx, ly + 8.0, bw, 8.0), 0.0, op(bar));
                    c.c.fill_rr((tx + (bw - 8.0).max(0.0), ly + 8.0, bw.min(8.0), 8.0), 4.0, op(bar));
                }
                let v: Vec<(String, F, u32)> = if s.fastest {
                    vec![("At once".into(), F::SmallBold, b.tx)]
                } else if let Some(word) = none {
                    vec![(word.into(), F::Small, b.tx2)]
                } else {
                    vec![(format!("{ms} ms"), F::SmallBold, b.tx), (each.into(), F::Small, b.tx2)]
                };
                let vw: f32 = v.iter().map(|(t, f, _)| c.c.width(t, *f)).sum();
                let runs: Vec<(&str, F, u32)> = v.iter().map(|(t, f, col)| (t.as_str(), *f, *col)).collect();
                c.c.para(vx - vw, ly + 4.0, vw + 1.0, 16.0, &runs);
                h += 24.0 + 6.0;
            }
            let ay = top + h;
            c.c.text(tx, ay, 16.0, "0", F::Small, b.tx2);
            let half = format!("{} ms", max as u32 / 2);
            let hw = c.c.width(&half, F::Small);
            c.c.text(tx + trw / 2.0 - hw / 2.0, ay, 16.0, &half, F::Small, b.tx2);
            let full = format!("{} ms", max as u32);
            c.c.text(tx + trw - c.c.width(&full, F::Small), ay, 16.0, &full, F::Small, b.tx2);
            h += 16.0 + 10.0;
            c.c.rule(nx, top + h, ww - 32.0, op(b.line));
            h += 8.0;
            let foot = [t("You set these times in "), P::Link("Indicator".into(), Ctl::Link(Link::Indicator)), t(" and "), P::Link("Overview".into(), Ctl::Link(Link::Overview)), t(".")];
            h += c.caption(nx, top + h, ww - 32.0, &foot);
            h + 10.0
        };
        let h = self.measured(|c| body(c, 0.0));
        self.c.boxed((wx, wy, ww, h), 6.0, b.track, Some(b.line));
        // One image for Narrator, read as a sentence built from the lanes drawn.
        let said = if s.fastest {
            "Every des-k motion happens at once".to_string()
        } else {
            let ind = if s.indicator_enabled { format!("{} ms", s.fade_ms) } else { "nothing, it is off".into() };
            let ov = if fade { format!("fades in and out over {} ms each", s.overview_fade_ms) } else { "has no fade".into() };
            format!("Indicator fades out over {ind}; the overview {ov}")
        };
        if !self.out.said.iter().any(|(c, _)| *c == Ctl::Chart) {
            let s = self.c.s;
            let rect = RECT { left: (wx * s) as i32, top: (wy * s) as i32, right: ((wx + ww) * s) as i32, bottom: ((wy + h) * s) as i32 };
            self.out.said.push((Ctl::Chart, Node { role: UIA_ImageControlTypeId, name: said, rect, ..Node::default() }));
        }
        body(self, wy);
        16.0 + h + 14.0
    }

    // ---- Touchpad ----

    fn touchpad(&mut self, y: f32) -> f32 {
        let st = self.w.pics.pad.unwrap_or(PadState { pad: true, enabled: self.s.touchpad_enabled, rows: [PadRow::Nothing; 4], off: true, saved: false });
        let back = self.w.pics.back;
        let (note, head, chips) = pad_state(&st, back);
        let mut h = 0.0;
        if let Some(n) = note {
            h += self.note(X0, y, self.cw, &n) + 12.0;
        }
        for (g, fingers) in [(0usize, 3u8), (1, 4)] {
            h += self.card(y + h, |c, y| {
                let cap = format!("{head} · click an action to change it");
                let mut h = c.header(y, &format!("{fingers} fingers"), Some(fingers), &cap);
                for (k, tap) in [(0usize, false), (1, true)] {
                    if k > 0 {
                        c.sep(y + h);
                    }
                    h += c.gesture_row(y + h, fingers, tap, chips.map(|ch| ch[g * 2 + k]));
                }
                h
            }) + 12.0;
        }
        let give = st.saved && back.is_none_or(|b| b.failed() || !b.cleared) && !matches!(head, "Kept for a touchpad");
        h += self.card(y + h, |c, y| {
            let mut h = c.setting("touchpad.enabled", y);
            c.sep(y + h);
            h += c.setting("touchpad.swipe_mm", y + h);
            c.sep(y + h);
            h += c.setting("touchpad.twice_ms", y + h);
            if give {
                c.sep(y + h);
                let cap = [t("Windows uses its own 3- and 4-finger gestures again from your next sign-in. Until then, none work. Quit does the same.")];
                let bw = c.btn_w("Give back", None);
                h += c.row(y + h, "Give back to Windows", &cap, (bw, 32.0), |c, x, y| { c.button(Ctl::Pad(Pad::GiveBack), x, y, "Give back", None, false, 1); }, None);
            }
            h
        });
        h
    }

    /// One gesture row: Swipe or Tap, Windows' state for it, and its pickers.
    fn gesture_row(&mut self, y: f32, fingers: u8, tap: bool, chip: Option<Chip>) -> f32 {
        let b = self.c.t.b;
        let motions: &[Motion] = if tap { &[Motion::Tap] } else { &[Motion::Left, Motion::Right, Motion::Up, Motion::Down] };
        let (x, px) = (X0 + 16.0, X0 + 16.0 + 116.0 + 16.0);
        let pw_all = X0 + self.cw - 16.0 - px;
        let cols = if tap { 1 } else { 2 };
        let pw = (pw_all - 8.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut ph: f32 = 0.0;
        for (i, m) in motions.iter().enumerate() {
            let Some(row) = settings::GESTURE_ROWS.iter().position(|r| *r == (fingers, *m)) else { continue };
            let (cx, cy) = (px + (i % cols) as f32 * (pw + 8.0), y + 8.0 + (i / cols) as f32 * 48.0);
            let h = self.picker(row, cx, cy, pw);
            ph = ph.max(cy - y - 8.0 + h);
        }
        self.c.text(x, y + 8.0 + 6.0, 20.0, if tap { "Tap" } else { "Swipe" }, F::Bold, b.tx);
        if let Some(ch) = chip {
            let (icon, text, col) = match ch {
                Chip::Ok => (Some(I_OK), "Off in Windows", b.good),
                Chip::On => (None, "Windows uses it", b.tx2),
                Chip::Err => (Some(I_WARN), "Can't read it", b.warn),
                Chip::Off => (None, "Off in Windows", b.tx2),
                Chip::Back => (None, "Back at next sign-in", b.tx2),
                Chip::Not => (Some(I_WARN), "Not written back", b.warn),
            };
            let mut cx = x;
            if let Some(i) = icon {
                self.icon(cx, y + 36.0, 16.0, i, F::IconSmall, col);
                cx += 16.0;
            }
            self.c.text(cx, y + 36.0, 16.0, text, F::Small, b.tx2);
        }
        8.0 + ph.max(52.0) + 8.0
    }

    /// A gesture picker: its direction mark, what it does, its twice action and default, ▾.
    fn picker(&mut self, row: usize, x: f32, y: f32, w: f32) -> f32 {
        let t = self.c.t;
        let b = t.b;
        let (fingers, motion) = settings::GESTURE_ROWS[row];
        let s = self.s;
        let once = s.rule(fingers, motion, 1);
        let twice = s.rule(fingers, motion, 2);
        let def = DEFAULTS.rule(fingers, motion, 1);
        let ctl = Ctl::Picker(row);
        let open = self.w.menu.is_some_and(|m| m.row == row);
        let waiting = self.w.rec == Some(Rec::Gesture(row, 1)) || self.w.rec == Some(Rec::Gesture(row, 2));
        let text = if waiting { "Press the key combo".to_string() } else { once_label(once.unwrap_or(Action::Nothing)) };
        let mut sub = vec![];
        if twice.is_some() {
            sub.push(format!("Twice: {}", twice_label(twice)));
        }
        if once != def && !(once.is_none() && def == Some(Action::Nothing)) {
            sub.push(format!("Default: {}", once_label(def.unwrap_or(Action::Nothing))));
        }
        let sub = sub.join(" · ");
        let h = if sub.is_empty() { 32.0 } else { 40.0 };
        if open || waiting {
            self.field((x, y, w, h), b.card, Some(b.acc));
        } else {
            let fill = if self.hovered(ctl) && !self.ro { mix(b.track, b.tx, 0.05) } else { b.track };
            self.field((x, y, w, h), fill, None);
        }
        let glyph = match motion {
            Motion::Left => "\u{E72B}",
            Motion::Right => "\u{E72A}",
            Motion::Up => "\u{E74A}",
            Motion::Down => "\u{E74B}",
            Motion::Tap => "\u{E815}",
        };
        let my = y + (h - 22.0) / 2.0;
        if t.hc {
            self.c.stroke_rr((x + 6.5, my + 0.5, 21.0, 21.0), 4.0, 1.0, op(b.acc));
        } else {
            self.c.fill_rr((x + 6.0, my, 22.0, 22.0), 4.0, op(b.held));
        }
        let mark = if t.dark && !t.hc { b.link } else { b.acc };
        let gw = self.c.width(glyph, F::IconTiny);
        self.icon(x + 6.0 + (22.0 - gw) / 2.0, my, 22.0, glyph, F::IconTiny, mark);
        let lx = x + 36.0;
        let lw = w - 36.0 - 26.0;
        let ink = if self.ro { b.tx3 } else { b.tx };
        if sub.is_empty() {
            let shown = self.c.fit(&text, F::Body, lw);
            self.c.text(lx, y, 32.0, &shown, F::Body, ink);
        } else {
            let shown = self.c.fit(&text, F::Body, lw);
            self.c.text(lx, y + 2.0, 20.0, &shown, F::Body, ink);
            let s2 = self.c.fit(&sub, F::Small, lw);
            self.c.text(lx, y + 21.0, 16.0, &s2, F::Small, b.tx2);
        }
        let cw = self.c.width(I_CHEV, F::IconTiny);
        self.icon(x + w - 10.0 - cw, y, h, I_CHEV, F::IconTiny, b.tx2);
        self.ring((x, y, w, h), 6.0, ctl);
        self.hit(ctl, (x, y, w, h), 1);
        let dir = match motion {
            Motion::Left => "swipe left",
            Motion::Right => "swipe right",
            Motion::Up => "swipe up",
            Motion::Down => "swipe down",
            Motion::Tap => "tap",
        };
        let name = format!("{fingers} fingers, {dir}");
        self.say(ctl, UIA_ComboBoxControlTypeId, &name, &sub, self.ro, (None, Some(text)));
        h
    }

    // ---- The side list, banners and menu ----

    fn side(&mut self) {
        let t = self.c.t;
        let b = t.b;
        let vh = self.w.vh;
        let problems = settings::problems();
        let count = |sec: Option<Section>| problems.iter().filter(|p| home(&p.key) == sec).count();
        let mut items: Vec<(Option<Section>, &str, &str)> = vec![];
        if count(None) > 0 {
            items.push((None, "Notes", I_NOTES));
        }
        items.extend(SECTIONS.iter().map(|s| (Some(*s), s.name(), s.icon())));
        let current = self.current();
        let mut y = 8.0;
        for (sec, name, icon) in items {
            let ctl = Ctl::Side(sec);
            let bx = (12.0, y, NAV - 24.0, 36.0);
            let here = current == Some(sec);
            let (ink, ic) = if here && t.hc { (b.on_acc, b.on_acc) } else if here { (b.tx, b.acc) } else { (b.tx, b.tx2) };
            if here {
                self.c.fill_rr(bx, 6.0, op(if t.hc { b.hi } else { mix(b.bg, b.acc, 0.10) }));
                self.c.fill_rr((12.0, y + 10.0, 3.0, 16.0), 1.5, op(if t.hc { b.on_acc } else { b.acc }));
            } else if self.hovered(ctl) {
                self.c.fill_rr(bx, 6.0, op(mix(b.bg, b.tx, 0.05)));
            }
            self.icon(24.0, y, 36.0, icon, F::Icon, ic);
            self.c.text(52.0, y, 36.0, name, if here { F::Bold } else { F::Body }, ink);
            let n = count(sec);
            if n > 0 {
                let label = n.to_string();
                let cw = (self.c.width(&label, F::SmallBold) + 12.0).max(20.0);
                let (fill, fg, edge) = match () {
                    _ if t.hc && here => (b.on_acc, b.hi, b.on_acc),
                    _ if t.hc => (b.card, b.tx, b.tx),
                    _ => (b.card, b.warn, b.warn),
                };
                self.c.boxed((NAV - 24.0 - cw, y + 8.0, cw, 20.0), 10.0, fill, Some(edge));
                self.c.text(NAV - 24.0 - cw + (cw - self.c.width(&label, F::SmallBold)) / 2.0, y + 8.0, 20.0, &label, F::SmallBold, fg);
            }
            self.ring(bx, 6.0, ctl);
            self.hit(ctl, bx, 0);
            let spoken = if n > 0 { format!("{name}, {n} {}", if n == 1 { "problem" } else { "problems" }) } else { name.to_string() };
            self.say(ctl, UIA_ListItemControlTypeId, &spoken, "", false, (None, None));
            y += 38.0;
        }
        // Welcome and the file, at the foot.
        let foot = [(Ctl::Welcome, "Welcome", "\u{EA80}"), (Ctl::Open, "Open settings.toml", "\u{E8A5}")];
        let mut fy = vh - 16.0 - 36.0 * 2.0 - 2.0;
        self.c.rule(24.0, fy - 7.0, NAV - 48.0, op(b.line));
        for (ctl, name, icon) in foot {
            let bx = (12.0, fy, NAV - 24.0, 36.0);
            if self.hovered(ctl) {
                self.c.fill_rr(bx, 6.0, op(mix(b.bg, b.tx, 0.05)));
            }
            self.icon(24.0, fy, 36.0, icon, F::Icon, b.tx2);
            self.c.text(52.0, fy, 36.0, name, F::Body, b.tx);
            let ow = self.c.width(I_OUT, F::IconSmall);
            self.icon(NAV - 24.0 - ow, fy, 36.0, I_OUT, F::IconSmall, b.tx2);
            self.ring(bx, 6.0, ctl);
            self.hit(ctl, bx, 0);
            self.say(ctl, UIA_HyperlinkControlTypeId, name, "", false, (None, None));
            fy += 38.0;
        }
    }

    /// The side list item the view is in: the last section whose top has scrolled past.
    fn current(&self) -> Option<Option<Section>> {
        current(self.w.scroll, self.out.height, self.w.vh - self.out.fixed, &self.out.tops)
    }

    /// The notes above the column, fixed while it scrolls: can't be read, couldn't save,
    /// changed by hand. Returns their height with their margins (0 with none).
    fn banners(&mut self) -> f32 {
        let mut list: Vec<NoteSpec> = vec![];
        if let Some(why) = settings::parse_error() {
            list.push(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "settings.toml can't be read, so nothing here can change".into(),
                lines: vec![t("des-k uses the settings it read before (at start, its defaults); they're shown here. Fix the file and this page works again. "), P::Mono(why)],
                buttons: vec![(Ctl::Open, "Open settings.toml")],
                group: 2,
            });
        }
        if let Some((what, why)) = &self.w.error {
            list.push(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: format!("Couldn't save “{what}”"),
                lines: vec![t("Your change isn't saved or in use. Close settings.toml in any program that has it open, then change this again. Windows said: "), P::Mono(why.clone())],
                buttons: vec![(Ctl::Dismiss, "Dismiss")],
                group: 2,
            });
        }
        if let Some((time, lines)) = &self.w.edited {
            list.push(NoteSpec {
                tone: Tone::Info,
                icon: I_INFO,
                title: format!("settings.toml changed at {time} and is in use"),
                lines: vec![t(if lines.is_empty() { "Nothing des-k shows here changed.".to_string() } else { lines.join(". ") + "." })],
                buttons: vec![(Ctl::DismissEdit, "Dismiss")],
                group: 2,
            });
        }
        if list.is_empty() {
            return 0.0;
        }
        let b = self.c.t.b;
        let total = 12.0 + self.measured(|c| list.iter().map(|n| c.note(X0, 0.0, c.cw, n) + 8.0).sum::<f32>()) - 8.0 + 12.0;
        self.c.fill_rr((NAV, 0.0, self.w.vw - NAV, total), 0.0, op(b.bg));
        let mut y = 12.0;
        for n in &list {
            y += self.note(X0, y, self.cw, n) + 8.0;
        }
        total
    }

    /// The open menu, over everything: its lines in two columns and a footer, the chosen one
    /// checked, the default marked, kept inside the window.
    fn menu(&mut self) {
        let Some(m) = self.w.menu else { return };
        let t = self.c.t;
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
        self.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.hit(Ctl::Item(usize::MAX), (mx, my, mw, mh), 3); // inside the menu, between lines
        self.c.text(mx + 14.0, my + 4.0, 24.0, &head, F::SmallBold, b.tx2);
        let mut ys = [my + 28.0; 2];
        if !small {
            self.c.text(mx + 14.0, my + 28.0, 24.0, "des-k", F::SmallBold, b.tx2);
            self.c.text(mx + 4.0 + colw + 4.0 + 10.0, my + 28.0, 24.0, "Windows and media", F::SmallBold, b.tx2);
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
                        self.c.rule(mx + 4.0, y - 5.0, mw - 8.0, op(b.line));
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
            let hot = self.hovered(ctl) || (self.w.kb && focus_line == i);
            let (fill, ink, check) = match () {
                _ if l.on && t.hc => (Some(b.hi), b.on_acc, b.on_acc),
                _ if l.on => (Some(b.track), b.tx, b.acc),
                _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx, b.acc),
                _ => (None, b.tx, b.acc),
            };
            if let Some(f) = fill {
                self.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
            }
            if l.on {
                self.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, check);
            }
            let tx = x + 30.0;
            let more = matches!(l.does, Does::Family(_) | Does::Twice);
            let room = w - 30.0 - 6.0 - if l.default { 52.0 } else { 0.0 } - if more { 16.0 } else { 0.0 };
            let shown = self.c.fit(&l.text, F::Body, room);
            self.c.text(tx, y, 28.0, &shown, F::Body, ink);
            let mut rx = x + w - 10.0;
            if more {
                let cw = self.c.width(I_MORE, F::IconTiny);
                rx -= cw;
                self.icon(rx, y, 28.0, I_MORE, F::IconTiny, b.tx2);
                rx -= 6.0;
            }
            if l.default {
                let dw = self.c.width("Default", F::Small);
                self.c.text(rx - dw, y, 28.0, "Default", F::Small, b.tx2);
            }
            if self.w.kb && focus_line == i {
                self.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (x, y, w, 28.0), 3);
        }
    }
}

/// A note as it is drawn: its tone and icon, title, caption pieces and buttons.
struct NoteSpec {
    tone: Tone,
    icon: &'static str,
    title: String,
    lines: Vec<P>,
    buttons: Vec<(Ctl, &'static str)>,
    group: u8,
}

/// Windows' state per gesture row (setup::ROWS' order), as each row's small line shows it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Chip {
    Ok,
    On,
    Err,
    Off,
    Back,
    Not,
}

/// The touchpad section's note, its cards' header words and each row's state, in the first
/// run's words (spec 0011's states, AC-SET-6).
fn pad_state(st: &PadState, back: Option<GaveBack>) -> (Option<NoteSpec>, &'static str, Option<[Chip; 4]>) {
    let note = |tone, icon, title: &str, line: String, buttons: Vec<(Ctl, &'static str)>| Some(NoteSpec { tone, icon, title: title.into(), lines: vec![t(line)], buttons, group: 1 });
    const LOG: &str = "Details: tray icon › Open log.";
    let windows = (Ctl::Pad(Pad::Windows), "Open Windows Settings");
    if let Some(b) = back.filter(|b| b.failed()) {
        let none = b.wrote.iter().all(|w| *w == Wrote::NotOne);
        let (title, line) = if none { ("Give back failed", format!("None went back. des-k keeps your old values to try again. {LOG}")) } else { ("Give back partly failed", format!("Some went back, some didn't. des-k keeps your old values to try again. {LOG}")) };
        let chips = b.wrote.map(|w| if w == Wrote::All { Chip::Back } else { Chip::Not });
        return (note(Tone::Warn, I_WARN, title, line, vec![(Ctl::Pad(Pad::GiveBack), "Try again")]), "des-k runs these", Some(chips));
    }
    if back.is_some() && !st.off {
        let cleared = back.is_some_and(|b| b.cleared);
        let line = if cleared {
            "des-k gave Windows its settings back and runs no gestures now. Until you sign in again, none work. To use them now, set them in Windows Settings."
        } else {
            "Nothing is wrong now, but des-k couldn't delete its copy of your old values. Give back again to clean it up."
        };
        return (note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", line.into(), vec![windows]), "des-k would run", Some([Chip::Back; 4]));
    }
    if !st.enabled {
        return if st.off {
            (note(Tone::Warn, I_WARN, "No 3- or 4-finger gesture works now", "Windows' own gestures are off too. Turn reading on, or turn them back on in Windows Settings.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on"), windows]), "des-k would run", Some([Chip::Off; 4]))
        } else {
            (note(Tone::Info, I_INFO, "Touchpad reading is off", "Windows runs its own 3- and 4-finger gestures. des-k runs none of these.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on")]), "des-k would run", Some([Chip::On; 4]))
        };
    }
    if !st.pad {
        return (note(Tone::Info, I_INFO, "No precision touchpad found", "Your gestures are kept. They run once one is plugged in and set up.".into(), vec![]), "Kept for a touchpad", None);
    }
    if let Some(i) = st.rows.iter().position(|r| *r == PadRow::CantRead) {
        let row = setup::ROWS[i].0;
        let one = row.trim_end_matches('s');
        let chips = st.rows.map(|r| match r {
            PadRow::Nothing => Chip::Ok,
            PadRow::StillOn => Chip::On,
            PadRow::CantRead => Chip::Err,
        });
        let line = format!("Until des-k can read it, it runs no gestures, and the ones you turned off don't work. Set {row} again in Windows Settings. {LOG}");
        return (note(Tone::Warn, I_WARN, &format!("Can't read Windows' {one} setting"), line, vec![windows]), "des-k would run", Some(chips));
    }
    let on: Vec<usize> = (0..4).filter(|&i| st.rows[i] == PadRow::StillOn).collect();
    let chips = st.rows.map(|r| if r == PadRow::StillOn { Chip::On } else { Chip::Ok });
    match on.len() {
        0 => (note(Tone::Good, I_OK, "des-k runs your 3- and 4-finger gestures", "All four are off in Windows, so each gesture does one thing, also in admin windows.".into(), vec![]), "des-k runs these", Some(chips)),
        4 => (
            note(Tone::Info, I_SETUP, "Turn off four gestures in Windows", "Settings › Touchpad: under Three-finger and Four-finger gestures, set Swipes and Taps to “Nothing”. des-k runs these once all four are off.".into(), vec![(Ctl::Pad(Pad::Windows), "Set up in Windows Settings")]),
            "des-k would run",
            Some(chips),
        ),
        n => {
            let names: Vec<String> = on.iter().map(|&i| spelled_row(setup::ROWS[i].0)).collect();
            let off = 4 - n;
            let line = format!("Still on: {}. des-k runs gestures once all four are off; until then, the {off} you turned off don't work.", names.join(", "));
            (note(Tone::Warn, I_WARN, &format!("{n} of 4 {} still on in Windows", if n == 1 { "is" } else { "are" }), line, vec![windows]), "des-k would run", Some(chips))
        }
    }
}

/// "3-finger swipes" → "three-finger swipes", as a sentence has it.
fn spelled_row(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}

/// The side list item the view is in, for a scroll, column height, the visible height (`view`)
/// and section tops.
fn current(scroll: f32, height: f32, view: f32, tops: &[(Option<Section>, f32)]) -> Option<Option<Section>> {
    if scroll >= height - view - 1.0 {
        return tops.last().map(|(s, _)| *s);
    }
    let pos = scroll + 60.0;
    tops.iter().take_while(|(_, top)| *top <= pos).last().or(tops.first()).map(|(s, _)| *s)
}

/// The scroll thumb's top and height, when the column is taller than the view.
fn thumb(scroll: f32, height: f32, view: f32, top: f32) -> Option<(f32, f32)> {
    (height > view + 12.0).then(|| {
        let h = ((view - 8.0) * view / height).max(24.0);
        (top + 4.0 + (view - 8.0 - h) * scroll / (height - view).max(1.0), h)
    })
}

/// What Esc does: it backs out of the newest thing first, like a dialog. An open menu closes;
/// the clash prompt presses Cancel, so a stray Esc never closes the page with a choice still
/// owed; then each Dismiss banner goes, save error first; None when nothing is left: it closes
/// the page.
fn escape(prompt: Option<&Prompt>, error: bool, edited: bool) -> Option<Ctl> {
    match prompt {
        Some(p) => Some(Ctl::Cancel(p.key)),
        None if error => Some(Ctl::Dismiss),
        None if edited => Some(Ctl::DismissEdit),
        None => None,
    }
}

/// The whole page, back to front: the column, then the banners over its top, the side list,
/// the scroll thumb and the open menu.
fn compose(c: &mut Canvas, w: &Win) -> Out {
    let s = settings::get();
    let cw = (w.vw - X0 - PAD_R).min(COL_MAX);
    let mut x = Ctx { c, w, s, ro: settings::parse_error().is_some(), cw, top0: 0.0, out: Out { hits: vec![], height: 0.0, fixed: 0.0, tops: vec![], said: vec![] } };
    let fixed = x.measured(|c| c.banners());
    x.out.fixed = fixed;
    let top0 = fixed - w.scroll;
    x.top0 = top0;
    let b = x.c.t.b;
    let mut y = top0 + 9.0;
    x.c.text(X0, y, 32.0, "Settings", F::Heading, b.tx);
    y += 36.0;
    y += x.c.para(X0, y, cw, 20.0, &[("Every change saves to settings.toml and works at once. Restore waits for des-k's next start.", F::Body, b.tx2)]);
    let notes: Vec<&Problem> = settings::problems().iter().filter(|p| home(&p.key).is_none()).collect();
    if !notes.is_empty() {
        y += 32.0;
        y += x.heading(None, y);
        for p in notes {
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, s), lines: vec![P::Mono(p.message.clone())], buttons: vec![(Ctl::Open, "Open settings.toml")], group: 1 };
            y += x.note(X0, y, cw, &n) + 8.0;
        }
    }
    for sec in SECTIONS {
        y += 32.0;
        y += x.section(sec, y);
    }
    x.out.height = y + 32.0 - top0;
    // The banners cover the column scrolled under them; a line marks a cut edge.
    x.banners();
    let view = w.vh - fixed;
    if w.scroll > 0.5 {
        x.c.rule(NAV - 1.0, fixed, w.vw - NAV + 1.0, op(b.line));
    }
    if w.scroll < x.out.height - view - 1.0 {
        x.c.rule(NAV - 1.0, w.vh - 1.0, w.vw - NAV + 1.0, op(b.line));
    }
    x.side();
    if let Some((ty, th)) = thumb(w.scroll, x.out.height, view, fixed) {
        x.c.fill_rr((w.vw - 6.0, ty, 2.0, th), 1.0, op(if w.drag.is_some() { b.tx2 } else { b.tx3 }));
    }
    x.menu();
    // The menu's hits first, then the banners', the side list's, the column's.
    x.out.hits.sort_by_key(|h| std::cmp::Reverse(h.group));
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

/// Lays the page out again for its scroll, keeps the focus on a stop that is still there, and
/// gives Narrator the stops as painted.
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
            let max = (out.height - (w.vh - out.fixed)).max(0.0);
            let scroll = w.scroll.clamp(0.0, max);
            let moved = scroll != w.scroll;
            w.scroll = scroll;
            w.height = out.height;
            w.fixed = out.fixed;
            w.tops = out.tops;
            w.hits = out.hits;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                // A Reset that just reset itself away: the focus moves to its row's control.
                w.focus = f.key().and_then(|k| w.hits.iter().find(|h| h.ctl.key() == Some(k) && h.ctl.stop() != Ctl::Reset(k)).map(|h| h.ctl.stop()));
            }
            let s = w.scale;
            let px = |b: Bx| RECT { left: (b.0 * s) as i32, top: (b.1 * s) as i32, right: ((b.0 + b.2) * s) as i32, bottom: ((b.1 + b.3) * s) as i32 };
            let kids: Vec<Node> = out
                .said
                .into_iter()
                .map(|(ctl, mut n)| {
                    let bx = w.hits.iter().filter(|h| h.ctl.stop() == ctl).map(|h| h.at).reduce(|a, b| {
                        let (l, t) = (a.0.min(b.0), a.1.min(b.1));
                        (l, t, (a.0 + a.2).max(b.0 + b.2) - l, (a.1 + a.3).max(b.1 + b.3) - t)
                    });
                    n.rect = bx.map(px).unwrap_or(n.rect);
                    n.focused = w.focus == Some(ctl) && n.focusable;
                    n
                })
                .collect();
            let root = Node { role: UIA_PaneControlTypeId, name: WHO.into(), rect: px((0.0, 0.0, w.vw, w.vh)), kids, ..Node::default() };
            if let Ok(mut t) = w.tree.lock() {
                *t = root;
            }
            moved
        });
        if moved != Some(true) {
            break;
        }
    }
    repaint();
}

/// Shows what des-k read back from the file. Called after every reload.
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

/// Scrolls the focused stop into view.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().filter(|h| Some(h.ctl.stop()) == w.focus && h.group == 1).map(|h| h.at).next()?;
        let (top, bottom) = (h.1 - 12.0, h.1 + h.3 + 12.0);
        if top < w.fixed {
            Some(top - w.fixed)
        } else if bottom > w.vh {
            Some(bottom - w.vh)
        } else {
            None
        }
    })
    .flatten();
    match dy {
        Some(dy) => scroll_by(dy),
        None => layout(), // the focus can show the removal note
    }
    if let Some((h, tree)) = get(|w| (w.hwnd, w.tree.clone())) {
        uia::focus_moved(h, &tree, WHO);
    }
}

fn jump(sec: Option<Section>) {
    if let Some(top) = get(|w| w.tops.iter().find(|(s, _)| *s == sec).map(|(_, t)| *t)).flatten() {
        scroll_to(top - 8.0);
    }
}

/// A caption link: scrolls there, and the Skip animations link focuses the switch.
fn follow(l: Link) {
    match l {
        Link::Skip => {
            jump(Some(Section::Motion));
            let _ = set(|w| w.focus = Some(Ctl::Toggle("fastest")));
            reveal();
        }
        Link::Indicator => jump(Some(Section::Indicator)),
        Link::Overview => jump(Some(Section::Overview)),
    }
}

/// Writes one value; a failure shows what stays and why until the next good save.
fn save(key: &'static str, v: i64) {
    let Some(row) = find_row(key) else { return };
    match settings::write(Edit::Value(key, file_value(row, v))) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Err(why) => failed(row.title, why),
    }
}

/// Windows' words from a failed write, "cannot save … (os error 32)": the part in brackets.
fn failed(what: &str, why: String) {
    let said = why.rsplit_once(" (").map_or(why.clone(), |(_, w)| w.trim_end_matches(')').to_string());
    let _ = set(|w| w.error = Some((what.to_string(), said)));
    refresh();
}

/// Writes an edit that is not one row's value; a failure shows like a failed save.
fn commit(edit: Edit, title: &str) {
    match settings::write(edit) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Err(why) => failed(title, why),
    }
}

/// Starts recording; the hook swallows every key until it stops.
fn start_rec(rec: Rec) {
    let _ = set(|w| {
        w.rec = Some(rec);
        w.refused = None;
        w.prompt = None;
        w.menu = None;
    });
    crate::hints::cancel_recording(); // one recording at a time
    if let Some(h) = hwnd() {
        crate::keys::record(h, true);
    }
    refresh();
}

/// Ends recording, after a key, Esc, Cancel, a click, focus loss or close.
fn stop_rec() {
    if let Some(h) = hwnd() {
        crate::keys::record(h, false);
    }
    let _ = set(|w| {
        w.rec = None;
        w.refused = None;
    });
    refresh();
}

/// The hints window starts recording, so a recording here stops (spec 0017 AC-HNT-4).
pub fn cancel_recording() {
    if get(|w| w.rec.is_some()) == Some(true) {
        stop_rec();
    }
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
            return refuse("Press a digit 1 to 9", "Hold the keys, then press any digit. Still recording; Esc alone cancels.");
        }
        if mods & (WIN | CTRL | ALT) == 0 {
            return refuse(if mods == SHIFT { "Shift alone can't be used" } else { "A digit needs Win, Ctrl or Alt" }, "Hold Win, Ctrl or Alt with the digit. Still recording.");
        }
        stop_rec();
        return digits_recorded(key, mods);
    }
    if !settings::savable(vk) {
        return refuse("This key can't be saved", "Use a letter, a digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Still recording.");
    }
    if mods == 0 {
        return refuse("A key needs Win, Ctrl, Alt or Shift", "Hold one of them with the key. Still recording; Esc alone cancels.");
    }
    stop_rec();
    let combo = Combo { mods, vk };
    match rec {
        Rec::List(key, replace) => combo_recorded(key, replace, combo),
        Rec::Gesture(row, times) => set_gesture(row, times, Some(Action::Combo(combo))),
        Rec::Digits(_) => {}
    }
}

/// A key for a list: saved, or the clash prompt when another action has it.
fn combo_recorded(key: &'static str, replace: Option<usize>, combo: Combo) {
    let (s, name) = (settings::get(), name_of(key));
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
    let caps = crate::hints::caps(&combo);
    let words = spelled(&caps);
    let edit = Edit::Combos(key, mine);
    let prompt = match s.combo_owner(name, combo) {
        None => return commit(edit, title),
        Some(Owner::List(other)) => {
            let (other_key, other_title) = entry(other);
            let rest: Vec<Combo> = s.list(other).iter().filter(|c| **c != combo).copied().collect();
            Prompt {
                key,
                held: caps,
                title: format!("{words} is already in use"),
                sub: format!("“{other_title}” uses it. Move it here and “{other_title}” loses this key. Cancel changes nothing."),
                edit: Edit::Pair(Box::new(edit), Box::new(Edit::Combos(other_key, rest))),
            }
        }
        // The hook tries the lists first, so the set only loses this one digit.
        Some(Owner::Set(set)) => {
            let set_title = entry(set).1;
            let d = combo.vk as u8 as char;
            Prompt {
                key,
                held: caps,
                title: format!("{words} is part of “{set_title}”"),
                sub: format!("Move it here, and “{set_title}” skips {d}; its other eight digits keep working. Cancel changes nothing."),
                edit,
            }
        }
    };
    ask(key, prompt);
}

fn ask(key: &'static str, prompt: Prompt) {
    let _ = set(|w| {
        w.prompt = Some(prompt);
        w.focus = Some(Ctl::Cancel(key));
    });
    layout();
    reveal();
}

/// Modifiers for a digit set: saved, or the clash prompt when another set has them. A list on
/// one of their digits is no clash: the set skips that digit.
fn digits_recorded(key: &'static str, mods: u8) {
    let (s, name) = (settings::get(), name_of(key));
    if s.set(name) == Some(mods) {
        return;
    }
    let edit = Edit::Digits(key, Some(mods));
    let Some(other) = s.set_owner(name, mods) else { return commit(edit, entry(name).1) };
    let (other_key, other_title) = entry(other);
    let caps = digit_caps(mods);
    let prompt = Prompt {
        key,
        title: format!("{} is already in use", spelled(&caps)),
        held: caps,
        sub: format!("“{other_title}” uses it. Move it here and “{other_title}” turns off. Cancel changes nothing."),
        edit: Edit::Pair(Box::new(edit), Box::new(Edit::Digits(other_key, None))),
    };
    ask(key, prompt);
}

/// × on a key: that list loses it, a digit set turns off. No key left means off.
fn remove(key: &'static str, i: usize) {
    let (s, name) = (settings::get(), name_of(key));
    if is_digits(key) {
        return commit(Edit::Digits(key, None), entry(name).1);
    }
    let mut mine = s.list(name).to_vec();
    if i < mine.len() {
        mine.remove(i);
        commit(Edit::Combos(key, mine), entry(name).1);
    }
}

/// des-k or Windows for an action's Windows key (AC-SET-4): Windows removes that one key and
/// keeps your others; des-k adds it back through the clash check.
fn give_key(key: &'static str, desk: bool) {
    let Some(sh) = shortcut(key) else { return };
    let Some((wk, _)) = sh.win else { return };
    let s = settings::get();
    if desk == has_win(s, sh) || (desk && took(s, sh).is_some()) {
        return;
    }
    match (wk, desk) {
        (WinKey::Combo(c), true) => combo_recorded(key, None, c),
        (WinKey::Digits(m), true) => digits_recorded(key, m),
        (WinKey::Combo(c), false) => {
            let rest: Vec<Combo> = s.list(name_of(key)).iter().filter(|x| **x != c).copied().collect();
            commit(Edit::Combos(key, rest), sh.title);
        }
        (WinKey::Digits(_), false) => commit(Edit::Digits(key, None), sh.title),
    }
}

/// Reset on a shortcut: its default keys, through the clash check (AC-SET-3).
fn reset_keys(key: &'static str) {
    let s = settings::get();
    let name = name_of(key);
    let title = entry(name).1;
    if is_digits(key) {
        return match DEFAULTS.set(name) {
            Some(m) => digits_recorded(key, m),
            None => commit(Edit::Digits(key, None), title),
        };
    }
    let def = DEFAULTS.list(name).to_vec();
    let edit = Edit::Combos(key, def.clone());
    for c in &def {
        if let Some(Owner::List(other)) = s.combo_owner(name, *c) {
            let (other_key, other_title) = entry(other);
            let rest: Vec<Combo> = s.list(other).iter().filter(|x| *x != c).copied().collect();
            let caps = crate::hints::caps(c);
            let prompt = Prompt {
                key,
                title: format!("{} is already in use", spelled(&caps)),
                held: caps,
                sub: format!("“{other_title}” uses it. Reset moves it here and “{other_title}” loses this key. Cancel changes nothing."),
                edit: Edit::Pair(Box::new(edit), Box::new(Edit::Combos(other_key, rest))),
            };
            return ask(key, prompt);
        }
    }
    commit(edit, title);
}

/// One gesture row's once or twice action; None: no twice rule. The whole map is written again,
/// every row's once rule included.
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

/// Opens a picker's menu, its chosen line with the keyboard focus.
fn open_menu(row: usize, times: u8, family: Option<usize>) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == Ctl::Picker(row)).map(|h| h.at)).flatten() else { return };
    let m = Menu { row, times, family, at, focus: 0 };
    let focus = menu_lines(&m, settings::get()).iter().position(|l| l.on).unwrap_or(0);
    let _ = set(|w| w.menu = Some(Menu { focus, ..m }));
    layout();
}

fn close_menu() {
    let _ = set(|w| w.menu = None);
    layout();
}

/// A menu line chosen, by click or Enter.
fn choose(i: usize) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let lines = menu_lines(&m, settings::get());
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

fn act(ctl: Ctl) {
    if ctl.writes() && settings::parse_error().is_some() {
        return; // read only
    }
    let s = settings::get();
    match ctl {
        Ctl::Side(sec) => jump(sec),
        Ctl::Welcome => crate::welcome::show(false),
        Ctl::Open => crate::tray::open(&settings::path()),
        Ctl::Toggle(key) => save(key, 1 - value(s, key)),
        Ctl::Minus(key) => step_key(key, false),
        Ctl::Plus(key) => step_key(key, true),
        Ctl::Stepper(_) | Ctl::Pick(_) | Ctl::Tiles => {} // focus only: Left and Right change them
        Ctl::Choice(key, i) => match find_row(key).map(|r| r.kind) {
            Some(Kind::Pair(..)) => save(key, (i == 0) as i64),
            _ if !unused(s, key).is_empty() => {}
            _ => save(key, i as i64),
        },
        Ctl::Reset("workspaces") => save("workspaces", DEFAULTS.workspaces as i64),
        Ctl::Reset(key) if shortcut(key).is_some() => reset_keys(key),
        Ctl::Reset(key) => save(key, value(&DEFAULTS, key)),
        Ctl::Link(l) => follow(l),
        Ctl::LastTile => {}
        Ctl::TileAdd => step_key("workspaces", true),
        Ctl::TileRemove => step_key("workspaces", false),
        Ctl::Forget(i) => {
            if let Some(id) = get(|w| w.pics.view.waiting.get(i).map(|x| x.id.clone())).flatten() {
                crate::ws::forget(&id);
                update_pictures(true);
            }
        }
        Ctl::KeyRow(key) | Ctl::AddKey(key) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, None) }),
        Ctl::Key(key, i) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, Some(i)) }),
        Ctl::KeyX(key, i) => remove(key, i),
        Ctl::WinSide(key, desk) => give_key(key, desk),
        Ctl::StopRec(_) => stop_rec(),
        Ctl::Move(_) => {
            if let Some(p) = set(|w| w.prompt.take()).flatten() {
                commit(p.edit, entry(name_of(p.key)).1);
            }
        }
        Ctl::Cancel(_) => {
            let _ = set(|w| w.prompt = None);
            refresh();
        }
        Ctl::Picker(row) => open_menu(row, 1, None),
        Ctl::Item(usize::MAX) => {}
        Ctl::Item(i) => choose(i),
        Ctl::Show => {
            if s.indicator_enabled {
                show();
            }
        }
        Ctl::Pad(Pad::Windows) => setup::use_gestures(),
        Ctl::Pad(Pad::ReadOn) => save("touchpad.enabled", 1),
        Ctl::Pad(Pad::GiveBack) => {
            crate::welcome::give();
            update_pictures(true);
        }
        Ctl::Chart => {}
        Ctl::Dismiss => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Ctl::DismissEdit => {
            let _ = set(|w| w.edited = None);
            refresh();
        }
    }
}

fn step_key(key: &'static str, up: bool) {
    if let Some(Row { kind: Kind::Step { lo, hi, by, .. }, .. }) = find_row(key) {
        // Greyed: kept but not in use, so − and + do nothing (AC-SET-8).
        if !unused(settings::get(), key).is_empty() {
            return;
        }
        let v = value(settings::get(), key);
        let n = stepped(v, *lo, *hi, *by, up);
        if n != v {
            save(key, n);
        }
    }
}

/// The real indicator on the screen the page is on, with the current hold and fade.
fn show() {
    let Some(h) = hwnd() else { return };
    crate::ws::show_indicator(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) });
}

/// The keyboard stops in one area, in order, each with its top: the side list, or the rest.
fn stops(w: &Win, side: bool) -> Vec<(Ctl, f32)> {
    let mut out: Vec<(Ctl, f32)> = vec![];
    let mut hits: Vec<&Hit> = w.hits.iter().filter(|h| h.ctl.in_side() == side && h.group != 3).collect();
    // Banners first, then the column, each in page order.
    hits.sort_by_key(|h| if h.group == 2 { 0 } else { 1 });
    for h in hits {
        let c = h.ctl.stop();
        if !out.iter().any(|(x, _)| *x == c) {
            out.push((c, h.at.1));
        }
    }
    out
}

/// Focuses `c` and shows it; a side list item also jumps to its section.
fn focus_on(c: Ctl) {
    let _ = set(|w| w.focus = Some(c));
    match c {
        Ctl::Side(sec) => {
            jump(sec);
            reveal();
        }
        _ => reveal(),
    }
}

/// Where the keyboard goes next, as in Windows Settings: Tab between the side list and the
/// settings, Up and Down from stop to stop.
fn next_stop(w: &Win, vk: VIRTUAL_KEY) -> Option<Ctl> {
    let side = w.focus.is_some_and(Ctl::in_side);
    let first_seen = || {
        let list = stops(w, false);
        list.iter().find(|(_, y)| *y >= w.fixed).or(list.first()).map(|(c, _)| *c)
    };
    match vk {
        VK_TAB if side => first_seen(),
        VK_TAB => current(w.scroll, w.height, w.vh - w.fixed, &w.tops).map(Ctl::Side),
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

/// Left and Right on the focused stop: a switch goes off or on, a number steps, a choice moves,
/// a shortcut row picks des-k or Windows, the tiles add or remove.
fn change(focus: Ctl, right: bool) {
    if settings::parse_error().is_some() {
        return; // read only
    }
    let s = settings::get();
    match focus {
        Ctl::Toggle(k) => {
            if value(s, k) != right as i64 {
                save(k, right as i64);
            }
        }
        Ctl::Stepper(k) => step_key(k, right),
        Ctl::Tiles => step_key("workspaces", right),
        Ctl::Pick(k) => match find_row(k).map(|r| r.kind) {
            Some(Kind::Pair(..)) if value(s, k) != !right as i64 => save(k, !right as i64),
            Some(Kind::Choice(words)) if unused(s, k).is_empty() => {
                let i = value(s, k) as usize;
                let n = if right { (i + 1).min(words.len() - 1) } else { i.saturating_sub(1) };
                if n != i {
                    save(k, n as i64);
                }
            }
            _ => {}
        },
        Ctl::KeyRow(k) => give_key(k, !right),
        _ => return,
    }
    reveal();
}

/// The open menu's keys: Up and Down move, Enter or Space picks, Esc closes or goes back.
fn menu_key(vk: VIRTUAL_KEY) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let n = menu_lines(&m, settings::get()).len();
    match vk {
        VK_ESCAPE if m.family.is_some() || m.times == 2 => open_menu(m.row, 1, None),
        VK_ESCAPE => close_menu(),
        VK_UP | VK_DOWN | VK_HOME | VK_END => {
            let f = match vk {
                VK_UP => m.focus.saturating_sub(1),
                VK_DOWN => (m.focus + 1).min(n - 1),
                VK_HOME => 0,
                _ => n - 1,
            };
            let _ = set(|w| w.menu = Some(Menu { focus: f, ..m }));
            layout();
        }
        VK_RETURN | VK_SPACE => choose(m.focus),
        _ => {}
    }
}

fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    if get(|w| w.menu.is_some()) == Some(true) {
        return menu_key(vk);
    }
    let focus = get(|w| w.focus).flatten();
    match vk {
        VK_ESCAPE => match get(|w| escape(w.prompt.as_ref(), w.error.is_some(), w.edited.is_some())).flatten() {
            Some(c) => {
                act(c);
                reveal();
            }
            None => unsafe { let _ = DestroyWindow(h); },
        },
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
            Some(Ctl::Stepper(_) | Ctl::Pick(_) | Ctl::Tiles) | None => {}
            Some(f) => {
                act(f);
                reveal();
            }
        },
        // On the tiles: removes the last workspace, as × does; on a shortcut row, its last key.
        VK_DELETE => match focus {
            Some(Ctl::Tiles) => change(Ctl::Tiles, false),
            Some(Ctl::KeyRow(k)) if settings::parse_error().is_none() => {
                let s = settings::get();
                if let Some(sh) = shortcut(k)
                    && let Some(&i) = drawn_order(s, sh).last()
                {
                    remove(k, i);
                }
            }
            _ => {}
        },
        VK_PRIOR | VK_NEXT => {
            let page = get(|w| w.vh - w.fixed - 40.0).unwrap_or(0.0);
            scroll_by(if vk == VK_PRIOR { -page } else { page });
        }
        _ => {}
    }
}

fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. Column controls count only inside the column's view; while the
/// menu is open, only its lines do.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let menu = w.menu.is_some();
    let shown = |h: &Hit| if menu { h.group == 3 } else { h.group != 1 || (x > NAV && y > w.fixed) };
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the page on the screen under the mouse, or brings the open one to the front (AC-SET-1).
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
    ui::gdiplus();
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
        let (work, _) = work_area(mon);
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("des-k settings"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("settings page: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        crate::ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        PAGE.set(Some(Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            scroll: 0.0,
            height: 0.0,
            fixed: 0.0,
            tops: vec![],
            hits: vec![],
            focus: None,
            kb: false,
            hover: None,
            press: None,
            drag: None,
            vw: W,
            vh: H,
            error: None,
            rec: None,
            refused: None,
            prompt: None,
            menu: None,
            pics: Pics::default(),
            edited: None,
            tree: Arc::new(Mutex::new(Node::default())),
        }));
        // Shorter than H when the screen is (1366 × 768 at 125%); WM_SIZE sets vw and vh.
        let (ww, wh) = size(h, W, H);
        let wh = wh.min(work.bottom - work.top);
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

/// The window's outer size for a client area `cw` × `ch` dip, at its dpi.
fn size(h: HWND, cw: f32, ch: f32) -> (i32, i32) {
    unsafe {
        let dpi = GetDpiForWindow(h);
        let s = dpi as f32 / 96.0;
        let mut r = RECT { left: 0, top: 0, right: (cw * s).round() as i32, bottom: (ch * s).round() as i32 };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        (r.right - r.left, r.bottom - r.top)
    }
}

/// The work area of the screen a window or rect is on, and that screen's whole rect.
fn work_area(mon: HMONITOR) -> (RECT, RECT) {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = unsafe { GetMonitorInfoW(mon, &mut mi) };
    (mi.rcWork, mi.rcMonitor)
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
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, w.theme.b.bg, None);
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
                if get(|w| w.menu.is_some()) == Some(true) {
                    return LRESULT(0);
                }
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
                // A click outside the open menu closes it, and does nothing else.
                if get(|w| w.menu.is_some() && hit_at(w, point(lp, w.scale)).is_none()) == Some(true) {
                    close_menu();
                    return LRESULT(0);
                }
                let _ = SetCapture(h);
                let _ = set(|w| {
                    let (x, y) = point(lp, w.scale);
                    w.kb = false;
                    match thumb(w.scroll, w.height, w.vh - w.fixed, w.fixed) {
                        Some((ty, th)) if x >= w.vw - 12.0 && y > w.fixed && w.menu.is_none() => {
                            // On the thumb: drag it; beside it: jump there, then drag.
                            let grab = if (ty..ty + th).contains(&y) { y - ty } else { th / 2.0 };
                            w.drag = Some(grab);
                        }
                        _ => {
                            w.press = hit_at(w, (x, y));
                            if let Some(p) = w.press.filter(|p| !matches!(p, Ctl::Item(_))) {
                                w.focus = Some(p.stop());
                            }
                        }
                    }
                });
                let _ = PostMessageW(Some(h), WM_MOUSEMOVE, wp, lp);
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let drag = get(|w| {
                    let (_, y) = point(lp, w.scale);
                    let view = w.vh - w.fixed;
                    let (_, th) = thumb(w.scroll, w.height, view, w.fixed)?;
                    let grab = w.drag?;
                    Some((y - grab - w.fixed - 4.0) / (view - 8.0 - th) * (w.height - view))
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
                    layout(); // pointing at × or Reset shows what a removal moves
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
            WM_SIZE => {
                if wp.0 as u32 != SIZE_MINIMIZED
                    && set(|w| {
                        w.vw = (lp.0 & 0xFFFF) as f32 / w.scale;
                        w.vh = ((lp.0 >> 16) & 0xFFFF) as f32 / w.scale;
                        w.menu = None;
                    })
                    .is_some()
                {
                    layout();
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                let (min_w, min_h) = size(h, W, MIN_H);
                mm.ptMinTrackSize = POINT { x: min_w, y: min_h };
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Recording ends when the page loses focus, and an open menu closes.
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    if get(|w| w.rec.is_some()) == Some(true) {
                        stop_rec();
                    }
                    if get(|w| w.menu.is_some()) == Some(true) {
                        close_menu();
                    }
                }
                refresh();
                LRESULT(0)
            }
            WM_GETOBJECT => match get(|w| w.tree.clone()) {
                Some(tree) => uia::answer_tree(h, wp, lp, &tree),
                None => DefWindowProcW(h, m, wp, lp),
            },
            WM_DESTROY => {
                crate::keys::record(h, false);
                let _ = UiaReturnRawElementProvider(h, WPARAM(0), LPARAM(0), None);
                drop(PAGE.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let to = *(lp.0 as *const RECT);
                let scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                let (vw, vh) = set(|w| {
                    w.scale = scale;
                    w.fonts = Fonts::new(scale);
                    (w.vw, w.vh)
                })
                .unwrap_or((W, H));
                // The same size in dip, but never taller than the new screen.
                let (ww, wh) = size(h, vw, vh);
                let (work, _) = work_area(MonitorFromRect(&to, MONITOR_DEFAULTTONEAREST));
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh.min(work.bottom - work.top), SWP_NOZORDER | SWP_NOACTIVATE);
                layout();
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                crate::ui::refresh();
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

    // A hand written value off the grid is kept as is, and the next step lands on the grid,
    // not one step away from the odd value.
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
        assert_eq!(stepped(1000, 0, 1000, 50, true), 1000); // the overview's fade time
    }

    // Esc with the clash prompt open is Cancel, never close: closing would drop the prompt,
    // and you expect Esc to back out of the question, not the page. Banners go next, one per
    // Esc, and only an Esc with nothing left closes the page.
    #[test]
    fn esc_backs_out_one_layer_at_a_time() {
        let p = Prompt { key: "shortcuts.next", held: vec![], title: String::new(), sub: String::new(), edit: Edit::Gestures(vec![]) };
        assert_eq!(escape(Some(&p), true, true), Some(Ctl::Cancel("shortcuts.next")));
        assert_eq!(escape(None, true, true), Some(Ctl::Dismiss));
        assert_eq!(escape(None, false, true), Some(Ctl::DismissEdit));
        assert_eq!(escape(None, false, false), None);
    }

    // No twice rule and a twice rule that does nothing behave apart (one waits for a second
    // go), so their labels must differ and say which one waits.
    #[test]
    fn twice_labels_tell_waiting_apart() {
        let (none, nothing) = (twice_label(None), twice_label(Some(Action::Nothing)));
        assert_ne!(none, nothing);
        assert!(nothing.contains("waits") && !none.contains("waits"));
    }

    // Every row writes a value the parser reads back as the same setting: a typo in a key or
    // a word here would save something des-k ignores. Covers the new fastest, style and
    // overview fade time rows.
    #[test]
    fn every_row_round_trips_through_the_file() {
        for row in ROWS {
            let d = value(&DEFAULTS, row.key);
            let v = match row.kind {
                Kind::Toggle | Kind::Pair(..) => 1 - d,
                Kind::Step { lo, hi, .. } => if d == hi { lo } else { hi },
                Kind::Choice(words) => (d + 1) % words.len() as i64,
            };
            let text = settings::edit_for_test(row.key, file_value(row, v));
            let (s, warn) = settings::parse_for_test(&text);
            assert!(warn.is_empty(), "{}: {warn:?}", row.key);
            assert_eq!(value(&s, row.key), v, "{}", row.key);
        }
    }

    // AC-SET-8: the overview's fade time is greyed while the animation is None or Skip
    // animations is on, naming each reason; the indicator's fade only with Skip animations
    // (O-1). A greyed value is kept, so its stepper must not change it.
    #[test]
    fn greyed_rows_name_every_reason() {
        let mut s = Settings::default();
        assert_eq!(unused(&s, "overview.fade_ms"), vec![Why::AnimationNone]);
        assert!(unused(&s, "indicator.fade_ms").is_empty());
        s.fastest = true;
        assert_eq!(unused(&s, "overview.fade_ms"), vec![Why::AnimationNone, Why::Skip]);
        assert_eq!(unused(&s, "indicator.fade_ms"), vec![Why::Skip]);
        s.fastest = false;
        s.overview_animation = settings::Animation::Fade;
        assert!(unused(&s, "overview.fade_ms").is_empty());
        s.indicator_enabled = false;
        assert_eq!(unused(&s, "indicator.style"), vec![Why::IndicatorOff]);
    }

    // AC-SET-5: each kind of log line gets a plain title, and the bad values the design draws
    // read as it does, so a problem never shows only des-k's own log words.
    #[test]
    fn problems_get_plain_titles() {
        let s = Settings::default();
        let p = |m: &str| {
            let key = m.split_once(": ").map_or("", |(k, _)| k);
            Problem { key: if key.contains(' ') { String::new() } else { key.to_string() }, message: m.to_string() }
        };
        assert_eq!(title(&p("unknown key workspace, ignored"), &s), "“workspace” isn't a setting, so des-k skips it");
        assert_eq!(title(&p("indicator.hold_ms: expected a whole number, got a string, using default 550"), &s), "des-k can't use this value, so it uses 550 ms");
        assert_eq!(title(&p("overview.fade_ms: 1500 is not between 0 and 1000, using default 250"), &s), "des-k can't use this value, so it uses 250 ms");
        assert_eq!(title(&p("fastest: expected true or false, got a string, using default false"), &s), "des-k can't use this value, so Skip animations is off");
        assert_eq!(title(&p("shortcuts: Win+F is in both finder and overview, overview turned off"), &s), "Win F was set for both Find a window and Open overview, so Open overview has no key now");
        assert_eq!(title(&p("gesture rule 3: fingers = 5, not 3 or 4, skipped"), &s), "Gesture rule 3 can't be used, so des-k skips it");
        assert!(!title(&p("something new des-k says"), &s).is_empty());
    }

    // A problem is shown under its own setting, or in its section, or in Notes; never lost.
    // fastest's is under Motion, the Copilot key's under Shortcuts.
    #[test]
    fn problems_have_a_home() {
        assert_eq!(home("indicator.hold_ms"), Some(Section::Indicator));
        assert_eq!(home("shortcuts"), Some(Section::Shortcuts));
        assert_eq!(home("copilot_as_right_ctrl"), Some(Section::Shortcuts));
        assert_eq!(home("fastest"), Some(Section::Motion));
        assert_eq!(home("gesture"), Some(Section::Touchpad));
        assert_eq!(home(""), None); // unknown keys and gesture rule N
    }

    // AC-SET-4: Windows' own key is drawn first, but × and a recording act on the file's own
    // index, so the drawn order never changes what is written.
    #[test]
    fn windows_key_first_maps_back_to_the_file() {
        let mut s = Settings::default();
        let win_f = Combo { mods: WIN, vk: 0x46 };
        let mine = Combo { mods: WIN | ALT, vk: 0x46 };
        s.finder = vec![mine, win_f];
        let sh = shortcut("shortcuts.finder").unwrap();
        assert_eq!(drawn_order(&s, sh), vec![1, 0]);
        assert!(has_win(&s, sh));
        s.finder = vec![mine];
        assert!(!has_win(&s, sh));
        assert_eq!(drawn_order(&s, sh), vec![0]);
    }

    // Every action a gesture can do is in the menu exactly once, so none is out of reach.
    #[test]
    fn the_menu_offers_every_action_once() {
        let s = Settings::default();
        let m = Menu { row: 0, times: 1, family: None, at: (0.0, 0.0, 0.0, 0.0), focus: 0 };
        let lines = menu_lines(&m, &s);
        for a in settings::actions() {
            let n = lines.iter().filter(|l| l.does == Does::Set(Some(a))).count();
            let in_family = FAMILIES.iter().any(|(_, f)| (0..9).any(|i| f(i) == a));
            assert_eq!(n, if in_family { 0 } else { 1 }, "{a:?}");
        }
    }

    // A hand edit shows in the page's words, as the design's banner does, never the file's.
    #[test]
    fn hand_edits_read_in_the_page_words() {
        assert_eq!(changed("indicator.hold_ms", "550", "800"), "Indicator stays: 550 → 800 ms");
        assert_eq!(changed("shortcuts.overview", "[Win+Tab]", "[]"), "Open overview: Win Tab → no key");
        assert_eq!(changed("fastest", "false", "true"), "Skip animations: off → on");
        assert_eq!(changed("overview.animation", "off", "fade"), "Overview animation: None → Fade");
    }

    // The Windows key of an action another action holds reads as taken by that one.
    #[test]
    fn a_taken_windows_key_names_its_holder() {
        let s = Settings { overview: vec![], finder: vec![Combo { mods: WIN, vk: 0x46 }, Combo { mods: WIN, vk: 0x09 }], ..Default::default() };
        let sh = shortcut("shortcuts.overview").unwrap();
        assert_eq!(took(&s, sh), Some("Find a window"));
    }

    // Not a check that can fail: draws the page in the design's states into
    // target/settings/*.png, to compare by eye with the approved captures (AC-1). Run with
    // `cargo test render_settings -- --ignored`.
    #[test]
    #[ignore]
    fn render_settings() {
        assert!(ui::gdiplus());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("settings");
        std::fs::create_dir_all(&dir).unwrap();
        let r = |l, t, rr, b| RECT { left: l, top: t, right: rr, bottom: b };
        let fake = || Pics {
            screens: vec![
                crate::ws::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 600, 1440, 1500), scale: 200 },
                crate::ws::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(1440, 0, 4000, 1440), scale: 150 },
            ],
            view: crate::ws::View {
                active: vec![("DISPLAY1".into(), 0), ("DISPLAY2".into(), 1)],
                windows: [("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY2", 0), ("DISPLAY2", 1), ("DISPLAY2", 1), ("DISPLAY2", 3)].iter().map(|(d, w)| (d.to_string(), *w, r(0, 0, 10, 10))).collect(),
                names: vec![("DISPLAY1".into(), "Built-in display".into()), ("DISPLAY2".into(), "DELL U2720Q".into())],
                removals: vec![
                    vec![
                        crate::ws::Moved { name: "Built-in display".into(), device: Some("DISPLAY1".into()), unplugged: false, windows: 0, from: vec![] },
                        crate::ws::Moved { name: "DELL U2720Q".into(), device: Some("DISPLAY2".into()), unplugged: false, windows: 1, from: vec![3] },
                    ],
                    vec![],
                ],
                waiting: vec![],
            },
            pad: Some(PadState { pad: true, enabled: true, rows: [PadRow::Nothing; 4], off: true, saved: true }),
            back: None,
            here: Some("DISPLAY2".into()),
        };
        let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5)];
        // Name, section to scroll to, what to set up.
        type Setup = fn(&mut Win);
        let states: [(&str, Option<Section>, Setup); 9] = [
            ("top", None, |_| {}),
            ("removing", None, |w| {
                w.hover = Some(Ctl::TileRemove);
                w.scroll = 140.0;
            }),
            ("clash", Some(Section::Shortcuts), |w| {
                let held = vec!["Win".to_string(), "Tab".to_string()];
                let sub = "“Open overview” uses it. Move it here and “Open overview” loses this key. Cancel changes nothing.".into();
                w.prompt = Some(Prompt { key: "shortcuts.finder", held, title: "Win Tab is already in use".into(), sub, edit: Edit::Gestures(vec![]) });
                w.focus = Some(Ctl::Cancel("shortcuts.finder"));
                w.scroll = 300.0;
            }),
            ("menu", Some(Section::Touchpad), |_| {}),
            ("keys", Some(Section::Shortcuts), |_| {}),
            ("indicator", Some(Section::Indicator), |_| {}),
            ("overview", Some(Section::Overview), |w| w.focus = Some(Ctl::Stepper("overview.fade_ms"))),
            ("motion", Some(Section::Motion), |w| w.focus = Some(Ctl::Toggle("fastest"))),
            ("touchpad", Some(Section::Touchpad), |_| {}),
        ];
        for (tname, theme, scale) in &runs {
            for (sname, sec, setup) in &states {
                let mut win = Win {
                    hwnd: HWND::default(), scale: *scale, theme: *theme, fonts: Fonts::new(*scale), scroll: 0.0, height: 0.0, fixed: 0.0, tops: vec![], hits: vec![],
                    focus: None, kb: true, hover: None, press: None, drag: None, vw: W, vh: H, error: None, rec: None, refused: None, prompt: None, menu: None,
                    pics: fake(), edited: None, tree: Arc::new(Mutex::new(Node::default())),
                };
                setup(&mut win);
                let measure = |win: &Win| unsafe {
                    let dc = GetDC(None);
                    let out = compose(&mut Canvas::new(dc, false, *scale, *theme, &win.fonts), win);
                    ReleaseDC(None, dc);
                    out
                };
                let out = measure(&win);
                if let Some(top) = out.tops.iter().find(|(s, _)| *s == *sec).map(|(_, t)| *t) {
                    win.scroll = (top - 8.0 + win.scroll).clamp(0.0, (out.height - win.vh).max(0.0));
                }
                win.hits = measure(&win).hits;
                if *sname == "menu"
                    && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::Picker(2)).map(|h| h.at)
                {
                    win.menu = Some(Menu { row: 2, times: 1, family: None, at, focus: 0 });
                }
                let (pw, ph) = ((W * scale).round() as i32, (H * scale).round() as i32);
                ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                    let mut c = Canvas::new(dc, true, *scale, *theme, &win.fonts);
                    c.boxed((0.0, 0.0, W + 1.0, H + 1.0), 0.0, theme.b.bg, None);
                    compose(&mut c, &win);
                });
            }
        }
    }
}
