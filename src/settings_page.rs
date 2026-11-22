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
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{Gdi::*, GdiPlus::DashStyleDash},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::*,
        Controls::{EM_SETLIMITTEXT, EM_SETMARGINS, EM_SETSEL, WM_MOUSELEAVE},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::settings::{self, Action, Combo, Edit, Motion, Owner, Pick, Problem, Settings};
use crate::setup::{self, GaveBack, Row as PadRow, State as PadState, Wrote};
use crate::ui::{self, Bx, Canvas, F, Fonts, Theme, buffered, dark_title, mix, op, theme};
use crate::uia::{self, Node, Tree};

const CLASS: PCWSTR = w!("des-k-settings");
// Clips its one child, the text box the popup words and the name boxes share (specs 0022, 0023),
// so a paint never covers it.
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_THICKFRAME.0 | WS_CLIPCHILDREN.0);
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
const APP_WS_W: f32 = 184.0; // an Apps rule's workspace menu (spec 0027)
const APP_SCREEN_W: f32 = 204.0; // and its screen menu
const KT_KEYS: f32 = 326.0; // a shortcut row's key column
const KT_SIDE: f32 = 112.0; // and its des-k or Windows column
const WALL_PICK_W: f32 = 264.0; // a Wallpaper row's menu button and its screen chooser (spec 0026)
const WALL_TIME_W: f32 = 148.0; // a Wallpaper row's room for its time per picture (spec 0028)

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
const I_APPS: &str = "\u{E71D}";
const I_APP: &str = "\u{E737}";
const I_SCREEN: &str = "\u{E7F4}";
const I_PIC: &str = "\u{EB9F}";
const I_FOLDER: &str = "\u{E8B7}";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Workspaces,
    Apps,
    Shortcuts,
    Indicator,
    Attention,
    Restore,
    Overview,
    Colors,
    Motion,
    Touchpad,
}

/// A card inside a section that the page can open at (spec 0029 AC-4): What's new's buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    Names,
    Wallpaper,
    Back, // the Back row under Shortcuts
}

const SECTIONS: [Section; 10] = [Section::Workspaces, Section::Apps, Section::Shortcuts, Section::Indicator, Section::Attention, Section::Restore, Section::Overview, Section::Colors, Section::Motion, Section::Touchpad];

impl Section {
    fn name(self) -> &'static str {
        match self {
            Section::Workspaces => "Workspaces",
            Section::Apps => "Apps",
            Section::Shortcuts => "Shortcuts",
            Section::Indicator => "Indicator",
            Section::Attention => "Attention",
            Section::Restore => "Restore",
            Section::Overview => "Overview",
            Section::Colors => "Colors",
            Section::Motion => "Motion",
            Section::Touchpad => "Touchpad",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Workspaces => "\u{E8A9}",
            Section::Apps => I_APPS,
            Section::Shortcuts => "\u{E765}",
            Section::Indicator => "\u{E712}",
            Section::Attention => "\u{EA8F}",
            Section::Restore => "\u{E777}",
            Section::Overview => "\u{E7C4}",
            Section::Colors => "\u{E790}",
            Section::Motion => "\u{EC4A}",
            Section::Touchpad => "\u{EFA5}",
        }
    }

    fn line(self) -> &'static str {
        match self {
            Section::Workspaces => "Every screen has its own workspaces, all the same count.",
            Section::Apps => "Open an app's new windows on its own workspace and screen. Your screens stay where they are, and a note on the indicator tells you where the window went.",
            Section::Shortcuts => "Click a key to change it, × to remove it, + to add another. An action with no key is off. Choosing Windows gives that Windows key back.",
            Section::Indicator => "Shows the workspace for a moment after a switch, a send or a pin, and when a screen comes or goes.",
            Section::Attention => "When a window on a workspace you can't see flashes for you, like a chat with a new message, its screen tells you.",
            Section::Restore => "Puts windows back on their workspaces after a reboot or sign-in.",
            Section::Overview => "Every workspace of every screen at once.",
            Section::Colors => "des-k's accent and each workspace's color, in every des-k window. One color serves light and dark.",
            Section::Motion => "How des-k's fades and moves play. Windows' own Animation effects setting doesn't change them.",
            Section::Touchpad => "What 3- and 4-finger gestures do on a precision touchpad.",
        }
    }
}

/// Which section a problem belongs to, by its key; None: no one setting (the Notes section).
fn home(key: &str) -> Option<Section> {
    Some(match key.split('.').next().unwrap_or("") {
        "workspaces" | "follow_activation" | "jump_back" | "names" | "wallpaper" => Section::Workspaces,
        "apps" => Section::Apps,
        "shortcuts" | "copilot_as_right_ctrl" => Section::Shortcuts,
        "indicator" => Section::Indicator,
        "attention" => Section::Attention,
        "restore" => Section::Restore,
        "overview" => Section::Overview,
        "colors" => Section::Colors,
        "fastest" => Section::Motion,
        "touchpad" | "gesture" => Section::Touchpad,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    Toggle,
    /// A switch drawn as two choices: the word for on, then for off.
    Pair(&'static str, &'static str),
    /// A number in spec 0001's range, in fixed steps.
    Step { lo: i64, hi: i64, by: i64, unit: &'static str },
    /// The words the file takes, each with its label.
    Choice(&'static [(&'static str, &'static str)]),
    /// Minutes on a fixed ladder of values, read "45 min" or "1 h 30 min" (spec 0028 AC-11).
    Ladder(&'static [i64]),
}

pub(crate) struct Row {
    pub(crate) key: &'static str,
    pub(crate) title: &'static str,
    pub(crate) cap: &'static str,
    pub(crate) kind: Kind,
}

// The first run's words (AC-SET-6): Never, None and Fade; the file's words stay.
const LABEL_WORDS: &[(&str, &str)] = &[("hover", "On hover"), ("always", "Always"), ("none", "Never")];
const ANIMATION_WORDS: &[(&str, &str)] = &[("off", "None"), ("fade", "Fade")];
const STYLE_WORDS: &[(&str, &str)] = &[("card", "Card"), ("dots", "Dots")];
const FIT_WORDS: &[(&str, &str)] = &[("fill", "Fill"), ("fit", "Fit"), ("stretch", "Stretch"), ("center", "Center")];
const TRANSITION_WORDS: &[(&str, &str)] = &[("instant", "Instant"), ("fade", "Fade")];
/// A slideshow's minutes per picture (spec 0028 AC-11): up to 45 min, then hours up to 24.
const LADDER: &[i64] = &[1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 360, 480, 720, 1440];
/// A stepper's unit for minutes shown in words, as `minutes` writes them.
const MINUTES: &str = "minutes";
/// A workspace's own time per picture, `[wallpaper.intervals] wsN`: its stepper's key.
const IV_KEYS: [&str; 9] = ["wallpaper.intervals.ws1", "wallpaper.intervals.ws2", "wallpaper.intervals.ws3", "wallpaper.intervals.ws4", "wallpaper.intervals.ws5", "wallpaper.intervals.ws6", "wallpaper.intervals.ws7", "wallpaper.intervals.ws8", "wallpaper.intervals.ws9"];

const fn row(key: &'static str, title: &'static str, cap: &'static str, kind: Kind) -> Row {
    Row { key, title, cap, kind }
}

const fn step(lo: i64, hi: i64, by: i64, unit: &'static str) -> Kind {
    Kind::Step { lo, hi, by, unit }
}

/// Every plain setting.
pub(crate) const ROWS: &[Row] = &[
    row("follow_activation", "Follow a window", "When a hidden window comes to the front (an app reuses it, a dialog pops up), go to its workspace.", Kind::Toggle),
    row("jump_back", "Same number goes back", "Pressing the number of the workspace you're on takes you back to the one before.", Kind::Toggle),
    row("copilot_as_right_ctrl", "Copilot key as Right Ctrl", "", Kind::Pair("des-k", "Windows")),
    row("indicator.enabled", "Workspace indicator", "", Kind::Toggle),
    row("indicator.style", "Style", "", Kind::Choice(STYLE_WORDS)),
    row("indicator.hold_ms", "Indicator stays", "How long it shows. 0 to 5000 ms. With a line under it, at least 1.5 s.", step(0, 5000, 50, "ms")),
    row("indicator.fade_ms", "Indicator fades out", "How long it takes to go. 0 to 2000 ms.", step(0, 2000, 50, "ms")),
    row("indicator.screen_name", "Show screen name", "The screen's name under the workspace's name, with two or more screens.", Kind::Toggle),
    row("attention.enabled", "Show windows that want you", "The indicator pops up on the window's screen, even with the indicator off, and that workspace keeps a dot until you go there.", Kind::Toggle),
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
    // The Wallpaper card's last rows (spec 0026 AC-13, AC-18): the same whichever screen it shows.
    row("wallpaper.fit", "Fit", "How a picture covers the screen. Fit and Center show bars in your Windows background color. Same on every screen.", Kind::Choice(FIT_WORDS)),
    row("wallpaper.dim", "Dim pictures", "Darkens des-k's pictures so white icon labels stay readable. Your Windows wallpaper isn't changed. Same on every screen.", step(0, 80, 5, "%")),
    row("wallpaper.transition", "Transition", "Fade crossfades in 150 ms, also to and from your Windows wallpaper. Same on every screen.", Kind::Choice(TRANSITION_WORDS)),
    // Spec 0028 AC-11: after Transition, as approved in design r3.
    row("wallpaper.interval", "Interval", "How long each slideshow picture shows, unless its workspace has its own time. A slideshow moves only while you can see it, and each change fades. Same on every screen.", Kind::Ladder(LADDER)),
    row("wallpaper.shuffle", "Shuffle", "Shows a slideshow's pictures in a random order instead of by name. Same on every screen.", Kind::Toggle),
];

/// The workspace count: drawn as the tiles, not as a row of its own (AC-SET-3).
pub(crate) const WORKSPACES: Row = row("workspaces", "Workspaces per screen", "", step(2, 9, 1, ""));

pub(crate) static DEFAULTS: LazyLock<Settings> = LazyLock::new(Settings::default);

pub(crate) fn find_row(key: &str) -> Option<&'static Row> {
    ROWS.iter().chain([&WORKSPACES]).find(|r| r.key == key)
}

fn word<T: Copy + PartialEq>(list: &[(&'static str, T)], v: T) -> &'static str {
    list.iter().find(|(_, x)| *x == v).map_or("", |(w, _)| w)
}

/// A row's value as a number: on and off as 1 and 0, a choice as its index.
pub(crate) fn value(s: &Settings, key: &str) -> i64 {
    let index = |w: &str, words: &[(&str, &str)]| words.iter().position(|(x, _)| *x == w).unwrap_or(0) as i64;
    match key {
        "workspaces" => s.workspaces as i64,
        "follow_activation" => s.follow_activation as i64,
        "jump_back" => s.jump_back as i64,
        "copilot_as_right_ctrl" => s.copilot_as_right_ctrl as i64,
        "fastest" => s.fastest as i64,
        "indicator.enabled" => s.indicator_enabled as i64,
        "indicator.style" => index(word(settings::STYLES, s.indicator_style), STYLE_WORDS),
        "indicator.hold_ms" => s.hold_ms as i64,
        "indicator.fade_ms" => s.fade_ms as i64,
        "indicator.screen_name" => s.indicator_screen_name as i64,
        "attention.enabled" => s.attention_enabled as i64,
        "restore.enabled" => s.restore_enabled as i64,
        "restore.minutes" => s.restore_minutes as i64,
        "overview.labels" => index(word(settings::LABELS, s.overview_labels), LABEL_WORDS),
        "overview.animation" => index(word(settings::ANIMATIONS, s.overview_animation), ANIMATION_WORDS),
        "overview.fade_ms" => s.overview_fade_ms as i64,
        "overview.live" => s.overview_live as i64,
        "touchpad.enabled" => s.touchpad_enabled as i64,
        "touchpad.swipe_mm" => s.swipe_mm as i64,
        "touchpad.twice_ms" => s.twice_ms as i64,
        "wallpaper.fit" => index(word(settings::FITS, s.wallpaper_fit), FIT_WORDS),
        "wallpaper.dim" => s.wallpaper_dim as i64,
        "wallpaper.transition" => s.wallpaper_fade as i64,
        "wallpaper.interval" => s.wallpaper_interval as i64,
        "wallpaper.shuffle" => s.wallpaper_shuffle as i64,
        _ => 0,
    }
}

pub(crate) fn file_value(row: &Row, v: i64) -> toml_edit::Value {
    match row.kind {
        Kind::Toggle | Kind::Pair(..) => (v != 0).into(),
        Kind::Step { .. } | Kind::Ladder(_) => v.into(),
        Kind::Choice(words) => words[v as usize].0.into(),
    }
}

/// A value in the page's words: "550 ms", "on", "des-k", "On hover".
pub(crate) fn shown(row: &Row, v: i64) -> String {
    match row.kind {
        Kind::Toggle => if v != 0 { "on" } else { "off" }.into(),
        Kind::Pair(on, off) => if v != 0 { on } else { off }.into(),
        Kind::Step { unit: "", .. } => v.to_string(),
        // Dim pictures: "Off" at 0, else "40%" (spec 0026 AC-18).
        Kind::Step { unit: "%", .. } if v == 0 => "Off".into(),
        Kind::Step { unit: "%", .. } => format!("{v}%"),
        Kind::Step { unit, .. } => format!("{v} {unit}"),
        Kind::Choice(words) => words.get(v as usize).map_or("", |(_, l)| l).into(),
        Kind::Ladder(_) => minutes(v),
    }
}

/// Minutes in the page's words: "45 min", "1 h 30 min", "24 h" (spec 0028 AC-11).
pub(crate) fn minutes(m: i64) -> String {
    match m {
        m if m < 60 => format!("{m} min"),
        m if m % 60 != 0 => format!("{} h {} min", m / 60, m % 60),
        m => format!("{} h", m / 60),
    }
}

/// The next value on a ladder in that direction; a hand typed value off it steps onto it, and
/// the ends stay put.
pub(crate) fn ladder_step(ladder: &[i64], v: i64, up: bool) -> i64 {
    let next = if up { ladder.iter().find(|&&x| x > v) } else { ladder.iter().rev().find(|&&x| x < v) };
    next.copied().unwrap_or(v)
}

/// The next value on the step grid. A hand written value off the grid steps onto it.
pub(crate) fn stepped(v: i64, lo: i64, hi: i64, by: i64, up: bool) -> i64 {
    let off = v.rem_euclid(by);
    let n = if up { v - off + by } else if off != 0 { v - off } else { v - by };
    n.clamp(lo, hi)
}

/// Why a kept value is not in use now (AC-SET-8): every reason, in the caption's order. The
/// Style and Show screen name rows grey with the indicator and need no words: the switch is
/// right above them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Why {
    AnimationNone,
    Skip,
    IndicatorOff,
    Dots, // the Dots style shows no names (spec 0023 AC-14)
}

pub(crate) fn unused(s: &Settings, key: &str) -> Vec<Why> {
    match key {
        "indicator.style" | "indicator.screen_name" if !s.indicator_enabled => vec![Why::IndicatorOff],
        "indicator.screen_name" if s.indicator_style == settings::Style::Dots => vec![Why::Dots],
        "indicator.fade_ms" | "wallpaper.transition" if s.fastest => vec![Why::Skip],
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
pub(crate) enum WinKey {
    Combo(Combo),
    Digits(u8),
}

pub(crate) struct Short {
    pub(crate) key: &'static str,
    pub(crate) title: &'static str,
    pub(crate) win: Option<(WinKey, &'static str)>, // and what it does in Windows
}

const fn combo(mods: u8, vk: u16) -> WinKey {
    WinKey::Combo(Combo { mods, vk })
}

const fn short(key: &'static str, title: &'static str, win: Option<(WinKey, &'static str)>) -> Short {
    Short { key, title, win }
}

use settings::{ALT, CTRL, SHIFT, WIN};

pub(crate) const MOVE: &[Short] = &[
    short("shortcuts.previous", "Previous workspace", Some((combo(WIN | CTRL, 0x25), "switches desktops"))),
    short("shortcuts.next", "Next workspace", Some((combo(WIN | CTRL, 0x27), "switches desktops"))),
    short("shortcuts.jump", "Go to workspace 1–9", Some((WinKey::Digits(WIN), "opens taskbar app 1–9"))),
    short("shortcuts.back", "Back to the last workspace", None),
    short("shortcuts.overview", "Open overview", Some((combo(WIN, 0x09), "opens Task View"))),
    short("shortcuts.finder", "Find a window", Some((combo(WIN, 0x46), "opens Feedback Hub"))),
    short("shortcuts.hints", "Open shortcut hints", None),
];

pub(crate) const SEND: &[Short] = &[
    short("shortcuts.send_previous", "Send window to previous workspace", None),
    short("shortcuts.send_next", "Send window to next workspace", None),
    short("shortcuts.send_previous_stay", "Send window to previous workspace and stay", None),
    short("shortcuts.send_next_stay", "Send window to next workspace and stay", None),
    short("shortcuts.send_to", "Send window to 1–9", Some((WinKey::Digits(WIN | SHIFT), "opens a new window of taskbar app 1–9"))),
    short("shortcuts.send_to_stay", "Send window to 1–9 and stay", Some((WinKey::Digits(WIN | CTRL), "goes to the last window of taskbar app 1–9"))),
    short("shortcuts.pin", "Pin window to all workspaces", None),
];

/// A [shortcuts] key's name: "shortcuts.finder" → "finder".
pub(crate) fn name_of(key: &str) -> &str {
    key.strip_prefix("shortcuts.").unwrap_or(key)
}

pub(crate) fn is_digits(key: &str) -> bool {
    settings::SETS.contains(&name_of(key))
}

pub(crate) fn shortcut(key: &str) -> Option<&'static Short> {
    MOVE.iter().chain(SEND).find(|s| s.key == key || name_of(s.key) == key)
}

/// An action by its name in [shortcuts]: its page key and title.
pub(crate) fn entry(name: &str) -> (&'static str, &'static str) {
    shortcut(name).map_or(("", ""), |s| (s.key, s.title))
}

/// A key in words, as its caps read: "Win Ctrl ←".
pub(crate) fn spelled(caps: &[String]) -> String {
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

pub(crate) fn digit_caps(mods: u8) -> Vec<String> {
    crate::hints::mods_caps(mods).into_iter().chain(["1–9".to_string()]).collect()
}

/// An action's keys as their caps, in the file's order, each with its place in the file.
pub(crate) fn keys_of(s: &Settings, key: &str) -> Vec<Vec<String>> {
    let name = name_of(key);
    if is_digits(key) {
        return s.set(name).map(digit_caps).into_iter().collect();
    }
    s.list(name).iter().map(crate::hints::caps).collect()
}

/// The order the row draws its keys: Windows' own key first (AC-SET-4), then the file's order.
/// Each item is its index in the file, so × and a recording act on the right one.
pub(crate) fn drawn_order(s: &Settings, sh: &Short) -> Vec<usize> {
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
pub(crate) fn has_win(s: &Settings, sh: &Short) -> bool {
    match sh.win {
        Some((WinKey::Combo(c), _)) => s.list(name_of(sh.key)).contains(&c),
        Some((WinKey::Digits(m), _)) => s.set(name_of(sh.key)) == Some(m),
        None => false,
    }
}

/// Another action holds this one's Windows key: its title.
pub(crate) fn took(s: &Settings, sh: &Short) -> Option<&'static str> {
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

// ---- Colors and Where it shows (spec 0020) ----

pub(crate) const ACCENT: &str = "colors.accent";
pub(crate) const WS_KEYS: [&str; 9] = ["colors.ws1", "colors.ws2", "colors.ws3", "colors.ws4", "colors.ws5", "colors.ws6", "colors.ws7", "colors.ws8", "colors.ws9"];
/// des-k's own workspace colors by name, as a default row says them.
const WS_NAMES: [&str; 9] = ["blue", "gold", "green", "violet", "orange", "cyan", "pink", "taupe", "red"];

// The place rows' keys; MODE is the page's Spots or Custom, never a key in the file.
pub(crate) const SPOT: &str = "indicator.spot";
const WORDS: &str = "attention.text";
const WORDS_W: f32 = 300.0;
const NAME_W: f32 = 300.0; // a name box (spec 0023)
// The popup preview's sample (spec 0022): an app everyone knows, on workspace 3.
const SAMPLE_APP: &str = "Slack";
const SAMPLE_WS: usize = 2;
pub(crate) const GAP: &str = "indicator.gap";
pub(crate) const ACROSS: &str = "indicator.x";
pub(crate) const DOWN: &str = "indicator.y";
const MODE: &str = "indicator.mode";
const SPOT_NAMES: [&str; 10] = ["Top left", "Top", "Top right", "Left", "Center", "Right", "Bottom left", "Bottom", "Bottom right", "Custom"];

fn is_place(key: &str) -> bool {
    matches!(key, SPOT | GAP | ACROSS | DOWN | MODE)
}

/// A problem a row of Colors or Where it shows shows itself, so no section note repeats it.
fn shown_in_row(key: &str) -> bool {
    key == ACCENT || WS_KEYS.contains(&key) || is_place(key) || (key.starts_with("indicator.screens.\"") && key.matches('"').count() == 2)
}

fn ws_index(key: &str) -> Option<usize> {
    WS_KEYS.iter().position(|k| *k == key)
}

fn path_of(key: &str) -> Vec<String> {
    key.split('.').map(String::from).collect()
}

/// What a hex box saves (AC-12): a workspace's own des-k color takes its key out, so the file
/// only holds what differs.
pub(crate) fn color_edit(key: &'static str, c: u32) -> Edit {
    match ws_index(key) {
        Some(i) if c == ui::WS[i] => Edit::Remove(path_of(key)),
        _ => Edit::At(path_of(key), format!("#{c:06X}").into()),
    }
}

/// Six hex digits as a color.
fn hex_color(digits: &str) -> Option<u32> {
    (digits.len() == 6).then(|| u32::from_str_radix(digits, 16).ok()).flatten()
}

/// What a paste puts in a hex box: exactly six hex digits once spaces and a leading # are
/// trimmed, in upper case; anything else is refused.
pub(crate) fn pasted(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t).trim();
    (t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit())).then(|| t.to_ascii_uppercase())
}

fn spot_index(spot: settings::Spot) -> usize {
    settings::SPOTS.iter().position(|(_, s)| *s == spot).unwrap_or(4)
}

/// A place key's name inside a table: "spot", "gap", "x" or "y".
fn leaf(key: &str) -> &str {
    key.rsplit('.').next().unwrap_or(key)
}

/// A place key's value as a number: a spot by its place in SPOTS.
fn pos_get(p: settings::Pos, key: &str) -> f64 {
    match leaf(key) {
        "spot" => spot_index(p.spot) as f64,
        "gap" => p.gap as f64,
        "x" => p.x,
        _ => p.y,
    }
}

fn ov_get(o: &settings::PosOverride, key: &str) -> Option<f64> {
    match leaf(key) {
        "spot" => o.spot.map(|s| spot_index(s) as f64),
        "gap" => o.gap.map(f64::from),
        "x" => o.x,
        _ => o.y,
    }
}

fn place_value(key: &str, v: f64) -> toml_edit::Value {
    match leaf(key) {
        "spot" => settings::SPOTS[v as usize].0.into(),
        _ if v.fract() == 0.0 => (v as i64).into(),
        _ => v.into(),
    }
}

/// The place a screen uses: its own keys over All screens' (spec 0020 AC-9).
fn pos_of(s: &Settings, screen: Option<&str>) -> settings::Pos {
    let d = s.indicator_pos;
    screen.and_then(|id| s.indicator_screens.get(id)).map_or(d, |o| o.over(d))
}

/// What setting a place key to `v` writes, None when nothing changes. All screens writes
/// [indicator]. A screen writes its own table, but a value equal to All screens' takes that
/// key out instead, so the screen follows All screens again; never the table's last key, so
/// walking the spots or stepping a number can't drop a kept place.
pub(crate) fn place_edit(s: &Settings, screen: Option<&str>, key: &str, v: f64) -> Option<Edit> {
    if pos_get(pos_of(s, screen), key) == v && screen.is_none_or(|id| s.indicator_screens.get(id).is_none_or(|o| ov_get(o, key).is_some())) {
        return None;
    }
    let Some(id) = screen else { return Some(Edit::At(path_of(key), place_value(key, v))) };
    let path = vec!["indicator".to_string(), "screens".into(), id.to_string(), leaf(key).to_string()];
    let all = pos_get(s.indicator_pos, key);
    let o = s.indicator_screens.get(id);
    let own = o.and_then(|o| ov_get(o, key));
    let keys = o.map_or(0, |o| [o.spot.is_some(), o.gap.is_some(), o.x.is_some(), o.y.is_some()].iter().filter(|b| **b).count());
    match () {
        _ if v != all => Some(Edit::At(path, place_value(key, v))),
        _ if own.is_none() => None,  // it follows All screens already
        _ if keys > 1 => Some(Edit::Remove(path)),
        _ if own == Some(v) => None, // its last key, kept
        _ => Some(Edit::At(path, place_value(key, v))),
    }
}

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
    let text = settings::clean_name(text);
    let path = name_path(screen, row)?;
    let (state, saved) = name_held(s, screen, row);
    match (state, row) {
        (NameState::Follow, Nm::Ws(i)) if text.is_empty() => (enter && !s.names[i].is_empty()).then(|| Edit::At(path, "".into())),
        (NameState::NoName, _) if text.is_empty() => None,
        _ if text.is_empty() => (!saved.is_empty()).then_some(Edit::Remove(path)),
        _ => (text != saved).then(|| Edit::At(path, text.into())),
    }
}

// ---- Wallpaper (spec 0026 AC-13, AC-15) ----

/// How a Wallpaper row reads, set by the file's keys alone, as a Names row does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum WallState {
    All,     // an All screens row: [wallpaper] wsN
    Follow,  // a screen's row with no key of its own: it shows All screens' picture
    Own,     // a screen's own picture, even one equal to All screens'
    Windows, // "" on this screen: your Windows wallpaper there
}

/// Row `k` for `screen` (None: All screens): its state and the value it shows, `Pick::None`
/// for your Windows wallpaper.
pub(crate) fn wall_held(s: &Settings, screen: Option<&str>, k: usize) -> (WallState, Pick) {
    let all = s.wallpaper[k].clone();
    let Some(id) = screen else { return (WallState::All, all) };
    match s.wallpaper_screens.get(id).and_then(|a| a[k].clone()) {
        None => (WallState::Follow, all),
        Some(Pick::None) => (WallState::Windows, Pick::None),
        Some(p) => (WallState::Own, p),
    }
}

/// A value's name on its menu button and in captions: the picture or folder, or "3 pictures"
/// for a set (spec 0028 AC-11); "" for none.
fn wall_label(p: &Pick) -> String {
    match p {
        Pick::None => String::new(),
        Pick::Picture(f) | Pick::Folder(f) => f.clone(),
        Pick::Set(v) => format!("{} pictures", v.len()),
    }
}

/// Why a value can't show, in its row's words: a set says it of its pictures (spec 0028 AC-8).
fn wall_why(p: &Pick, problem: crate::wallpaper::Problem) -> &'static str {
    use crate::wallpaper::Problem;
    match (p, problem) {
        (Pick::Set(_), Problem::Missing) => "none are in the folder",
        (Pick::Set(_), Problem::Unreadable) => "none can be read",
        _ => problem.why(),
    }
}

/// A value as the file holds it: a name, or a list for a set (spec 0028 AC-14).
fn wall_value(p: &Pick) -> toml_edit::Value {
    match p {
        Pick::None => "".into(),
        Pick::Picture(f) | Pick::Folder(f) => f.as_str().into(),
        Pick::Set(v) => {
            let mut list: toml_edit::Array = v.iter().map(String::as_str).collect();
            list.fmt();
            list.into()
        }
    }
}

fn wall_path(screen: Option<&str>, k: usize) -> Vec<String> {
    let key = format!("ws{}", k + 1);
    match screen {
        Some(id) => vec!["wallpaper".into(), "screens".into(), id.into(), key],
        None => vec!["wallpaper".into(), key],
    }
}

/// What Use this there too writes for row `k`: each plugged in screen's own value taken out,
/// in one write, so those screens follow All screens again. A screen not plugged in keeps its
/// own, since you can't see what it would lose.
fn wall_use_edit(s: &Settings, present: &[String], k: usize) -> Option<Edit> {
    present
        .iter()
        .filter(|id| s.wallpaper_screens.get(*id).is_some_and(|o| o[k].is_some()))
        .map(|id| Edit::Remove(wall_path(Some(id), k)))
        .reduce(|a, b| Edit::Pair(Box::new(a), Box::new(b)))
}

/// What choosing `pick` (`Pick::None` for Windows wallpaper) in row `k` writes, if anything
/// (spec 0026 AC-15, spec 0028 AC-11). What the row already follows takes its key out; on All
/// screens Windows wallpaper is no key. Values compare by their key, so a set by its names.
/// Windows wallpaper on All screens is a reset (amended 2026-11-22): every screen's own value
/// for the row goes too, plugged in or not, so no screen keeps a picture there.
pub(crate) fn wall_edit(s: &Settings, screen: Option<&str>, k: usize, pick: &Pick) -> Option<Edit> {
    let path = wall_path(screen, k);
    let (all, key) = (s.wallpaper[k].key(), pick.key());
    let Some(id) = screen else {
        return match pick {
            Pick::None => {
                let own = s.wallpaper_screens.iter().filter(|(_, o)| o[k].is_some()).map(|(id, _)| Edit::Remove(wall_path(Some(id), k)));
                (!all.is_empty()).then_some(Edit::Remove(path)).into_iter().chain(own).reduce(|a, b| Edit::Pair(Box::new(a), Box::new(b)))
            }
            _ => (key != all).then(|| Edit::At(path, wall_value(pick))),
        };
    };
    let own = s.wallpaper_screens.get(id).and_then(|a| a[k].as_ref().map(Pick::key));
    match own {
        _ if key == all => own.is_some().then_some(Edit::Remove(path)),
        Some(o) if o == key => None,
        _ => Some(Edit::At(path, wall_value(pick))),
    }
}

/// What ticking or unticking `file` in row `k`'s menu writes (spec 0028 AC-11): the row's
/// pictures with `file` added or taken out, sorted by name; one picture is a plain name, none is
/// Windows wallpaper. A row that follows all screens starts from all screens' value.
pub(crate) fn wall_tick(s: &Settings, screen: Option<&str>, k: usize, file: &str) -> Option<Edit> {
    let mut names = match wall_held(s, screen, k).1 {
        Pick::Picture(f) => vec![f],
        Pick::Set(v) => v,
        Pick::None | Pick::Folder(_) => vec![],
    };
    match names.iter().position(|n| n == file) {
        Some(i) => {
            names.remove(i);
        }
        None => names.push(file.to_string()),
    }
    names.sort_by(|a, b| settings::name_order(a, b));
    let pick = match names.len() {
        0 => Pick::None,
        1 => Pick::Picture(names.remove(0)),
        _ => Pick::Set(names),
    };
    wall_edit(s, screen, k, &pick)
}

/// What a step of row `k`'s time per picture writes (spec 0028 AC-11): the next ladder value as
/// `[wallpaper.intervals] wsN`, or the key taken out when that is the default, so the file never
/// keeps one that changes nothing.
pub(crate) fn wall_interval_edit(s: &Settings, k: usize, up: bool) -> Option<Edit> {
    let v = s.interval_for(k) as i64;
    let n = ladder_step(LADDER, v, up);
    let path = vec!["wallpaper".to_string(), "intervals".to_string(), format!("ws{}", k + 1)];
    match n {
        n if n == v => None,
        n if n == s.wallpaper_interval as i64 => s.wallpaper_intervals[k].is_some().then_some(Edit::Remove(path)),
        n => Some(Edit::At(path, n.into())),
    }
}

/// The screen a preview takes its shape and Windows wallpaper from: the chosen one while it is
/// plugged in, else the main display (the one at 0, 0), else the first.
fn wall_rect(v: &crate::ws::View, screen: Option<&str>) -> Option<RECT> {
    let main = |p: &&crate::ws::Present| p.full.left <= 0 && p.full.top <= 0 && p.full.right > 0 && p.full.bottom > 0;
    screen.and_then(|id| v.present.iter().find(|p| p.id == id)).or_else(|| v.present.iter().find(main)).or_else(|| v.present.first()).map(|p| p.full)
}

/// The screen whose slideshow a row's preview shows (spec 0028 AC-11): the chosen one while it
/// is plugged in, the main display for All screens, and None for a screen not plugged in.
fn wall_shown_id(v: &crate::ws::View, screen: Option<&str>) -> Option<String> {
    let main = |p: &&crate::ws::Present| p.full.left <= 0 && p.full.top <= 0 && p.full.right > 0 && p.full.bottom > 0;
    match screen {
        Some(id) => v.present.iter().find(|p| p.id == id).map(|p| p.id.clone()),
        None => v.present.iter().find(main).or_else(|| v.present.first()).map(|p| p.id.clone()),
    }
}

/// A preview's size in dip: the screen's shape inside 64 × 40.
fn wall_box(rect: Option<RECT>) -> (f32, f32) {
    let aspect = rect.map_or(1.6, |r| (r.right - r.left) as f32 / (r.bottom - r.top).max(1) as f32);
    if aspect >= 1.6 { (64.0, (64.0 / aspect).round()) } else { ((40.0 * aspect).round(), 40.0) }
}

/// A preview's size in pixels at `scale`, the size its picture is decoded at.
fn wall_px(rect: Option<RECT>, scale: f32) -> (i32, i32) {
    let (w, h) = wall_box(rect);
    ((w * scale).round() as i32, (h * scale).round() as i32)
}

/// The places a Wallpaper menu's list scrolls: its `n` pictures, then with `m` folders a
/// Slideshows heading and the folders (spec 0028 AC-11).
fn wall_total(n: usize, m: usize) -> usize {
    n + m + usize::from(m > 0)
}

/// Menu line `line` (0 Windows wallpaper, 1 to n pictures, then folders, then Open folder) as
/// a place in the list, counting the heading: the list's places are 1 to `wall_total`.
fn wall_slot(line: usize, n: usize, m: usize) -> usize {
    if line > n && m > 0 { line + 1 } else { line }
}

/// How many pictures a Wallpaper menu shows at once in a `vh` dip window: up to 10.
fn wall_rows(vh: f32, n: usize) -> usize {
    (((vh - 200.0) / 28.0).floor().max(3.0) as usize).min(10).min(n)
}

/// The first picture shown so menu line `line` (1 to n are the pictures) is in view.
fn wall_reveal(top: usize, line: usize, n: usize, rows: usize) -> usize {
    let top = match line {
        l if l == 0 || l > n => top,
        l if l - 1 < top => l - 1,
        l if l > top + rows => l - rows,
        _ => top,
    };
    top.min(n.saturating_sub(rows))
}

/// The wallpapers folder's pictures, by name with case ignored (AC-13, AC-14), read as a
/// row's menu opens. A missing folder lists none.
fn wall_folder() -> Vec<String> {
    wall_names(&crate::ws::data_dir().join("wallpapers"), true, settings::is_picture_name)
}

/// The folder's slideshow folders and their picture counts (spec 0028 AC-11, AC-13), read only
/// as a row's menu opens: subfolders whose names the file's reader takes as a folder.
fn wall_dirs() -> Vec<(String, usize)> {
    let dir = crate::ws::data_dir().join("wallpapers");
    wall_names(&dir, false, settings::is_folder_name)
        .into_iter()
        .map(|f| {
            let n = wall_count(&f);
            (f, n)
        })
        .collect()
}

/// How many pictures folder `name` holds at its top level; 0 when it isn't there.
fn wall_count(name: &str) -> usize {
    wall_names(&crate::ws::data_dir().join("wallpapers").join(name), true, settings::is_picture_name).len()
}

/// The names of `dir`'s files (or subfolders) that pass `keep`, in slideshow order.
fn wall_names(dir: &std::path::Path, files: bool, keep: fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| if files { t.is_file() } else { t.is_dir() }))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| keep(n))
        .collect();
    names.sort_by(|a, b| settings::name_order(a, b));
    names
}

/// What the Wallpaper card's previews need: the screen, the preview's size in pixels, and the
/// pictures its rows show, a slideshow's the one showing now (spec 0028 AC-11).
fn wall_wanted(w: &Win, s: &Settings) -> Option<(RECT, (i32, i32), Vec<String>)> {
    let screen = wall_screen(w, s);
    let rect = wall_rect(&w.pics.view, screen.as_deref())?;
    let mut files: Vec<String> = (0..s.workspaces).filter_map(|k| wall_shows(w, s, screen.as_deref(), k)).filter(|f| !f.is_empty()).collect();
    files.sort();
    files.dedup();
    Some((rect, wall_px(Some(rect), w.scale), files))
}

/// The picture row `k`'s preview shows: its picture, "" for your Windows wallpaper, and for a
/// slideshow the picture showing now on the row's screen. When that screen runs no such
/// slideshow (it has its own value, isn't plugged in, or hasn't chosen yet) a set shows its
/// first picture and a folder None, a folder tile, since naming one would mean reading it.
fn wall_shows(w: &Win, s: &Settings, screen: Option<&str>, k: usize) -> Option<String> {
    let pick = wall_held(s, screen, k).1;
    let now = || {
        let id = wall_shown_id(&w.pics.view, screen)?;
        // The main display may run its own slideshow for this workspace, not All screens'.
        (wall_held(s, Some(&id), k).1 == pick).then(|| w.pics.wall_now.get(&(id, k)).cloned())?
    };
    match pick {
        Pick::None => Some(String::new()),
        Pick::Picture(f) => Some(f),
        Pick::Folder(_) => now(),
        Pick::Set(ref v) => now().or_else(|| v.first().cloned()),
    }
}

/// The title a failed save's note names: "Couldn't save “Workspace 2 name”" (AC-13).
fn name_title(row: Nm) -> String {
    match row {
        Nm::Screen => "Screen name".into(),
        Nm::Ws(i) => format!("Workspace {} name", i + 1),
    }
}

// ---- What the page shows of des-k and Windows ----

/// What the recorder is recording for (spec 0012 AC-7, AC-12).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Rec {
    List(&'static str, Option<usize>), // a shortcut list, and the key it replaces (None: +)
    Digits(&'static str),
    Gesture(usize, u8), // a GESTURE_ROWS row and its times: Key combo…
}

/// A clash the recorder found, waiting for Move it here or Cancel. Nothing is saved until you choose.
pub(crate) struct Prompt {
    pub(crate) key: &'static str, // the row it shows under
    pub(crate) held: Vec<String>, // the caps of the key it found
    pub(crate) title: String,
    pub(crate) sub: String,
    pub(crate) edit: Edit,
}

/// Where a caption link goes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Link {
    Skip, // the Skip animations switch
    Indicator,
    Overview,
    Workspaces,
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
    WhatsNew, // spec 0029 AC-8
    Open,     // Open settings.toml
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Stepper(&'static str), // focus only: a number's − and + are one stop, Left and Right step
    Choice(&'static str, usize),
    Pick(&'static str), // focus only: a choice is one stop, Left and Right move
    Reset(&'static str),
    Link(Link),
    Tiles, // focus only: the workspace tiles, one stop for every screen
    // A plugged in screen's tile: its row in Pics::screens, its workspace. A click opens the color
    // picker (spec 0021); pointing at the last one shows its ×.
    WsTile(usize, usize),
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
    Hex(&'static str), // a color's hex box, by its key: ACCENT or one of WS_KEYS (spec 0020)
    Words,             // the attention text box (spec 0022)
    Screens,           // Where it shows' screen chooser: opens its menu
    ScreenItem(usize), // a line of the open screen menu
    SameAll,           // Same as all screens: takes out the chosen screen's table
    NameScreens,       // the Names card's screen chooser (spec 0023)
    Name(Nm),          // a name box
    NameSame(usize),   // a workspace row's Same as all screens: takes out the screen's key
    WallScreens,       // the Wallpaper card's screen chooser (spec 0026)
    WallPick(usize),   // a workspace row's picture menu
    WallSame(usize),   // a workspace row's Same as all screens: takes out the screen's key
    WallTick(usize),   // a picture's tick box in an open Wallpaper menu, by its line (spec 0028)
    WallUse(usize),    // an All screens row's Use this there too: takes out the plugged in screens' keys
    // The Apps section (spec 0027): Add, and a rule row's two menus and remove, by its place in
    // Settings::apps.
    AppAdd,
    AppWs(usize),
    AppScreen(usize),
    AppX(usize),
    Picture,           // focus only: the place picture is one stop; Left and Right move
    Spot(usize),       // a third of the picture across and down, 0 to 8 in reading order
    ShowThere,         // Show it there: the real indicator on the chosen screen
    // The color picker (spec 0021): a live tile of the Overview picture (its screen row, its
    // workspace), a Colors swatch by key, and the open picker's own parts.
    ShowTile(usize, usize),
    Swatch(&'static str),
    Preset(usize),
    Ring,
    Square,
    PickDone,
    PickCancel,
    PickCard, // inside the picker, between its parts
}

impl Ctl {
    fn key(self) -> Option<&'static str> {
        match self {
            Ctl::Toggle(k) | Ctl::Minus(k) | Ctl::Plus(k) | Ctl::Stepper(k) | Ctl::Choice(k, _) | Ctl::Pick(k) | Ctl::Reset(k) => Some(k),
            Ctl::KeyRow(k) | Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) | Ctl::StopRec(k) | Ctl::Move(k) | Ctl::Cancel(k) => Some(k),
            Ctl::Hex(k) => Some(k),
            Ctl::Words => Some(WORDS),
            Ctl::Picture | Ctl::Spot(_) => Some(SPOT),
            _ => None,
        }
    }

    /// Changes the file (or starts recording a change); a read only page still lets you focus
    /// it and follow links.
    fn writes(self) -> bool {
        matches!(
            self,
            Ctl::Toggle(_) | Ctl::Minus(_) | Ctl::Plus(_) | Ctl::Choice(..) | Ctl::Reset(_) | Ctl::WsTile(..) | Ctl::TileAdd | Ctl::TileRemove | Ctl::Forget(_)
                | Ctl::KeyRow(_) | Ctl::Key(..) | Ctl::KeyX(..) | Ctl::AddKey(_) | Ctl::WinSide(..) | Ctl::Move(_) | Ctl::Picker(_) | Ctl::Item(_) | Ctl::Pad(Pad::ReadOn | Pad::GiveBack)
                | Ctl::Hex(_) | Ctl::SameAll | Ctl::Picture | Ctl::Spot(_) | Ctl::ShowTile(..) | Ctl::Swatch(_) | Ctl::Words | Ctl::Name(_) | Ctl::NameSame(_)
                | Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_) | Ctl::AppX(_) | Ctl::WallPick(_) | Ctl::WallSame(_) | Ctl::WallTick(_) | Ctl::WallUse(_)
        )
    }

    /// The keyboard stop this control belongs to: one per row (spec 0012 AC-16).
    fn stop(self) -> Ctl {
        match self {
            Ctl::Minus(k) | Ctl::Plus(k) => Ctl::Stepper(k),
            Ctl::Choice(k, _) => Ctl::Pick(k),
            Ctl::WsTile(..) | Ctl::TileAdd | Ctl::TileRemove => Ctl::Tiles,
            Ctl::Spot(_) => Ctl::Picture,
            Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) => Ctl::KeyRow(k),
            c => c,
        }
    }

    fn in_side(self) -> bool {
        matches!(self, Ctl::Side(_) | Ctl::Welcome | Ctl::WhatsNew | Ctl::Open)
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

/// What the tiles, the preview and the touchpad draw, copied out of ws and Windows.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::ScreenInfo>,
    view: crate::ws::View,
    pad: Option<PadState>,
    back: Option<GaveBack>,
    here: Option<String>, // the device the page is on, for the indicator preview
    apps: Vec<crate::apps::Seen>, // apps with a window open now (spec 0027 AC-13)
    wall_failed: bool, // des-k can't draw behind the desktop icons (spec 0026 AC-12)
    wall_problems: Vec<(String, crate::wallpaper::Problem)>, // values in settings.toml that can't show, by key (AC-8)
    wall_now: BTreeMap<(String, usize), String>, // each slideshow's picture now, by screen id and workspace (spec 0028)
    wall_counts: Vec<(String, usize)>, // the folders settings.toml names, with their picture counts
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
    let apps = crate::apps::seen(&view.open);
    let s = settings::get();
    let mut files: Vec<String> = s.wallpaper.iter().chain(s.wallpaper_screens.values().flat_map(|a| a.iter().flatten())).map(Pick::key).filter(|f| !f.is_empty()).collect();
    files.sort();
    files.dedup();
    let wall_problems = files.into_iter().filter_map(|f| Some((f.clone(), crate::wallpaper::problem(&f)?))).collect();
    // Only folders a value names are read, for their rows' captions (spec 0028 AC-13).
    let mut wall_counts: Vec<(String, usize)> = s
        .wallpaper
        .iter()
        .chain(s.wallpaper_screens.values().flat_map(|a| a.iter().flatten()))
        .filter_map(|p| match p {
            Pick::Folder(f) => Some((f.clone(), wall_count(f))),
            _ => None,
        })
        .collect();
    wall_counts.sort();
    wall_counts.dedup();
    let wall_now = crate::backdrop::current_all();
    Some(Pics { screens, view, pad: Some(setup::state()), back: crate::welcome::gave_back(), here, apps, wall_failed: crate::backdrop::failed(), wall_problems, wall_now, wall_counts })
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
        wall_previews();
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
    let note = Some((format!("{:02}:{:02}", t.wHour, t.wMinute), lines));
    // While the color picker is open the page's notes hold still; it shows this one on close
    // (spec 0021 AC-13). The reload calls this before refresh, so a hand edit that also closes
    // the picker is held already.
    let _ = set(|w| match w.pick.as_mut() {
        Some(p) => p.held_edit = note,
        None => w.edited = note,
    });
}

/// One hand edit line in the page's words: "Indicator stays: 550 → 800 ms", "Open overview:
/// Win Tab → no key".
fn changed(key: &str, was: &str, now: &str) -> String {
    if key == "gestures" {
        return "Gestures: changed".into();
    }
    // The change line lists only which names are set; the page shows the names themselves.
    if key == "names" {
        return "Workspace names: changed".into();
    }
    if key == "wallpaper" {
        return "Wallpapers: changed".into();
    }
    // The Apps section shows the rules themselves; the fallback below would read them as keys.
    if key == "apps" {
        return "App rules: changed".into();
    }
    // A workspace's own time per picture, or "default" (spec 0028 AC-12).
    if let Some(n) = key.strip_prefix("wallpaper.intervals.ws") {
        let words = |v: &str| v.parse().map_or(v.to_string(), minutes);
        return format!("Workspace {n} time per picture: {} → {}", words(was), words(now));
    }
    let name = title_of(key).unwrap_or(key);
    let words = |v: &str| -> String {
        if let Some(r) = find_row(key) {
            return match r.kind {
                Kind::Toggle | Kind::Pair(..) => shown(r, (v == "true") as i64),
                Kind::Choice(list) => list.iter().find(|(w, _)| *w == v).map_or(v.to_string(), |(_, l)| l.to_string()),
                Kind::Step { .. } | Kind::Ladder(_) => v.parse().map_or(v.to_string(), |n| shown(r, n)),
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
    cards: Vec<(Card, f32)>,           // and each card What's new opens at (spec 0029 AC-4)
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
    hexes: Vec<Hex>,              // hex boxes holding digits not saved yet (spec 0020)
    own: Option<u32>,             // the last own accent while the page is open (AC-12)
    screen: Option<String>,       // the screen Where it shows changes, by id; None: All screens
    chooser: Option<(Ctl, Bx, usize)>, // the open screen menu: the chooser it opened from, where, its focused line
    dragging: Option<(f64, f64)>, // Custom: where a drag on the picture has the card, in percent
    pick: Option<ColorPick>,      // the open color picker (spec 0021)
    geo: Geo,                     // where its anchors were last drawn
    ease: Option<Ease>,           // the page scrolling on its own, for the picker
    down_at: (i32, i32),          // the last press, in screen pixels, for the picker's guard
    words: Option<String>,        // the popup words as typed, before they save (spec 0022)
    names_screen: Option<String>, // the screen the Names card shows, by id; None: All screens (spec 0023)
    typed: Option<String>,        // the name box with focus, as typed
    unsaved: Vec<(Option<String>, Nm, String)>, // name boxes whose save failed: screen, row, your text (AC-13)
    wall_screen: Option<String>, // the screen the Wallpaper card shows, by id; None: All screens (spec 0026)
    wall_files: Vec<String>,     // the wallpapers folder's pictures, read as a row's menu opened
    wall_dirs: Vec<(String, usize)>, // its slideshow folders and their picture counts (spec 0028)
    wall_top: usize,             // the first picture that menu shows when they don't all fit
    wall_waiting: bool,          // a preview was still being decoded at the last tick
}

/// The color picker while it is open (spec 0021): one key, the color shown, and what it saved.
struct ColorPick {
    key: &'static str,      // ACCENT or one of WS_KEYS
    from: Option<Ctl>,      // the swatch that opened it, for focus on close; None for a tile
    anchor: Anchor,         // what was clicked
    before: Option<toml_edit::Value>, // the key's raw value in the file when it opened; None: no key
    start: u32,             // the color shown when it opened, for the header's start chip and hex
    file: u32,              // the color the file is known to hold: the start, then each write that worked
    color: u32,             // the color shown: the source of truth
    hsv: ui::Hsv,           // where the ring and square sit
    preview: Option<Theme>, // the page's theme with `color` in place, while the page shows otherwise
    written: Option<u32>,   // the value of the write in flight only: set before the call, cleared after
    failed: Option<String>, // Windows' words from the last failed write, while Not saved shows
    saved: bool,            // a write from this picker worked
    held_edit: Option<(String, Vec<String>)>, // a hand edit's note that arrived while open
    placed: Option<Place>,  // the card's window position and the keep set it was placed with
    scroll0: f32,           // the page's scroll when it opened, for the way back (AC-14)
    guard: Option<Guard>,   // the opening click: presses close to it are ignored for a moment (AC-15)
    focus: Stop,
    last_swatch: usize,     // the swatch Tab comes back to
    drag: Option<Part>,     // Ring or Square while the mouse is down
    held: Option<VIRTUAL_KEY>, // the arrow held down: its key up saves
}

/// What the picker was opened from, found again in `Geo` after every layout.
#[derive(Clone, PartialEq, Debug)]
enum Anchor {
    Tile(String, usize), // a Workspaces tile: its screen's device and its workspace
    Show(String, usize), // an Overview picture tile
    Swatch(&'static str),
}

/// The picker's keyboard stops, in Tab order (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Stop {
    Swatches(usize),
    Ring,
    Square,
    Done,
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    Ring,
    Square,
}

/// The opening click: where its button went down, in screen pixels, and when presses near it
/// count again.
#[derive(Clone, Copy, Debug)]
struct Guard {
    at: (i32, i32),
    until: Instant,
}

/// The page scrolling to `to` on its own (AC-14): to make room for the picker (`opening`: the
/// card shows when it ends) or back to where it was.
#[derive(Clone, Copy, Debug)]
struct Ease {
    from: f32,
    to: f32,
    start: Instant,
    ms: u32,
    opening: bool,
}

/// Where the picker's anchors and what it keeps clear were drawn, in window dip.
#[derive(Clone, Default, Debug)]
struct Geo {
    rows: Vec<(String, Bx)>,             // each plugged in screen's tile row, by device
    tiles: Vec<(String, usize, Bx)>,     // each of their tiles
    band: Option<Bx>,                    // the Overview picture
    groups: Vec<(String, Bx)>,           // a screen's four tiles in it
    shows: Vec<(String, usize, Bx)>,     // its live tiles
    swatches: Vec<(&'static str, Bx)>,
    legend: Option<Bx>,                  // the Colors sample's legend, kept clear for Your accent
}

/// A hex box being typed in: its digits, the caret and the other end of the selection, and
/// the line under its row.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Hex {
    pub(crate) key: &'static str,
    pub(crate) digits: String,
    pub(crate) caret: usize,
    pub(crate) anchor: usize,
    pub(crate) line: Option<&'static str>,
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
    cards: Vec<(Card, f32)>, // where each card What's new opens at starts, like tops
    said: Vec<(Ctl, Node)>, // what Narrator reads for each stop, in page order
    geo: Geo,
}

/// A piece of a caption: words, words in the text color (a reason), a link, or a warning with
/// its mark, in the warning color.
enum P {
    T(String),
    Why(String),
    Link(String, Ctl),
    Mono(String),
    Warn(String),
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
        let (hits, tops, said, geo) = (self.out.hits.len(), self.out.tops.len(), self.out.said.len(), self.out.geo.clone());
        let cards = self.out.cards.len();
        let h = f(self);
        self.out.hits.truncate(hits);
        self.out.tops.truncate(tops);
        self.out.cards.truncate(cards);
        self.out.said.truncate(said);
        self.out.geo = geo;
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

    /// A click on a tile or swatch opens the color picker: not in high contrast or read only.
    fn picks(&self) -> bool {
        !self.c.t.hc && !self.ro
    }

    /// The pointer is on the last workspace's tile, on any screen: its × shows.
    /// The hover cue on a tile or swatch that opens the picker: an inset 1.5 dip edge in `ink`.
    fn cue(&self, (x, y, w, h): Bx, ink: u32) {
        self.c.stroke_rr((x + 0.75, y + 0.75, w - 1.5, h - 1.5), 3.25, 1.5, ui::alpha(ink, 0.6));
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

    /// `na`: greyed, kept but not in use, drawn as read only.
    fn switch(&mut self, key: &'static str, x: f32, y: f32, on: bool, na: bool) {
        let b = self.c.t.b;
        let ctl = Ctl::Toggle(key);
        let label = if on { "On" } else { "Off" };
        let lw = self.c.width(label, F::Body);
        self.c.text(x, y, 20.0, label, F::Body, b.tx2);
        let (sx, ro) = (x + lw + 10.0, self.ro || na);
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
    /// its − and + do nothing. `grey`: a value that follows a default, still live (spec 0028).
    #[allow(clippy::too_many_arguments)]
    fn stepper(&mut self, key: &'static str, x: f32, y: f32, (v, unit): (i64, &str), (down, up): (bool, bool), na: bool, grey: bool) {
        let b = self.c.t.b;
        let (num, unit) = match unit {
            MINUTES => (minutes(v), String::new()),
            "%" if v == 0 => ("Off".to_string(), String::new()),
            "" | "%" => (v.to_string(), unit.to_string()),
            u => (v.to_string(), format!(" {u}")),
        };
        let (nw, uw) = (self.c.width(&num, F::Body), self.c.width(&unit, F::Small));
        let w = 64.0 + (nw + uw + 16.0).max(56.0);
        self.field((x, y, w, 32.0), b.card, None);
        let mid = w - 64.0;
        let tx = x + 32.0 + (mid - nw - uw) / 2.0;
        let off = na || self.ro;
        self.c.text(tx, y, 32.0, &num, F::Body, if off { b.tx3 } else if grey { b.tx2 } else { b.tx });
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
        // A warning is two runs, its mark and its words; `at` is each piece's first run.
        let (mut runs, mut at): (Vec<(&str, F, u32)>, Vec<usize>) = (vec![], vec![]);
        for p in pieces {
            at.push(runs.len());
            match p {
                P::T(s) => runs.push((s.as_str(), F::Small, b.tx2)),
                P::Why(s) => runs.push((s.as_str(), F::Small, b.tx)),
                P::Link(s, _) => runs.push((s.as_str(), F::SmallBold, b.link)),
                P::Mono(s) => runs.push((s.as_str(), F::Mono, b.tx2)),
                P::Warn(s) => {
                    runs.push((I_WARN, F::IconTiny, b.warn));
                    runs.push((s.as_str(), F::Small, b.warn));
                }
            }
        }
        let (h, boxes) = self.c.para_boxes(x, y, w, 16.0, &runs);
        for (p, &i) in pieces.iter().zip(&at) {
            let P::Link(text, ctl) = p else { continue };
            let bxs = &boxes[i];
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
        let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.09) };
        self.c.fill_rr((X0 + 1.0, y + 1.0, self.cw - 2.0, 36.0), 7.0, op(tint));
        self.c.fill_rr((X0 + 1.0, y + 20.0, self.cw - 2.0, 17.0), 0.0, op(tint));
        self.c.rule(X0 + 1.0, y + 37.0, self.cw - 2.0, op(b.line));
        let mut x = X0 + 16.0;
        if let Some(n) = n {
            self.c.circle(x + 11.0, y + 19.0, 11.0, Some(op(b.acc_b)), None);
            let d = n.to_string();
            let dw = self.c.width(&d, F::SmallBold);
            self.c.text(x + 11.0 - dw / 2.0, y + 8.0, 22.0, &d, F::SmallBold, b.on_acc_b);
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
        // No title: the caption alone, on the icon's line (a hex box's typing line).
        let mut h = if n.title.is_empty() { 2.0 } else { self.c.para(tx, y, tw, 20.0, &[(&n.title, F::Bold, b.tx)]) };
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
            // What the card shows (spec 0023 AC-14).
            "indicator.style" if v == 0 && self.s.indicator_screen_name => "The number and a bar per workspace, with its name and, with two or more screens, the screen's name.",
            "indicator.style" if v == 0 => "The number and a bar per workspace, with its name.",
            "indicator.style" => "A dot per workspace, the current one wide.",
            "copilot_as_right_ctrl" if v != 0 => "So Win Ctrl keys work with it. Taken from Windows, where it opens Copilot, or what you set.",
            "copilot_as_right_ctrl" => "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl.",
            // With Skip animations a change is instant (spec 0028 AC-6), so no fade is promised.
            "wallpaper.interval" if self.s.fastest => "How long each slideshow picture shows, unless its workspace has its own time. A slideshow moves only while you can see it. Same on every screen.",
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
                    Why::Dots => cap.push(P::Why("the dots show no names".into())),
                    Why::IndicatorOff => {}
                }
            }
            cap.push(P::Why(".".into()));
        }
        cap.extend(self.default_pieces(key, note.is_some()));
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str() }).collect::<String>().trim().to_string();
        let ro = self.ro;
        match r.kind {
            Kind::Toggle => {
                let size = (self.switch_w(v != 0), 20.0);
                self.say(Ctl::Toggle(key), UIA_ButtonControlTypeId, r.title, &help, ro || na, (Some(v != 0), None));
                self.row(y, r.title, &cap, size, |c, x, y| c.switch(key, x, y, v != 0, na), note.as_ref())
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
                let text = shown(r, v);
                let size = (self.stepper_w(&text), 32.0);
                let spoken = match unit {
                    "%" if v == 0 => "Off".to_string(),
                    "%" => format!("{v} percent"),
                    _ => format!("{v} {}", if unit == "ms" { "milliseconds" } else if unit == "min" { "minutes" } else if unit == "mm" { "millimetres" } else { "" }).trim().to_string(),
                };
                self.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, r.title, &help, ro || na, (None, Some(spoken)));
                self.row(y, r.title, &cap, size, |c, x, y| c.stepper(key, x, y, (v, unit), (v > lo, v < hi), na, false), note.as_ref())
            }
            Kind::Ladder(ladder) => {
                let text = shown(r, v);
                let size = (self.stepper_w(&text), 32.0);
                self.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, r.title, &help, ro || na, (None, Some(text)));
                let (lo, hi) = (ladder[0], ladder[ladder.len() - 1]);
                self.row(y, r.title, &cap, size, |c, x, y| c.stepper(key, x, y, (v, MINUTES), (v > lo, v < hi), na, false), note.as_ref())
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
        // The color picker's way in, said only when a click works (spec 0021 AC-1, AC-2).
        let live = self.picks() && !self.w.pics.screens.is_empty();
        let more = match sec {
            Some(Section::Workspaces) if live => " Click a tile to change its color.",
            Some(Section::Overview) if live => " Click a workspace in the picture to change its color.",
            _ => "",
        };
        let line = format!("{line}{more}");
        let h = 30.0 + self.c.para(X0, y + 30.0, self.cw, 20.0, &[(&line, F::Body, b.tx2)]);
        h + 12.0
    }

    /// The purple chip after a heading.
    fn chip(&mut self, x: f32, y: f32, icon: &str, text: &str) {
        let b = self.c.t.b;
        let w = 10.0 + 12.0 + 6.0 + self.c.width(text, F::Small) + 10.0;
        let (fill, edge, ic) = if self.c.t.hc { (b.card, b.tx, b.tx) } else { (mix(b.card, b.acc_b, 0.10), mix(b.card, b.acc_b, 0.30), b.acc_b) };
        self.c.boxed((x, y, w, 24.0), 12.0, fill, Some(edge));
        self.icon(x + 10.0, y, 24.0, icon, F::IconSmall, ic);
        self.c.text(x + 28.0, y, 24.0, text, F::Small, b.tx);
    }

    /// This section's problems no row owns (a key des-k turned off), above its cards.
    fn section_notes(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = 0.0;
        let owned = |k: &str| find_row(k).is_some() || shortcut(k).is_some() || k == "workspaces" || shown_in_row(k);
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
            Section::Workspaces => {
                let first = self.card(y + h, |c, y| c.workspaces(y)) + 12.0;
                let second = self.card(y + h + first, |c, y| c.names(y)) + 12.0;
                first + second + self.card(y + h + first + second, |c, y| c.wallpaper(y))
            }
            Section::Apps => {
                let first = self.card(y + h, |c, y| c.app_add(y)) + 12.0;
                first + self.card(y + h + first, |c, y| c.app_rules(y))
            }
            Section::Shortcuts => self.shortcuts(y + h),
            Section::Restore => self.card(y + h, |c, y| c.restore(y)),
            Section::Indicator => {
                let first = self.card(y + h, |c, y| c.indicator(y)) + 12.0;
                first + self.card(y + h + first, |c, y| c.place(y))
            }
            Section::Attention => self.card(y + h, |c, y| c.attention(y)),
            Section::Colors => self.colors(y + h),
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

    // ---- Names (spec 0023 AC-12, AC-13) ----

    /// The Names card. With two or more known screens: the chooser, then for a chosen screen its
    /// Screen name row, then a row per workspace in use. With one: that screen's Screen name
    /// row, then All screens' rows.
    fn names(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let (list, i) = names_chosen(self.w);
        let two = list.len() > 2;
        self.out.cards.push((Card::Names, y - self.top0));
        self.header(y, "Names", None, "");
        if two {
            let who = if i == 0 { "all screens".to_string() } else { list[i].name.clone() };
            let rw = self.c.width("For ", F::Small) + self.c.width(&who, F::SmallBold);
            self.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[("For ", F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
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
        self.say(Ctl::NameScreens, UIA_ComboBoxControlTypeId, "Screen", &cap, false, (None, Some(name.clone())));
        self.row(y, "Screen", &[t(cap)], (240.0, 32.0), |c, x, y| c.screen_picker(Ctl::NameScreens, x, y, 240.0, &name), None)
    }

    /// One name row: its title and caption, the chip of a workspace row, the box, and the
    /// focused box's note.
    fn name_row(&mut self, y: f32, row: Nm) -> f32 {
        let look = name_look(self.w, self.s, row);
        let ctl = Ctl::Name(row);
        let focused = self.w.focus == Some(ctl) && !self.ro;
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
            (NameState::Own, Nm::Ws(k)) => [Some(t("Its own name on this screen.")), same(k, self.ro)].into_iter().flatten().collect(),
            (NameState::NoName, Nm::Ws(k)) => [Some(t("Only the number shows here.")), same(k, self.ro)].into_iter().flatten().collect(),
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
        self.say(ctl, UIA_EditControlTypeId, &name_title(row), help.trim(), self.ro, (None, Some(text.clone())));
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

    /// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
    /// rename, else the id.
    fn screen_called(&self, id: &str) -> String {
        screen_called(&self.w.pics.view, self.s, id)
    }

    fn ws_chip_w(&self, k: usize) -> f32 {
        (self.c.width(&(k + 1).to_string(), F::SmallBold) + 10.0).max(20.0)
    }

    /// Workspace `k`'s chip, as the overview draws a shown workspace's: its color, its number.
    fn ws_chip(&self, x: f32, y: f32, k: usize) {
        let b = self.c.t.b;
        let (w, color) = (self.ws_chip_w(k), b.ws[k.min(8)]);
        self.c.boxed((x, y, w, 18.0), 4.0, color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)));
        let label = (k + 1).to_string();
        let tw = self.c.width(&label, F::SmallBold);
        self.c.text(x + (w - tw) / 2.0, y, 18.0, &label, F::SmallBold, b.on_ws[k.min(8)]);
    }

    // ---- Wallpaper (spec 0026 AC-8, AC-12 to AC-15, AC-18), as approved in
    // pdesk-design/wallpaper/wallpaper-design.html r3; its slideshow parts are spec 0028's ----

    /// The Wallpaper card, built like the Names card: with two or more known screens the
    /// chooser, then a row per workspace in use, then Fit, Dim and Transition.
    fn wallpaper(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let (list, i) = wall_chosen(self.w);
        let two = wall_two(self.w, self.s);
        self.out.cards.push((Card::Wallpaper, y - self.top0));
        self.header(y, "Wallpaper", None, "");
        if two {
            let who = if i == 0 { "all screens".to_string() } else { list[i].name.clone() };
            let rw = self.c.width("For ", F::Small) + self.c.width(&who, F::SmallBold);
            self.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[("For ", F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
        }
        let mut h = 38.0;
        if self.w.pics.wall_failed {
            let n = NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "des-k can't draw behind your desktop icons right now.".into(),
                lines: vec![t("Every screen shows your Windows wallpaper. des-k keeps trying every 5 seconds and right after explorer restarts; your choices below stay as they are.")],
                buttons: vec![],
                group: 1,
            };
            h += 12.0 + self.note_body(X0 + 16.0, y + h + 12.0, self.cw - 32.0, &n) + 12.0;
            self.sep(y + h);
        }
        if two {
            h += self.wall_chooser_row(y + h, &list, i);
        }
        // While any row is a slideshow every row keeps its time's space, so the menus line up.
        let screen = wall_screen(self.w, self.s);
        let slides = (0..self.s.workspaces).any(|k| matches!(wall_held(self.s, screen.as_deref(), k).1, Pick::Folder(_) | Pick::Set(_)));
        for k in 0..self.s.workspaces {
            if k > 0 || two || self.w.pics.wall_failed {
                self.sep(y + h);
            }
            h += self.wall_row(y + h, k, slides);
        }
        for key in ["wallpaper.fit", "wallpaper.dim", "wallpaper.transition", "wallpaper.interval", "wallpaper.shuffle"] {
            self.sep(y + h);
            h += self.setting(key, y + h);
        }
        h
    }

    fn wall_chooser_row(&mut self, y: f32, list: &[NameChoice], i: usize) -> f32 {
        let cap = match &list[i].id {
            None => "Pictures every screen shows, unless it has its own. They come from des-k's wallpapers folder; each row's menu opens it.".to_string(),
            Some(id) => {
                let own = self.s.wallpaper_screens.get(id);
                match (0..self.s.workspaces).filter(|&k| own.is_some_and(|o| o[k].is_some())).count() {
                    0 => "Every row follows all screens.".to_string(),
                    1 => "1 workspace differs on this screen. Other rows follow all screens.".to_string(),
                    n => format!("{n} workspaces differ on this screen. Other rows follow all screens."),
                }
            }
        };
        let name = list[i].name.clone();
        self.say(Ctl::WallScreens, UIA_ComboBoxControlTypeId, "Screen", &cap, false, (None, Some(name.clone())));
        self.row(y, "Screen", &[t(cap)], (WALL_PICK_W, 32.0), |c, x, y| c.screen_picker(Ctl::WallScreens, x, y, WALL_PICK_W, &name), None)
    }

    /// One workspace's row: its chip, a preview in the screen's shape fitted and dimmed as the
    /// screen shows it, its menu button, and for a slideshow its time per picture (spec 0028
    /// AC-11); the caption says how it follows, or why it can't show. `slides`: some row on the
    /// card is a slideshow, so this one keeps the time's space too.
    fn wall_row(&mut self, y: f32, k: usize, slides: bool) -> f32 {
        let screen = wall_screen(self.w, self.s);
        let (state, pick) = wall_held(self.s, screen.as_deref(), k);
        let key = pick.key();
        let name = wall_label(&pick);
        let problem = self.w.pics.wall_problems.iter().find(|(f, _)| !key.is_empty() && *f == key).map(|p| p.1);
        let all_none = self.s.wallpaper[k] == Pick::None;
        let same = |ro: bool| (!ro).then(|| P::Link(" Same as all screens".into(), Ctl::WallSame(k)));
        let show = matches!(pick, Pick::Folder(_) | Pick::Set(_));
        let count = match &pick {
            Pick::Set(v) => v.len(),
            Pick::Folder(f) => self.w.pics.wall_counts.iter().find(|(n, _)| n == f).map_or(0, |c| c.1),
            _ => 0,
        };
        let pictures = if count == 1 { "1 picture".to_string() } else { format!("{count} pictures") };
        let cap: Vec<P> = match (problem, state) {
            (Some(p), _) => vec![P::Warn(format!(" Can't show {name} · {}", wall_why(&pick, p)))],
            (None, WallState::All) => {
                let mut cap = vec![];
                if show {
                    let ticked = if matches!(pick, Pick::Set(_)) { " you ticked" } else { "" };
                    let time = if self.s.wallpaper_intervals[k].is_some() { "its own time" } else { "the default time below" };
                    cap.push(t(format!("Slideshow of {pictures}{ticked}, each for {time}.")));
                }
                // Every screen you have overrides this row, so it shows nowhere: said like
                // Transition's "Not in use", ahead of which screens differ.
                let present = &self.w.pics.view.present;
                let unused = !present.is_empty() && present.iter().all(|p| self.s.wallpaper_screens.get(&p.id).is_some_and(|o| o[k].is_some()));
                if unused {
                    cap.push(P::Why(format!("{}Not in use: ", if cap.is_empty() { "" } else { " " })));
                    cap.push(t(if present.len() == 1 { "your only screen has its own." } else { "every screen you have has its own." }));
                }
                let differ = self.wall_differ(k);
                if !differ.is_empty() {
                    cap.push(t(format!("{}{differ}", if cap.is_empty() { "" } else { " " })));
                }
                // One click from the reason to the fix, for the screens plugged in only (spec 0026
                // AC-13 grown on 2026-11-21: you asked for it, VS Code's "also modified in" style).
                if unused && !self.ro {
                    cap.push(P::Link(" Use this there too".into(), Ctl::WallUse(k)));
                }
                cap
            }
            (None, WallState::Follow) if all_none => vec![t("Follows all screens, which has no picture for it.")],
            (None, WallState::Follow) if show => vec![t(format!("Follows all screens: a slideshow of {pictures}."))],
            (None, WallState::Follow) => vec![t("Follows all screens.")],
            (None, WallState::Own) if show => [Some(t("Its own slideshow on this screen.")), same(self.ro)].into_iter().flatten().collect(),
            (None, WallState::Own) => [Some(t("Its own picture on this screen.")), same(self.ro)].into_iter().flatten().collect(),
            (None, WallState::Windows) => [Some(t("Your Windows wallpaper on this screen.")), same(self.ro)].into_iter().flatten().collect(),
        };
        let (label, grey) = match state {
            WallState::Follow => (format!("{} · all screens", if key.is_empty() { "Windows wallpaper" } else { name.as_str() }), true),
            _ if key.is_empty() => ("Windows wallpaper".to_string(), true),
            _ => (name.clone(), false),
        };
        let mark = match pick {
            Pick::Folder(_) => Mark::Glyph(I_FOLDER),
            Pick::Set(_) => Mark::Stack,
            _ => Mark::Glyph(I_PIC),
        };
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str() }).collect();
        self.say(Ctl::WallPick(k), UIA_ComboBoxControlTypeId, &format!("Workspace {} wallpaper", k + 1), help.trim(), self.ro, (None, Some(label.clone())));
        // A value that can't show leaves your Windows wallpaper on the screen (AC-8).
        let shown = if problem.is_some() { Some(String::new()) } else { wall_shows(self.w, self.s, screen.as_deref(), k) };
        let rect = wall_rect(&self.w.pics.view, screen.as_deref());
        let chip = self.ws_chip_w(k) + 10.0;
        let cw = chip + 64.0 + 10.0 + WALL_PICK_W + if slides { 10.0 + WALL_TIME_W } else { 0.0 };
        // The time is the same on every screen, so it is never greyed by a screen; grey means
        // it follows the Interval row.
        let iv = self.s.interval_for(k) as i64;
        let iv_grey = self.s.wallpaper_intervals[k].is_none();
        if show {
            self.say(Ctl::Stepper(IV_KEYS[k]), UIA_SpinnerControlTypeId, &format!("Workspace {} time per picture", k + 1), "", self.ro, (None, Some(minutes(iv))));
        }
        let ends = (LADDER[0], LADDER[LADDER.len() - 1]);
        self.row(y, &format!("Workspace {}", k + 1), &cap, (cw, 40.0), |c, x, y| {
            c.ws_chip(x, y + 11.0, k);
            c.wall_preview(x + chip, y, rect, shown.as_deref());
            let px = x + chip + 74.0;
            c.app_picker(Ctl::WallPick(k), (px, y + 4.0, WALL_PICK_W), mark, &label, grey);
            if show {
                c.stepper(IV_KEYS[k], px + WALL_PICK_W + 10.0, y + 4.0, (iv, MINUTES), (iv > ends.0, iv < ends.1), false, iv_grey);
            }
        }, None)
    }

    /// An All screens row's caption words: each screen whose own value differs; "" for none.
    fn wall_differ(&self, k: usize) -> String {
        let line: Vec<String> = self
            .s
            .wallpaper_screens
            .iter()
            .filter_map(|(id, o)| match o[k].as_ref()? {
                Pick::None => Some(format!("{}: Windows wallpaper there.", self.screen_called(id))),
                own => Some(format!("{} has its own: {}.", self.screen_called(id), wall_label(own))),
            })
            .collect();
        line.join(" ")
    }

    /// A row's preview in a 64 × 40 slot: `file` fitted for `rect`'s screen and dimmed as it shows
    /// there (AC-18), or with "" that screen's Windows wallpaper, never dimmed. A plain tile
    /// until its decode is ready; None, a slideshow with no picture yet, is a folder tile.
    fn wall_preview(&mut self, x: f32, y: f32, rect: Option<RECT>, file: Option<&str>) {
        let b = self.c.t.b;
        let (w, h) = wall_box(rect);
        let bx = (x + (64.0 - w) / 2.0, y + (40.0 - h) / 2.0, w, h);
        let size = wall_px(rect, self.w.scale);
        let Some(file) = file else {
            self.c.fill_rr(bx, 3.0, op(b.track));
            let gw = self.c.width(I_FOLDER, F::IconSmall);
            self.icon(bx.0 + (w - gw) / 2.0, bx.1, h, I_FOLDER, F::IconSmall, b.tx2);
            let edge = if self.c.t.hc { op(b.line) } else { ui::alpha(b.tx, 0.18) };
            self.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, bx.2 - 1.0, bx.3 - 1.0), 3.0, 1.0, edge);
            return;
        };
        let mut drawn = false;
        match rect {
            Some(r) if !file.is_empty() => {
                if let Some(p) = crate::wallpaper::cached_ws(r, file, size) {
                    self.c.picture_rr(bx, 3.0, &p);
                    let a = crate::backdrop::dim_alpha(self.s.wallpaper_dim);
                    if a > 0 {
                        self.c.fill_rr(bx, 3.0, ui::alpha(0, a as f32 / 255.0));
                    }
                    drawn = true;
                }
            }
            Some(r) => match crate::wallpaper::cached(r, size) {
                Some(crate::wallpaper::Face::Picture(p)) => {
                    self.c.picture_rr(bx, 3.0, &p);
                    drawn = true;
                }
                Some(crate::wallpaper::Face::Color(c)) => {
                    self.c.fill_rr(bx, 3.0, op(c));
                    drawn = true;
                }
                _ => {}
            },
            None => {}
        }
        if !drawn {
            self.c.fill_rr(bx, 3.0, op(b.track));
        }
        let edge = if self.c.t.hc { op(b.line) } else { ui::alpha(b.tx, 0.18) };
        self.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, bx.2 - 1.0, bx.3 - 1.0), 3.0, 1.0, edge);
    }

    /// A Wallpaper row's open menu (spec 0026 AC-13, AC-14; spec 0028 AC-11): Windows wallpaper,
    /// then one list that scrolls when it doesn't fit, of the folder's pictures with their tick
    /// boxes, a Slideshows heading and its folders with their picture counts, then Open folder and
    /// a line on how ticking works. On one screen that line adds that choosing All screens' value
    /// follows again; an empty folder says which files to put there.
    fn wall_menu(&mut self) {
        let Some((Ctl::WallPick(k), at, focus)) = self.w.chooser else { return };
        let t = self.c.t;
        let b = t.b;
        let screen = wall_screen(self.w, self.s);
        let files = self.w.wall_files.clone();
        let dirs = self.w.wall_dirs.clone();
        let (n, m) = (files.len(), dirs.len());
        let total = wall_total(n, m);
        let (_, cur) = chooser_lines(self.w, self.s, Ctl::WallPick(k));
        let held = wall_held(self.s, screen.as_deref(), k).1;
        let ticked = |f: &str| match &held {
            Pick::Picture(p) => p == f,
            Pick::Set(v) => v.iter().any(|x| x == f),
            _ => false,
        };
        let all = self.s.wallpaper[k].key();
        let rows = wall_rows(self.w.vh, total);
        let top = self.w.wall_top.min(total.saturating_sub(rows));
        let mw = at.2.max(if screen.is_some() { 320.0 } else { 300.0 });
        let empty = "No pictures yet. Put .jpg, .png, .bmp or .gif files in the folder, or a folder of them for a slideshow.";
        let hint = "Tick pictures for a slideshow of them. Click a name to pick just that one.";
        let follow = "Choosing All screens' picture makes this row follow all screens again.";
        let foot_text = match (n > 0, screen.is_some() && total > 0) {
            (true, true) => format!("{hint} {follow}"),
            (true, false) => hint.to_string(),
            (false, true) => follow.to_string(),
            (false, false) => String::new(),
        };
        let para_h = |c: &mut Self, text: &str| c.measured(|c| c.c.para(0.0, 0.0, mw - 28.0, 16.0, &[(text, F::Small, b.tx2)]));
        let slot = if total == 0 { para_h(self, empty) + 12.0 } else { rows as f32 * 28.0 };
        let foot = if foot_text.is_empty() { 0.0 } else { para_h(self, &foot_text) + 12.0 };
        let mh = 4.0 + 24.0 + 28.0 + 9.0 + slot + 9.0 + 28.0 + foot + 4.0;
        let below = at.1 + at.3 + 4.0;
        let my = if below + mh <= self.w.vh - 8.0 { below } else { (at.1 - mh - 4.0).max(8.0) };
        let mx = at.0.min(self.w.vw - mw - 16.0).max(NAV);
        self.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.c.text(mx + 14.0, my + 4.0, 24.0, "Picture", F::SmallBold, b.tx2);
        let (x, w) = (mx + 4.0, mw - 8.0);
        // On one screen, the line All screens uses says so.
        let tag = |name: &str| -> &'static str { if screen.is_some() && name == all { "All screens" } else { "" } };
        let mut ly = my + 28.0;
        self.wall_line(0, (x, ly, w), "Windows wallpaper", tag(""), Lead::Check(cur == Some(0)), focus == 0);
        ly += 28.0;
        self.c.rule(mx + 10.0, ly + 4.0, mw - 20.0, op(b.line));
        ly += 9.0;
        if total == 0 {
            self.c.para(mx + 14.0, ly + 6.0, mw - 28.0, 16.0, &[(empty, F::Small, b.tx2)]);
        } else {
            for (row, place) in (top..total).take(rows).enumerate() {
                let ry = ly + row as f32 * 28.0;
                if place < n {
                    let name = &files[place];
                    self.wall_line(place + 1, (x, ry, w), name, tag(name), Lead::Tick(ticked(name)), focus == place + 1);
                } else if place == n {
                    if n > 0 {
                        self.c.rule(mx + 10.0, ry + 4.0, mw - 20.0, op(b.line));
                    }
                    self.c.text(mx + 14.0, ry + 8.0, 20.0, "Slideshows", F::Small, b.tx2);
                } else {
                    let (name, count) = &dirs[place - n - 1];
                    let i = place;
                    let side = match tag(name) {
                        "" if *count == 1 => "1 picture".to_string(),
                        "" => format!("{count} pictures"),
                        tagged => tagged.to_string(),
                    };
                    self.wall_line(i, (x, ry, w), name, &side, Lead::Folder(cur == Some(i)), focus == i);
                }
            }
            if total > rows {
                let th = (slot * rows as f32 / total as f32).max(16.0);
                let ty = ly + (slot - th) * top as f32 / (total - rows) as f32;
                self.c.fill_rr((mx + mw - 6.0, ty + 2.0, 3.0, th - 4.0), 1.5, op(b.tx3));
            }
        }
        ly += slot;
        self.c.rule(mx + 10.0, ly + 4.0, mw - 20.0, op(b.line));
        ly += 9.0;
        self.wall_line(n + m + 1, (x, ly, w), "Open folder", "", Lead::Open, focus == n + m + 1);
        ly += 28.0;
        if foot > 0.0 {
            self.c.para(mx + 14.0, ly + 6.0, mw - 28.0, 16.0, &[(&foot_text, F::Small, b.tx2)]);
        }
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.hit(Ctl::ScreenItem(usize::MAX), (mx, my, mw, mh), 3);
    }

    /// One line of a Wallpaper menu: what leads it (a check, a picture's tick box, a folder mark,
    /// Open folder's mark with its open mark on the right), the name, and on the right "All
    /// screens" or a folder's picture count.
    fn wall_line(&mut self, i: usize, (x, y, w): (f32, f32, f32), name: &str, side: &str, lead: Lead, focus: bool) {
        let t = self.c.t;
        let b = t.b;
        let ctl = Ctl::ScreenItem(i);
        let on = matches!(lead, Lead::Check(true) | Lead::Folder(true));
        let hot = self.hovered(ctl) || self.hovered(Ctl::WallTick(i)) || (self.w.kb && focus);
        let (fill, ink) = match () {
            _ if on && t.hc => (Some(b.hi), b.on_acc),
            _ if on => (Some(b.track), b.tx),
            _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx),
            _ => (None, b.tx),
        };
        if let Some(f) = fill {
            self.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
        }
        if on {
            self.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, if t.hc { b.on_acc } else { b.acc });
        }
        let mut nx = x + 30.0;
        let side_ink = if on && t.hc { b.on_acc } else { b.tx2 };
        match lead {
            Lead::Tick(ticked) => {
                // Filled with the accent and checked when ticked (design r3).
                let bx = (x + 8.0, y + 6.0, 16.0, 16.0);
                if ticked {
                    // High contrast pairs its check ink with the highlight, as a checked line does.
                    self.c.fill_rr(bx, 3.0, op(if t.hc { b.hi } else { b.acc }));
                    let iw = self.c.width(I_OK, F::IconTiny);
                    self.icon(bx.0 + (16.0 - iw) / 2.0, y, 28.0, I_OK, F::IconTiny, b.on_acc);
                } else {
                    self.c.fill_rr(bx, 3.0, op(b.card));
                    let edge = if self.hovered(Ctl::WallTick(i)) { b.tx } else { b.tx2 };
                    self.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, 15.0, 15.0), 3.0, 1.0, op(edge));
                }
                nx = x + 34.0;
                // Before the line's own hit, since a click takes the first hit it is in.
                self.hit(Ctl::WallTick(i), (x, y, 30.0, 28.0), 3);
            }
            Lead::Folder(_) => {
                self.icon(nx, y, 28.0, I_FOLDER, F::IconSmall, side_ink);
                nx += 24.0;
            }
            Lead::Open => {
                self.icon(nx, y, 28.0, I_FOLDER, F::IconSmall, side_ink);
                nx += 24.0;
                let ow = self.c.width(I_OUT, F::IconSmall);
                self.icon(x + w - 10.0 - ow, y, 28.0, I_OUT, F::IconSmall, side_ink);
            }
            Lead::Check(_) => {}
        }
        let sw = if side.is_empty() { 0.0 } else { self.c.width(side, F::Small) + 10.0 };
        let shown = self.c.fit(name, F::Body, x + w - nx - 6.0 - sw);
        self.c.text(nx, y, 28.0, &shown, F::Body, ink);
        if !side.is_empty() {
            self.c.text(x + w - sw, y, 28.0, side, F::Small, side_ink);
        }
        if self.w.kb && focus {
            self.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
        }
        self.hit(ctl, (x, y, w, 28.0), 3);
    }

    // ---- Apps (spec 0027 AC-13): an Add card, then the rules, as approved in
    // pdesk-design/apps/apps-design.html r1 ----

    fn app_add(&mut self, y: f32) -> f32 {
        let bw = self.btn_w("Add app", Some(I_ADD));
        self.row(y, "Add an app", &[t("Pick an app that's open now. Its rule starts where its window is now.")], (bw, 32.0), |c, x, y| {
            c.button(Ctl::AppAdd, x, y, "Add app", Some(I_ADD), false, 1);
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

    fn app_rules(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let n = self.s.apps.len();
        self.header(y, "Rules", Some(n.min(99) as u8), "");
        let (plain, bold) = ("New windows only · ", "your screens stay");
        let rw = self.c.width(plain, F::Small) + self.c.width(bold, F::SmallBold);
        self.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[(plain, F::Small, b.tx2), (bold, F::SmallBold, b.tx)]);
        let mut h = 38.0;
        if n == 0 {
            return h + self.apps_empty(y + h);
        }
        let (_, name, _, ws, screen) = self.app_cols();
        for (text, x) in [("App", name), ("Workspace", ws), ("Screen", screen)] {
            self.c.text(x, y + h, 33.0, text, F::Small, b.tx2);
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
        let b = self.c.t.b;
        let (key, rule) = &self.s.apps[i];
        let (x, nx, nw, wx, sx) = self.app_cols();
        let seen = self.w.pics.apps.iter().find(|a| a.key == *key).cloned();
        let name = seen.as_ref().map_or_else(|| key.clone(), |a| a.name.clone());
        let note = app_note(&self.w.pics.view, self.s, rule);
        let note_ink = |warn: bool| if warn { b.warn } else { b.tx2 };
        let sub_h = match &note {
            Some((text, warn)) => self.measured(|c| c.c.para(nx + 18.0, 0.0, nw - 18.0, 16.0, &[(text, F::Small, note_ink(*warn))])),
            None => 16.0,
        };
        let h = (10.0 + 20.0 + 2.0 + sub_h + 10.0).max(60.0);
        let ty = y + (h - 22.0 - sub_h) / 2.0;
        let iy = y + (h - 24.0) / 2.0;
        match &seen {
            Some(a) => self.c.icon(x, iy, 24.0, a.icon),
            None => {
                self.c.stroke_rr((x + 0.5, iy + 0.5, 23.0, 23.0), 6.0, 1.0, op(b.tx3));
                let gw = self.c.width(I_APP, F::IconSmall);
                self.icon(x + (24.0 - gw) / 2.0, iy, 24.0, I_APP, F::IconSmall, b.tx3);
            }
        }
        let shown = self.c.fit(&name, F::Bold, nw);
        self.c.text(nx, ty, 20.0, &shown, F::Bold, b.tx);
        match &note {
            Some((text, warn)) => {
                self.icon(nx, ty + 22.0, 16.0, if *warn { I_WARN } else { I_INFO }, F::IconTiny, note_ink(*warn));
                self.c.para(nx + 18.0, ty + 22.0, nw - 18.0, 16.0, &[(text, F::Small, note_ink(*warn))]);
            }
            None if seen.is_none() => self.c.text(nx, ty + 22.0, 16.0, "Not running", F::Small, b.tx2),
            None => {
                // A Store app's id is long and spelled as the file holds it.
                let f = if key.contains('!') { F::Mono } else { F::Small };
                let k = self.c.fit(key, f, nw);
                self.c.text(nx, ty + 22.0, 16.0, &k, f, b.tx2);
            }
        }
        let my = y + (h - 32.0) / 2.0;
        let (ws, grey) = ws_label(self.s, rule.screen.as_deref(), rule.workspace);
        let screen = rule.screen.as_deref().map_or_else(|| "Any screen".to_string(), |id| self.screen_called(id));
        let help = note.as_ref().map_or(String::new(), |(text, _)| text.clone());
        self.say(Ctl::AppWs(i), UIA_ComboBoxControlTypeId, &format!("Workspace for {name}"), &help, self.ro, (None, Some(ws.clone())));
        self.app_picker(Ctl::AppWs(i), (wx, my, APP_WS_W), Mark::Chip(rule.workspace), &ws, grey);
        self.say(Ctl::AppScreen(i), UIA_ComboBoxControlTypeId, &format!("Screen for {name}"), "", self.ro, (None, Some(screen.clone())));
        self.app_picker(Ctl::AppScreen(i), (sx, my, APP_SCREEN_W), Mark::Glyph(I_SCREEN), &screen, false);
        self.app_x(i, sx + APP_SCREEN_W + 12.0, my, &name);
        h
    }

    /// A menu button, as the screen chooser draws one: a workspace's chip and name, or a mark
    /// (a screen, a picture) and a name.
    fn app_picker(&mut self, ctl: Ctl, (x, y, w): (f32, f32, f32), mark: Mark, label: &str, grey: bool) {
        let b = self.c.t.b;
        if matches!(self.w.chooser, Some((from, ..)) if from == ctl) {
            self.field((x, y, w, 32.0), b.card, Some(b.acc));
        } else {
            let fill = if self.hovered(ctl) && !self.ro { mix(b.track, b.tx, 0.05) } else { b.track };
            self.field((x, y, w, 32.0), fill, None);
        }
        let mut tx = x + 10.0;
        match mark {
            Mark::Chip(k) => {
                self.ws_chip(tx, y + 7.0, k);
                tx += self.ws_chip_w(k) + 8.0;
            }
            Mark::Glyph(g) => {
                self.icon(tx, y, 32.0, g, F::Icon, b.tx2);
                tx += 26.0;
            }
            Mark::Stack => {
                // Two pictures, the front one masked by the button's own fill (spec 0028 AC-11).
                let fill = if matches!(self.w.chooser, Some((from, ..)) if from == ctl) { b.card } else if self.hovered(ctl) && !self.ro { mix(b.track, b.tx, 0.05) } else { b.track };
                self.icon(tx + 4.0, y - 3.0, 32.0, I_PIC, F::IconSmall, mix(fill, b.tx2, 0.6));
                let gw = self.c.width(I_PIC, F::IconSmall);
                self.c.fill_rr((tx - 1.0, y + 11.0, gw + 2.0, 14.0), 2.0, op(fill));
                self.icon(tx, y + 2.0, 32.0, I_PIC, F::IconSmall, b.tx2);
                tx += 26.0;
            }
        }
        let cw = self.c.width(I_CHEV, F::IconTiny);
        let shown = self.c.fit(label, F::Body, x + w - 18.0 - cw - tx);
        self.c.text(tx, y, 32.0, &shown, F::Body, if grey || self.ro { b.tx2 } else { b.tx });
        self.icon(x + w - 10.0 - cw, y, 32.0, I_CHEV, F::IconTiny, b.tx2);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), 1);
    }

    /// A rule's remove button: an × that shows its square under the pointer.
    fn app_x(&mut self, i: usize, x: f32, y: f32, name: &str) {
        let b = self.c.t.b;
        let ctl = Ctl::AppX(i);
        let hot = self.hovered(ctl) && !self.ro;
        if hot {
            self.c.boxed((x, y, 32.0, 32.0), 4.0, mix(b.card, b.tx, 0.06), Some(b.line));
        }
        let iw = self.c.width(I_X, F::IconTiny);
        let ink = match () {
            _ if self.ro => b.tx3,
            _ if hot => b.tx,
            _ => b.tx2,
        };
        self.icon(x + (32.0 - iw) / 2.0, y, 32.0, I_X, F::IconTiny, ink);
        self.ring((x, y, 32.0, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, 32.0, 32.0), 1);
        self.say(ctl, UIA_ButtonControlTypeId, &format!("Remove {name}"), "Takes this app's rule out of settings.toml.", self.ro, (None, None));
    }

    /// No rules yet: what a rule does, and where else you can write one.
    fn apps_empty(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let hc = self.c.t.hc;
        let cx = X0 + self.cw / 2.0;
        let tile = if hc { b.card } else { mix(b.card, b.acc_a, 0.09) };
        self.c.boxed((cx - 24.0, y + 28.0, 48.0, 48.0), 12.0, tile, hc.then_some(b.line));
        let gw = self.c.width(I_APPS, F::Icon);
        self.icon(cx - gw / 2.0, y + 28.0, 48.0, I_APPS, F::Icon, if hc { b.tx } else { b.acc });
        let mut ly = y + 86.0;
        for (text, f, ink, lh) in [
            ("No rules yet", F::Bold, b.tx, 20.0),
            ("Add an app and its new windows open on the workspace you pick, wherever you are.", F::Small, b.tx2, 16.0),
            ("You can also write rules under [apps] in settings.toml.", F::Small, b.tx2, 16.0),
        ] {
            let tw = self.c.width(text, f);
            self.c.text(cx - tw / 2.0, ly, lh, text, f, ink);
            ly += lh + 2.0;
        }
        ly - y + 26.0
    }

    /// A name box, drawn as the popup words' box: an empty one shows its hint. On a read only
    /// page names are greyed, empty boxes show no hint, and no box takes focus.
    fn name_box(&mut self, ctl: Ctl, x: f32, y: f32, text: &str, hint: &str, focused: bool) {
        let b = self.c.t.b;
        let bx = (x, y, NAME_W, 32.0);
        if !self.ro {
            self.hit(ctl, bx, 1);
        }
        self.field(bx, b.card, None);
        if focused {
            self.c.fill_rr((x + 1.0, y + 29.0, NAME_W - 2.0, 2.0), 0.0, op(b.acc));
        }
        let room = NAME_W - 24.0;
        let shown = self.c.fit(text, F::Body, room);
        if !text.is_empty() {
            self.c.text(x + 12.0, y, 32.0, &shown, F::Body, if self.ro { b.tx3 } else { b.tx });
        } else if !self.ro {
            self.c.text(x + 12.0, y, 32.0, &self.c.fit(hint, F::Body, room), F::Body, b.hint);
        }
        if focused {
            let tw = if text.is_empty() { 0.0 } else { self.c.width(&shown, F::Body) + 1.0 };
            self.c.fill_rr((x + 12.0 + tw, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
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

    /// Screen `device`'s workspace names, as its indicator shows them (spec 0023 build
    /// conditions): the same lookup as `ws::names`, by the id the page's view already holds,
    /// since the page can lay out from inside a switch and the ws lock is not reentrant.
    fn names_here(&self, device: &str) -> Vec<String> {
        let Some(p) = self.w.pics.view.present.iter().find(|p| p.device == device) else { return vec![] };
        (0..self.s.workspaces).map(|i| self.s.ws_name(&p.id, i).unwrap_or_default().to_string()).collect()
    }

    fn active_on(&self, device: &str) -> usize {
        self.w.pics.view.active.iter().find(|(d, _)| d == device).map_or(0, |(_, a)| *a)
    }

    fn tiles_w(&self, n: usize, add: bool) -> f32 {
        let tw = if n > 6 { 40.0 } else { 48.0 };
        n as f32 * tw + (n as f32 - 1.0) * 6.0 + if add { 6.0 + tw } else { 0.0 }
    }

    /// One present screen: its name and size, and its workspaces as tiles (AC-SET-3).
    fn screen_row(&mut self, y: f32, row: usize, sc: &crate::ws::ScreenInfo, gone: &[usize]) -> f32 {
        let n = self.s.workspaces;
        let name = self.name_of(sc);
        let a = self.active_on(&sc.device);
        let (rw, rh) = (sc.rect.right - sc.rect.left, sc.rect.bottom - sc.rect.top);
        let cap = vec![t(format!("{rw} × {rh} · {}% · on workspace {}", sc.scale, a + 1))];
        let counts: Vec<usize> = (0..n).map(|ws| self.w.pics.view.windows.iter().filter(|(d, k, _)| *d == sc.device && *k == ws).count()).collect();
        let add = n < 9;
        let tw = self.tiles_w(n, add);
        let mut h = self.row(y, &name, &cap, (tw, 34.0), |c, x, y| c.tiles(x, y, &counts, Some((a, row)), gone, add), None);
        h = h.max(60.0);
        h
    }

    /// The tiles: a workspace each in its color, with up to three window marks; the one in
    /// view ringed; × on the last one while you point at it, + after them. `now`: a plugged in
    /// screen's workspace in view and its row in Pics::screens; its tiles open the color picker.
    #[allow(clippy::too_many_arguments)]
    fn tiles(&mut self, x: f32, y: f32, counts: &[usize], now: Option<(usize, usize)>, gone: &[usize], add: bool) {
        let t = self.c.t;
        let b = t.b;
        let n = counts.len();
        let tw = if n > 6 { 40.0 } else { 48.0 };
        let away = now.is_none();
        let (now, row) = (now.map(|(a, _)| a), now.map_or(0, |(_, r)| r));
        let device = self.w.pics.screens.get(row).map(|s| s.device.clone()).unwrap_or_default();
        for (i, &k) in counts.iter().enumerate() {
            let tx = x + i as f32 * (tw + 6.0);
            let bx = (tx, y, tw, 34.0);
            let cur = now == Some(i);
            let (fill, ink) = match () {
                _ if away => (b.track, b.tx2),
                _ if t.hc && cur => (b.hi, b.on_acc),
                _ if t.hc => (b.card, b.tx),
                _ => (b.ws[i], b.on_ws[i]),
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
                let col = if away { b.tx3 } else if t.hc { if cur { b.on_acc } else { b.tx } } else { b.mk_ws[i] };
                self.c.fill_rr((mx, y + 4.0, 8.0, 10.0), 2.0, ui::alpha(col, if away || t.hc { 1.0 } else { 0.9 }));
            }
            if gone.contains(&i) && !away {
                self.c.stroke_dash((tx + 1.0, y + 1.0, tw - 2.0, 32.0), 3.0, 2.0, op(b.tx), DashStyleDash);
            }
            if away {
                continue;
            }
            let ctl = Ctl::WsTile(row, i);
            if self.hovered(ctl) && self.picks() && !cur {
                self.cue(bx, ink);
            }
            // Before the ×'s hit, so a click inside the tile opens the picker and never removes
            // a workspace (spec 0021 AC-1).
            self.hit(ctl, bx, 1);
            self.out.geo.tiles.push((device.clone(), i, bx));
            if i + 1 == n && n > 2 && !self.ro {
                let show = self.hovered(ctl) || self.hovered(Ctl::TileRemove) || self.focused(Ctl::Tiles);
                if show {
                    let (cx, cy) = (tx + tw + 2.0, y - 2.0);
                    let hov = self.hovered(Ctl::TileRemove);
                    let (bg, fg) = if hov { (b.tx, b.card) } else { (b.card, b.tx) };
                    self.c.circle(cx, cy, 9.0, Some(op(bg)), Some((op(b.tx3), 1.0)));
                    self.c.lines(&[(cx - 3.0, cy - 3.0), (cx + 3.0, cy + 3.0)], 1.2, op(fg));
                    self.c.lines(&[(cx + 3.0, cy - 3.0), (cx - 3.0, cy + 3.0)], 1.2, op(fg));
                    self.hit(Ctl::TileRemove, (cx - 10.0, cy - 10.0, 20.0, 20.0), 1);
                }
            }
        }
        if !away {
            self.out.geo.rows.push((device, (x, y, self.tiles_w(n, add), 34.0)));
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
            let (fill, ink) = if self.c.t.hc { (b.card, b.tx) } else { (b.ws[a.min(8)], b.on_ws[a.min(8)]) };
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
            self.c.stroke_dash((tx(1) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, 1.0, op(b.on_ws[1]), DashStyleDash);
            self.c.boxed((tx(2) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, ui::LIGHT.b.card, Some(b.on_ws[2]));
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
        if key == "shortcuts.back" {
            self.out.cards.push((Card::Back, y - self.top0));
        }
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
            let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.06) };
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
        let named = self.w.pics.screens.len() > 1 && self.s.indicator_screen_name;
        let label = self.w.pics.view.names.iter().find(|(d, _)| *d == here).map(|(_, l)| l.clone());
        let card = crate::indicator::Card { names: self.names_here(&here), ..crate::indicator::Card::new(a, n, if named { label } else { None }) };
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
        h += self.setting("indicator.fade_ms", y + h);
        self.sep(y + h);
        h + self.setting("indicator.screen_name", y + h)
    }

    // ---- Attention (spec 0022) ----

    /// The switch, your words, and the popup they make, drawn by the indicator itself for a
    /// sample app on another workspace of this screen.
    fn attention(&mut self, y: f32) -> f32 {
        use crate::indicator::{Card, Line};
        let b = self.c.t.b;
        let mut h = self.setting("attention.enabled", y);
        self.sep(y + h);
        h += self.words_row(y + h);
        let n = self.s.workspaces;
        let here = self.w.pics.here.clone().unwrap_or_default();
        let a = self.active_on(&here);
        let named = self.w.pics.screens.len() > 1 && self.s.indicator_screen_name;
        let label = self.w.pics.view.names.iter().find(|(d, _)| *d == here).map(|(_, l)| l.clone());
        let ws = if SAMPLE_WS.min(n - 1) == a { (a + 1) % n } else { SAMPLE_WS.min(n - 1) };
        let text = settings::fill(self.w.words.as_deref().unwrap_or(&self.s.attention_text), SAMPLE_APP);
        let card = Card { wants: vec![ws], names: self.names_here(&here), ..Card::new(a, n, if named { label } else { None }) }.with(Line::Wants { text, ws });
        let style = self.s.indicator_style;
        let (pw, ph) = crate::indicator::draw_preview(&self.c.measuring(), 0.0, 0.0, style, &card);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + h + 4.0;
        let well_h = 16.0 + ph + 10.0 + 20.0 + 10.0;
        self.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        crate::indicator::draw_preview(self.c, wx + (ww - pw) / 2.0, wy + 16.0, style, &card);
        let on = self.s.attention_enabled;
        if !on && !self.c.t.hc {
            self.c.fill_rr((wx + 1.0, wy + 1.0, ww - 2.0, well_h - 2.0), 5.0, ui::alpha(b.track, 0.6));
        }
        let line = if on {
            format!("When {SAMPLE_APP} flashes on workspace {}, its screen shows this for 3 s.", ws + 1)
        } else {
            "Off: a window that flashes on a workspace you can't see shows nothing.".into()
        };
        let lw = self.c.width(&line, F::Small);
        self.c.text(wx + (ww - lw) / 2.0, wy + 16.0 + ph + 10.0, 20.0, &line, F::Small, b.tx2);
        h + 4.0 + well_h + 16.0
    }

    /// Your words for the popup: a text box, with the tag explained and the default to go back to.
    fn words_row(&mut self, y: f32) -> f32 {
        let text = self.w.words.clone().unwrap_or_else(|| self.s.attention_text.clone());
        let off = !self.s.attention_enabled;
        let focused = self.w.focus == Some(Ctl::Words) && !self.ro && !off;
        let note = self.bad(WORDS).or_else(|| {
            let line = "Enter saves them; Esc puts back the saved words. Leaving the box saves them too.";
            focused.then(|| NoteSpec { tone: Tone::Info, icon: I_INFO, title: String::new(), lines: vec![t(line)], buttons: vec![], group: 1 })
        });
        let mut cap = vec![t(format!("Up to {} characters. ", settings::ATTENTION_MAX)), P::Mono("{{appname}}".into()), t(" becomes the app's name.")];
        if self.s.attention_text != settings::ATTENTION_TEXT {
            cap.push(t(" Default: "));
            cap.push(P::Mono(settings::ATTENTION_TEXT.into()));
            cap.push(t("."));
            cap.push(P::Link(" Reset".into(), Ctl::Reset(WORDS)));
        }
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str() }).collect();
        self.say(Ctl::Words, UIA_EditControlTypeId, "Popup words", help.trim(), self.ro || off, (None, Some(text.clone())));
        self.row(y, "Popup words", &cap, (WORDS_W, 32.0), |c, x, y| c.words_box(x, y, &text, focused, off), note.as_ref())
    }

    /// The text box, drawn as the hex box: focus shows a 2 dip accent bottom edge and a caret.
    fn words_box(&mut self, x: f32, y: f32, text: &str, focused: bool, na: bool) {
        let b = self.c.t.b;
        let bx = (x, y, WORDS_W, 32.0);
        self.hit(Ctl::Words, bx, 1);
        self.field(bx, b.card, None);
        if focused {
            self.c.fill_rr((x + 1.0, y + 29.0, WORDS_W - 2.0, 2.0), 0.0, op(b.acc));
        }
        let room = WORDS_W - 24.0;
        let shown = self.c.fit(text, F::Body, room);
        let ink = if na || self.ro { b.tx3 } else { b.tx };
        self.c.text(x + 12.0, y, 32.0, &shown, F::Body, ink);
        if focused {
            let cx = x + 12.0 + self.c.width(&shown, F::Body) + 1.0;
            self.c.fill_rr((cx, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
        if self.focused(Ctl::Words) && !focused {
            self.ring(bx, 6.0, Ctl::Words);
        }
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

    // ---- Where it shows (spec 0020 AC-13) ----

    /// The indicator's place, for All screens or one screen: the chooser, the picture with
    /// Spots or Custom, the distance, and Across and Down for Custom.
    fn place(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let (choices, i) = chosen(self.w, self.s);
        let ch = choices[i].clone();
        self.header(y, "Where it shows", None, "");
        let (lead, who) = match &ch.id {
            None => ("For ", "all screens".to_string()),
            Some(_) => ("For ", ch.name.clone()),
        };
        let rw = self.c.width(lead, F::Small) + self.c.width(&who, F::SmallBold);
        self.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[(lead, F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
        let mut h = 38.0;
        if choices.len() > 2 {
            h += self.chooser_row(y + h, &choices, i);
            self.sep(y + h);
        }
        let id = ch.id.as_deref();
        h += self.place_row(y + h, &ch);
        self.sep(y + h);
        h += self.place_num(GAP, y + h, &ch);
        if pos_of(self.s, id).spot == settings::Spot::Custom {
            for key in [ACROSS, DOWN] {
                self.sep(y + h);
                h += self.place_num(key, y + h, &ch);
            }
        }
        h
    }

    fn chooser_row(&mut self, y: f32, choices: &[ScreenChoice], i: usize) -> f32 {
        let ch = &choices[i];
        let mut cap = vec![];
        match &ch.id {
            None => {
                cap.push(t("The place for every screen without its own."));
                let named: Vec<String> = settings::problems()
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
        self.say(Ctl::Screens, UIA_ComboBoxControlTypeId, "Screen", "", false, (None, Some(name.clone())));
        self.row(y, "Screen", &cap, (pw, 32.0), |c, x, y| c.screen_picker(Ctl::Screens, x, y, pw, &name), None)
    }

    /// The chooser: the touchpad's picker, with a screen for its mark. Never greyed: it only
    /// picks which place you see (live with the indicator off and in read only).
    fn screen_picker(&mut self, ctl: Ctl, x: f32, y: f32, w: f32, name: &str) {
        let b = self.c.t.b;
        if matches!(self.w.chooser, Some((from, ..)) if from == ctl) {
            self.field((x, y, w, 32.0), b.card, Some(b.acc));
        } else {
            let fill = if self.hovered(ctl) { mix(b.track, b.tx, 0.05) } else { b.track };
            self.field((x, y, w, 32.0), fill, None);
        }
        self.icon(x + 10.0, y, 32.0, "\u{E7F4}", F::Icon, b.tx2);
        let shown = self.c.fit(name, F::Body, w - 36.0 - 26.0);
        self.c.text(x + 36.0, y, 32.0, &shown, F::Body, b.tx);
        let cw = self.c.width(I_CHEV, F::IconTiny);
        self.icon(x + w - 10.0 - cw, y, 32.0, I_CHEV, F::IconTiny, b.tx2);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), 1);
    }

    /// The words a place value is shown in: "Top right", "24 px", "50 %".
    fn place_shown(key: &str, v: f64) -> String {
        match leaf(key) {
            "spot" => SPOT_NAMES.get(v as usize).copied().unwrap_or("Center").to_string(),
            "gap" => format!("{} px", v.round()),
            _ => format!("{} %", v.round()),
        }
    }

    /// The problem key a place row shows: [indicator]'s, or the chosen screen table's.
    fn place_problem(key: &str, id: Option<&str>) -> String {
        id.map_or(key.to_string(), |id| format!("indicator.screens.\"{id}\".{}", leaf(key)))
    }

    /// The end of a place row's caption: All screens says its default and Reset; a screen says
    /// All screens' value and Reset for a key it sets, From All screens for one it doesn't.
    fn place_pieces(&self, key: &'static str, ch: &ScreenChoice) -> Vec<P> {
        let bad = settings::problems().iter().any(|p| p.key == Self::place_problem(key, ch.id.as_deref()));
        match &ch.id {
            None => {
                let (v, d) = (pos_get(self.s.indicator_pos, key), pos_get(DEFAULTS.indicator_pos, key));
                if v == d && !bad {
                    return vec![];
                }
                vec![t(format!(" Default: {}.", Self::place_shown(key, d))), P::Link(" Reset".into(), Ctl::Reset(key))]
            }
            Some(id) => {
                let own = self.s.indicator_screens.get(id).and_then(|o| ov_get(o, key));
                if own.is_none() && !bad {
                    return vec![t(" From All screens.")];
                }
                vec![t(format!(" All screens: {}.", Self::place_shown(key, pos_get(self.s.indicator_pos, key)))), P::Link(" Reset".into(), Ctl::Reset(key))]
            }
        }
    }

    fn place_bad(&self, key: &str, ch: &ScreenChoice) -> Option<NoteSpec> {
        let p = settings::problems().iter().find(|p| p.key == Self::place_problem(key, ch.id.as_deref()))?;
        let (title, fix) = match &ch.id {
            None => {
                let d = Self::place_shown(key, pos_get(DEFAULTS.indicator_pos, key));
                (format!("des-k can't use this value, so it uses {d}"), format!(" Change it here, or Reset puts {d} back in the file."))
            }
            Some(_) => (format!("des-k can't use this value, so {} uses All screens' {}", ch.name, Self::place_shown(key, pos_get(self.s.indicator_pos, key))), " Change it here, or Reset removes the line.".into()),
        };
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title, lines: vec![P::Mono(p.message.clone()), t(fix)], buttons: vec![], group: 1 })
    }

    /// Place: what it is in words, Spots or Custom, then the picture in the chosen screen's
    /// shape with the name, size and Show it there under it. The picture is drawn first so it
    /// is the stop before Spots or Custom (Reset after both).
    fn place_row(&mut self, y: f32, ch: &ScreenChoice) -> f32 {
        let b = self.c.t.b;
        let id = ch.id.as_deref();
        let na = !self.s.indicator_enabled;
        let mut pos = pos_of(self.s, id);
        if let (Some((x, yy)), settings::Spot::Custom) = (self.w.dragging, pos.spot) {
            (pos.x, pos.y) = (x, yy);
        }
        let custom = pos.spot == settings::Spot::Custom;
        let mut cap = vec![t(match pos.spot {
            settings::Spot::Custom => format!("Its center {} % across and {} % down. Click or drag on the screen to move it; Spots goes to the nearest spot.", pos.x.round(), pos.y.round()),
            settings::Spot::Center => "Center. Click a spot on the screen.".to_string(),
            s => format!("{}, {} px from the edges. Click a spot on the screen.", SPOT_NAMES[spot_index(s)], pos.gap),
        })];
        if custom && self.kept_inside(ch, pos) {
            cap.push(t(" des-k keeps the card inside the screen, so it sits a little further in."));
        }
        cap.extend(self.place_pieces(SPOT, ch));
        let note = self.place_bad(SPOT, ch);
        let words = ["Spots", "Custom"];
        let size = (self.seg_w(&words), 28.0);
        let rh = self.measured(|c| c.row(y, "Place", &cap, size, |_, _, _| {}, note.as_ref()));
        let (full, work, scale, label) = self.shape(ch);
        let pw = 352.0;
        let ph = (pw * (full.bottom - full.top) as f32 / (full.right - full.left).max(1) as f32).clamp(120.0, 300.0);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + rh;
        let well_h = 16.0 + ph + 12.0 + 32.0 + 12.0;
        self.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        self.picture((wx + (ww - pw) / 2.0, wy + 16.0, pw, ph), (full, work, scale), pos, na);
        let help = format!("{} {}", SPOT_NAMES[spot_index(pos.spot)], "Left and Right move it.");
        self.say(Ctl::Picture, UIA_SpinnerControlTypeId, "Place", &help, na || self.ro, (None, Some(SPOT_NAMES[spot_index(pos.spot)].into())));
        self.say(Ctl::Pick(MODE), UIA_ComboBoxControlTypeId, "Spots or Custom", "", na || self.ro, (None, Some(words[custom as usize].into())));
        self.row(y, "Place", &cap, size, |c, x, y| c.seg(x, y, &words, Some(custom as usize), na, |i| Ctl::Choice(MODE, i), Ctl::Pick(MODE)), note.as_ref());
        let by = wy + 16.0 + ph + 12.0;
        let sw = self.btn_w("Show it there", None);
        let line = self.c.fit(&label, F::Small, ww - 24.0 - sw - 12.0);
        self.c.text(wx + 12.0, by, 32.0, &line, F::Small, b.tx2);
        let off = na || (ch.id.is_some() && ch.present.is_none());
        self.button(Ctl::ShowThere, wx + ww - 12.0 - sw, by, "Show it there", None, off, 1);
        rh + well_h + 16.0
    }

    /// The chosen screen's whole rect, work area, scale and the line under the picture. All
    /// screens and a screen not plugged in: 16:9 with the taskbar at the bottom.
    fn shape(&self, ch: &ScreenChoice) -> (RECT, RECT, f32, String) {
        let full = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let plain = (full, RECT { bottom: 1032, ..full }, 1.0);
        let present = ch.present.and_then(|i| self.w.pics.view.present.get(i));
        let (full, work, scale) = present.map_or(plain, |p| {
            let sc = self.w.pics.screens.iter().find(|s| s.device == p.device).map_or(1.0, |s| s.scale as f32 / 100.0);
            (p.full, p.work, sc)
        });
        let label = match (&ch.id, present) {
            (None, _) => "Any screen: des-k fits the place to each one. Show it there uses the screen this window is on.".to_string(),
            (Some(_), Some(p)) => format!("{} · {} × {} · {}%", ch.name, p.full.right - p.full.left, p.full.bottom - p.full.top, (scale * 100.0).round()),
            (Some(_), None) if ch.side == "Unknown screen" => format!("{} · des-k hasn't seen this screen", ch.name),
            (Some(_), None) => format!("{} · not plugged in", ch.name),
        };
        (full, work, scale, label)
    }

    /// Whether des-k moves a Custom card inward on this screen: indicator::place with the plain
    /// card's size at the screen's scale, against the unclamped center. All screens asks for
    /// the screen this window is on.
    fn kept_inside(&self, ch: &ScreenChoice, pos: settings::Pos) -> bool {
        let view = &self.w.pics.view;
        let p = match &ch.id {
            Some(_) => ch.present.and_then(|i| view.present.get(i)),
            None => self.w.pics.here.as_ref().and_then(|d| view.present.iter().find(|p| p.device.trim_start_matches(r"\\.\") == d.as_str())),
        };
        let Some(p) = p else { return false };
        let scale = self.w.pics.screens.iter().find(|s| s.device == p.device).map_or(1.0, |s| s.scale as f32 / 100.0);
        let card = crate::indicator::Card::new(0, self.s.workspaces, None);
        let (cw, chh) = crate::indicator::draw_preview(&self.c.measuring(), 0.0, 0.0, self.s.indicator_style, &card);
        let (w, h) = ((cw * scale).round() as i32, (chh * scale).round() as i32);
        let (x, y) = crate::indicator::place(p.work, w, h, pos, scale);
        let r = p.work;
        let ux = r.left as f64 + pos.x / 100.0 * (r.right - r.left) as f64;
        let uy = r.top as f64 + pos.y / 100.0 * (r.bottom - r.top) as f64;
        ((x + w / 2) as f64 - ux).abs() > 1.5 || ((y + h / 2) as f64 - uy).abs() > 1.5
    }

    /// The place picture: a screen in the chosen shape, the taskbar on its edge, and the card
    /// where des-k puts it, at a fixed 52 × 16 so it reads; the distance drawn to scale. Spots:
    /// the other eight dashed, each third across and down picks one. Custom: the card's center
    /// on two dotted lines, and the whole screen takes clicks and drags.
    fn picture(&mut self, (x, y, pw, ph): Bx, (full, work, scale): (RECT, RECT, f32), pos: settings::Pos, na: bool) {
        let b = self.c.t.b;
        let hc = self.c.t.hc;
        self.c.fill_rr((x, y, pw, ph), 6.0, op(b.bez));
        let (sx, sy, sw, sh) = (x + 4.0, y + 4.0, pw - 8.0, ph - 8.0);
        let screen = if hc { b.card } else { mix(b.card, b.tx, 0.12) };
        self.c.fill_rr((sx, sy, sw, sh), 2.0, op(screen));
        let k = sw / (full.right - full.left).max(1) as f32;
        let kv = sh / (full.bottom - full.top).max(1) as f32;
        let wl = sx + (work.left - full.left) as f32 * k;
        let wt = sy + (work.top - full.top) as f32 * kv;
        let wr = sx + (work.right - full.left) as f32 * k;
        let wb = sy + (work.bottom - full.top) as f32 * kv;
        let bar = if hc { b.tx } else { mix(b.card, b.tx, 0.40) };
        for r in [(sx, sy, sw, wt - sy), (sx, wb, sw, sy + sh - wb), (sx, sy, wl - sx, sh), (wr, sy, sx + sw - wr, sh)] {
            if r.2 > 0.5 && r.3 > 0.5 {
                self.c.fill_rr(r, 0.0, op(bar));
            }
        }
        let (cw, chh) = (52.0, 16.0);
        let gap = (pos.gap as f32 * scale * k).max(2.0);
        let at = |spot: usize| -> (f32, f32) {
            let (x0, xc, x1) = (wl + gap, (wl + wr - cw) / 2.0, wr - gap - cw);
            let (y0, yc, y1) = (wt + gap, (wt + wb - chh) / 2.0, wb - gap - chh);
            let (px, py) = match spot {
                9 => (wl + pos.x as f32 / 100.0 * (wr - wl) - cw / 2.0, wt + pos.y as f32 / 100.0 * (wb - wt) - chh / 2.0),
                4 => (xc, yc),
                s => ([x0, xc, x1][s % 3], [y0, yc, y1][s / 3]),
            };
            (px.clamp(wl, (wr - cw).max(wl)), py.clamp(wt, (wb - chh).max(wt)))
        };
        let chosen = spot_index(pos.spot);
        let custom = chosen == 9;
        if custom {
            let (cx, cy) = at(9);
            let (mx, my) = (cx + cw / 2.0, cy + chh / 2.0);
            let line = op(if na { b.tx3 } else { b.tx2 });
            self.c.dash_line((mx, wt), (mx, wb), 1.0, line);
            self.c.dash_line((wl, my), (wr, my), 1.0, line);
            self.c.text(mx + 4.0, wt + 1.0, 16.0, &format!("{} %", pos.x.round()), F::Small, b.tx);
            self.c.text(wl + 4.0, my - 17.0, 16.0, &format!("{} %", pos.y.round()), F::Small, b.tx);
            self.hit(Ctl::Picture, (wl, wt, wr - wl, wb - wt), 1);
        } else {
            for s in 0..9 {
                if s != chosen {
                    let (px, py) = at(s);
                    self.c.stroke_dash((px, py, cw, chh), 2.0, 1.0, op(if na { b.tx3 } else { b.tx2 }), DashStyleDash);
                }
                let (tw, th) = ((wr - wl) / 3.0, (wb - wt) / 3.0);
                self.hit(Ctl::Spot(s), (wl + (s % 3) as f32 * tw, wt + (s / 3) as f32 * th, tw, th), 1);
            }
        }
        let (px, py) = at(chosen);
        let n = self.s.workspaces;
        let a = self.active_on(&self.w.pics.here.clone().unwrap_or_default()).min(8);
        let (lit, off) = if na || hc { (b.tx3, b.tx3) } else { (b.ws[a], b.bar_off) };
        self.c.boxed((px, py, cw, chh), 3.0, b.card, Some(if hc { b.tx } else { b.line }));
        self.c.fill_rr((px + 3.0, py + 3.0, 10.0, 10.0), 2.0, op(lit));
        for i in 0..n.min(3) {
            self.c.fill_rr((px + 17.0 + i as f32 * 10.0, py + 6.5, 8.0, 3.0), 1.5, op(if i == 0 { lit } else { off }));
        }
        self.ring((px, py, cw, chh), 3.0, Ctl::Picture);
    }

    /// Distance from the edges, Across or Down: a number for the chosen screen, greyed with
    /// the indicator off, and the distance with its reason for Center and Custom.
    fn place_num(&mut self, key: &'static str, y: f32, ch: &ScreenChoice) -> f32 {
        let pos = pos_of(self.s, ch.id.as_deref());
        let (title, base, lo, hi, unit) = match key {
            GAP => ("Distance from the edges", "From the screen's edges, above the taskbar, at 100% scale; des-k scales it with each screen. 0 to 400 px.", 0, 400, "px"),
            ACROSS => ("Across", "Where the card's center sits, from the left edge. 0 to 100 %.", 0, 100, "%"),
            _ => ("Down", "Where the card's center sits, from the top edge. 0 to 100 %.", 0, 100, "%"),
        };
        let v = pos_get(pos, key).round() as i64;
        let mut cap = vec![t(base)];
        let idle = key == GAP && matches!(pos.spot, settings::Spot::Center | settings::Spot::Custom);
        if idle {
            let why = if pos.spot == settings::Spot::Center { "Center sits in the middle" } else { "Custom places it by percent" };
            cap.push(P::Why(format!(" Not in use: {why}.")));
        }
        cap.extend(self.place_pieces(key, ch));
        let note = self.place_bad(key, ch);
        let na = !self.s.indicator_enabled || idle;
        let text = format!("{v} {unit}");
        let size = (self.stepper_w(&text), 32.0);
        self.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, title, base, self.ro || na, (None, Some(text)));
        self.row(y, title, &cap, size, |c, x, y| c.stepper(key, x, y, (v, unit), (v > lo, v < hi), na, false), note.as_ref())
    }

    // ---- Colors (spec 0020 AC-12) ----

    fn colors(&mut self, y: f32) -> f32 {
        let mut h = 0.0;
        if self.c.t.hc {
            let n = NoteSpec { tone: Tone::Info, icon: I_INFO, title: "High contrast is on, so des-k uses Windows' contrast colors".into(), lines: vec![t("Your colors stay in the file and come back when it's off.")], buttons: vec![], group: 1 };
            h += self.note(X0, y, self.cw, &n) + 12.0;
        }
        h += self.card(y + h, |c, y| {
            let mut h = 0.0;
            if !c.c.t.hc {
                h += c.sample(y);
                c.sep(y + h);
            }
            h += c.accent_row(y + h);
            if matches!(c.s.accent, settings::Accent::Rgb(_)) {
                c.sep(y + h);
                h += c.hex_row(ACCENT, y + h);
            }
            h
        });
        h += 12.0;
        h + self.card(y + h, |c, y| {
            let mut h = c.header(y, "Workspace colors", None, "In every des-k window");
            let n = c.s.workspaces;
            for (i, key) in WS_KEYS.iter().take(n).enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                h += c.hex_row(key, y + h);
            }
            let cap = [t(format!("One row per workspace you have ({n} now, set under ")), P::Link("Workspaces".into(), Ctl::Link(Link::Workspaces)), t("). Colors for workspaces you don't have are kept.")];
            h + 4.0 + c.caption(X0 + 16.0, y + h + 4.0, c.cw - 32.0, &cap) + 14.0
        })
    }

    /// des-k's own parts in the colors in use, and what each of the three colors paints.
    fn sample(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + 16.0;
        let foot = "des-k makes both neighbors from the accent, and picks black or white text for each. The des-k logo keeps its own colors.";
        let fh = self.measured(|c| c.c.para(0.0, 0.0, ww - 32.0, 16.0, &[(foot, F::Small, b.tx2)]));
        let body = 3.0 * 44.0;
        let well_h = 16.0 + body + 12.0 + 1.0 + 10.0 + fh + 12.0;
        self.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        // The little card: a header with its badge, a row with a switch and a button.
        let (cx, cy, cw) = (wx + 16.0, wy + 16.0 + (body - 80.0) / 2.0, 300.0);
        self.c.boxed((cx, cy, cw, 80.0), 6.0, b.card, Some(b.line));
        self.c.fill_rr((cx + 1.0, cy + 1.0, cw - 2.0, 34.0), 5.0, op(mix(b.card, b.acc_a, 0.09)));
        self.c.fill_rr((cx + 1.0, cy + 20.0, cw - 2.0, 15.0), 0.0, op(mix(b.card, b.acc_a, 0.09)));
        self.c.rule(cx + 1.0, cy + 35.0, cw - 2.0, op(b.line));
        self.c.circle(cx + 23.0, cy + 18.0, 11.0, Some(op(b.acc_b)), None);
        let dw = self.c.width("3", F::SmallBold);
        self.c.text(cx + 23.0 - dw / 2.0, cy + 7.0, 22.0, "3", F::SmallBold, b.on_acc_b);
        self.c.text(cx + 44.0, cy + 7.0, 22.0, "A section header", F::Bold, b.tx);
        self.c.text(cx + 12.0, cy + 36.0, 44.0, "A row", F::Bold, b.tx);
        let bw = self.c.width("Button", F::Body) + 28.0;
        let bx = cx + cw - 12.0 - bw;
        self.c.fill_rr((bx, cy + 42.0, bw, 32.0), 4.0, op(b.acc));
        self.c.text(bx + 14.0, cy + 42.0, 32.0, "Button", F::Body, b.on_acc);
        let sx = bx - 16.0 - 40.0;
        self.c.fill_rr((sx, cy + 48.0, 40.0, 20.0), 10.0, op(b.acc));
        self.c.circle(sx + 30.0, cy + 58.0, 6.0, Some(op(b.on_acc)), None);
        // What each color paints.
        let lx = cx + cw + 20.0;
        let legend = [
            (b.acc, "Buttons and switches", "the accent: also the chosen row and links"),
            (b.acc_a, "Headers and bands", "the accent turned 30° one way"),
            (b.acc_b, "Badges and marks", "the accent turned 30° the other way"),
        ];
        let mut lw: f32 = 0.0;
        for (i, (col, name, line)) in legend.iter().enumerate() {
            let ly = wy + 16.0 + i as f32 * 44.0;
            self.c.fill_rr((lx, ly + 6.0, 28.0, 20.0), 4.0, op(*col));
            self.c.text(lx + 38.0, ly, 22.0, name, F::Bold, b.tx);
            self.c.text(lx + 38.0, ly + 21.0, 16.0, line, F::Small, b.tx2);
            lw = lw.max(38.0 + self.c.width(name, F::Bold).max(self.c.width(line, F::Small)));
        }
        self.out.geo.legend = Some((lx, wy + 16.0, lw, body));
        let ry = wy + 16.0 + body + 12.0;
        self.c.rule(wx + 16.0, ry, ww - 32.0, op(b.line));
        self.c.para(wx + 16.0, ry + 11.0, ww - 32.0, 16.0, &[(foot, F::Small, b.tx2)]);
        16.0 + well_h + 16.0
    }

    /// A bad color in the file, as its row's note.
    fn color_bad(&self, key: &str) -> Option<NoteSpec> {
        let p = settings::problems().iter().find(|p| p.key == key)?;
        let what = ws_index(key).map_or("Windows' accent".to_string(), |i| format!("des-k's {}", WS_NAMES[i]));
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: format!("des-k can't use this value, so it uses {what}"), lines: vec![P::Mono(p.message.clone()), t(" Type a color here, or Reset takes the line out.")], buttons: vec![], group: 1 })
    }

    /// Accent color: Windows or My own, one stop. Greyed and blank in high contrast.
    fn accent_row(&mut self, y: f32) -> f32 {
        let own = matches!(self.s.accent, settings::Accent::Rgb(_));
        let hc = self.c.t.hc;
        let note = self.color_bad(ACCENT);
        let mut cap = vec![];
        if own {
            cap.push(t("Your own color, in light and dark. Default: Windows."));
        } else if ui::look().accent.is_none() {
            cap.push(t("des-k can't read Windows' accent color, so it uses des-k purple."));
        } else {
            cap.push(t("Follows Windows' accent color, in the shade Windows uses for light or dark: Settings › Personalization › Colors."));
        }
        if !own && let Some(c) = self.w.own {
            cap.push(t(format!(" My own goes back to #{c:06X}.")));
        }
        if own || note.is_some() {
            cap.push(P::Link(" Reset".into(), Ctl::Reset(ACCENT)));
        }
        let words = ["Windows", "My own"];
        let size = (self.seg_w(&words), 28.0);
        self.say(Ctl::Pick(ACCENT), UIA_ComboBoxControlTypeId, "Accent color", "", self.ro || hc, (None, Some(words[own as usize].into())));
        self.row(y, "Accent color", &cap, size, |c, x, y| c.seg(x, y, &words, Some(own as usize), hc, |i| Ctl::Choice(ACCENT, i), Ctl::Pick(ACCENT)), note.as_ref())
    }

    /// Your accent or Workspace N: the swatch and the hex box, with the default and Reset; a
    /// tinted band and a line under it while you type.
    fn hex_row(&mut self, key: &'static str, y: f32) -> f32 {
        let b = self.c.t.b;
        let hc = self.c.t.hc;
        let wi = ws_index(key);
        let saved = match wi {
            Some(i) => self.s.ws_colors[i],
            None => match self.s.accent {
                settings::Accent::Rgb(c) => Some(c),
                settings::Accent::Windows => None,
            },
        };
        let hex = self.w.hexes.iter().find(|x| x.key == key).cloned();
        let focused = self.w.focus == Some(Ctl::Hex(key));
        let title = wi.map_or("Your accent".to_string(), |i| format!("Workspace {}", i + 1));
        let mut cap = vec![];
        match (wi, saved) {
            (None, _) => cap.push(t("Six digits 0–9 or A–F after the #, or paste a color.")),
            (Some(i), None) => cap.push(t(format!("des-k's {}.", WS_NAMES[i]))),
            (Some(i), Some(_)) => {
                cap.push(t(format!("Your color. Default: #{:06X}.", ui::WS[i])));
                cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
            }
        }
        let bad = self.color_bad(key);
        if bad.is_some() && wi.is_some() && saved.is_none() {
            cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
        }
        let typing = !hc && hex.as_ref().is_some_and(|x| focused && Some(x.digits.as_str()) != shown_digits(self.c.t, self.s, key).as_deref());
        let note = bad.or_else(|| match &hex {
            Some(Hex { line: Some(l), .. }) => Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: (*l).into(), lines: vec![], buttons: vec![], group: 1 }),
            _ if typing => Some(NoteSpec { tone: Tone::Info, icon: I_INFO, title: String::new(), lines: vec![t("Enter saves it; Esc puts back the saved color. Leaving the box saves it too.")], buttons: vec![], group: 1 }),
            _ => None,
        });
        let digits = hex.as_ref().map_or_else(|| shown_digits(self.c.t, self.s, key).unwrap_or_default(), |x| x.digits.clone());
        // The color picker's preview shows here too, live (spec 0021 AC-5).
        let digits = match &self.w.pick {
            Some(p) if p.key == key && p.preview.is_some() => format!("{:06X}", p.color),
            _ => digits,
        };
        let swatch = (!hc).then(|| hex_color(&digits).unwrap_or(wi.map_or(b.acc, |i| b.ws[i])));
        let warn = hex.as_ref().is_some_and(|x| x.line.is_some());
        let sel = hex.as_ref().map(|x| (x.caret, x.anchor));
        let spoken = if hc { String::new() } else { format!("#{digits}") };
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str() }).collect();
        self.say(Ctl::Swatch(key), UIA_ButtonControlTypeId, &format!("{title} color"), "Opens the color picker.", self.ro || hc, (None, None));
        self.say(Ctl::Hex(key), UIA_EditControlTypeId, &format!("{title} color"), help.trim(), self.ro || hc, (None, Some(spoken)));
        let draw = |c: &mut Self, y: f32| {
            c.row(y, &title, &cap, (164.0, 32.0), |c, x, y| {
                c.swatch(key, x, y, wi, swatch);
                c.hex_box(key, x + 56.0, y, if hc { "" } else { &digits }, sel, warn, hc);
            }, note.as_ref())
        };
        if typing && !hc {
            let rh = self.measured(|c| draw(c, y));
            let band = op(mix(b.card, b.acc_a, 0.06));
            // Your accent is its card's last row: the band keeps the card's round bottom corners.
            self.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, rh - 1.0), 7.0, band);
            self.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, rh - 9.0), 0.0, band);
        }
        draw(self, y)
    }

    /// A color's swatch: a workspace's is its tile with its number in its ink; None (high
    /// contrast) draws it greyed and blank. A button: it opens the color picker (spec 0021 AC-3).
    fn swatch(&mut self, key: &'static str, x: f32, y: f32, wi: Option<usize>, color: Option<u32>) {
        let b = self.c.t.b;
        let bx = (x, y, 48.0, 32.0);
        let ctl = Ctl::Swatch(key);
        self.hit(ctl, bx, 1);
        self.out.geo.swatches.push((key, bx));
        match color {
            None => self.c.boxed(bx, 4.0, b.card, Some(b.tx3)),
            Some(c) => {
                self.c.fill_rr(bx, 4.0, op(c));
                if b.ws_edge > 0.0 {
                    self.c.stroke_rr((x + 0.5, y + 0.5, 47.0, 31.0), 3.5, 1.0, op(mix(c, 0, b.ws_edge)));
                }
            }
        }
        if let Some(i) = wi {
            let ink = color.map_or(b.tx3, ui::ink);
            self.c.text(x + 6.0, y + 32.0 - 17.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
        }
        if let Some(c) = color
            && self.hovered(ctl)
            && self.picks()
            && !self.focused(ctl)
        {
            self.cue(bx, ui::ink(c));
        }
        self.ring(bx, 4.0, ctl);
    }

    /// The hex box (spec 0020): a fixed grey #, then up to six digits in Cascadia Mono. Focus
    /// shows a 2 dip accent bottom edge and a caret, as WinUI's text box, no ring; fewer than
    /// six shows a warn edge. `na`: greyed and blank (high contrast).
    #[allow(clippy::too_many_arguments)]
    fn hex_box(&mut self, key: &'static str, x: f32, y: f32, digits: &str, sel: Option<(usize, usize)>, warn: bool, na: bool) {
        let b = self.c.t.b;
        let ctl = Ctl::Hex(key);
        let focused = self.w.focus == Some(ctl) && !na && !self.ro;
        let bx = (x, y, 108.0, 32.0);
        if warn {
            self.c.fill_rr(bx, 6.0, op(b.warn));
            self.c.fill_rr((x + 1.0, y + 1.0, 106.0, 30.0), 5.0, op(b.card));
            self.c.fill_rr((x + 1.0, y + 29.0, 106.0, 2.0), 0.0, op(b.warn));
        } else {
            self.field(bx, b.card, None);
            if focused {
                self.c.fill_rr((x + 1.0, y + 29.0, 106.0, 2.0), 0.0, op(b.acc));
            }
        }
        let cw = self.c.width("0", F::Code);
        let hx = x + 10.0;
        self.c.text(hx, y, 32.0, "#", F::Code, b.tx3);
        let dx = hx + cw;
        let (lo, hi) = sel.filter(|_| focused).map_or((0, 0), |(c, a)| (c.min(a), c.max(a)));
        if hi > lo {
            self.c.fill_rr((dx + lo as f32 * cw, y + 7.0, (hi - lo) as f32 * cw, 18.0), 2.0, op(b.acc));
        }
        for (i, ch) in digits.chars().enumerate() {
            let ink = if na || self.ro { b.tx3 } else if i >= lo && i < hi { b.on_acc } else { b.tx };
            self.c.text(dx + i as f32 * cw, y, 32.0, &ch.to_string(), F::Code, ink);
        }
        if focused && hi == lo && let Some((c, _)) = sel {
            self.c.fill_rr((dx + c as f32 * cw, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
        if self.focused(ctl) && !focused {
            self.ring(bx, 6.0, ctl);
        }
        self.hit(ctl, bx, 1);
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
        self.out.geo.band = Some((x0, y0, w2, h2));
        for (r, sc) in screens.iter().enumerate() {
            let ry = y0 + 8.0 + r as f32 * (th + 22.0);
            let name = self.name_of(sc);
            self.c.text(x0 + 16.0, ry, 14.0, &name, F::Small, if t.hc { b.card } else { white });
            let a = self.active_on(&sc.device);
            self.out.geo.groups.push((sc.device.clone(), (x0 + 16.0, ry + 16.0, 4.0 * tw + 3.0 * gap, th)));
            for i in 0..4 {
                let (x, yy) = (x0 + 16.0 + i as f32 * (tw + gap), ry + 16.0);
                // Past your count: an empty slot, as a screen not plugged in is drawn (spec 0021 AC-2).
                if i >= self.s.workspaces {
                    if t.hc {
                        self.c.boxed((x, yy, tw, th), 4.0, b.bez, Some(b.card));
                    } else {
                        self.c.fill_rr((x, yy, tw, th), 4.0, ui::alpha(white, 0.06));
                        self.c.stroke_rr((x + 0.5, yy + 0.5, tw - 1.0, th - 1.0), 3.5, 1.0, ui::alpha(white, 0.22));
                    }
                    continue;
                }
                let ctl = Ctl::ShowTile(r, i);
                self.hit(ctl, (x, yy, tw, th), 1);
                self.out.geo.shows.push((sc.device.clone(), i, (x, yy, tw, th)));
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
                if self.hovered(ctl) && self.picks() && i != a {
                    self.cue((x, yy, tw, th), 0x14141A);
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
        let mark = if t.dark && !t.hc { mix(b.acc_b, 0xFFFFFF, 0.28) } else { b.acc_b };
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
        // What's new opens inside des-k, so it has no open elsewhere mark (spec 0029 AC-8).
        let foot = [(Ctl::Welcome, "Welcome", "\u{EA80}", true), (Ctl::WhatsNew, "What's new", "\u{E734}", false), (Ctl::Open, "Open settings.toml", "\u{E8A5}", true)];
        let mut fy = vh - 14.0 - 38.0 * foot.len() as f32;
        self.c.rule(24.0, fy - 7.0, NAV - 48.0, op(b.line));
        for (ctl, name, icon, out) in foot {
            let bx = (12.0, fy, NAV - 24.0, 36.0);
            if self.hovered(ctl) {
                self.c.fill_rr(bx, 6.0, op(mix(b.bg, b.tx, 0.05)));
            }
            self.icon(24.0, fy, 36.0, icon, F::Icon, b.tx2);
            self.c.text(52.0, fy, 36.0, name, F::Body, b.tx);
            if out {
                let ow = self.c.width(I_OUT, F::IconSmall);
                self.icon(NAV - 24.0 - ow, fy, 36.0, I_OUT, F::IconSmall, b.tx2);
            }
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
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.hit(Ctl::Item(usize::MAX), (mx, my, mw, mh), 3);
    }
}

impl Ctx<'_, '_> {
    /// The open screen menu, placed like the touchpad's: the chosen line over the chooser.
    fn screens_menu(&mut self) {
        let Some((from, at, focus)) = self.w.chooser else { return };
        if matches!(from, Ctl::WallPick(_)) {
            return self.wall_menu();
        }
        let t = self.c.t;
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
        self.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.c.text(mx + 14.0, my + 4.0, 24.0, title, F::SmallBold, b.tx2);
        for (i, line) in choices.iter().enumerate() {
            let (name, side) = (&line.name, &line.side);
            let (x, y, w) = (mx + 4.0 + (i / rows) as f32 * col_w, my + 28.0 + (i % rows) as f32 * 28.0, col_w - 8.0);
            let ctl = Ctl::ScreenItem(i);
            let on = i == cur;
            let hot = self.hovered(ctl) || (self.w.kb && focus == i);
            let (fill, ink, check) = match () {
                _ if on && t.hc => (Some(b.hi), b.on_acc, b.on_acc),
                _ if on => (Some(b.track), b.tx, b.acc),
                _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx, b.acc),
                _ => (None, b.tx, b.acc),
            };
            if let Some(f) = fill {
                self.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
            }
            if on {
                self.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, check);
            }
            // Add's lines: the app's icon first, and the chip of where it starts before the side words.
            let mut nx = x + 30.0;
            if let Some(icon) = line.icon {
                self.c.icon(x + 8.0, y + 6.0, 16.0, icon);
                nx = x + 32.0;
            }
            let side_chip = line.chip.filter(|_| add);
            if let Some(k) = line.chip.filter(|_| !add) {
                self.ws_chip(nx, y + 5.0, k);
                nx += self.ws_chip_w(k) + 8.0;
            }
            let sw = if side.is_empty() { 0.0 } else { self.c.width(side, F::Small) + 10.0 };
            let cw = side_chip.map_or(0.0, |k| self.ws_chip_w(k) + 6.0);
            let shown = self.c.fit(name, F::Body, x + w - nx - 6.0 - sw - cw);
            let ink = if line.grey && !(on && t.hc) { if add { b.tx3 } else { b.tx2 } } else { ink };
            self.c.text(nx, y, 28.0, &shown, F::Body, ink);
            if let Some(k) = side_chip {
                self.ws_chip(x + w - sw - cw, y + 5.0, k);
            }
            if !side.is_empty() {
                self.c.text(x + w - sw, y, 28.0, side, F::Small, if on && t.hc { b.on_acc } else { b.tx2 });
            }
            if self.w.kb && focus == i {
                self.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (x, y, w, 28.0), 3);
        }
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.hit(Ctl::ScreenItem(usize::MAX), (mx, my, mw, mh), 3);
    }

    /// The open color picker, over everything (spec 0021 AC-4, sizes from the approved design):
    /// the header, the 12 presets, the wheel, the status line, Done and Cancel. Hidden while
    /// the page eases to make room for it.
    fn pick_card(&mut self) {
        let Some(p) = self.w.pick.as_ref() else { return };
        let Some(at) = p.placed else { return };
        if self.w.ease.is_some_and(|e| e.opening) {
            return;
        }
        let t = self.c.t;
        let b = t.b;
        let (x, y, kb) = (at.x, at.y, self.w.kb);
        let wi = ws_index(p.key);
        // The page's popup, as its menus are: edge, shadow, card.
        if !t.hc {
            self.c.fill_rr((x, y + 4.0, PICK_W, PICK_H), 8.0, ui::alpha(0, if t.dark { 0.30 } else { 0.08 }));
        }
        self.c.fill_rr((x - 1.0, y - 1.0, PICK_W + 2.0, PICK_H + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        self.c.fill_rr((x, y, PICK_W, PICK_H), 8.0, op(b.card));
        // Header: the chip (start and now once they differ), the title, the hex.
        let changed = p.color != p.start;
        let pale = if t.hc { op(b.tx) } else if t.dark { op(mix(b.bg, b.tx, 0.10)) } else { ui::alpha(0, 0.10) };
        let (hx, hy) = (x + 16.0, y + 12.0);
        let chip_w = match wi {
            None => 22.0,
            Some(_) if changed => 40.0,
            Some(_) => 28.0,
        };
        if wi.is_none() {
            self.c.circle(hx + 11.0, hy + 14.0, 11.0, Some(op(p.color)), None);
            self.c.circle(hx + 11.0, hy + 14.0, 10.5, None, Some((pale, 1.0)));
        } else {
            self.c.fill_rr((hx, hy + 3.0, chip_w, 22.0), 4.0, op(if changed { p.start } else { p.color }));
            if changed {
                self.c.fill_rr((hx + 20.0, hy + 3.0, 20.0, 22.0), 4.0, op(p.color));
                self.c.fill_rr((hx + 20.0, hy + 3.0, 4.0, 22.0), 0.0, op(p.color));
                self.c.fill_rr((hx + 20.0, hy + 3.0, 1.0, 22.0), 0.0, op(b.card));
            }
            // Above the fill, so a pale color still has an outline.
            self.c.stroke_rr((hx + 0.5, hy + 3.5, chip_w - 1.0, 21.0), 3.5, 1.0, pale);
        }
        let title = wi.map_or("Accent".to_string(), |i| format!("Workspace {}", i + 1));
        self.c.text(hx + chip_w + 10.0, hy, 28.0, &title, F::Bold, b.tx);
        let now = format!("#{:06X}", p.color);
        let right = x + PICK_W - 16.0 - self.c.width(&now, F::Mono);
        self.c.text(right, hy, 28.0, &now, F::Mono, b.tx);
        if changed {
            let was = format!("#{:06X} → ", p.start);
            self.c.text(right - self.c.width(&was, F::Mono), hy, 28.0, &was, F::Mono, b.tx2);
        }
        self.c.text(x + 16.0, y + 44.0, 20.0, "des-k colors", F::SmallBold, b.tx2);
        self.c.text(x + 172.0, y + 44.0, 20.0, "Any color", F::SmallBold, b.tx2);
        // The presets, 3 by 4: the one shown checked, a workspace's own color with its number.
        for (i, &c) in ui::PRESETS.iter().enumerate() {
            let (sx, sy) = (x + 16.0 + (i % 3) as f32 * 48.0, y + 68.0 + (i / 3) as f32 * 48.0);
            let bx = (sx, sy, 40.0, 40.0);
            let ink = ui::ink(c);
            self.c.fill_rr(bx, 4.0, op(c));
            if b.ws_edge > 0.0 {
                self.c.stroke_rr((sx + 0.5, sy + 0.5, 39.0, 39.0), 3.5, 1.0, op(mix(c, 0, b.ws_edge)));
            }
            if wi == Some(i) {
                self.c.text(sx + 5.0, sy + 22.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
            }
            if self.hovered(Ctl::Preset(i)) {
                self.cue(bx, ink);
            }
            if c == p.color {
                let (cx, cy) = (sx + 28.0, sy + 12.0);
                self.c.circle(cx, cy, 9.0, Some(op(b.card)), Some((ui::alpha(0, 0.12), 1.0)));
                let iw = self.c.width(I_OK, F::IconTiny);
                self.icon(cx - iw / 2.0, cy - 9.0, 18.0, I_OK, F::IconTiny, b.tx);
            }
            if kb && p.focus == Stop::Swatches(i) {
                self.c.stroke_rr((sx - 3.0, sy - 3.0, 46.0, 46.0), 7.0, 2.0, op(b.tx));
            }
            self.hit(Ctl::Preset(i), bx, 3);
        }
        // The wheel: the toned ring and the square at the hue, drawn pixel for pixel.
        let s = self.c.s;
        let ((cx, cy), sq) = wheel_at(at, s);
        let (n, ro, ri, _) = wheel_px(s);
        let half = n as f32 / 2.0 / s;
        RING.with_borrow_mut(|r| {
            if r.as_ref().is_none_or(|(k, _)| *k != s.to_bits()) {
                *r = Some((s.to_bits(), ring_px(s)));
            }
            if let Some((_, pic)) = r {
                self.c.image(cx - half, cy - half, pic);
            }
        });
        let hue = p.hsv.0;
        SQUARE.with_borrow_mut(|q| {
            if q.as_ref().is_none_or(|(k, h, _)| *k != s.to_bits() || *h != hue.to_bits()) {
                *q = Some((s.to_bits(), hue.to_bits(), square_px(s, hue)));
            }
            if let Some((_, _, pic)) = q {
                self.c.image(sq.0, sq.1, pic);
            }
        });
        if !t.dark && !t.hc {
            let line = ui::alpha(0, 0.10);
            self.c.circle(cx, cy, ro / s - 0.5, None, Some((line, 1.0)));
            self.c.circle(cx, cy, ri / s + 0.5, None, Some((line, 1.0)));
            self.c.stroke_rr((sq.0 + 0.5, sq.1 + 0.5, sq.2 - 1.0, sq.3 - 1.0), 2.5, 1.0, line);
        }
        let (h, sat, val) = p.hsv;
        let (mr, a) = ((ro + ri) / 2.0 / s, h.to_radians());
        self.mark(cx + a.sin() * mr, cy - a.cos() * mr, 9.0, ui::ring_color(h.round()), kb && p.focus == Stop::Ring);
        self.mark(sq.0 + sat * sq.2, sq.1 + (1.0 - val) * sq.3, 7.0, p.color, kb && p.focus == Stop::Square);
        self.hit(Ctl::Square, (sq.0 - 8.0, sq.1 - 8.0, sq.2 + 16.0, sq.3 + 16.0), 3);
        self.hit(Ctl::Ring, (cx - 106.0, cy - 106.0, 212.0, 212.0), 3);
        // Footer: the status line, Done, Cancel.
        let fy = y + 282.0;
        let cw = (self.c.width("Cancel", F::Body) + 28.0).max(96.0);
        let dw = (self.c.width("Done", F::Body) + 28.0).max(96.0);
        let (cancel_x, done_x) = (x + PICK_W - 16.0 - cw, x + PICK_W - 16.0 - cw - 8.0 - dw);
        let done = (done_x, fy, dw, 32.0);
        self.c.fill_rr(done, 6.0, op(if self.hovered(Ctl::PickDone) { mix(b.acc, b.card, 0.10) } else { b.acc }));
        if ui::contrast(b.acc, b.card) < 1.5 {
            self.c.stroke_rr((done_x + 0.5, fy + 0.5, dw - 1.0, 31.0), 5.5, 1.0, op(b.tx2));
        }
        let lw = self.c.width("Done", F::Body);
        self.c.text(done_x + (dw - lw) / 2.0, fy, 32.0, "Done", F::Body, b.on_acc);
        let cancel = (cancel_x, fy, cw, 32.0);
        self.field(cancel, if self.hovered(Ctl::PickCancel) { mix(b.card, b.tx, 0.05) } else { b.card }, None);
        let lw = self.c.width("Cancel", F::Body);
        self.c.text(cancel_x + (cw - lw) / 2.0, fy, 32.0, "Cancel", F::Body, b.tx);
        for (bx, stop) in [(done, Stop::Done), (cancel, Stop::Cancel)] {
            if kb && p.focus == stop {
                self.c.stroke_rr((bx.0 - 3.0, bx.1 - 3.0, bx.2 + 6.0, bx.3 + 6.0), 9.0, 2.0, op(b.tx));
            }
        }
        self.hit(Ctl::PickDone, done, 3);
        self.hit(Ctl::PickCancel, cancel, 3);
        let (sx, room) = (x + 16.0, done_x - 12.0 - x - 16.0);
        if p.failed.is_some() {
            let th = self.c.measuring().para(0.0, 0.0, room - 24.0, 16.0, &[(NOT_SAVED, F::SmallBold, b.tx)]);
            let top = fy + (32.0 - th) / 2.0;
            self.icon(sx, top, 16.0, I_WARN, F::IconSmall, b.warn);
            self.c.para(sx + 24.0, top, room - 24.0, 16.0, &[(NOT_SAVED, F::SmallBold, b.tx)]);
        } else if let Some(i) = wi
            && let Some((j, same)) = dup_of(p.color, i, &b.ws, self.s.workspaces)
        {
            let line = format!("Workspace {} {}", j + 1, if same { "uses this color too." } else { "has a very close color." });
            let th = self.c.measuring().para(0.0, 0.0, room - 30.0, 16.0, &[(&line, F::Small, b.tx2)]);
            let top = fy + (32.0 - th) / 2.0;
            self.c.fill_rr((sx, top, 22.0, 16.0), 3.0, op(b.ws[j]));
            if b.ws_edge > 0.0 {
                self.c.stroke_rr((sx + 0.5, top + 0.5, 21.0, 15.0), 2.5, 1.0, op(mix(b.ws[j], 0, b.ws_edge)));
            }
            self.c.text(sx + 4.0, top + 3.0, 13.0, &(j + 1).to_string(), F::TinyBold, b.on_ws[j]);
            self.c.para(sx + 30.0, top, room - 30.0, 16.0, &[(&line, F::Small, b.tx2)]);
        }
        // Last, so every part above wins the click.
        self.hit(Ctl::PickCard, (x, y, PICK_W, PICK_H), 3);
    }

    /// A mark on the ring or square: a white disc with a dark halo, filled in the middle.
    fn mark(&self, x: f32, y: f32, r: f32, fill: u32, focus: bool) {
        self.c.circle(x, y, r + 1.5, Some(ui::alpha(0, 0.45)), None);
        self.c.circle(x, y, r, Some(op(0xFFFFFF)), None);
        self.c.circle(x, y, r - 3.0, Some(op(fill)), None);
        if focus {
            self.c.circle(x, y, r + 4.5, None, Some((op(self.c.t.b.tx), 2.0)));
        }
    }
}

/// The screen menu's choices and the chosen one: All screens when the page's screen is gone.
fn chosen(w: &Win, s: &Settings) -> (Vec<ScreenChoice>, usize) {
    let list = screen_choices(&w.pics.view, s);
    let i = w.screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// The Names card's choices and the chosen one: All screens when the card's screen is gone.
fn names_chosen(w: &Win) -> (Vec<NameChoice>, usize) {
    let list = name_choices(&w.pics.view);
    let i = w.names_screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// The Wallpaper card's chooser lines, the Names card's (spec 0026 AC-13), and the chosen one.
fn wall_chosen(w: &Win) -> (Vec<NameChoice>, usize) {
    let list = name_choices(&w.pics.view);
    let i = w.wall_screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// Whether the Wallpaper card shows its screen chooser: with two or more known screens, and
/// with one that has its own pictures, which otherwise no row could reach: every change would
/// go to All screens, which that screen ignores (tried live 2026-11-21).
fn wall_two(w: &Win, s: &Settings) -> bool {
    let (list, _) = wall_chosen(w);
    list.len() > 2 || list.get(1).and_then(|c| c.id.as_ref()).is_some_and(|id| s.wallpaper_screens.get(id).is_some_and(|o| o.iter().any(Option::is_some)))
}

/// The screen the Wallpaper card's rows write for: None for All screens, and while the card
/// has no chooser.
fn wall_screen(w: &Win, s: &Settings) -> Option<String> {
    let (list, i) = wall_chosen(w);
    if wall_two(w, s) { list[i].id.clone() } else { None }
}

/// What leads a Wallpaper menu line (spec 0028 AC-11): a check (Windows wallpaper), a tick box
/// (a picture), a folder mark and a check (a slideshow folder), or Open folder's marks.
#[derive(Clone, Copy)]
enum Lead {
    Check(bool),
    Tick(bool),
    Folder(bool),
    Open,
}

/// What a menu button shows before its name.
#[derive(Clone, Copy)]
enum Mark {
    Chip(usize),
    Glyph(&'static str),
    Stack, // a Wallpaper row's set of pictures (spec 0028)
}

/// A line of the open screen menu: its name and the words on its right, and for the Apps
/// section's menus an app's icon, a workspace chip, or a grey name.
#[derive(Clone, Default)]
struct Choice {
    name: String,
    side: String,
    icon: Option<HICON>,
    chip: Option<usize>,
    grey: bool,
}

impl Choice {
    fn new(name: &str, side: &str) -> Choice {
        Choice { name: name.into(), side: side.into(), ..Choice::default() }
    }
}

/// The open screen menu's lines and the chosen one, if any: Where it shows' chooser, the
/// Names card's, or an Apps menu (spec 0027 AC-13).
fn chooser_lines(w: &Win, s: &Settings, from: Ctl) -> (Vec<Choice>, Option<usize>) {
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

// ---- Apps (spec 0027 AC-13, AC-14) ----

/// Screen `id` as des-k names it: plugged in or remembered, as `ws::label` gave it, else your
/// rename, else the id.
fn screen_called(v: &crate::ws::View, s: &Settings, id: &str) -> String {
    v.present
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .or_else(|| v.waiting.iter().find(|w| w.id == id).map(|w| w.name.clone()))
        .or_else(|| s.screen_rename(id).map(String::from))
        .unwrap_or_else(|| id.to_string())
}

/// Workspace `k` as a rule for `screen` (None: any) shows it: that screen's name for it, else
/// every screen's, else "Workspace N" in grey (true).
fn ws_label(s: &Settings, screen: Option<&str>, k: usize) -> (String, bool) {
    let name = match screen {
        Some(id) => s.ws_name(id, k).map(String::from),
        None => s.names.get(k).filter(|n| !n.is_empty() && k < s.workspaces).cloned(),
    };
    name.map_or_else(|| (format!("Workspace {}", k + 1), true), |n| (n, false))
}

/// A rule's screen menu, each line with the id it writes: Any screen, the screens plugged in,
/// remembered ones, then the rule's own id if des-k never saw that screen.
fn app_screens(v: &crate::ws::View, rule: &settings::Rule) -> Vec<(Option<String>, Choice)> {
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
fn addable<'a>(w: &'a Win, s: &Settings) -> Vec<&'a crate::apps::Seen> {
    w.pics.apps.iter().filter(|a| !s.apps.iter().any(|(k, _)| *k == a.key)).collect()
}

/// A rule row's note, if one applies, and whether it is a warning: a rule past your count or
/// for an unplugged screen still works; one for a screen des-k never saw is skipped (AC-7, AC-9).
fn app_note(v: &crate::ws::View, s: &Settings, rule: &settings::Rule) -> Option<(String, bool)> {
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
fn app_choice(w: &Win, s: &Settings, from: Ctl, i: usize) -> Option<Edit> {
    match from {
        Ctl::AppAdd => {
            let a = *addable(w, s).get(i)?;
            let screen = w.pics.view.present.iter().find(|p| p.device == a.device).map(|p| p.id.clone());
            Some(app_edit(&a.key, AppChange::Add(a.ws, screen)))
        }
        Ctl::AppWs(r) => {
            let (key, rule) = s.apps.get(r)?;
            (i != rule.workspace && i < s.workspaces).then(|| app_edit(&settings::app_key_in_file(key), AppChange::Ws(i)))
        }
        Ctl::AppScreen(r) => {
            let (key, rule) = s.apps.get(r)?;
            let id = app_screens(&w.pics.view, rule).into_iter().nth(i)?.0;
            (id != rule.screen).then(|| app_edit(&settings::app_key_in_file(key), AppChange::Screen(id)))
        }
        _ => None,
    }
}

/// The digits a hex box shows when no one is typing: the color in use, None in high contrast.
fn shown_digits(t: Theme, s: &Settings, key: &str) -> Option<String> {
    if t.hc {
        return None;
    }
    let c = match ws_index(key) {
        Some(i) => s.ws_colors[i].unwrap_or(ui::WS[i]),
        None => match s.accent {
            settings::Accent::Rgb(c) => c,
            settings::Accent::Windows => t.b.acc,
        },
    };
    Some(format!("{c:06X}"))
}

/// A name row as the Names card draws it and the text box takes it (spec 0023 AC-12, AC-13).
struct NameLook {
    screen: Option<String>, // the screen the row writes for; None: All screens
    state: NameState,
    saved: String, // what its box holds when saved
    shown: String, // what it shows without focus: your text that failed to save, else the saved
    unsaved: bool,
    hint: String, // what it shows empty
    all: String,  // All screens' name for its workspace
}

fn name_look(w: &Win, s: &Settings, row: Nm) -> NameLook {
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
    let mut x = Ctx { c, w, s, ro: settings::parse_error().is_some(), cw, top0: 0.0, out: Out { hits: vec![], height: 0.0, fixed: 0.0, tops: vec![], cards: vec![], said: vec![], geo: Geo::default() } };
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
    x.screens_menu();
    x.pick_card();
    // The menu's hits first, then the banners', the side list's, the column's.
    x.out.hits.sort_by_key(|h| std::cmp::Reverse(h.group));
    x.out
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

// ---- The page's text box (spec 0022 AC-10, spec 0023 AC-13) ----
//
// A Windows EDIT, as the finder's search box is, so typing, selection, IME and the clipboard
// all work. It serves whichever text field has the page's focus, the popup words or a name, and
// shows only while one does, over the field the page paints; the rest of the time the page
// paints the text, so a menu or the color picker drawn over the page is never covered by it.

thread_local! {
    static BOX_EDIT: std::cell::Cell<Option<HWND>> = const { std::cell::Cell::new(None) };
    static BOX_PROC: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    static BOX_FONT: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    // The field it serves, and for a name the screen it writes for, as it was when the box
    // took that field: leaving saves where you typed, whatever the card shows by then.
    static BOX_FOR: RefCell<Option<(Ctl, Option<String>)>> = const { RefCell::new(None) };
    // What it shows while empty, and in which ink: a name box's hint.
    static BOX_HINT: RefCell<(String, u32)> = const { RefCell::new((String::new(), 0)) };
}

fn make_box(page: HWND, inst: windows::Win32::Foundation::HINSTANCE) {
    unsafe {
        let style = WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        let Ok(e) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), style, 0, 0, 0, 0, Some(page), None, Some(inst), None) else {
            return warn!("settings page: no text box for the popup words and names");
        };
        BOX_PROC.set(SetWindowLongPtrW(e, GWLP_WNDPROC, box_proc as *const () as isize));
        BOX_EDIT.set(Some(e));
    }
}

fn edit_text(e: HWND) -> String {
    let mut buf = [0u16; settings::ATTENTION_MAX + 1];
    let n = unsafe { GetWindowTextW(e, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Puts `text` in the box with the caret at its end.
fn set_edit(e: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(e, &windows::core::HSTRING::from(text));
        let end = GetWindowTextLengthW(e) as usize;
        SendMessageW(e, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
    }
}

/// Shows the box over the field that has the page's focus while it can be typed in, and moves
/// Windows' keyboard focus to match; moving on to another field saves the one it leaves first.
/// It takes the file's text only while it has no focus, so a reload never overwrites what you
/// are typing.
fn place_box() {
    let Some(e) = BOX_EDIT.get() else { return };
    let Some((page, want, at, font, held, hint)) = get(|w| {
        let s = settings::get();
        let open = w.menu.is_some() || w.pick.is_some() || w.chooser.is_some();
        let can = settings::parse_error().is_none() && !open;
        let (want, held, hint) = match w.focus {
            Some(Ctl::Words) if can && s.attention_enabled => (Some((Ctl::Words, None)), s.attention_text.clone(), (String::new(), 0)),
            Some(Ctl::Name(row)) if can => {
                let l = name_look(w, s, row);
                (Some((Ctl::Name(row), l.screen)), l.shown, (l.hint, painted(w).b.hint))
            }
            _ => (None, String::new(), (String::new(), 0)),
        };
        let at = want.as_ref().and_then(|(c, _)| w.hits.iter().find(|h| h.ctl == *c && h.group == 1)).map(|h| h.at);
        let at = at.filter(|b| b.1 >= w.fixed && b.1 + b.3 <= w.vh);
        let s = w.scale;
        let px = |(x, y, bw, bh): Bx| ((x * s).round() as i32, (y * s).round() as i32, (bw * s).round() as i32, (bh * s).round() as i32);
        (w.hwnd, want, at.map(|b| px((b.0 + 12.0, b.1 + 6.0, b.2 - 24.0, 20.0))), w.fonts.get(F::Body), held, hint)
    }) else {
        return;
    };
    unsafe {
        let has = GetFocus() == e;
        let Some((x, y, bw, bh)) = at else {
            let _ = ShowWindow(e, SW_HIDE);
            if has {
                let _ = SetFocus(Some(page)); // leaving the box saves it
            }
            return;
        };
        // Another field has it: leaving that one saves it, and lays out, which places it again.
        if has && BOX_FOR.with_borrow(|f| *f != want) {
            let _ = SetFocus(Some(page));
            if GetFocus() != e {
                place_box();
            }
            return;
        }
        if !has {
            if BOX_FOR.with_borrow(|f| *f != want) {
                // Windows counts UTF-16 units, as the parser does (spec 0022 AC-9, spec 0023 AC-2).
                let (limit, name) = match want {
                    Some((Ctl::Name(row), _)) => (settings::NAME_MAX, name_title(row)),
                    _ => (settings::ATTENTION_MAX, "Popup words".to_string()),
                };
                SendMessageW(e, EM_SETLIMITTEXT, Some(WPARAM(limit)), Some(LPARAM(0)));
                uia::name_control(e, &name);
                BOX_FOR.set(want);
            }
            if edit_text(e) != held {
                set_edit(e, &held);
            }
        }
        if BOX_HINT.replace(hint.clone()) != hint {
            let _ = InvalidateRect(Some(e), None, true);
        }
        // A new font (the page's dpi changed) sets the margins back to the font's own.
        if BOX_FONT.replace(font.0 as isize) != font.0 as isize {
            SendMessageW(e, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(0)));
            SendMessageW(e, EM_SETMARGINS, Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)), Some(LPARAM(0)));
        }
        let _ = SetWindowPos(e, None, x, y, bw, bh, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        if !has && GetFocus() == page {
            let _ = SetFocus(Some(e));
            let end = GetWindowTextLengthW(e) as usize;
            SendMessageW(e, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
        }
    }
}

/// Saves the box's words: empty or the default removes the key (AC-10), anything else is
/// written as typed, and unchanged words write nothing.
fn save_words(text: &str) {
    let saved = settings::get().attention_text.clone();
    if text.trim().is_empty() || text == settings::ATTENTION_TEXT {
        if saved != settings::ATTENTION_TEXT {
            commit(Edit::Remove(path_of(WORDS)), "Popup words");
        }
    } else if text != saved {
        commit(Edit::Value(WORDS, text.into()), "Popup words");
    }
}

/// Saves a name box (spec 0023 AC-12, AC-13). A failed write keeps your text for its row, shown
/// as Not saved, until a good save or Esc; a save, or text that writes nothing, forgets it.
fn save_name(screen: Option<String>, row: Nm, text: &str, enter: bool) {
    let _ = set(|w| w.unsaved.retain(|u| !(u.0 == screen && u.1 == row)));
    match name_edit(settings::get(), screen.as_deref(), row, text, enter) {
        None => layout(),
        Some(edit) => {
            if !wrote(edit, &name_title(row)) {
                let _ = set(|w| w.unsaved.push((screen, row, text.to_string())));
                layout();
            }
        }
    }
}

/// Saves the field the box serves.
fn save_box(text: &str, enter: bool) {
    match BOX_FOR.with_borrow(|f| f.clone()) {
        Some((Ctl::Name(row), screen)) => save_name(screen, row, text, enter),
        Some((Ctl::Words, _)) => save_words(text),
        _ => {}
    }
}

/// The text the box's field holds in the file, and whether your text for it failed to save.
fn box_saved() -> (String, bool) {
    match BOX_FOR.with_borrow(|f| f.clone()) {
        Some((Ctl::Name(row), screen)) => {
            let failed = get(|w| w.unsaved.iter().any(|u| u.0 == screen && u.1 == row)) == Some(true);
            (name_held(settings::get(), screen.as_deref(), row).1, failed)
        }
        _ => (settings::get().attention_text.clone(), false),
    }
}

unsafe extern "system" fn box_proc(e: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let own: WNDPROC = std::mem::transmute(BOX_PROC.get());
        let page = GetParent(e).unwrap_or_default();
        match m {
            WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                // The page's own keys: Tab, Up and Down move on, as from any stop.
                VK_TAB | VK_UP | VK_DOWN => {
                    let _ = PostMessageW(Some(page), m, wp, lp);
                    return LRESULT(0);
                }
                // Saves and stays. Text that failed to save stays in the box to try again.
                VK_RETURN => {
                    save_box(&edit_text(e), true);
                    let (now, failed) = box_saved();
                    if !failed && edit_text(e) != now {
                        set_edit(e, &now);
                    }
                    return LRESULT(0);
                }
                // Esc puts back the saved text; with nothing changed it is the page's Esc.
                VK_ESCAPE => {
                    let (saved, failed) = box_saved();
                    if edit_text(e) == saved && !failed {
                        let _ = PostMessageW(Some(page), m, wp, lp);
                    } else {
                        if let Some((Ctl::Name(row), screen)) = BOX_FOR.with_borrow(|f| f.clone()) {
                            let _ = set(|w| w.unsaved.retain(|u| !(u.0 == screen && u.1 == row)));
                        }
                        set_edit(e, &saved);
                        layout();
                    }
                    return LRESULT(0);
                }
                _ => {}
            },
            // Their characters would only beep.
            WM_CHAR if matches!(wp.0, 0x09 | 0x0D | 0x1B) => return LRESULT(0),
            WM_MOUSEWHEEL => return SendMessageW(page, m, Some(wp), Some(lp)),
            WM_KILLFOCUS => {
                let r = CallWindowProcW(own, e, m, wp, lp);
                let text = edit_text(e);
                let _ = set(|w| {
                    w.words = None;
                    w.typed = None;
                });
                save_box(&text, false);
                layout();
                return r;
            }
            // An empty name box shows its hint as the page paints it: without Common Controls 6,
            // which des-k's manifest does not ask for, an EDIT has no cue banner of its own.
            WM_PAINT => {
                let r = CallWindowProcW(own, e, m, wp, lp);
                let (hint, ink) = BOX_HINT.with_borrow(|h| h.clone());
                if !hint.is_empty() && GetWindowTextLengthW(e) == 0 {
                    let dc = GetDC(Some(e));
                    let font = HGDIOBJ(SendMessageW(e, WM_GETFONT, None, None).0 as *mut _);
                    let was = SelectObject(dc, font);
                    SetTextColor(dc, ui::cref(ink));
                    SetBkMode(dc, TRANSPARENT);
                    let _ = HideCaret(Some(e));
                    let text: Vec<u16> = hint.encode_utf16().collect();
                    let _ = TextOutW(dc, 0, 0, &text);
                    let _ = ShowCaret(Some(e));
                    SelectObject(dc, was);
                    ReleaseDC(Some(e), dc);
                }
                return r;
            }
            _ => {}
        }
        CallWindowProcW(own, e, m, wp, lp)
    }
}

/// The theme the page paints with: the color picker's preview while it shows a color the
/// file doesn't hold (spec 0021 AC-5).
fn painted(w: &Win) -> Theme {
    w.pick.as_ref().and_then(|p| p.preview).unwrap_or(w.theme)
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the page out again for its scroll, keeps the focus on a stop that is still there, and
/// gives Narrator the stops as painted. An open color picker keeps its place while it still
/// fits, is placed again (never scrolling) when it doesn't, and closes when what you clicked is
/// gone (spec 0021 AC-14).
fn layout() {
    lay();
    match pick_fit() {
        Some(Fit::Placed) => lay(),
        Some(Fit::Gone) => return pick_close(How::Quiet),
        _ => {}
    }
    place_box();
    repaint();
}

fn lay() {
    for _ in 0..2 {
        let Some(out) = get(|w| unsafe {
            let dc = GetDC(Some(w.hwnd));
            let out = {
                let mut c = Canvas::new(dc, false, w.scale, painted(w), &w.fonts);
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
            w.cards = out.cards;
            w.hits = out.hits;
            w.geo = out.geo;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                // A Reset or Same as all screens that just took itself away: the focus moves to
                // its row's control.
                w.focus = match f {
                    Ctl::NameSame(k) => Some(Ctl::Name(Nm::Ws(k))).filter(|c| w.hits.iter().any(|h| h.ctl == *c)),
                    Ctl::WallUse(k) => Some(Ctl::WallPick(k)).filter(|c| w.hits.iter().any(|h| h.ctl == *c)),
                    _ => f.key().and_then(|k| w.hits.iter().find(|h| h.ctl.key() == Some(k) && h.ctl.stop() != Ctl::Reset(k)).map(|h| h.ctl.stop())),
                };
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
}

/// Shows what des-k read back from the file. Called after every reload, so the color picker's
/// close rules that follow a reload live here (spec 0021 AC-10, AC-12).
pub fn refresh() {
    if hwnd().is_some() {
        if get(must_close) == Some(true) {
            pick_close(How::Quiet);
        }
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

/// Scrolls so `card` sits at the top, as `jump` does for its section.
fn jump_card(card: Card) {
    if let Some(top) = get(|w| w.cards.iter().find(|(c, _)| *c == card).map(|(_, t)| *t)).flatten() {
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
        Link::Workspaces => jump(Some(Section::Workspaces)),
    }
}

/// The page's own writers change its notes: never while the color picker is open, which takes
/// every click and key and holds the notes still (spec 0021 AC-13).
fn assert_no_pick() {
    debug_assert!(get(|w| w.pick.is_none()) != Some(false), "the page wrote while the color picker was open");
}

/// Writes one value; a failure shows what stays and why until the next good save.
fn save(key: &'static str, v: i64) {
    assert_no_pick();
    let Some(row) = find_row(key) else { return };
    match settings::write(Edit::Value(key, file_value(row, v))) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Err(why) => failed(row.title, why),
    }
}

/// A failed write's note, with Windows' words.
fn failed(what: &str, why: String) {
    assert_no_pick();
    let said = windows_words(&why);
    let _ = set(|w| w.error = Some((what.to_string(), said)));
    refresh();
}

/// Writes an edit that is not one row's value; a failure shows like a failed save.
fn commit(edit: Edit, title: &str) {
    let _ = wrote(edit, title);
}

/// `commit`, saying whether it saved.
fn wrote(edit: Edit, title: &str) -> bool {
    assert_no_pick();
    match settings::write(edit) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
            true
        }
        Err(why) => {
            failed(title, why);
            false
        }
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
    crate::welcome::cancel_recording();
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

/// What a key change does: nothing, a write, or a clash to ask about first.
pub(crate) enum Plan {
    Nothing,
    Commit(Edit, &'static str), // and the action's title, for a failed save
    Ask(Prompt),
}

/// Carries out a plan here: writes it, or shows its clash under its row.
fn run(plan: Plan) {
    match plan {
        Plan::Nothing => {}
        Plan::Commit(edit, title) => commit(edit, title),
        Plan::Ask(p) => ask(p.key, p),
    }
}

/// A key for a list: saved, or the clash prompt when another action has it.
fn combo_recorded(key: &'static str, replace: Option<usize>, combo: Combo) {
    run(plan_combo(key, replace, combo));
}

pub(crate) fn plan_combo(key: &'static str, replace: Option<usize>, combo: Combo) -> Plan {
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
        return Plan::Nothing;
    }
    let title = entry(name).1;
    let caps = crate::hints::caps(&combo);
    let words = spelled(&caps);
    let edit = Edit::Combos(key, mine);
    let prompt = match s.combo_owner(name, combo) {
        None => return Plan::Commit(edit, title),
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
    Plan::Ask(prompt)
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
    run(plan_digits(key, mods));
}

pub(crate) fn plan_digits(key: &'static str, mods: u8) -> Plan {
    let (s, name) = (settings::get(), name_of(key));
    if s.set(name) == Some(mods) {
        return Plan::Nothing;
    }
    let edit = Edit::Digits(key, Some(mods));
    let Some(other) = s.set_owner(name, mods) else { return Plan::Commit(edit, entry(name).1) };
    let (other_key, other_title) = entry(other);
    let caps = digit_caps(mods);
    let prompt = Prompt {
        key,
        title: format!("{} is already in use", spelled(&caps)),
        held: caps,
        sub: format!("“{other_title}” uses it. Move it here and “{other_title}” turns off. Cancel changes nothing."),
        edit: Edit::Pair(Box::new(edit), Box::new(Edit::Digits(other_key, None))),
    };
    Plan::Ask(prompt)
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
    run(plan_give(key, desk));
}

pub(crate) fn plan_give(key: &'static str, desk: bool) -> Plan {
    let Some(sh) = shortcut(key) else { return Plan::Nothing };
    let Some((wk, _)) = sh.win else { return Plan::Nothing };
    let s = settings::get();
    if desk == has_win(s, sh) || (desk && took(s, sh).is_some()) {
        return Plan::Nothing;
    }
    match (wk, desk) {
        (WinKey::Combo(c), true) => plan_combo(key, None, c),
        (WinKey::Digits(m), true) => plan_digits(key, m),
        (WinKey::Combo(c), false) => {
            let rest: Vec<Combo> = s.list(name_of(key)).iter().filter(|x| **x != c).copied().collect();
            Plan::Commit(Edit::Combos(key, rest), sh.title)
        }
        (WinKey::Digits(_), false) => Plan::Commit(Edit::Digits(key, None), sh.title),
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
fn open_menu(row: usize, times: u8, family: Option<usize>) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == Ctl::Picker(row)).map(|h| h.at)).flatten() else { return };
    let m = Menu { row, times, family, at, focus: 0 };
    let focus = menu_lines(&m, settings::get()).iter().position(|l| l.on).unwrap_or(0);
    let _ = set(|w| w.menu = Some(Menu { focus, ..m }));
    layout();
}

fn close_menu() {
    let _ = set(|w| {
        w.menu = None;
        w.chooser = None;
    });
    layout();
}

/// The gesture menu, the screen menu or the color picker: while one is open, only it takes clicks.
fn menu_open(w: &Win) -> bool {
    w.menu.is_some() || w.chooser.is_some() || w.pick.is_some()
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
        Ctl::Welcome => crate::welcome::show(),
        Ctl::WhatsNew => crate::whats_new::show_current(),
        Ctl::Open => crate::tray::open(&settings::path()),
        Ctl::Toggle(key) if unused(s, key).is_empty() => save(key, 1 - value(s, key)),
        Ctl::Toggle(_) => {} // greyed: kept but not in use
        Ctl::Choice(ACCENT, i) => set_accent(i == 1),
        Ctl::Choice(MODE, i) => set_mode(i == 1),
        Ctl::Minus(key) if is_place(key) => step_place(key, false),
        Ctl::Plus(key) if is_place(key) => step_place(key, true),
        Ctl::Reset(key) if key == ACCENT || ws_index(key).is_some() => reset_color(key),
        Ctl::Reset(key) if is_place(key) => reset_place(key),
        Ctl::Reset(WORDS) => commit(Edit::Remove(path_of(WORDS)), "Popup words"),
        Ctl::Hex(_) | Ctl::Words | Ctl::Name(_) | Ctl::Picture | Ctl::ScreenItem(usize::MAX) => {}
        Ctl::NameSame(k) => same_name(k),
        Ctl::Spot(i) => set_place(SPOT, i as f64),
        Ctl::Screens | Ctl::NameScreens | Ctl::WallScreens | Ctl::WallPick(_) | Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_) => open_chooser(ctl),
        Ctl::WallSame(k) => {
            if let Some(screen) = get(|w| wall_screen(w, settings::get())).flatten() {
                commit(Edit::Remove(wall_path(Some(&screen), k)), &format!("Workspace {} wallpaper", k + 1));
            }
        }
        Ctl::AppX(r) => {
            if let Some((key, _)) = s.apps.get(r) {
                commit(app_edit(&settings::app_key_in_file(key), AppChange::Remove), "Apps");
            }
        }
        Ctl::ScreenItem(i) => choose_screen(i),
        Ctl::WallTick(i) => tick_wall(i),
        Ctl::WallUse(k) => {
            let ids: Vec<String> = get(|w| w.pics.view.present.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
            if let Some(edit) = wall_use_edit(settings::get(), &ids, k) {
                commit(edit, &format!("Workspace {} wallpaper", k + 1));
            }
        }
        Ctl::SameAll => same_as_all(),
        Ctl::ShowThere => show_there(),
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
        Ctl::WsTile(row, i) | Ctl::ShowTile(row, i) => {
            if let Some(dev) = get(|w| w.pics.screens.get(row).map(|s| s.device.clone())).flatten() {
                let anchor = if matches!(ctl, Ctl::WsTile(..)) { Anchor::Tile(dev, i) } else { Anchor::Show(dev, i) };
                pick_open(WS_KEYS[i], anchor, None);
            }
        }
        Ctl::Swatch(key) => pick_open(key, Anchor::Swatch(key), Some(ctl)),
        Ctl::Preset(i) => pick_preset(i),
        Ctl::PickDone => pick_close(How::Keep(true)),
        Ctl::PickCancel => pick_close(How::Undo),
        Ctl::Ring | Ctl::Square | Ctl::PickCard => {} // the wheel acts on the press
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
    if let Some(k) = IV_KEYS.iter().position(|x| *x == key) {
        if let Some(edit) = wall_interval_edit(settings::get(), k, up) {
            commit(edit, &format!("Workspace {} time per picture", k + 1));
        }
        return;
    }
    if let Some(Row { kind: Kind::Ladder(ladder), .. }) = find_row(key) {
        let v = value(settings::get(), key);
        let n = ladder_step(ladder, v, up);
        if n != v {
            save(key, n);
        }
        return;
    }
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

// ---- Colors and Where it shows: what the controls do (spec 0020) ----

const INCOMPLETE: &str = "Six digits are needed. Nothing is saved until there are six; Esc puts back the saved color.";
const REFUSED: &str = "That isn't a color. Paste six digits 0–9 or A–F; nothing changed.";
const COULDNT: &str = "Couldn't save your color. Enter tries again.";

/// Windows or My own (AC-12). My own writes the accent des-k uses now, so nothing changes
/// until you type, or the last own color while the page is open. Off in high contrast.
fn set_accent(own: bool) {
    let Some((hc, acc, last)) = get(|w| (w.theme.hc, w.theme.b.acc, w.own)) else { return };
    if hc {
        return;
    }
    match (own, settings::get().accent) {
        (false, settings::Accent::Rgb(c)) => {
            let _ = set(|w| w.own = Some(c));
            commit(Edit::At(path_of(ACCENT), "windows".into()), "Accent color");
        }
        (true, settings::Accent::Windows) => commit(Edit::At(path_of(ACCENT), format!("#{:06X}", last.unwrap_or(acc)).into()), "Accent color"),
        _ => {}
    }
}

fn color_title(key: &str) -> String {
    ws_index(key).map_or("Your accent".into(), |i| format!("Workspace {} color", i + 1))
}

/// Reset takes a color's key out of the file; the page keeps your last own accent.
fn reset_color(key: &'static str) {
    if key == ACCENT
        && let settings::Accent::Rgb(c) = settings::get().accent
    {
        let _ = set(|w| w.own = Some(c));
    }
    let _ = set(|w| w.hexes.retain(|x| x.key != key));
    commit(Edit::Remove(path_of(key)), &color_title(key));
}

/// Focus came to a hex box: Tab selects its digits, a click puts the caret there. Nothing to
/// type in high contrast or read only.
fn enter_hex(key: &'static str, caret: Option<usize>) {
    let _ = set(|w| {
        if w.theme.hc || settings::parse_error().is_some() {
            return;
        }
        if !w.hexes.iter().any(|x| x.key == key) {
            let digits = shown_digits(w.theme, settings::get(), key).unwrap_or_default();
            w.hexes.push(Hex { key, caret: digits.len(), anchor: 0, digits, line: None });
        }
        if let Some(x) = w.hexes.iter_mut().find(|x| x.key == key) {
            x.caret = caret.unwrap_or(x.digits.len()).min(x.digits.len());
            x.anchor = if caret.is_some() { x.caret } else { 0 };
        }
    });
}

fn enter_if_new(key: &'static str) {
    if get(|w| w.hexes.iter().any(|x| x.key == key)) == Some(false) {
        enter_hex(key, None);
    }
}

/// Focus left a hex box: six digits save; fewer keep the digits and the line until you fix
/// them or press Esc.
fn leave_hex(key: &'static str) {
    let Some((x, shown)) = get(|w| Some((w.hexes.iter().find(|x| x.key == key)?.clone(), shown_digits(w.theme, settings::get(), key)))).flatten() else { return };
    match hex_color(&x.digits) {
        Some(_) if Some(&x.digits) == shown.as_ref() => {
            let _ = set(|w| w.hexes.retain(|x| x.key != key));
        }
        Some(_) => save_hex(key),
        None => {
            let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(INCOMPLETE)));
        }
    }
}

/// Enter, or leaving with six digits: saves in upper case. A workspace's own default takes its
/// key out. A failed save keeps your digits and says Enter tries again.
fn save_hex(key: &'static str) {
    let Some(x) = get(|w| w.hexes.iter().find(|x| x.key == key).cloned()).flatten() else { return };
    let Some(c) = hex_color(&x.digits) else {
        let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(INCOMPLETE)));
        return layout();
    };
    let edit = if key == ACCENT { Edit::At(path_of(ACCENT), format!("#{c:06X}").into()) } else { color_edit(key, c) };
    match settings::write(edit) {
        Ok(()) => {
            let still = set(|w| {
                w.error = None;
                w.hexes.retain(|x| x.key != key);
                w.focus == Some(Ctl::Hex(key))
            });
            if still == Some(true) {
                enter_hex(key, Some(6));
            }
            refresh();
        }
        Err(why) => {
            let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(COULDNT)));
            failed(&color_title(key), why);
        }
    }
}

/// Typed or pasted digits go in at the caret, over the selection, never more than six.
fn hex_insert(key: &'static str, text: &str) {
    let _ = set(|w| {
        let Some(x) = w.hexes.iter_mut().find(|x| x.key == key) else { return };
        let (lo, hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
        let mut d = x.digits.clone();
        d.replace_range(lo..hi, "");
        let put: String = text.chars().filter(char::is_ascii_hexdigit).take(6 - d.len()).map(|c| c.to_ascii_uppercase()).collect();
        d.insert_str(lo, &put);
        x.digits = d;
        x.caret = lo + put.len();
        x.anchor = x.caret;
        if x.line == Some(REFUSED) || x.digits.len() == 6 {
            x.line = None;
        }
    });
    layout();
}

/// A hex box's own keys; false lets the page have the key (Up, Down, Tab move on).
fn hex_key(key: &'static str, vk: VIRTUAL_KEY) -> bool {
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
    let (ctrl, shift) = (down(VK_CONTROL), down(VK_SHIFT));
    // Focus can land here without focus_on (a Reset that went): the first key starts it.
    enter_if_new(key);
    let has = get(|w| w.hexes.iter().any(|x| x.key == key)) == Some(true);
    if !has {
        // Read only or high contrast: Esc never closes the page from inside a box.
        return vk == VK_ESCAPE;
    }
    let edit = |f: &mut dyn FnMut(&mut Hex)| {
        let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(&mut *f));
        layout();
    };
    match vk {
        VK_LEFT | VK_RIGHT | VK_HOME | VK_END => edit(&mut |x| {
            let (lo, hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
            x.caret = match vk {
                VK_HOME => 0,
                VK_END => x.digits.len(),
                VK_LEFT if !shift && hi > lo => lo,
                VK_RIGHT if !shift && hi > lo => hi,
                VK_LEFT => x.caret.saturating_sub(1),
                _ => (x.caret + 1).min(x.digits.len()),
            };
            if !shift {
                x.anchor = x.caret;
            }
        }),
        VK_BACK | VK_DELETE => edit(&mut |x| {
            let (mut lo, mut hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
            if lo == hi {
                if vk == VK_BACK && lo > 0 {
                    lo -= 1;
                } else if vk == VK_DELETE && hi < x.digits.len() {
                    hi += 1;
                }
            }
            x.digits.replace_range(lo..hi, "");
            (x.caret, x.anchor) = (lo, lo);
        }),
        VK_RETURN => save_hex(key),
        VK_ESCAPE => {
            let shown = get(|w| shown_digits(w.theme, settings::get(), key)).flatten().unwrap_or_default();
            edit(&mut |x| {
                if x.digits != shown || x.line.is_some() {
                    (x.digits, x.line, x.caret, x.anchor) = (shown.clone(), None, shown.len(), 0);
                }
            });
        }
        VK_SPACE => {}
        _ if ctrl && vk.0 == b'A' as u16 => edit(&mut |x| (x.caret, x.anchor) = (x.digits.len(), 0)),
        _ if ctrl && (vk.0 == b'C' as u16 || vk.0 == b'X' as u16) => {
            let cut = vk.0 == b'X' as u16;
            let text = get(|w| w.hexes.iter().find(|x| x.key == key).map(|x| x.digits[x.caret.min(x.anchor)..x.caret.max(x.anchor)].to_string())).flatten().unwrap_or_default();
            if !text.is_empty() {
                clip_set(&text);
                if cut {
                    hex_insert(key, "");
                }
            }
        }
        _ if ctrl && vk.0 == b'V' as u16 => match clip_get().as_deref().and_then(pasted) {
            Some(d) => edit(&mut |x| (x.digits, x.caret, x.anchor, x.line) = (d.clone(), 6, 6, None)),
            None => edit(&mut |x| x.line = Some(REFUSED)),
        },
        _ => return false,
    }
    true
}

/// The clipboard's text, if it has any.
fn clip_get() -> Option<String> {
    use windows::Win32::{Foundation::HGLOBAL, System::DataExchange::*, System::Memory::*};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        OpenClipboard(hwnd()).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let n = (0..GlobalSize(g) / 2).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        });
        let _ = CloseClipboard();
        text
    }
}

fn clip_set(text: &str) {
    use windows::Win32::{Foundation::{GlobalFree, HANDLE}, System::DataExchange::*, System::Memory::*};
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) else { return };
        let p = GlobalLock(g) as *mut u16;
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(g);
        if OpenClipboard(hwnd()).is_err() {
            let _ = GlobalFree(Some(g));
            return;
        }
        let _ = EmptyClipboard();
        // The clipboard owns the memory once it takes it.
        if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).is_err() {
            let _ = GlobalFree(Some(g));
        }
        let _ = CloseClipboard();
    }
}

// ---- The color picker (spec 0021, drawn to the approved design pass, handoff-4) ----

const PICK_W: f32 = 388.0; // the card, in dip
const PICK_H: f32 = 326.0;
const NOT_SAVED: &str = "Not saved: couldn't update the file.";
const EASE_TIMER: usize = 1;
const EASE_MS: u32 = 150;
const MARGIN: f32 = 8.0; // the card's gap from what it sits by, and from the window's and notes' edges
const OUTSET: f32 = 4.0; // a tile's in view ring and hover edge reach this far outside it

thread_local! {
    // The open picker's ring at one scale, and its square at one scale and hue: built once,
    // drawn on every paint, dropped when it closes.
    static RING: RefCell<Option<(u32, ui::Picture)>> = const { RefCell::new(None) };
    static SQUARE: RefCell<Option<(u32, u32, ui::Picture)>> = const { RefCell::new(None) };
}

/// The wheel's bitmap in pixels at scale `s`: its side, the ring's outer and inner radius, and
/// the square's side. Whole pixels, so the ring's center is a pixel corner at every scale (at
/// 125% the 90 dip middle line falls on half a pixel).
fn wheel_px(s: f32) -> (i32, f32, f32, i32) {
    (2 * (108.0 * s).round() as i32, (100.0 * s).round(), (80.0 * s).round(), 2 * (52.0 * s).round() as i32)
}

/// The wheel of a card at `at`: its center and the square, in dip, from the same whole pixels
/// the bitmaps are drawn at. The bitmap is 8 dip larger than the 200 dip wheel on every side,
/// so the focus circle around the ring's mark is never cut.
fn wheel_at(at: Place, s: f32) -> ((f32, f32), Bx) {
    let (n, _, _, side) = wheel_px(s);
    let (ox, oy) = (((at.x + 164.0) * s).round(), ((at.y + 62.0) * s).round());
    let (cx, cy) = ((ox + n as f32 / 2.0) / s, (oy + n as f32 / 2.0) / s);
    let half = side as f32 / 2.0 / s;
    ((cx, cy), (cx - half, cy - half, 2.0 * half, 2.0 * half))
}

/// A color at opacity `a`, premultiplied.
fn premul(c: u32, a: f32) -> u32 {
    let ch = |shift: u32| ((c >> shift & 0xFF) as f32 * a).round() as u32;
    ((a * 255.0).round() as u32) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// The toned ring, hue 0 at the top going clockwise, its edges smoothed; clear inside and out.
fn ring_px(s: f32) -> ui::Picture {
    let (n, ro, ri, _) = wheel_px(s);
    let c = n as f32 / 2.0;
    let hues: Vec<u32> = (0..360).map(|h| ui::ring_color(h as f32)).collect();
    let mut px = vec![0u32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = dx.hypot(dy);
            let a = (ro - d + 0.5).min(d - ri + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                let h = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                px[(y * n + x) as usize] = premul(hues[h.round() as usize % 360], a);
            }
        }
    }
    ui::Picture { w: n, h: n, px }
}

/// The square at `hue`: saturation 0 to 1 left to right, value 1 to 0 top to bottom, its 3 dip
/// corners smoothed.
fn square_px(s: f32, hue: f32) -> ui::Picture {
    let (_, _, _, n) = wheel_px(s);
    let (r, side) = (3.0 * s, n as f32);
    let mut px = vec![0u32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (ox, oy) = ((r - fx).max(0.0).max(fx - (side - r)), (r - fy).max(0.0).max(fy - (side - r)));
            let a = (r - ox.hypot(oy) + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                px[(y * n + x) as usize] = premul(ui::rgb_of((hue, fx / side, 1.0 - fy / side)), a);
            }
        }
    }
    ui::Picture { w: n, h: n, px }
}

/// Which part of the wheel a point is on, given from the wheel's center in dip (the design's
/// hit areas): the square plus 8 dip, else the ring's band plus 6 dip either side.
fn part_at((dx, dy): (f32, f32), half: f32) -> Option<Part> {
    if dx.abs() <= half + 8.0 && dy.abs() <= half + 8.0 {
        Some(Part::Square)
    } else {
        (74.0..=106.0).contains(&dx.hypot(dy)).then_some(Part::Ring)
    }
}

/// AC-8's line: the lowest other workspace in use whose color is `color` (true) or very close to
/// it (false), each counted with the color it shows.
fn dup_of(color: u32, me: usize, ws: &[u32; 9], n: usize) -> Option<(usize, bool)> {
    (0..n.min(9)).filter(|&j| j != me).find_map(|j| match ws[j] {
        c if c == color => Some((j, true)),
        c if ui::distance(c, color) < 10.0 => Some((j, false)),
        _ => None,
    })
}

/// The card's window position, and the keep set it was placed with.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Place {
    x: f32,
    y: f32,
    level: usize,
}

/// What the card is placed in, in window dip: the window's height, the notes over the column,
/// the card's least and most x, the scroll and its limit.
#[derive(Clone, Copy, Debug)]
struct Room {
    vh: f32,
    fixed: f32,
    lo: f32,
    hi: f32,
    scroll: f32,
    smax: f32,
}

/// One set the card keeps clear (AC-14): the box it sits by, and every box that must stay in
/// view and uncovered.
#[derive(Clone, Debug)]
struct Level {
    next_to: Bx,
    keep: Vec<Bx>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Way {
    Beside,
    Below,
    Above,
}

fn union(a: Bx, b: Bx) -> Bx {
    let (l, t) = (a.0.min(b.0), a.1.min(b.1));
    (l, t, (a.0 + a.2).max(b.0 + b.2) - l, (a.1 + a.3).max(b.1 + b.3) - t)
}

/// A kept box with its ring and hover edge, the page scrolled `d` further.
fn grown(r: Bx, d: f32) -> Bx {
    (r.0 - OUTSET, r.1 - d - OUTSET, r.2 + 2.0 * OUTSET, r.3 + 2.0 * OUTSET)
}

fn covers(r: Bx, (x, y): (f32, f32), (cw, ch): (f32, f32)) -> bool {
    r.0 < x + cw && x < r.0 + r.2 && r.1 < y + ch && y < r.1 + r.3
}

/// Every kept box is in view, 8 dip inside the window's bottom and below the notes.
fn in_view(room: &Room, keep: &[Bx]) -> bool {
    keep.iter().all(|r| r.1 >= room.fixed + MARGIN - 0.5 && r.1 + r.3 <= room.vh - MARGIN + 0.5)
}

/// Where the card fits beside, below or above `level`'s box with the page scrolled `d`
/// further, or None. `cover`: it may lie over the page's notes, from y 8, as the menus do.
/// Beside sits level with what you clicked, sliding only as far as the window and the kept
/// boxes need.
#[allow(clippy::too_many_arguments)]
fn spot(room: &Room, level: &Level, anchor: Bx, way: Way, cover: bool, d: f32, card: (f32, f32)) -> Option<(f32, f32)> {
    let (cw, ch) = card;
    let keep: Vec<Bx> = level.keep.iter().map(|r| grown(*r, d)).collect();
    if !in_view(room, &keep) {
        return None;
    }
    let (a, g) = ((anchor.0, anchor.1 - d, anchor.2, anchor.3), (level.next_to.0, level.next_to.1 - d, level.next_to.2, level.next_to.3));
    let (top, bottom) = (if cover { MARGIN } else { room.fixed + MARGIN }, room.vh - MARGIN - ch);
    let centered = (a.0 + a.2 / 2.0 - cw / 2.0).min(room.hi).max(room.lo);
    let (x, ys) = match way {
        Way::Below => (centered, vec![g.1 + g.3 + MARGIN]),
        Way::Above => (centered, vec![g.1 - MARGIN - ch]),
        Way::Beside => {
            let x = if g.0 - MARGIN - cw >= room.lo {
                g.0 - MARGIN - cw
            } else if g.0 + g.2 + MARGIN <= room.hi {
                g.0 + g.2 + MARGIN
            } else {
                return None;
            };
            let want = a.1 + a.3 / 2.0 - ch / 2.0;
            let mut ys: Vec<f32> = std::iter::once(want).chain(keep.iter().flat_map(|r| [r.1 + OUTSET - MARGIN - ch, r.1 + r.3 - OUTSET + MARGIN])).map(|y| y.min(bottom).max(top)).collect();
            ys.sort_by(|m, n| (m - want).abs().total_cmp(&(n - want).abs()));
            (x, ys)
        }
    };
    ys.into_iter().find(|&y| y >= top - 0.5 && y <= bottom + 0.5 && !keep.iter().any(|r| covers(*r, (x, y), card))).map(|y| (x, y))
}

/// Where the card opens (AC-14), each set tried widest first: a place as the page is, else (with
/// `scroll`) the least scroll that lets one fit, equal scrolls by the order of places; then the
/// same over the notes; only then the next, narrower set. Tiles and the picture: beside, below,
/// above; swatches: below, above, beside. Returns the place after the scroll, and the scroll's
/// change.
fn place(room: &Room, levels: &[Level], anchor: Bx, beside_first: bool, card: (f32, f32), scroll: bool) -> (Place, f32) {
    let order = if beside_first { [Way::Beside, Way::Below, Way::Above] } else { [Way::Below, Way::Above, Way::Beside] };
    let reach = if scroll { room.scroll.max(room.smax - room.scroll).ceil() as i32 } else { 0 };
    for (level, l) in levels.iter().enumerate() {
        for cover in [false, true] {
            for k in 0..=reach {
                for d in if k == 0 { vec![0.0] } else { vec![k as f32, -(k as f32)] } {
                    if room.scroll + d < -0.5 || room.scroll + d > room.smax + 0.5 {
                        continue;
                    }
                    for way in order {
                        if let Some((x, y)) = spot(room, l, anchor, way, cover, d, card) {
                            return (Place { x, y, level }, d);
                        }
                    }
                }
            }
        }
    }
    // Nothing fits even over the notes: below what you clicked, kept in the window.
    let x = (anchor.0 + anchor.2 / 2.0 - card.0 / 2.0).min(room.hi).max(room.lo);
    let y = (anchor.1 + anchor.3 + MARGIN).min(room.vh - MARGIN - card.1).max(MARGIN);
    (Place { x, y, level: levels.len().saturating_sub(1) }, 0.0)
}

/// The card can stay at `at`: inside the window, clear of its keep set, which is still in view.
fn kept(room: &Room, levels: &[Level], at: Place, card: (f32, f32)) -> bool {
    let Some(l) = levels.get(at.level) else { return false };
    let keep: Vec<Bx> = l.keep.iter().map(|r| grown(*r, 0.0)).collect();
    in_view(room, &keep)
        && at.y >= MARGIN - 0.5
        && at.y <= room.vh - MARGIN - card.1 + 0.5
        && at.x >= room.lo - 0.5
        && at.x <= room.hi + 0.5
        && !keep.iter().any(|r| covers(*r, (at.x, at.y), card))
}

/// What you clicked and the sets the card keeps clear, widest first (AC-14), from where the
/// page last drew them; None when what you clicked is gone.
fn levels_of(anchor: &Anchor, g: &Geo) -> Option<(Bx, Vec<Level>)> {
    let one = |b: Bx| Level { next_to: b, keep: vec![b] };
    match anchor {
        Anchor::Tile(dev, i) => {
            let tile = g.tiles.iter().find(|(d, j, _)| d == dev && j == i)?.2;
            let row = g.rows.iter().find(|(d, _)| d == dev).map_or(tile, |r| r.1);
            // Every plugged in screen's tiles: a screen not plugged in shows grey tiles, nothing to watch.
            let all = g.rows.iter().map(|r| r.1).reduce(union).unwrap_or(row);
            Some((tile, vec![one(all), one(row), one(tile)]))
        }
        Anchor::Show(dev, i) => {
            let tile = g.shows.iter().find(|(d, j, _)| d == dev && j == i)?.2;
            let four = g.groups.iter().find(|(d, _)| d == dev).map_or(tile, |r| r.1);
            Some((tile, vec![one(g.band.unwrap_or(four)), one(four), one(tile)]))
        }
        Anchor::Swatch(k) => {
            let sw = g.swatches.iter().find(|(x, _)| x == k)?.1;
            // Your accent first keeps the sample's legend clear too: it names what repaints.
            match g.legend.filter(|_| *k == ACCENT) {
                Some(legend) => Some((sw, vec![Level { next_to: sw, keep: vec![sw, legend] }, one(sw)])),
                None => Some((sw, vec![one(sw)])),
            }
        }
    }
}

fn room_of(w: &Win) -> Room {
    let cw = (w.vw - X0 - PAD_R).min(COL_MAX);
    Room { vh: w.vh, fixed: w.fixed, lo: X0, hi: X0 + cw - PICK_W, scroll: w.scroll, smax: (w.height - (w.vh - w.fixed)).max(0.0) }
}

enum Fit {
    Kept,
    Placed,
    Gone,
}

/// After a layout: the open picker keeps its window position while it still fits there; else it
/// is placed again, scrolling the page only when it first opens; it closes when what you
/// clicked is gone (see `layout`). Nothing while the page eases.
fn pick_fit() -> Option<Fit> {
    let (fit, to) = set(|w| {
        if w.ease.is_some() {
            return None;
        }
        let p = w.pick.as_ref()?;
        let Some((anchor, levels)) = levels_of(&p.anchor, &w.geo) else { return Some((Fit::Gone, None)) };
        let room = room_of(w);
        if p.placed.is_some_and(|at| kept(&room, &levels, at, (PICK_W, PICK_H))) {
            return Some((Fit::Kept, None));
        }
        let (first, beside) = (p.placed.is_none(), !matches!(p.anchor, Anchor::Swatch(_)));
        let (at, d) = place(&room, &levels, anchor, beside, (PICK_W, PICK_H), first);
        w.pick.as_mut()?.placed = Some(at);
        Some((Fit::Placed, (d != 0.0).then_some(w.scroll + d)))
    })
    .flatten()?;
    if let Some(to) = to {
        ease_to(to, true);
    }
    Some(fit)
}

/// Scrolls the page to `to`, eased out over 150 ms, or at once with Skip animations (AC-14).
/// `opening`: the picker's card shows when it ends. The caller lays the page out.
fn ease_to(to: f32, opening: bool) {
    let ms = ui::motion_ms(EASE_MS);
    let h = set(|w| {
        let to = to.clamp(0.0, (w.height - (w.vh - w.fixed)).max(0.0));
        if ms == 0 || (to - w.scroll).abs() < 0.5 {
            w.scroll = to;
            return None;
        }
        w.ease = Some(Ease { from: w.scroll, to, start: Instant::now(), ms, opening });
        Some(w.hwnd)
    })
    .flatten();
    if let Some(h) = h {
        unsafe { SetTimer(Some(h), EASE_TIMER, 16, None) };
    }
}

/// One frame of the ease: its progress clamped to 0..1 and the scroll to the page's limits.
fn ease_step() {
    let done = set(|w| {
        let Some(e) = w.ease else { return true };
        let k = (e.start.elapsed().as_secs_f32() * 1000.0 / e.ms as f32).clamp(0.0, 1.0);
        let max = (w.height - (w.vh - w.fixed)).max(0.0);
        w.scroll = (e.from + (e.to - e.from) * (1.0 - (1.0 - k).powi(3))).clamp(0.0, max);
        k >= 1.0
    });
    match done {
        Some(true) => {
            if let Some(h) = hwnd() {
                unsafe { let _ = KillTimer(Some(h), EASE_TIMER); }
            }
            ease_finish();
        }
        Some(false) => {
            lay();
            repaint();
        }
        None => {}
    }
}

/// Ends the ease at once, at its end: any press, key or wheel turn during it, a resize, a close.
fn ease_finish() {
    let Some(h) = set(|w| w.ease.take().map(|e| {
        w.scroll = e.to;
        w.hwnd
    }))
    .flatten() else {
        return;
    };
    unsafe { let _ = KillTimer(Some(h), EASE_TIMER); }
    layout();
}

/// The color the file gives `key` now; None: the accent follows Windows.
fn file_color(s: &Settings, key: &str) -> Option<u32> {
    match ws_index(key) {
        Some(i) => Some(s.ws_colors[i].unwrap_or(ui::WS[i])),
        None => match s.accent {
            settings::Accent::Rgb(c) => Some(c),
            settings::Accent::Windows => None,
        },
    }
}

/// A reload brought a value for the picker's key that it did not write: neither what the file
/// is known to hold nor the write in flight. Windows' accent always counts (AC-12).
fn outside(now: Option<u32>, file: u32, written: Option<u32>) -> bool {
    now.is_none_or(|c| c != file && Some(c) != written)
}

/// The open picker must close, writing nothing (AC-10, AC-12): high contrast or read only came
/// on, its workspace went, or its key changed outside it.
fn must_close(w: &Win) -> bool {
    let Some(p) = &w.pick else { return false };
    let s = settings::get();
    w.theme.hc || settings::parse_error().is_some() || ws_index(p.key).is_some_and(|i| i >= s.workspaces) || outside(file_color(s, p.key), p.file, p.written)
}

/// The page's theme with `color` for `key`, while the page doesn't show that color already.
fn preview_for(t: Theme, key: &str, color: u32) -> Option<Theme> {
    let shown = ws_index(key).map_or(t.b.acc, |i| t.b.ws[i]);
    (!t.hc && shown != color).then(|| {
        let l = ui::look();
        ui::theme_preview(l.apps_light, l, ws_index(key), color)
    })
}

/// Shows `color` on the card and the page, the ring and square at `hsv`. A pick, a drag or an
/// arrow ends Not saved, even one that writes nothing (AC-13).
fn show_color(w: &mut Win, color: u32, hsv: ui::Hsv) {
    let theme = w.theme;
    if let Some(p) = w.pick.as_mut() {
        (p.color, p.hsv, p.failed) = (color, hsv, None);
        p.preview = preview_for(theme, p.key, color);
    }
}

/// Opens the color picker for `key` (AC-1 to AC-3): nothing in high contrast or read only, for
/// a workspace you don't have, or for the accent while it follows Windows. Opened by a click,
/// it ignores presses near that click for a moment (AC-15); from the keyboard it doesn't.
fn pick_open(key: &'static str, anchor: Anchor, from: Option<Ctl>) {
    ease_finish();
    let s = settings::get();
    let Some(color) = file_color(s, key) else { return };
    if ws_index(key).is_some_and(|i| i >= s.workspaces) || settings::parse_error().is_some() {
        return;
    }
    // What the file holds now, after any hex box save, for Esc and Cancel to put back.
    let before = settings::raw_at(&path_of(key));
    let wait = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
    let _ = set(|w| {
        if w.theme.hc || w.pick.is_some() {
            return;
        }
        let preset = ui::PRESETS.iter().position(|&c| c == color);
        w.pick = Some(ColorPick {
            key,
            from,
            anchor,
            before,
            start: color,
            file: color,
            color,
            hsv: ui::hsv_of(color, (0.0, 0.0, 0.0)),
            preview: None,
            written: None,
            failed: None,
            saved: false,
            held_edit: None,
            placed: None,
            scroll0: w.scroll,
            guard: (!w.kb).then(|| Guard { at: w.down_at, until: Instant::now() + wait }),
            focus: preset.map_or(Stop::Ring, Stop::Swatches),
            last_swatch: preset.unwrap_or(0),
            drag: None,
            held: None,
        });
    });
    layout();
}

/// Windows' words from a failed write, "cannot save … (os error 32)": the part in brackets.
fn windows_words(why: &str) -> String {
    why.rsplit_once(" (").map_or(why.to_string(), |(_, w)| w.trim_end_matches(')').to_string())
}

/// One picker write, `value` being the color it leaves the key at. `written` is set before the
/// call: the write reloads before it returns, and the reload must not take it for an outside
/// edit. A failure shows Not saved and puts the file's color back on the card and the page
/// (AC-13); the page's own notes wait for the close.
fn pick_write(edit: Edit, value: u32) -> bool {
    if set(|w| w.pick.as_mut().map(|p| p.written = Some(value))).flatten().is_none() {
        return false;
    }
    let result = settings::write(edit);
    let _ = set(|w| {
        let theme = w.theme;
        // The reload inside the write may have closed it.
        let Some(p) = w.pick.as_mut() else { return };
        p.written = None;
        match &result {
            Ok(()) => (p.file, p.saved, p.failed) = (value, true, None),
            Err(why) => {
                (p.failed, p.color) = (Some(windows_words(why)), p.file);
                p.hsv = ui::hsv_of(p.file, p.hsv);
            }
        }
        p.preview = preview_for(theme, p.key, p.color);
    });
    result.is_ok()
}

/// Saves the color shown unless the file holds it already (AC-5): one write per release, key
/// up, swatch or close, never one per move.
fn pick_save() {
    if let Some((key, color)) = get(|w| w.pick.as_ref().filter(|p| p.color != p.file).map(|p| (p.key, p.color))).flatten() {
        pick_write(color_edit(key, color), color);
    }
    layout();
}

/// What Esc and Cancel write (AC-6): the key exactly as the file had it when the picker opened
/// (no key then: take it out again), never through `color_edit`, and nothing when the file
/// holds that already.
fn undo_edit(key: &str, before: Option<&toml_edit::Value>, now: Option<&toml_edit::Value>) -> Option<Edit> {
    let bare = |v: &toml_edit::Value| {
        let mut v = v.clone();
        v.decor_mut().clear();
        v.to_string()
    };
    if before.map(bare) == now.map(bare) {
        return None;
    }
    Some(match before {
        Some(v) => Edit::At(path_of(key), v.clone()),
        None => Edit::Remove(path_of(key)),
    })
}

/// How the picker closes (AC-6): keeping (from the card: Done, Enter, Space on Done; or not: a
/// click outside, a resize, the page losing focus), undoing (Esc, Cancel), or writing nothing
/// (high contrast, read only, the count, an outside edit, what you clicked gone).
#[derive(Clone, Copy, PartialEq, Debug)]
enum How {
    Keep(bool), // true: from the card
    Undo,
    Quiet,
}

/// The page's couldn't save note once the picker closes (AC-13): shown while Not saved still
/// shows and the close is no undo, gone after a save in it worked, else as it was.
fn note_after(failed: Option<&String>, saved: bool, how: How, title: String, was: Option<(String, String)>) -> Option<(String, String)> {
    match failed {
        Some(why) if how != How::Undo => Some((title, why.clone())),
        _ if saved => None,
        _ => was,
    }
}

/// Every close goes through here: the write it needs, then the page's notes catch up, the page
/// eases back (closes from the card only) and focus goes back to the swatch that opened it.
/// Never call it inside `get` or `set`: it writes, and the write reloads.
fn pick_close(how: How) {
    ease_finish();
    let Some((h, drag, key, color, file, start, before)) = set(|w| {
        let p = w.pick.as_mut()?;
        p.held = None;
        Some((w.hwnd, p.drag.take(), p.key, p.color, p.file, p.start, p.before.clone()))
    })
    .flatten() else {
        return;
    };
    if drag.is_some() {
        unsafe { let _ = ReleaseCapture(); }
    }
    let wrote = match how {
        How::Keep(_) if color != file => pick_write(color_edit(key, color), color),
        How::Undo => match undo_edit(key, before.as_ref(), settings::raw_at(&path_of(key)).as_ref()) {
            Some(edit) => pick_write(edit, start),
            None => true,
        },
        _ => true,
    };
    if !wrote {
        return layout(); // stays open with Not saved
    }
    let fixed0 = get(|w| w.fixed).unwrap_or(0.0);
    let Some(p) = set(|w| w.pick.take()).flatten() else { return layout() };
    RING.set(None);
    SQUARE.set(None);
    let _ = set(|w| {
        w.error = note_after(p.failed.as_ref(), p.saved, how, color_title(p.key), w.error.take());
        if p.held_edit.is_some() {
            w.edited = p.held_edit.clone();
        }
        if p.from.is_some() {
            w.focus = p.from; // never reveal(): the way back is the only scroll on close
        }
        (w.hover, w.press) = (None, None);
    });
    // The notes changed height: the page scrolls by as much, so what was under the pointer stays.
    lay();
    let dt = get(|w| w.fixed).unwrap_or(fixed0) - fixed0;
    if dt != 0.0 {
        let _ = set(|w| w.scroll += dt);
        lay();
    }
    if matches!(how, How::Keep(true) | How::Undo) {
        ease_to(p.scroll0 + dt, false);
    }
    layout();
    if let Some(tree) = get(|w| w.tree.clone()) {
        uia::focus_moved(h, &tree, WHO);
    }
}

/// A press the opening click's guard drops (AC-15): inside the double click rectangle around
/// where that click went down, before the double click time has passed since the picker opened.
fn guarded(g: Option<Guard>, at: (i32, i32), now: Instant, (cx, cy): (i32, i32)) -> bool {
    g.is_some_and(|g| now < g.until && (at.0 - g.at.0).abs() * 2 <= cx && (at.1 - g.at.1).abs() * 2 <= cy)
}

/// A press while the picker is open: dropped near the opening click; outside the card it
/// closes and keeps, and does nothing else (AC-6); on the ring or square it picks at once and
/// drags; on a preset or a button it acts when the button comes up, as the page's controls do.
fn pick_down(h: HWND, lp: LPARAM) {
    let rect = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    let Some((pt, near, inside)) = get(|w| {
        let p = w.pick.as_ref()?;
        let pt = point(lp, w.scale);
        let inside = p.placed.is_none_or(|at| pt.0 >= at.x && pt.0 < at.x + PICK_W && pt.1 >= at.y && pt.1 < at.y + PICK_H);
        Some((pt, guarded(p.guard, w.down_at, Instant::now(), rect), inside))
    })
    .flatten() else {
        return;
    };
    if near {
        return;
    }
    if !inside {
        return pick_close(How::Keep(false));
    }
    unsafe { let _ = SetCapture(h); }
    let drag = set(|w| {
        w.kb = false;
        let at = w.pick.as_ref()?.placed?;
        let ((cx, cy), sq) = wheel_at(at, w.scale);
        let Some(part) = part_at((pt.0 - cx, pt.1 - cy), sq.2 / 2.0) else {
            w.press = hit_at(w, pt);
            return None;
        };
        let p = w.pick.as_mut()?;
        p.drag = Some(part);
        p.focus = if part == Part::Ring { Stop::Ring } else { Stop::Square };
        Some(part)
    })
    .flatten();
    match drag {
        Some(part) => pick_move(part, pt),
        None => layout(),
    }
}

/// The ring or square under the mouse: shows that color at once; letting go saves it.
fn pick_move(part: Part, pt: (f32, f32)) {
    let _ = set(|w| {
        let Some((at, (h, s, v))) = w.pick.as_ref().and_then(|p| Some((p.placed?, p.hsv))) else { return };
        let ((cx, cy), sq) = wheel_at(at, w.scale);
        let hsv = match part {
            Part::Ring => ((pt.0 - cx).atan2(-(pt.1 - cy)).to_degrees().rem_euclid(360.0), s, v),
            Part::Square => (h, ((pt.0 - sq.0) / sq.2).clamp(0.0, 1.0), (1.0 - (pt.1 - sq.1) / sq.3).clamp(0.0, 1.0)),
        };
        show_color(w, ui::rgb_of(hsv), hsv);
    });
    layout();
}

/// The mouse let go of the ring or square, or lost the capture: saves what it shows, once.
/// Letting go calls ReleaseCapture, which comes here first, so the release finds nothing to do.
fn finish_drag() {
    if set(|w| w.pick.as_mut().and_then(|p| p.drag.take())).flatten().is_some() {
        pick_save();
    }
}

/// A preset picked by a click or Space: shown and saved at once (AC-5).
fn pick_preset(i: usize) {
    let _ = set(|w| {
        let Some(p) = w.pick.as_mut() else { return };
        (p.focus, p.last_swatch) = (Stop::Swatches(i), i);
        let (c, hsv) = (ui::PRESETS[i], ui::hsv_of(ui::PRESETS[i], p.hsv));
        show_color(w, c, hsv);
    });
    pick_save();
}

/// What an arrow does on a picker stop (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Arrow {
    Swatch(usize),
    Hsv(ui::Hsv),
}

/// On the swatches the arrows move and pick, 3 to a row, stopping at the first and last; on
/// the ring Left and Right turn the hue 1 degree (Shift: 10), wrapping; in the square the
/// arrows change saturation or value by 1% (Shift: 10%), clamped. None: nothing.
fn arrow(focus: Stop, vk: VIRTUAL_KEY, shift: bool, (h, s, v): ui::Hsv) -> Option<Arrow> {
    let step = if shift { 10.0 } else { 1.0 };
    match focus {
        Stop::Swatches(i) => {
            let n = match vk {
                VK_LEFT => i.checked_sub(1)?,
                VK_RIGHT => i + 1,
                VK_UP => i.checked_sub(3)?,
                VK_DOWN => i + 3,
                _ => return None,
            };
            (n < ui::PRESETS.len()).then_some(Arrow::Swatch(n))
        }
        Stop::Ring => match vk {
            VK_LEFT => Some(Arrow::Hsv(((h.round() - step).rem_euclid(360.0), s, v))),
            VK_RIGHT => Some(Arrow::Hsv(((h.round() + step).rem_euclid(360.0), s, v))),
            _ => None,
        },
        Stop::Square => {
            let d = step / 100.0;
            Some(Arrow::Hsv(match vk {
                VK_LEFT => (h, (s - d).max(0.0), v),
                VK_RIGHT => (h, (s + d).min(1.0), v),
                VK_UP => (h, s, (v + d).min(1.0)),
                VK_DOWN => (h, s, (v - d).max(0.0)),
                _ => return None,
            }))
        }
        Stop::Done | Stop::Cancel => None,
    }
}

/// Tab and Shift+Tab: five stops, wrapping inside the picker; coming back to the swatches lands
/// on the one equal to the color shown, else the last one focused.
fn tab(focus: Stop, back: bool, color: u32, last: usize) -> Stop {
    const ORDER: [Stop; 5] = [Stop::Swatches(0), Stop::Ring, Stop::Square, Stop::Done, Stop::Cancel];
    let i = ORDER.iter().position(|s| std::mem::discriminant(s) == std::mem::discriminant(&focus)).unwrap_or(0);
    match ORDER[(i + if back { 4 } else { 1 }) % 5] {
        Stop::Swatches(_) => Stop::Swatches(ui::PRESETS.iter().position(|&c| c == color).unwrap_or(last)),
        s => s,
    }
}

/// What a key down does in the picker, apart from moving (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
enum KeyDoes {
    Close(How),
    Pick(usize),
    Move, // Tab or an arrow
    Nothing,
}

/// Esc anywhere, and Enter or Space on Cancel, undo; Enter from every other stop keeps and
/// closes, and never picks; Space on Done keeps, on a swatch picks it. Held Enter, Space and
/// Esc act once.
fn key_does(vk: VIRTUAL_KEY, focus: Stop, repeat: bool) -> KeyDoes {
    match (vk, focus) {
        (VK_RETURN | VK_SPACE | VK_ESCAPE, _) if repeat => KeyDoes::Nothing,
        (VK_ESCAPE, _) | (VK_RETURN | VK_SPACE, Stop::Cancel) => KeyDoes::Close(How::Undo),
        (VK_RETURN, _) | (VK_SPACE, Stop::Done) => KeyDoes::Close(How::Keep(true)),
        (VK_SPACE, Stop::Swatches(i)) => KeyDoes::Pick(i),
        (VK_TAB | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN, _) => KeyDoes::Move,
        _ => KeyDoes::Nothing,
    }
}

/// The picker's keys, taken before the page's, as the open menu's are (AC-9).
fn pick_key(vk: VIRTUAL_KEY, repeat: bool) {
    let Some(focus) = set(|w| {
        w.kb = true;
        w.pick.as_ref().map(|p| p.focus)
    })
    .flatten() else {
        return;
    };
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match key_does(vk, focus, repeat) {
        KeyDoes::Close(how) => pick_close(how),
        KeyDoes::Pick(i) => pick_preset(i),
        KeyDoes::Nothing => repaint(),
        KeyDoes::Move if vk == VK_TAB => {
            let _ = set(|w| {
                if let Some(p) = w.pick.as_mut() {
                    p.focus = tab(p.focus, shift, p.color, p.last_swatch);
                    if let Stop::Swatches(i) = p.focus {
                        p.last_swatch = i;
                    }
                }
            });
            layout();
        }
        KeyDoes::Move => {
            // An arrow: shown at once; the key up saves.
            let _ = set(|w| {
                let Some(p) = w.pick.as_mut() else { return };
                let (c, hsv) = match arrow(p.focus, vk, shift, p.hsv) {
                    Some(Arrow::Swatch(n)) => {
                        (p.focus, p.last_swatch) = (Stop::Swatches(n), n);
                        (ui::PRESETS[n], ui::hsv_of(ui::PRESETS[n], p.hsv))
                    }
                    Some(Arrow::Hsv(hsv)) => (ui::rgb_of(hsv), hsv),
                    None => return,
                };
                p.held = Some(vk);
                show_color(w, c, hsv);
            });
            layout();
        }
    }
}

/// A key up: the held arrow's saves what it showed (AC-9). The key that opened the picker
/// comes up here too, and does nothing.
fn pick_key_up(vk: VIRTUAL_KEY) {
    let up = set(|w| w.pick.as_mut().is_some_and(|p| p.held == Some(vk) && p.held.take().is_some()));
    if up == Some(true) {
        pick_save();
    }
}

/// The page lost the keyboard during a held arrow: saves what it showed, as its key up would.
fn pick_lost_keys() {
    if set(|w| w.pick.as_mut().is_some_and(|p| p.held.take().is_some())) == Some(true) {
        pick_save();
    }
}

/// A click opens the picker over this control (spec 0021 AC-1): the hand shows there.
fn opens(w: &Win, ctl: Option<Ctl>) -> bool {
    !w.theme.hc && settings::parse_error().is_none() && w.rec.is_none() && matches!(ctl, Some(Ctl::WsTile(..) | Ctl::ShowTile(..) | Ctl::Swatch(_)))
}

/// The screen Where it shows changes; None: All screens.
fn screen_id() -> Option<String> {
    get(|w| {
        let (list, i) = chosen(w, settings::get());
        list[i].id.clone()
    })
    .flatten()
}

/// A place key changed from the page; nothing while the indicator is off.
fn set_place(key: &'static str, v: f64) {
    let s = settings::get();
    if !s.indicator_enabled {
        return;
    }
    if let Some(e) = place_edit(s, screen_id().as_deref(), key, v) {
        commit(e, "Where it shows");
    }
}

/// − and + on the distance, Across or Down; a decimal in the file lands on a whole number.
fn step_place(key: &'static str, up: bool) {
    let s = settings::get();
    let pos = pos_of(s, screen_id().as_deref());
    if key == GAP && matches!(pos.spot, settings::Spot::Center | settings::Spot::Custom) {
        return;
    }
    let (hi, by) = if key == GAP { (400, 4) } else { (100, 1) };
    let v = pos_get(pos, key);
    let n = if v.fract() != 0.0 { if up { v.ceil() } else { v.floor() } } else { stepped(v as i64, 0, hi, by, up) as f64 };
    if n != v {
        set_place(key, n);
    }
}

/// The spot nearest a Custom card: the third it is in across and down.
fn nearest(x: f64, y: f64) -> usize {
    let third = |v: f64| if v < 100.0 / 3.0 { 0 } else if v < 200.0 / 3.0 { 1 } else { 2 };
    third(y) * 3 + third(x)
}

/// Spots or Custom. Spots picks the spot nearest the card and keeps x and y in the file, so
/// Custom comes back where it was.
fn set_mode(custom: bool) {
    let pos = pos_of(settings::get(), screen_id().as_deref());
    if custom != (pos.spot == settings::Spot::Custom) {
        set_place(SPOT, if custom { 9.0 } else { nearest(pos.x, pos.y) as f64 });
    }
}

/// Left and Right on the picture: the nine spots in reading order, stopping at the ends; with
/// Custom, the card 1 % across.
fn walk_picture(right: bool) {
    let pos = pos_of(settings::get(), screen_id().as_deref());
    if pos.spot == settings::Spot::Custom {
        let n = if right { (pos.x.floor() + 1.0).min(100.0) } else { (pos.x.ceil() - 1.0).max(0.0) };
        set_place(ACROSS, n);
    } else {
        let i = spot_index(pos.spot);
        let n = if right { (i + 1).min(8) } else { i.saturating_sub(1) };
        if n != i {
            set_place(SPOT, n as f64);
        }
    }
}

/// Reset: All screens writes the default back, as every [indicator] row does; a screen's
/// Reset takes its key out (its last key takes the table).
fn reset_place(key: &'static str) {
    match screen_id() {
        None => commit(Edit::At(path_of(key), place_value(key, pos_get(DEFAULTS.indicator_pos, key))), "Where it shows"),
        Some(id) => {
            commit(Edit::Remove(vec!["indicator".into(), "screens".into(), id, leaf(key).into()]), "Where it shows");
            after_table(false);
        }
    }
}

/// Same as all screens on a Names row: the screen's key goes, so the row follows all screens
/// again, and any text of yours that failed to save there goes with it.
fn same_name(k: usize) {
    let Some(screen) = get(|w| name_look(w, settings::get(), Nm::Ws(k)).screen).flatten() else { return };
    let _ = set(|w| w.unsaved.retain(|u| !(u.0.as_deref() == Some(screen.as_str()) && u.1 == Nm::Ws(k))));
    if let Some(path) = name_path(Some(&screen), Nm::Ws(k)) {
        commit(Edit::Remove(path), &name_title(Nm::Ws(k)));
    }
}

/// Same as all screens: the chosen screen's whole table goes.
fn same_as_all() {
    let Some(id) = screen_id() else { return };
    commit(Edit::Remove(vec!["indicator".into(), "screens".into(), id]), "Where it shows");
    after_table(true);
}

/// After a screen's table changed: a screen with no other reason to be listed goes, the
/// chooser to All screens with the focus on it, or on Place when the chooser itself went.
fn after_table(keep_focus: bool) {
    let _ = set(|w| {
        let list = screen_choices(&w.pics.view, settings::get());
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

fn open_chooser(from: Ctl) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == from).map(|h| h.at)).flatten() else { return };
    // A Wallpaper menu lists the folder as it is now (AC-14), and its folders (spec 0028 AC-13).
    let files = matches!(from, Ctl::WallPick(_)).then(|| (wall_folder(), wall_dirs()));
    let _ = set(|w| {
        if let Some((files, dirs)) = files {
            w.wall_files = files;
            w.wall_dirs = dirs;
        }
        let (_, i) = chooser_lines(w, settings::get(), from);
        // A set checks no line: the focus starts on its first ticked picture.
        let i = i.or_else(|| match chooser_wall(w).map(|k| wall_held(settings::get(), wall_screen(w, settings::get()).as_deref(), k).1) {
            Some(Pick::Set(v)) => w.wall_files.iter().position(|f| v.contains(f)).map(|p| p + 1),
            _ => None,
        });
        let i = i.unwrap_or(0);
        w.chooser = Some((from, at, i));
        wall_show_line(w, i, true);
    });
    layout();
}

/// The row whose Wallpaper menu is open, if one is.
fn chooser_wall(w: &Win) -> Option<usize> {
    match w.chooser {
        Some((Ctl::WallPick(k), ..)) => Some(k),
        _ => None,
    }
}

/// Scrolls an open Wallpaper menu's list so line `line` is in view; `fresh`: from the top.
fn wall_show_line(w: &mut Win, line: usize, fresh: bool) {
    let (n, m) = (w.wall_files.len(), w.wall_dirs.len());
    let total = wall_total(n, m);
    let top = if fresh { 0 } else { w.wall_top };
    w.wall_top = wall_reveal(top, wall_slot(line, n, m), total, wall_rows(w.vh, total));
}

/// A Wallpaper menu's line chosen: Windows wallpaper, a picture or a folder writes the row's
/// key; Open folder opens it. The menu closes first, since a write lays the page out again.
fn choose_wall(k: usize, i: usize) {
    // None: Open folder.
    let Some((pick, screen)) = get(|w| {
        let (n, m) = (w.wall_files.len(), w.wall_dirs.len());
        let pick = match i {
            0 => Some(Pick::None),
            i if i <= n => Some(Pick::Picture(w.wall_files[i - 1].clone())),
            i if i <= n + m => Some(Pick::Folder(w.wall_dirs[i - n - 1].0.clone())),
            _ => None,
        };
        (pick, wall_screen(w, settings::get()))
    }) else { return };
    let _ = set(|w| {
        w.chooser = None;
        w.focus = Some(Ctl::WallPick(k));
    });
    layout();
    match pick {
        None => open_wallpapers(),
        Some(p) => {
            if let Some(edit) = wall_edit(settings::get(), screen.as_deref(), k, &p) {
                commit(edit, &format!("Workspace {} wallpaper", k + 1));
            }
        }
    }
}

/// A picture's tick box in an open Wallpaper menu: adds it to the row's set or takes it out,
/// and the menu stays open for more (spec 0028 AC-11).
fn tick_wall(i: usize) {
    let Some(Some((k, file, screen))) = get(|w| Some((chooser_wall(w)?, w.wall_files.get(i.checked_sub(1)?)?.clone(), wall_screen(w, settings::get())))) else { return };
    let _ = set(|w| {
        if let Some((from, at, _)) = w.chooser {
            w.chooser = Some((from, at, i));
        }
    });
    if let Some(edit) = wall_tick(settings::get(), screen.as_deref(), k, &file) {
        commit(edit, &format!("Workspace {} wallpaper", k + 1));
    } else {
        layout();
    }
}

/// Open folder (AC-14): the wallpapers folder in explorer, running as you, made first if it is
/// missing. `create_dir` fails rather than follows anything already at that name.
fn open_wallpapers() {
    let dir = crate::ws::data_dir().join("wallpapers");
    if !dir.is_dir()
        && let Err(e) = std::fs::create_dir(&dir)
    {
        warn!("settings page: the wallpapers folder was not made ({e})");
    }
    crate::tray::open(&dir);
}

/// The wheel over an open Wallpaper menu moves its pictures, three a notch.
fn wall_wheel(delta: f32) {
    let moved = set(|w| {
        let n = wall_total(w.wall_files.len(), w.wall_dirs.len());
        let rows = wall_rows(w.vh, n);
        let notches = (-delta / 120.0 * 3.0).round() as isize;
        let top = (w.wall_top as isize + notches).clamp(0, n.saturating_sub(rows) as isize) as usize;
        std::mem::replace(&mut w.wall_top, top) != top
    });
    if moved == Some(true) {
        layout();
    }
}

/// Main loop's tick: asks for the Wallpaper card's previews not decoded yet, and paints once
/// they are (AC-13). Nothing is asked again once each was tried.
fn wall_previews() {
    let s = settings::get();
    let Some((want, waiting)) = get(|w| (wall_wanted(w, s), w.wall_waiting)) else { return };
    let missing = want.as_ref().is_some_and(|(r, size, files)| crate::wallpaper::cached(*r, *size).is_none() || files.iter().any(|f| !crate::wallpaper::tried_ws(*r, f, *size, s.wallpaper_fit)));
    if missing && let Some(want) = want {
        crate::wallpaper::check(vec![want], s.wallpaper_fit);
    }
    let _ = set(|w| w.wall_waiting = missing);
    if waiting {
        layout();
    }
}

/// A line of the open screen menu chosen. A new screen on the Names card drops the text of
/// boxes that failed to save (spec 0023 AC-13).
fn choose_screen(i: usize) {
    if let Some(Some((Ctl::WallPick(k), ..))) = get(|w| w.chooser) {
        return choose_wall(k, i);
    }
    let apps = |c: Ctl| matches!(c, Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_));
    if let Some((from, edit)) = get(|w| w.chooser.filter(|(from, ..)| apps(*from)).map(|(from, ..)| (from, app_choice(w, settings::get(), from, i)))).flatten() {
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
        } else if let Some(c) = chosen(w, settings::get()).0.get(i) {
            w.screen = c.id.clone();
        }
        w.chooser = None;
        w.focus = Some(from);
    });
    layout();
}

/// The screen menu's keys: Up and Down move, Enter or Space picks, Esc closes.
fn chooser_key(vk: VIRTUAL_KEY) {
    let Some((from, at, f, n)) = get(|w| w.chooser.map(|(from, at, f)| (from, at, f, chooser_lines(w, settings::get(), from).0.len()))).flatten() else { return };
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

/// Show it there: the real indicator at its place on the chosen screen; All screens shows it
/// on the screen this window is on. Off with the indicator off or a screen not plugged in.
fn show_there() {
    let s = settings::get();
    if !s.indicator_enabled {
        return;
    }
    let target = get(|w| {
        let (list, i) = chosen(w, s);
        match (&list[i].id, list[i].present) {
            (None, _) => Some(None),
            (Some(_), Some(p)) => w.pics.view.present.get(p).map(|p| Some(p.full)),
            _ => None,
        }
    })
    .flatten();
    match target {
        Some(None) => show(),
        Some(Some(full)) => crate::ws::show_indicator(unsafe { MonitorFromRect(&full, MONITOR_DEFAULTTONEAREST) }),
        None => {}
    }
}

/// Where a click in a hex box puts the caret: the digit edge nearest it.
fn caret_at(at: Bx, x: f32) -> usize {
    let cw = get(|w| unsafe {
        let dc = GetDC(Some(w.hwnd));
        let c = Canvas::new(dc, false, w.scale, w.theme, &w.fonts);
        let cw = c.width("0", F::Code);
        drop(c);
        ReleaseDC(Some(w.hwnd), dc);
        cw
    })
    .unwrap_or(8.0);
    ((x - at.0 - 10.0 - cw) / cw).round().clamp(0.0, 6.0) as usize
}

/// The real indicator on the screen the page is on, with the current hold and fade.
fn show() {
    let Some(h) = hwnd() else { return };
    crate::ws::show_indicator(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) });
}

/// The keyboard stops in one area, in order, each with its top: the side list, or the rest.
fn stops(w: &Win, side: bool) -> Vec<(Ctl, f32)> {
    let mut out: Vec<(Ctl, f32)> = vec![];
    // The Overview picture's tiles take clicks only, never the keyboard (spec 0021).
    let mut hits: Vec<&Hit> = w.hits.iter().filter(|h| h.ctl.in_side() == side && h.group != 3 && !matches!(h.ctl, Ctl::ShowTile(..))).collect();
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
    if let Some(Some(Ctl::Hex(k))) = get(|w| w.focus)
        && Ctl::Hex(k) != c
    {
        leave_hex(k);
    }
    let _ = set(|w| w.focus = Some(c));
    if let Ctl::Hex(k) = c {
        enter_hex(k, None);
    }
    match c {
        Ctl::Side(sec) => {
            jump(sec);
            reveal();
        }
        _ => reveal(),
    }
    place_box();
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
            if value(s, k) != right as i64 && unused(s, k).is_empty() {
                save(k, right as i64);
            }
        }
        Ctl::Stepper(k) if is_place(k) => step_place(k, right),
        Ctl::Pick(ACCENT) => set_accent(right),
        Ctl::Pick(MODE) => set_mode(right),
        Ctl::Picture => walk_picture(right),
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
    if get(|w| w.chooser.is_some()) == Some(true) {
        return chooser_key(vk);
    }
    if get(|w| w.menu.is_some()) == Some(true) {
        return menu_key(vk);
    }
    let focus = get(|w| w.focus).flatten();
    if let Some(Ctl::Hex(k)) = focus
        && hex_key(k, vk)
    {
        return;
    }
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
            Some(Ctl::Stepper(_) | Ctl::Pick(_) | Ctl::Tiles | Ctl::Picture) | None => {}
            // The color picker: nothing on the page moves once it is open (spec 0021).
            Some(f @ Ctl::Swatch(_)) => act(f),
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

/// A point on the place picture as whole percent of its work area, kept 0 to 100.
fn percent(at: Bx, (x, y): (f32, f32)) -> (f64, f64) {
    let p = |v: f32, o: f32, len: f32| (((v - o) / len.max(1.0)) as f64 * 100.0).round().clamp(0.0, 100.0);
    (p(x, at.0, at.2), p(y, at.1, at.3))
}

fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. Column controls count only inside the column's view; while the
/// menu is open, only its lines do.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let menu = menu_open(w);
    let shown = |h: &Hit| if menu { h.group == 3 } else { h.group != 1 || (x > NAV && y > w.fixed) };
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the page on the screen under the mouse, or brings the open one to the front (AC-SET-1).
/// `section`: scroll to it.
pub fn open(section: Option<Section>) {
    open_at(section, None);
}

/// `open`, scrolled to `card` inside `section` when given (What's new's buttons, spec 0029 AC-4).
pub fn open_at(section: Option<Section>, card: Option<Card>) {
    let go = || match card {
        Some(c) => jump_card(c),
        None if section.is_some() => jump(section),
        None => {}
    };
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        go();
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
            Err(e) => return warn!("settings page: no window: {e}"),
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
            cards: vec![],
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
            hexes: vec![],
            own: None,
            screen: None,
            chooser: None,
            dragging: None,
            pick: None,
            geo: Geo::default(),
            ease: None,
            down_at: (0, 0),
            words: None,
            names_screen: None,
            typed: None,
            unsaved: vec![],
            wall_screen: None,
            wall_files: vec![],
            wall_dirs: vec![],
            wall_top: 0,
            wall_waiting: false,
        }));
        make_box(h, inst.into());
        // Shorter than H when the screen is (1366 × 768 at 125%); WM_SIZE sets vw and vh.
        let (ww, wh) = size(h, W, H);
        let wh = wh.min(work.bottom - work.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        update_pictures(true);
        go();
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
                        let t = painted(w);
                        let mut c = Canvas::new(mem, true, w.scale, t, &w.fonts);
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, t.b.bg, None);
                        compose(&mut c, w);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // The text box: the page's colors; the popup words' preview and a name box's note
            // follow each key typed.
            WM_CTLCOLOREDIT => {
                let (dc, b) = (HDC(wp.0 as *mut _), get(|w| painted(w).b).unwrap_or(theme().b));
                SetTextColor(dc, ui::cref(b.tx));
                SetBkColor(dc, ui::cref(b.card));
                LRESULT(crate::finder::edit_brush(b.card).0 as isize)
            }
            WM_COMMAND if (wp.0 >> 16) as u32 == EN_CHANGE => {
                if let Some(e) = BOX_EDIT.get()
                    && e.0 as isize == lp.0
                    && GetFocus() == e
                {
                    let text = edit_text(e);
                    let words = BOX_FOR.with_borrow(|f| matches!(f, Some((Ctl::Words, _))));
                    let _ = set(|w| if words { w.words = Some(text) } else { w.typed = Some(text) });
                    // The hint shows only while it is empty: paint all of it again, not only
                    // what changed.
                    let _ = InvalidateRect(Some(e), None, true);
                    layout();
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let (vk, repeat) = (VIRTUAL_KEY(wp.0 as u16), lp.0 >> 30 & 1 == 1);
                ease_finish();
                if get(|w| w.pick.is_some()) == Some(true) {
                    pick_key(vk, repeat);
                } else if !(vk == VK_DELETE && repeat) {
                    // A held Delete removes one workspace only, not one per repeat.
                    on_key(h, vk);
                }
                LRESULT(0)
            }
            WM_KEYUP => {
                pick_key_up(VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                pick_lost_keys();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == EASE_TIMER => {
                ease_step();
                LRESULT(0)
            }
            // The hand over what opens the color picker, the arrow elsewhere; the window's edges
            // keep their own cursors (spec 0021 AC-1).
            WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(h, &mut pt);
                let hand = get(|w| opens(w, hit_at(w, (pt.x as f32 / w.scale, pt.y as f32 / w.scale)))) == Some(true);
                let _ = SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).ok());
                LRESULT(1)
            }
            // TranslateMessage in main's loop makes these: a hex box takes its digits, never
            // while the color picker has the keys.
            WM_CHAR if get(|w| w.pick.is_some()) == Some(true) => LRESULT(0),
            WM_CHAR => {
                if let Some(Some(Ctl::Hex(k))) = get(|w| w.focus)
                    && let Some(c) = char::from_u32(wp.0 as u32).filter(char::is_ascii_hexdigit)
                    && {
                        enter_if_new(k);
                        get(|w| w.hexes.iter().any(|x| x.key == k)) == Some(true)
                    }
                {
                    hex_insert(k, &c.to_string());
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                ease_finish();
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                if get(menu_open) == Some(true) {
                    if get(|w| matches!(w.chooser, Some((Ctl::WallPick(_), ..)))) == Some(true) {
                        wall_wheel(delta);
                    }
                    return LRESULT(0);
                }
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Where it went down on screen, for the color picker's opening guard (AC-15).
                let pos = GetMessagePos();
                let _ = set(|w| w.down_at = ((pos & 0xFFFF) as i16 as i32, (pos >> 16 & 0xFFFF) as i16 as i32));
                ease_finish();
                // A click while recording only cancels it: the note goes, and the page moves
                // under the mouse, so the click must not land on whatever is there now.
                if get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                    return LRESULT(0);
                }
                if get(|w| w.pick.is_some()) == Some(true) {
                    pick_down(h, lp);
                    return LRESULT(0);
                }
                // A click outside the open menu closes it, and does nothing else.
                if get(|w| menu_open(w) && hit_at(w, point(lp, w.scale)).is_none()) == Some(true) {
                    close_menu();
                    return LRESULT(0);
                }
                let _ = SetCapture(h);
                let was = get(|w| w.focus).flatten();
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
                            // Tiles open the color picker and are no keyboard stop: the focus stays.
                            if let Some(p) = w.press.filter(|p| !matches!(p, Ctl::Item(_) | Ctl::ScreenItem(_) | Ctl::WallTick(_) | Ctl::WsTile(..) | Ctl::ShowTile(..))) {
                                w.focus = Some(p.stop());
                            }
                            // Custom: a press on the picture puts the card's center there.
                            if w.press == Some(Ctl::Picture)
                                && settings::get().indicator_enabled
                                && settings::parse_error().is_none()
                                && let Some(at) = w.hits.iter().find(|h| h.ctl == Ctl::Picture).map(|h| h.at)
                            {
                                w.dragging = Some(percent(at, (x, y)));
                            }
                        }
                    }
                });
                // Leaving a hex box saves six digits; a click in one puts the caret there.
                let now = get(|w| w.focus).flatten();
                if let Some(Ctl::Hex(k)) = was
                    && now != was
                {
                    leave_hex(k);
                }
                if let Some(Ctl::Hex(k)) = now
                    && let Some(at) = get(|w| w.hits.iter().find(|x| x.ctl == Ctl::Hex(k)).map(|x| x.at)).flatten()
                {
                    let x = get(|w| point(lp, w.scale).0).unwrap_or(0.0);
                    enter_hex(k, Some(caret_at(at, x)));
                }
                place_box();
                let _ = PostMessageW(Some(h), WM_MOUSEMOVE, wp, lp);
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some((part, pt)) = get(|w| w.pick.as_ref().and_then(|p| p.drag).map(|d| (d, point(lp, w.scale)))).flatten() {
                    pick_move(part, pt);
                    return LRESULT(0);
                }
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
                    let at = w.hits.iter().find(|h| h.ctl == Ctl::Picture).map(|h| h.at);
                    match (w.dragging, at) {
                        (Some(_), Some(at)) => {
                            w.dragging = Some(percent(at, point(lp, w.scale)));
                            true
                        }
                        _ => false,
                    }
                }) == Some(true)
                {
                    layout();
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
                // A Custom drag ends: Across, then Down, each against the file as it is then.
                if let Some(Some((x, y))) = set(|w| w.dragging.take()) {
                    set_place(ACROSS, x);
                    set_place(DOWN, y);
                }
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
                let _ = set(|w| {
                    w.drag = None;
                    w.dragging = None;
                });
                finish_drag();
                LRESULT(0)
            }
            WM_SIZE => {
                if wp.0 as u32 != SIZE_MINIMIZED {
                    ease_finish();
                    if set(|w| {
                        w.vw = (lp.0 & 0xFFFF) as f32 / w.scale;
                        w.vh = ((lp.0 >> 16) & 0xFFFF) as f32 / w.scale;
                        w.menu = None;
                        w.chooser = None;
                    })
                    .is_some()
                    {
                        // A resize (a move to a screen of another scale is one) closes the
                        // color picker and keeps its color; never from inside the set above.
                        pick_close(How::Keep(false));
                        layout();
                    }
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
                    // The color picker closes as the menus do, keeping its color (spec 0021 AC-6).
                    pick_close(How::Keep(false));
                    if get(menu_open) == Some(true) {
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
                // Losing focus closed the color picker already; this drops it writing nothing.
                let _ = KillTimer(Some(h), EASE_TIMER);
                RING.set(None);
                SQUARE.set(None);
                BOX_EDIT.set(None);
                BOX_FONT.set(0);
                BOX_FOR.set(None);
                BOX_HINT.set((String::new(), 0));
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
                // The color picker's preview is rebuilt with the theme, and goes once the page shows its color.
                if set(|w| {
                    if let Some(p) = w.pick.as_mut() {
                        p.preview = preview_for(t, p.key, p.color);
                    }
                    std::mem::replace(&mut w.theme, t) != t
                }) == Some(true)
                {
                    dark_title(h, &t);
                    layout();
                }
                // High contrast coming on closes it, writing nothing (spec 0021 AC-10).
                if get(must_close) == Some(true) {
                    pick_close(How::Quiet);
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

    fn loaded(text: &str) -> &'static Settings {
        Box::leak(Box::new(settings::parse_for_test(text).0))
    }

    /// An edit in words, since toml values don't compare: "at colors.ws2 = \"#DDB43D\"".
    fn said(e: Option<Edit>) -> String {
        match e {
            Some(Edit::At(p, v)) => format!("at {} = {}", p.join("."), v.to_string().trim()),
            Some(Edit::Remove(p)) => format!("remove {}", p.join(".")),
            Some(_) => "other".into(),
            None => "none".into(),
        }
    }

    // AC-12: the file only holds colors that differ, so typing des-k's own gold takes ws2 out
    // and the workspace follows des-k's palette again.
    #[test]
    fn a_default_color_takes_its_key_out() {
        assert_eq!(said(Some(color_edit("colors.ws2", 0xDDB43C))), "remove colors.ws2");
        assert_eq!(said(Some(color_edit("colors.ws2", 0xDDB43D))), "at colors.ws2 = \"#DDB43D\"");
        assert_eq!(said(Some(color_edit(ACCENT, 0xE0457B))), "at colors.accent = \"#E0457B\"");
    }

    // AC-13: a screen key equal to All screens' goes, so the screen follows All screens again,
    // but never the table's last key: walking the spots must not silently drop a kept place.
    #[test]
    fn a_screen_key_equal_to_all_screens_goes_but_never_the_last() {
        let id = "DEL-A0F5-1234";
        let two = loaded("[indicator]\ngap = 24\n[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\ngap = 40\n");
        assert_eq!(said(place_edit(two, Some(id), GAP, 24.0)), "remove indicator.screens.DEL-A0F5-1234.gap");
        let one = loaded("[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\n");
        let center = spot_index(settings::Spot::Center) as f64;
        assert_eq!(said(place_edit(one, Some(id), SPOT, center)), "at indicator.screens.DEL-A0F5-1234.spot = \"center\"");
        assert_eq!(said(place_edit(one, Some(id), GAP, 24.0)), "none", "it follows All screens' gap already");
        assert_eq!(said(place_edit(one, Some(id), GAP, 40.0)), "at indicator.screens.DEL-A0F5-1234.gap = 40");
        let none = loaded("");
        assert_eq!(said(place_edit(none, Some(id), SPOT, center)), "none");
        assert_eq!(said(place_edit(none, None, SPOT, 2.0)), "at indicator.spot = \"top-right\"");
        assert_eq!(said(place_edit(none, None, ACROSS, 70.0)), "at indicator.x = 70");
        // And what the screen then uses: its spot from its table, the rest from All screens.
        let text = "[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\n";
        let after = settings::parse_for_test(&settings::apply_for_test(text, &place_edit(one, Some(id), GAP, 40.0).unwrap())).0;
        assert_eq!(pos_of(&after, Some(id)).gap, 40);
        assert_eq!(pos_of(&after, Some(id)).spot, settings::Spot::TopRight);
    }

    // The hex box's paste takes exactly six hex digits, trimming spaces and a leading #, so a
    // stray paste never saves half a color.
    #[test]
    fn paste_takes_exactly_six_hex_digits() {
        assert_eq!(pasted(" #2e8b57 "), Some("2E8B57".into()));
        assert_eq!(pasted("2E8B57"), Some("2E8B57".into()));
        assert_eq!(pasted("#2E8B5"), None);
        assert_eq!(pasted("#2E8B577"), None);
        assert_eq!(pasted("seagreen"), None);
        assert_eq!(hex_color("2E8B57"), Some(0x2E8B57));
        assert_eq!(hex_color("2E8B5"), None);
    }

    // Spots, from Custom, picks the spot nearest the card: the third it sits in.
    #[test]
    fn spots_from_custom_is_the_nearest_third() {
        assert_eq!(SPOT_NAMES[nearest(70.0, 20.0)], "Top right");
        assert_eq!(SPOT_NAMES[nearest(50.0, 50.0)], "Center");
        assert_eq!(SPOT_NAMES[nearest(0.0, 100.0)], "Bottom left");
    }

    // The chooser lists every screen with a reason to be there: plugged in, remembered, or
    // named by a table des-k never saw; All screens always first.
    #[test]
    fn the_chooser_lists_every_known_screen() {
        let r = RECT { left: 0, top: 0, right: 10, bottom: 10 };
        let view = crate::ws::View {
            present: vec![crate::ws::Present { id: "DEL-1".into(), device: "DISPLAY2".into(), name: "DELL U2720Q".into(), windows_name: "DELL U2720Q".into(), full: r, work: r }],
            waiting: vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }],
            ..Default::default()
        };
        let s = loaded("[indicator.screens.\"DEL-1\"]\nspot = \"top\"\n[indicator.screens.\"ACR-9\"]\nspot = \"left\"\n");
        let list = screen_choices(&view, s);
        let shown: Vec<(&str, &str)> = list.iter().map(|c| (c.name.as_str(), c.side)).collect();
        assert_eq!(shown, [("All screens", ""), ("DELL U2720Q", "Own place"), ("LG 27UL850", "Not plugged in"), ("ACR-9", "Unknown screen")]);
    }

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
                Kind::Ladder(ladder) => if d == ladder[0] { ladder[1] } else { ladder[0] },
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
        // Spec 0023 AC-14: Show screen name greys with the indicator, without words, and in
        // the Dots style, which shows no names; greyed, it keeps its value.
        assert_eq!(unused(&s, "indicator.screen_name"), vec![Why::IndicatorOff]);
        s.indicator_enabled = true;
        assert!(unused(&s, "indicator.screen_name").is_empty());
        s.indicator_style = settings::Style::Dots;
        assert_eq!(unused(&s, "indicator.screen_name"), vec![Why::Dots]);
        assert_eq!(value(&s, "indicator.screen_name"), 1);
    }

    // Spec 0023 AC-12: a name box writes its own key and nothing else. An emptied All screens
    // box takes out names.wsN; an emptied own row takes out the screen's key, so the row
    // follows all screens again; Enter on an empty following row writes "" (no name there) only
    // when all screens has a name to hide, and leaving it writes nothing. Text is cleaned as the
    // file is read, so unchanged text, give or take spaces, writes nothing.
    #[test]
    fn a_name_box_writes_only_its_own_key() {
        let s = loaded("[names]\nws1 = \"Mail\"\nws2 = \"Code\"\n[names.screens.internal]\nscreen = \"Left\"\nws2 = \"Notes\"\nws3 = \"\"\n");
        let e = |screen: Option<&str>, row: Nm, text: &str, enter: bool| said(name_edit(s, screen, row, text, enter));
        assert_eq!(e(None, Nm::Ws(0), "", false), "remove names.ws1");
        assert_eq!(e(None, Nm::Ws(0), " Mail ", true), "none");
        assert_eq!(e(None, Nm::Ws(0), "Post", false), "at names.ws1 = \"Post\"");
        assert_eq!(e(None, Nm::Ws(2), "", true), "none"); // nothing there to take out
        assert_eq!(e(Some("internal"), Nm::Ws(1), "", false), "remove names.screens.internal.ws2");
        assert_eq!(e(Some("internal"), Nm::Ws(0), "", true), "at names.screens.internal.ws1 = \"\"");
        assert_eq!(e(Some("internal"), Nm::Ws(0), "", false), "none");
        assert_eq!(e(Some("internal"), Nm::Ws(3), "", true), "none"); // all screens has no name for it
        assert_eq!(e(Some("internal"), Nm::Ws(2), "", true), "none"); // no name stays no name
        assert_eq!(e(Some("internal"), Nm::Ws(2), "Mail", false), "at names.screens.internal.ws3 = \"Mail\"");
        assert_eq!(e(Some("internal"), Nm::Screen, "", false), "remove names.screens.internal.screen");
        assert_eq!(e(Some("DEL-1"), Nm::Screen, "Right", true), "at names.screens.DEL-1.screen = \"Right\"");
        assert_eq!(e(None, Nm::Screen, "Right", true), "none"); // All screens has no screen name
    }

    // Spec 0023 AC-12: a screen id holding # or . gets its own quoted table, never a table
    // path made from its parts, and taking out its last key takes the table with it.
    #[test]
    fn a_screen_id_with_odd_letters_keeps_its_own_table() {
        let id = "DEL#1.A";
        let at = name_edit(loaded(""), Some(id), Nm::Ws(1), "Notes", false).unwrap();
        let text = settings::apply_for_test("[names]\nws2 = \"Code\"\n", &at);
        let (read, warn) = settings::parse_for_test(&text);
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!((read.ws_name(id, 1), read.ws_name("internal", 1)), (Some("Notes"), Some("Code")));
        let gone = settings::apply_for_test(&text, &Edit::Remove(name_path(Some(id), Nm::Ws(1)).unwrap()));
        assert!(!gone.contains("screens"), "{gone}");
        assert!(gone.contains("ws2 = \"Code\""), "{gone}");
    }

    // Spec 0026 AC-15: a Wallpaper menu writes only what differs from what the row follows, so
    // the file holds no key that changes nothing. On All screens, Windows wallpaper is no key at
    // all. On a screen, choosing All screens' picture takes the screen's key out (and its table
    // with its last key); Windows wallpaper there is "" only while All screens has a picture.
    #[test]
    fn a_wallpaper_choice_writes_only_what_differs() {
        let s = loaded("[wallpaper]\nws1 = \"forest.jpg\"\n[wallpaper.screens.internal]\nws2 = \"city.jpg\"\nws3 = \"\"\n");
        let e = |screen: Option<&str>, k: usize, pick: &str| said(wall_edit(s, screen, k, &pic(pick)));
        assert_eq!(e(None, 0, "forest.jpg"), "none");
        assert_eq!(e(None, 0, "desk.png"), "at wallpaper.ws1 = \"desk.png\"");
        assert_eq!(e(None, 0, ""), "remove wallpaper.ws1");
        assert_eq!(e(None, 3, ""), "none"); // nothing there to take out
        assert_eq!(e(Some("internal"), 0, "forest.jpg"), "none"); // it follows that already
        assert_eq!(e(Some("internal"), 0, ""), "at wallpaper.screens.internal.ws1 = \"\"");
        assert_eq!(e(Some("internal"), 0, "desk.png"), "at wallpaper.screens.internal.ws1 = \"desk.png\"");
        assert_eq!(e(Some("internal"), 1, "city.jpg"), "none");
        assert_eq!(e(Some("internal"), 1, ""), "remove wallpaper.screens.internal.ws2"); // All screens has none for it
        assert_eq!(e(Some("internal"), 2, ""), "remove wallpaper.screens.internal.ws3");
        assert_eq!(e(Some("internal"), 3, ""), "none");
        let own = "[wallpaper.screens.internal]\nws2 = \"city.jpg\"\n";
        let gone = settings::apply_for_test(own, &wall_edit(loaded(own), Some("internal"), 1, &Pick::None).unwrap());
        assert!(!gone.contains("screens"), "{gone}");
        let (read, warn) = settings::parse_for_test(&settings::apply_for_test("", &wall_edit(loaded(""), Some("DEL#1.A"), 2, &pic("a b.png")).unwrap()));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(read.wall_name("DEL#1.A", 2), Some("a b.png"));
    }

    // Spec 0026 AC-13: each row says how it reads, from the file's keys alone, and its menu
    // checks the picture the screen shows; a picture the folder no longer has checks nothing.
    #[test]
    fn a_wallpaper_row_reads_its_keys() {
        let s = loaded("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = \"gone.jpg\"\n[wallpaper.screens.internal]\nws2 = \"city.jpg\"\nws3 = \"\"\n");
        assert_eq!(wall_held(s, None, 0), (WallState::All, pic("forest.jpg")));
        assert_eq!(wall_held(s, Some("internal"), 0), (WallState::Follow, pic("forest.jpg")));
        assert_eq!(wall_held(s, Some("internal"), 1), (WallState::Own, pic("city.jpg")));
        assert_eq!(wall_held(s, Some("internal"), 2), (WallState::Windows, Pick::None));
        assert_eq!(wall_held(s, Some("internal"), 3), (WallState::Follow, Pick::None));
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.wall_files = vec!["city.jpg".into(), "desk.png".into(), "forest.jpg".into()];
        let (lines, cur) = chooser_lines(&w, s, Ctl::WallPick(0));
        let names: Vec<String> = lines.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Windows wallpaper", "city.jpg", "desk.png", "forest.jpg", "Open folder"]);
        assert_eq!(cur, Some(3));
        assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, None, "gone.jpg isn't in the folder");
        assert_eq!(chooser_lines(&w, s, Ctl::WallPick(3)).1, Some(0));
        w.wall_screen = Some("internal".into());
        assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, Some(1), "the screen's own city.jpg");
    }

    // Spec 0028 AC-11: a tick adds a picture to the row's pictures or takes it out, and writes
    // what the reader takes back as that same value: one picture a plain name, two or more a
    // list by name, none your Windows wallpaper. On a screen, a row that follows all screens
    // starts from their pictures, and a set equal to all screens' takes the screen's key out.
    #[test]
    fn a_tick_writes_the_rows_pictures_by_name() {
        let text = "[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"city.jpg\", \"desk.png\"]\nws4 = \"beach\"\n[wallpaper.screens.internal]\nws3 = \"dunes.jpg\"\n";
        let s = loaded(text);
        let e = |screen: Option<&str>, k: usize, f: &str| said(wall_tick(s, screen, k, f));
        assert_eq!(e(None, 0, "desk.png"), "at wallpaper.ws1 = [\"desk.png\", \"forest.jpg\"]");
        assert_eq!(e(None, 0, "forest.jpg"), "remove wallpaper.ws1", "its last picture: Windows wallpaper");
        assert_eq!(e(None, 1, "city.jpg"), "at wallpaper.ws2 = \"desk.png\"", "one left is a plain name");
        assert_eq!(e(None, 1, "Alps.jpg"), "at wallpaper.ws2 = [\"Alps.jpg\", \"city.jpg\", \"desk.png\"]", "sorted with case ignored");
        assert_eq!(e(None, 3, "a.jpg"), "at wallpaper.ws4 = \"a.jpg\"", "a tick replaces a folder");
        assert_eq!(e(Some("internal"), 1, "dunes.jpg"), "at wallpaper.screens.internal.ws2 = [\"city.jpg\", \"desk.png\", \"dunes.jpg\"]");
        assert_eq!(e(Some("internal"), 2, "dunes.jpg"), "remove wallpaper.screens.internal.ws3", "none, as all screens has");
        let own = loaded("[wallpaper]\nws2 = [\"city.jpg\", \"desk.png\"]\n[wallpaper.screens.internal]\nws2 = [\"city.jpg\", \"desk.png\", \"x.jpg\"]\n");
        assert_eq!(said(wall_tick(own, Some("internal"), 1, "x.jpg")), "remove wallpaper.screens.internal.ws2", "the same set as all screens");
        // What it writes reads back as the set it meant.
        let (read, warn) = settings::parse_for_test(&settings::apply_for_test(text, &wall_tick(s, None, 0, "desk.png").unwrap()));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(read.wallpaper[0], Pick::Set(vec!["desk.png".into(), "forest.jpg".into()]));
    }

    // Spec 0026 AC-13: with one known screen the card hides its chooser and writes All screens,
    // but a screen with its own pictures keeps the chooser, or those pictures could be neither
    // changed nor cleared and every change would go where that screen never looks (tried live
    // 2026-11-21 on a laptop alone: nothing changed, whatever was picked).
    #[test]
    fn a_lone_screen_with_its_own_pictures_keeps_the_chooser() {
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.pics.view.present.truncate(1);
        w.pics.view.waiting.clear();
        w.wall_screen = Some("internal".into());
        let plain = loaded("[wallpaper]\nws2 = \"a.jpg\"\n");
        assert!(!wall_two(&w, plain));
        assert_eq!(wall_screen(&w, plain), None, "no chooser: the rows write All screens");
        let own = loaded("[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws2 = \"b.jpg\"\n");
        assert!(wall_two(&w, own));
        assert_eq!(wall_screen(&w, own).as_deref(), Some("internal"));
        w.wall_screen = None;
        assert_eq!(wall_screen(&w, own), None, "All screens until you pick the screen");
    }

    // Spec 0026 AC-13 (grown 2026-11-21): Use this there too takes out the plugged in screens'
    // own value for that row alone, so they follow All screens again; a screen not plugged in
    // keeps its own, and other rows keep theirs, since you asked for one row on what you see.
    #[test]
    fn use_this_there_too_clears_only_plugged_in_screens_for_the_row() {
        let text = "[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws1 = \"x.jpg\"\nws2 = \"b.jpg\"\n[wallpaper.screens.\"DEL-1\"]\nws2 = \"c.jpg\"\n[wallpaper.screens.\"LG-2\"]\nws2 = \"d.jpg\"\n";
        let present = ["internal".to_string(), "DEL-1".to_string()];
        let edit = wall_use_edit(loaded(text), &present, 1).expect("two screens to clear");
        let (read, warn) = settings::parse_for_test(&settings::apply_for_test(text, &edit));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(read.wallpaper_screens.get("internal").map(|o| (o[0].clone(), o[1].clone())), Some((Some(pic("x.jpg")), None)), "row 1 kept");
        assert!(!read.wallpaper_screens.contains_key("DEL-1"), "its last key gone, with its table");
        assert_eq!(read.wallpaper_screens.get("LG-2").map(|o| o[1].clone()), Some(Some(pic("d.jpg"))), "not plugged in");
        assert!(wall_use_edit(loaded(text), &present, 2).is_none(), "nothing to clear");
    }

    // Spec 0026 AC-15 (amended 2026-11-22): Windows wallpaper on All screens is a reset, so
    // every screen shows your Windows wallpaper on that workspace: each screen's own value for
    // the row goes, plugged in or not (one coming back must not bring a picture back), even
    // when All screens had no picture. Other rows keep theirs, and a picture on All screens
    // still leaves a screen's own alone (Use this there too is for that).
    #[test]
    fn windows_wallpaper_on_all_screens_resets_the_row_everywhere() {
        let text = "[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws1 = \"x.jpg\"\nws2 = \"b.jpg\"\nws3 = \"\"\n[wallpaper.screens.\"LG-2\"]\nws2 = \"d.jpg\"\nws3 = \"e.jpg\"\n";
        let reset = |k: usize| {
            let edit = wall_edit(loaded(text), None, k, &Pick::None).expect("something to take out");
            let (read, warn) = settings::parse_for_test(&settings::apply_for_test(text, &edit));
            assert!(warn.is_empty(), "{warn:?}");
            read
        };
        let two = reset(1);
        assert_eq!(two.wallpaper[1], Pick::None);
        assert_eq!(two.wallpaper_screens.get("internal").map(|o| (o[0].clone(), o[1].clone())), Some((Some(pic("x.jpg")), None)), "row 1 kept");
        assert_eq!(two.wallpaper_screens.get("LG-2").map(|o| o[1].clone()), Some(None), "not plugged in, still reset");
        let three = reset(2);
        assert_eq!(three.wallpaper_screens.get("internal").map(|o| o[2].clone()), Some(None), "All screens had none: still reset");
        assert_eq!(three.wallpaper_screens.get("LG-2").map(|o| o[2].clone()), Some(None));
        assert_eq!(said(wall_edit(loaded(text), None, 3, &Pick::None)), "none", "nothing anywhere");
        assert_eq!(said(wall_edit(loaded(text), None, 1, &pic("c.jpg"))), "at wallpaper.ws2 = \"c.jpg\"", "a picture keeps screens' own");
    }

    // Spec 0028 AC-11: an All screens slideshow previews the picture its screen shows now, but
    // only if that screen runs it: a screen with its own value must not lend its picture. With
    // none running, a set previews its first picture rather than an empty tile (tried live
    // 2026-11-21: the only screen had its own picture and the row looked broken); a folder
    // keeps its tile, since naming a picture would mean reading the folder.
    #[test]
    fn a_slideshow_previews_only_what_its_screen_runs() {
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.pics.wall_now.insert(("internal".into(), 0), "x.jpg".into());
        let own = loaded("[wallpaper]\nws1 = [\"a.jpg\", \"b.jpg\"]\nws2 = \"beach\"\n[wallpaper.screens.internal]\nws1 = [\"x.jpg\", \"y.jpg\"]\nws2 = \"trips\"\n");
        assert_eq!(wall_shows(&w, own, None, 0).as_deref(), Some("a.jpg"), "not the screen's own slideshow");
        assert_eq!(wall_shows(&w, own, None, 1), None, "a folder keeps its tile");
        assert_eq!(wall_shows(&w, own, Some("internal"), 0).as_deref(), Some("x.jpg"), "the screen's own runs");
        w.pics.wall_now.insert(("internal".into(), 0), "b.jpg".into());
        let follows = loaded("[wallpaper]\nws1 = [\"a.jpg\", \"b.jpg\"]\n");
        assert_eq!(wall_shows(&w, follows, None, 0).as_deref(), Some("b.jpg"), "the one showing now");
    }

    // Spec 0028 AC-11: a menu lists the folder's pictures, then its slideshow folders, and checks
    // the row's folder; a set checks no line, since its pictures show ticks. The list scrolls
    // as one, the Slideshows heading counted, so a folder the keys move to is in view.
    #[test]
    fn a_wallpaper_menu_lists_folders_after_pictures() {
        let s = loaded("[wallpaper]\nws1 = \"beach\"\nws2 = [\"city.jpg\", \"desk.png\"]\n");
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.wall_files = vec!["city.jpg".into(), "desk.png".into()];
        w.wall_dirs = vec![("beach".into(), 12), ("trips".into(), 0)];
        let (lines, cur) = chooser_lines(&w, s, Ctl::WallPick(0));
        let names: Vec<String> = lines.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Windows wallpaper", "city.jpg", "desk.png", "beach", "trips", "Open folder"]);
        assert_eq!(cur, Some(3), "the beach folder");
        assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, None, "a set");
        assert_eq!(wall_total(2, 2), 5);
        assert_eq!(wall_total(2, 0), 2, "no heading without folders");
        assert_eq!((wall_slot(2, 2, 2), wall_slot(3, 2, 2), wall_slot(5, 2, 2)), (2, 4, 6));
        assert_eq!(wall_reveal(0, wall_slot(4, 2, 2), wall_total(2, 2), 3), 2, "trips is the last line in view");
        assert_eq!(wall_reveal(2, wall_slot(5, 2, 2), wall_total(2, 2), 3), 2, "Open folder never moves the list");
    }

    // Spec 0028 AC-11: a workspace's time per picture moves on the ladder; a hand typed value
    // off it steps onto it, the ends stay put, and the words read as the design shows them.
    #[test]
    fn time_per_picture_steps_on_its_ladder() {
        assert_eq!((ladder_step(LADDER, 30, true), ladder_step(LADDER, 30, false)), (45, 20));
        assert_eq!((ladder_step(LADDER, 25, true), ladder_step(LADDER, 25, false)), (30, 20));
        assert_eq!((ladder_step(LADDER, 1, false), ladder_step(LADDER, 1440, true)), (1, 1440));
        assert_eq!([minutes(45), minutes(60), minutes(90), minutes(1440)], ["45 min", "1 h", "1 h 30 min", "24 h"]);
        assert_eq!(changed("wallpaper.interval", "30", "60"), "Interval: 30 min → 1 h");
        assert_eq!(changed("wallpaper.intervals.ws3", "default", "10"), "Workspace 3 time per picture: default → 10 min");
    }

    // Spec 0028 AC-11: a workspace's own time is written only while it differs from Interval;
    // stepping back to Interval's takes the key out with its table, so the file never keeps an
    // empty [wallpaper.intervals] or a key that changes nothing.
    #[test]
    fn a_workspace_time_back_at_the_default_leaves_no_key() {
        let text = "[wallpaper]\ninterval = 30\nws3 = \"beach\"\n";
        assert_eq!(said(wall_interval_edit(loaded(text), 2, true)), "at wallpaper.intervals.ws3 = 45");
        let own = settings::apply_for_test(text, &wall_interval_edit(loaded(text), 2, true).unwrap());
        assert_eq!(loaded(&own).interval_for(2), 45);
        assert_eq!(said(wall_interval_edit(loaded(&own), 2, false)), "remove wallpaper.intervals.ws3");
        let back = settings::apply_for_test(&own, &wall_interval_edit(loaded(&own), 2, false).unwrap());
        assert!(!back.contains("intervals"), "{back}");
        assert_eq!(said(wall_interval_edit(loaded("[wallpaper]\ninterval = 1440\n"), 0, true)), "none", "already 24 h");
    }

    // Spec 0026 AC-13: a long folder scrolls inside the menu, and the line the keys move to is
    // always in view; Windows wallpaper and Open folder never move the list.
    #[test]
    fn a_long_wallpaper_menu_keeps_its_focus_in_view() {
        assert_eq!(wall_rows(H, 40), 10);
        assert_eq!(wall_rows(MIN_H, 40), 10);
        assert_eq!(wall_rows(300.0, 40), 3);
        assert_eq!(wall_rows(H, 2), 2);
        assert_eq!(wall_reveal(0, 12, 40, 10), 2); // picture 12 is the last line in view
        assert_eq!(wall_reveal(5, 3, 40, 10), 2);
        assert_eq!(wall_reveal(5, 0, 40, 10), 5);
        assert_eq!(wall_reveal(5, 41, 40, 10), 5);
        assert_eq!(wall_reveal(35, 40, 40, 10), 30);
        assert_eq!(wall_reveal(0, 2, 2, 2), 0);
    }

    // Spec 0026 AC-18: Dim reads "Off" at 0 and "40%" otherwise, on its row and in a hand
    // edit's line; a hand typed value off the 5 grid steps onto it.
    #[test]
    fn dim_reads_off_or_a_percent() {
        let r = find_row("wallpaper.dim").unwrap();
        assert_eq!((shown(r, 0), shown(r, 40)), ("Off".to_string(), "40%".to_string()));
        assert_eq!(changed("wallpaper.dim", "0", "40"), "Dim pictures: Off → 40%");
        assert_eq!(changed("wallpaper.dim", "30", "40"), "Dim pictures: 30 → 40%");
        assert_eq!(changed("wallpaper.fit", "fill", "center"), "Fit: Fill → Center");
        assert_eq!((stepped(42, 0, 80, 5, true), stepped(42, 0, 80, 5, false)), (45, 40));
        let mut s = Settings::default();
        assert!(unused(&s, "wallpaper.transition").is_empty());
        s.fastest = true;
        assert_eq!(unused(&s, "wallpaper.transition"), vec![Why::Skip]);
    }

    // Spec 0023 AC-12: the Names chooser lists the screens plugged in, then remembered ones, and
    // tells two screens you gave the same name apart by Windows' own name.
    #[test]
    fn the_names_chooser_tells_same_names_apart() {
        let r = RECT::default();
        let p = |id: &str, name: &str, windows: &str| crate::ws::Present { id: id.into(), device: String::new(), name: name.into(), windows_name: windows.into(), full: r, work: r };
        let view = crate::ws::View {
            present: vec![p("DEL-1", "Left", "DELL U2720Q"), p("LG-2", "Left", "LG 27UL850"), p("internal", "Built-in display", "Built-in display")],
            waiting: vec![crate::ws::Waiting { id: "HP-3".into(), name: "HP E24".into(), windows_name: "HP E24".into(), windows: vec![] }],
            ..Default::default()
        };
        let shown: Vec<(String, String)> = name_choices(&view).into_iter().map(|c| (c.name, c.side)).collect();
        let want = [("All screens", ""), ("Left", "DELL U2720Q"), ("Left", "LG 27UL850"), ("Built-in display", ""), ("HP E24", "Not plugged in")];
        assert_eq!(shown, want.map(|(a, b)| (a.to_string(), b.to_string())));
    }

    // Spec 0023 AC-13: after a failed save the box keeps your text, whatever a reload brings,
    // until a good save or Esc; the same workspace on a screen is another box.
    #[test]
    fn a_name_that_failed_to_save_outlives_a_reload() {
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.unsaved = vec![(None, Nm::Ws(1), "Codex".into())];
        let l = name_look(&w, loaded("[names]\nws2 = \"Code\"\n"), Nm::Ws(1));
        assert!(l.unsaved);
        assert_eq!((l.shown.as_str(), l.saved.as_str()), ("Codex", "Code"));
        let edited = loaded("[names]\nws2 = \"Mail\"\n");
        assert_eq!(name_look(&w, edited, Nm::Ws(1)).shown, "Codex");
        assert_eq!(name_look(&w, edited, Nm::Ws(0)).shown, "");
        w.names_screen = Some("internal".into());
        assert!(!name_look(&w, edited, Nm::Ws(1)).unsaved);
    }

    // Spec 0023 AC-12: with one known screen the card has no chooser: its Screen name row
    // renames that screen, with Windows' name as the hint, and the workspace rows name every
    // screen, whatever screen was chosen before.
    #[test]
    fn one_screen_renames_itself_and_names_all_screens() {
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.pics.view.present.truncate(1);
        w.names_screen = Some("DEL-A0F5-1234".into());
        let s = loaded("");
        let screen = name_look(&w, s, Nm::Screen);
        assert_eq!((screen.screen.as_deref(), screen.hint.as_str()), (Some("internal"), "Built-in display"));
        assert_eq!(name_look(&w, s, Nm::Ws(0)).screen, None);
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
        // App rules are not shortcut keys: they once read "notepad.exe: 3 or …" (tried 2026-11-22).
        assert_eq!(changed("apps", "[notepad.exe: 3]", "[notepad.exe: 3, mspaint.exe: 2 on XYZ-0000-1]"), "App rules: changed");
    }

    // The Windows key of an action another action holds reads as taken by that one.
    #[test]
    fn a_taken_windows_key_names_its_holder() {
        let s = Settings { overview: vec![], finder: vec![Combo { mods: WIN, vk: 0x46 }, Combo { mods: WIN, vk: 0x09 }], ..Default::default() };
        let sh = shortcut("shortcuts.overview").unwrap();
        assert_eq!(took(&s, sh), Some("Find a window"));
    }

    // The page tests that set settings::get() for every reader run one at a time.
    static SETTINGS: Mutex<()> = Mutex::new(());

    fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT { left: l, top: t, right: r, bottom: b }
    }

    /// Two screens as the render states draw them: a laptop on workspace 1 and a DELL on 2.
    fn fake_pics() -> Pics {
        let r = rect;
        Pics {
            screens: vec![
                crate::ws::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 600, 1440, 1500), scale: 200 },
                crate::ws::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(1440, 0, 4000, 1440), scale: 150 },
            ],
            view: crate::ws::View {
                active: vec![("DISPLAY1".into(), 0), ("DISPLAY2".into(), 1)],
                windows: [("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY2", 0), ("DISPLAY2", 1), ("DISPLAY2", 1), ("DISPLAY2", 3)].iter().map(|(d, w)| (d.to_string(), *w, r(0, 0, 10, 10))).collect(),
                names: vec![("DISPLAY1".into(), "Built-in display".into()), ("DISPLAY2".into(), "DELL U2720Q".into())],
                present: vec![
                    crate::ws::Present { id: "internal".into(), device: "DISPLAY1".into(), name: "Built-in display".into(), windows_name: "Built-in display".into(), full: r(0, 600, 1440, 1500), work: r(0, 600, 1440, 1452) },
                    crate::ws::Present { id: "DEL-A0F5-1234".into(), device: "DISPLAY2".into(), name: "DELL U2720Q".into(), windows_name: "DELL U2720Q".into(), full: r(1440, 0, 4000, 1440), work: r(1440, 0, 4000, 1392) },
                ],
                removals: vec![
                    vec![
                        crate::ws::Moved { name: "Built-in display".into(), device: Some("DISPLAY1".into()), unplugged: false, windows: 0, from: vec![] },
                        crate::ws::Moved { name: "DELL U2720Q".into(), device: Some("DISPLAY2".into()), unplugged: false, windows: 1, from: vec![3] },
                    ],
                    vec![],
                ],
                waiting: vec![],
                open: vec![],
            },
            pad: Some(PadState { pad: true, enabled: true, rows: [PadRow::Nothing; 4], off: true, saved: true }),
            back: None,
            here: Some("DISPLAY2".into()),
            apps: vec![],
            wall_failed: false,
            wall_problems: vec![],
            wall_now: BTreeMap::new(),
            wall_counts: vec![],
        }
    }

    /// Apps with a window open, as the Apps render states show them: the three ruled ones
    /// running (Spotify isn't), and, unless `ruled_only`, three more for the Add menu.
    fn fake_apps(w: &mut Win, ruled_only: bool) {
        let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
        let seen = |key: &str, name: &str, device: &str, ws: usize| crate::apps::Seen { key: key.into(), name: name.into(), icon, device: device.into(), ws };
        w.pics.apps = vec![
            seen("slack.exe", "Slack", "DISPLAY2", 1),
            seen("olk.exe", "Outlook", "DISPLAY1", 0),
            seen("microsoft.windowscalculator_8wekyb3d8bbwe!app", "Calculator", "DISPLAY1", 0),
        ];
        if !ruled_only {
            w.pics.apps.insert(1, seen("msedge.exe", "Microsoft Edge", "DISPLAY1", 0));
            w.pics.apps.push(seen("windowsterminal.exe", "Terminal", "DISPLAY2", 1));
            w.pics.apps.push(seen("code.exe", "Visual Studio Code", "DISPLAY2", 1));
        }
    }

    // Spec 0027 AC-14: Add writes one inline table where the app's window is, a menu writes its
    // rule's one key, Any screen takes the screen out, and remove takes the rule out, the last
    // one leaving [apps] for the next. Each reads back as the rule the page showed.
    #[test]
    fn app_rules_write_one_thing_each() {
        let file = "[apps]\n# mine\n";
        let added = settings::apply_for_test(file, &app_edit("slack.exe", AppChange::Add(2, Some("DEL-1".into()))));
        // It goes in right under [apps]; the comment stays where it was, after it.
        assert_eq!(added, "[apps]\n\"slack.exe\" = { workspace = 3, screen = \"DEL-1\" }\n# mine\n");
        let rule = |text: &str| settings::parse_for_test(text).0.apps;
        let moved = settings::apply_for_test(&added, &app_edit("slack.exe", AppChange::Ws(0)));
        assert_eq!(rule(&moved), vec![("slack.exe".to_string(), settings::Rule { workspace: 0, screen: Some("DEL-1".into()) })]);
        let other = settings::apply_for_test(&moved, &app_edit("slack.exe", AppChange::Screen(Some("internal".into()))));
        assert!(other.contains("screen = \"internal\""), "{other}");
        let any = settings::apply_for_test(&other, &app_edit("slack.exe", AppChange::Screen(None)));
        assert_eq!(rule(&any), vec![("slack.exe".to_string(), settings::Rule { workspace: 0, screen: None })]);
        let gone = settings::apply_for_test(&any, &app_edit("slack.exe", AppChange::Remove));
        assert_eq!(gone, file);
        // Added with no screen when its window's screen isn't plugged in.
        let bare = settings::apply_for_test(file, &app_edit("spotify.exe", AppChange::Add(0, None)));
        assert!(bare.contains("\"spotify.exe\" = { workspace = 1 }\n"), "{bare}");
    }

    // Spec 0027 AC-13: a row says why its rule waits or is skipped; Add lists only apps with no
    // rule; a workspace past the count is never offered, and nothing is chosen in that menu.
    #[test]
    fn app_rows_say_what_waits() {
        let mut w = fake_win(ui::LIGHT, 1.0);
        w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
        let s = loaded("[apps]\n\"slack.exe\" = { workspace = 6 }\n\"olk.exe\" = { workspace = 1, screen = \"LG-2\" }\n\"code.exe\" = { workspace = 1, screen = \"GSM-9\" }\n\"msedge.exe\" = { workspace = 2, screen = \"internal\" }\n");
        let notes: Vec<Option<(String, bool)>> = s.apps.iter().map(|(_, r)| app_note(&w.pics.view, s, r)).collect();
        assert_eq!(notes, vec![
            Some(("Waits: you have 4 workspaces".to_string(), false)),
            Some(("Screen not plugged in, windows wait on the main display".to_string(), false)),
            Some(("des-k hasn't seen this screen yet".to_string(), true)),
            None,
        ]);
        fake_apps(&mut w, false);
        let add: Vec<String> = chooser_lines(&w, s, Ctl::AppAdd).0.into_iter().map(|c| c.name).collect();
        assert_eq!(add, ["Calculator", "Terminal"]);
        let (lines, cur) = chooser_lines(&w, s, Ctl::AppWs(0));
        assert_eq!((lines.len(), cur), (4, None));
        let screens: Vec<(String, String)> = chooser_lines(&w, s, Ctl::AppScreen(2)).0.into_iter().map(|c| (c.name, c.side)).collect();
        let want = [("Any screen", "Where it opens"), ("Built-in display", ""), ("DELL U2720Q", ""), ("LG 27UL850", "Not plugged in"), ("GSM-9", "Not seen yet")];
        assert_eq!(screens, want.map(|(a, b)| (a.to_string(), b.to_string())));
    }

    /// A picture value, or none for "".
    fn pic(name: &str) -> Pick {
        if name.is_empty() { Pick::None } else { Pick::Picture(name.into()) }
    }

    fn fake_win(theme: Theme, scale: f32) -> Win {
        Win {
            hwnd: HWND::default(), scale, theme, fonts: Fonts::new(scale), scroll: 0.0, height: 0.0, fixed: 0.0, tops: vec![], cards: vec![], hits: vec![],
            focus: None, kb: true, hover: None, press: None, drag: None, vw: W, vh: H, error: None, rec: None, refused: None, prompt: None, menu: None,
            pics: fake_pics(), edited: None, tree: Arc::new(Mutex::new(Node::default())),
            hexes: vec![], own: None, screen: None, chooser: None, dragging: None, pick: None, geo: Geo::default(), ease: None, down_at: (0, 0), words: None,
            names_screen: None, typed: None, unsaved: vec![], wall_screen: None, wall_files: vec![], wall_dirs: vec![], wall_top: 0, wall_waiting: false,
        }
    }

    /// Decoded stand ins for the Wallpaper card's previews: a gradient per picture, and a blue
    /// one for the screen's Windows wallpaper, as the approved design draws them.
    fn fake_wall(w: &Win) {
        let Some((rect, size, files)) = wall_wanted(w, settings::get()) else { return };
        let pic = |top: u32, bottom: u32| {
            let (pw, ph) = size;
            let px = (0..ph).flat_map(|y| std::iter::repeat_n(0xFF00_0000 | mix(top, bottom, y as f32 / (ph - 1).max(1) as f32), pw as usize)).collect();
            ui::Picture { w: pw, h: ph, px }
        };
        crate::wallpaper::put_for_test(rect, None, size, pic(0x9CD0FF, 0x0B2C6B));
        for f in files {
            // A slideshow's picture is "folder\name": colored by its name.
            let (a, b) = match f.rsplit('\\').next().unwrap_or_default() {
                "beach.jpg" => (0x7FD3F2, 0xF2DDA9),
                "forest.jpg" => (0x8FCBE6, 0x2F6B45),
                "desk.png" => (0xEAD9C3, 0x9A6644),
                "city.jpg" => (0x0E1A3F, 0x47306F),
                "dunes.jpg" => (0xF9D58C, 0xB65A2A),
                _ => (0xF4C7C3, 0x9CB6D6),
            };
            crate::wallpaper::put_for_test(rect, Some(&f), size, pic(a, b));
        }
    }

    /// One layout pass of the page, measuring only, as `lay` does.
    fn measure(win: &Win) -> Out {
        unsafe {
            let dc = GetDC(None);
            let out = compose(&mut Canvas::new(dc, false, win.scale, painted(win), &win.fonts), win);
            ReleaseDC(None, dc);
            out
        }
    }

    fn take(win: &mut Win, out: Out) {
        (win.hits, win.height, win.fixed, win.tops, win.geo) = (out.hits, out.height, out.fixed, out.tops, out.geo);
    }

    /// An open picker for `key`, showing `color` and holding it in the file.
    fn picking(key: &'static str, anchor: Anchor, color: u32) -> ColorPick {
        let start = file_color(settings::get(), key).unwrap_or(color);
        ColorPick {
            key, from: None, anchor, before: None, start, file: color, color, hsv: ui::hsv_of(color, (0.0, 0.0, 0.0)), preview: None, written: None, failed: None,
            saved: false, held_edit: None, placed: None, scroll0: 0.0, guard: None, focus: Stop::Ring, last_swatch: 0, drag: None, held: None,
        }
    }

    // AC-6: Esc and Cancel put back exactly what the file had. No key then: it goes again (des-k's
    // own gold stays des-k's). des-k's gold typed by hand: it is written back as typed, not
    // taken out, since the file is yours. Nothing is written when the file holds it already.
    #[test]
    fn undo_writes_back_exactly_what_the_file_had() {
        let k = |t: &str| settings::raw_for_test(t, &path_of("colors.ws2"));
        let saved = k("[colors]\nws2 = \"#92BF4F\"\n");
        assert_eq!(said(undo_edit("colors.ws2", None, saved.as_ref())), "remove colors.ws2");
        let typed = k("[colors]\nws2 = \"#DDB43C\" # mine\n");
        assert_eq!(said(undo_edit("colors.ws2", typed.as_ref(), saved.as_ref())), "at colors.ws2 = \"#DDB43C\" # mine");
        assert_eq!(said(undo_edit("colors.ws2", typed.as_ref(), k("[colors]\nws2 = \"#DDB43C\"\n").as_ref())), "none", "a comment is no difference");
        assert_eq!(said(undo_edit("colors.ws2", None, None)), "none");
        // An inline table reads the same as a section.
        assert_eq!(said(undo_edit("colors.ws2", None, k("colors = { ws2 = \"#92BF4F\" }\n").as_ref())), "remove colors.ws2");
    }

    // AC-12: the picker's own write reloads before it returns, so the value in flight is never an
    // outside edit; a save that works, then one that fails, then the 1 s reload (the file still
    // holding the first) leaves the picker open; anything else closes it, and Windows' accent always does.
    #[test]
    fn only_a_value_the_picker_did_not_write_closes_it() {
        assert!(!outside(Some(0x92BF4F), 0xDDB43C, Some(0x92BF4F)), "its own write, in flight");
        assert!(!outside(Some(0x92BF4F), 0x92BF4F, None), "the reload after a save that worked");
        assert!(!outside(Some(0x92BF4F), 0x92BF4F, None), "a failed save left the file as it was");
        assert!(outside(Some(0x123456), 0x92BF4F, None), "an edit by hand");
        assert!(outside(None, 0xE0457B, None), "the accent switched to Windows");
    }

    // AC-13: the page's notes hold still while the picker is open, and catch up on any close.
    #[test]
    fn notes_catch_up_when_the_picker_closes() {
        let why = Some("os error 32".to_string());
        let title = || color_title("colors.ws2");
        let old = || Some(("Workspace 2 color".to_string(), "os error 5".to_string()));
        // A failure, then Esc: undone, no note.
        assert_eq!(note_after(why.as_ref(), false, How::Undo, title(), None), None);
        // A failure, then Done: the note, with Windows' words, under the color's title.
        assert_eq!(note_after(why.as_ref(), false, How::Keep(true), title(), None), Some(("Workspace 2 color".into(), "os error 32".into())));
        // A failure, then a re-pick of the file's color (Not saved gone, nothing written), then Done.
        assert_eq!(note_after(None, false, How::Keep(true), title(), None), None);
        // A note from before it opened, and a save in it that worked: the note goes.
        assert_eq!(note_after(None, true, How::Keep(true), title(), old()), None);
        // A failure, then an outside edit to its key: the page caused the close, the note shows.
        assert_eq!(note_after(why.as_ref(), false, How::Quiet, title(), None).map(|n| n.0), Some("Workspace 2 color".into()));
        // Nothing saved, nothing failed: the note stays as it was.
        assert_eq!(note_after(None, false, How::Undo, title(), old()), old());
    }

    /// The card never covers what you clicked (ring and hover edge included), stays in the
    /// window past the side list, keeps what you clicked in view 8 dip from the edges and below
    /// the notes, and scrolls only within the page's limits.
    fn sound(room: &Room, anchor: Bx, (at, d): (Place, f32)) {
        let card = (PICK_W, PICK_H);
        assert!(at.x >= room.lo - 0.5 && at.x <= room.hi + 0.5, "x {at:?}");
        assert!(at.y >= MARGIN - 0.5 && at.y + PICK_H <= room.vh - MARGIN + 0.5, "y {at:?}");
        let a = grown(anchor, d);
        assert!(!covers(a, (at.x, at.y), card), "covers what you clicked: {at:?} {a:?}");
        assert!(a.1 >= room.fixed + MARGIN - 0.5 && a.1 + a.3 <= room.vh - MARGIN + 0.5, "what you clicked is out of view: {a:?}");
        assert!(room.scroll + d >= -0.5 && room.scroll + d <= room.smax + 0.5, "scrolled past the page: {d}");
    }

    fn room(vh: f32, fixed: f32, scroll: f32, smax: f32) -> Room {
        Room { vh, fixed, lo: X0, hi: X0 + (W - X0 - PAD_R) - PICK_W, scroll, smax }
    }

    fn tile_levels(rows: &[Bx], row: usize, tile: Bx) -> Vec<Level> {
        let one = |b: Bx| Level { next_to: b, keep: vec![b] };
        vec![one(rows.iter().copied().reduce(union).unwrap()), one(rows[row]), one(tile)]
    }

    // AC-14, the design pass's tight cases: the card finds a place without covering what you
    // clicked, scrolling the least it must, and over the notes only when nothing else fits.
    #[test]
    fn the_card_never_covers_what_you_clicked() {
        let card = (PICK_W, PICK_H);
        // A 528 window with both notes (210 dip of them): below the notes there is no room for
        // the card, so it opens beside the tiles, over the notes.
        let (rows, tile) = ([(744.0, 300.0, 264.0, 34.0), (744.0, 360.0, 264.0, 34.0)], (798.0, 360.0, 48.0, 34.0));
        let r = room(MIN_H, 210.0, 400.0, 1800.0);
        let got = place(&r, &tile_levels(&rows, 1, tile), tile, true, card, true);
        sound(&r, tile, got);
        assert!(got.0.y < 210.0, "over the notes: {got:?}");
        // 9 workspaces, the page scrolled to its end: what you clicked sits 8 dip from the bottom.
        let (rows, tile) = ([(632.0, 560.0, 376.0, 34.0), (632.0, 600.0, 376.0, 34.0)], (920.0, 600.0, 40.0, 34.0));
        let r = room(H, 0.0, 1800.0, 1800.0);
        let got = place(&r, &tile_levels(&rows, 1, tile), tile, true, card, true);
        sound(&r, tile, got);
        // Your accent with both notes in 648: the swatch always stays clear, the legend when it can.
        let (sw, legend) = ((828.0, 420.0, 48.0, 32.0), (604.0, 300.0, 300.0, 132.0));
        let levels = [Level { next_to: sw, keep: vec![sw, legend] }, Level { next_to: sw, keep: vec![sw] }];
        let r = room(H, 230.0, 900.0, 1800.0);
        let got = place(&r, &levels, sw, false, card, true);
        sound(&r, sw, got);
        // A swatch near the window's bottom: below doesn't fit, above does, nothing scrolls.
        let sw = (828.0, 560.0, 48.0, 32.0);
        let r = room(H, 0.0, 900.0, 1800.0);
        let (at, d) = place(&r, &[Level { next_to: sw, keep: vec![sw] }], sw, false, card, true);
        assert_eq!((d, at.y), (0.0, 560.0 - MARGIN - PICK_H));
        // A tile 10 dip into the bottom margin: the page scrolls the least that brings it in view
        // with its ring, and no more.
        let (rows, tile) = ([(744.0, 608.0, 264.0, 34.0)], (798.0, 608.0, 48.0, 34.0));
        let r = room(H, 0.0, 300.0, 1800.0);
        let (at, d) = place(&r, &tile_levels(&rows, 0, tile), tile, true, card, true);
        assert_eq!(d, 608.0 + 34.0 + OUTSET - (H - MARGIN));
        sound(&r, tile, (at, d));
        // Placed again without the scroll step, the page never moves.
        assert_eq!(place(&r, &tile_levels(&rows, 0, tile), tile, true, card, false).1, 0.0);
    }

    // AC-14: once placed, the card keeps its window position while it still fits; when what
    // it keeps clear moves under it, it is placed again.
    #[test]
    fn the_card_stays_put_while_it_fits() {
        let card = (PICK_W, PICK_H);
        let sw = (828.0, 200.0, 48.0, 32.0);
        let r = room(H, 0.0, 0.0, 1800.0);
        let levels = [Level { next_to: sw, keep: vec![sw] }];
        let (at, _) = place(&r, &levels, sw, false, card, true);
        assert!(kept(&r, &levels, at, card));
        let moved = [Level { next_to: (828.0, 300.0, 48.0, 32.0), keep: vec![(828.0, 300.0, 48.0, 32.0)] }];
        assert!(!kept(&r, &moved, at, card), "the swatch moved under the card");
    }

    // AC-15: a double click on a tile opens the picker and does nothing else, but a quick pick
    // right after opening still works.
    #[test]
    fn the_opening_click_guards_only_its_own_spot_and_moment() {
        let now = Instant::now();
        let g = Some(Guard { at: (500, 300), until: now + Duration::from_millis(500) });
        let soon = now + Duration::from_millis(200);
        assert!(guarded(g, (501, 300), soon, (4, 4)), "the second press of a double click");
        assert!(!guarded(g, (503, 300), soon, (4, 4)), "3 px away: a press of its own");
        assert!(!guarded(g, (501, 300), now + Duration::from_millis(600), (4, 4)), "after the double click time");
        assert!(!guarded(None, (500, 300), soon, (4, 4)), "opened from the keyboard: no guard");
    }

    // AC-9: the picker's keys.
    #[test]
    fn the_keys_move_pick_keep_and_undo() {
        let h = |a: Option<Arrow>| match a {
            Some(Arrow::Hsv(hsv)) => hsv,
            other => panic!("{other:?}"),
        };
        assert_eq!(h(arrow(Stop::Ring, VK_LEFT, false, (0.0, 0.5, 0.5))).0, 359.0, "Left at hue 0 wraps");
        assert_eq!(h(arrow(Stop::Ring, VK_RIGHT, true, (355.0, 0.5, 0.5))).0, 5.0, "Shift turns 10");
        assert_eq!(h(arrow(Stop::Square, VK_UP, true, (10.0, 0.5, 0.95))).2, 1.0, "Shift+Up stops at full value");
        assert_eq!(h(arrow(Stop::Square, VK_LEFT, false, (10.0, 0.0, 0.5))).1, 0.0, "saturation stops at 0");
        assert_eq!(arrow(Stop::Swatches(2), VK_RIGHT, false, (0.0, 0.0, 0.0)), Some(Arrow::Swatch(3)), "the third of a row goes on to the next row");
        assert_eq!(arrow(Stop::Swatches(11), VK_RIGHT, false, (0.0, 0.0, 0.0)), None, "the last stays");
        assert_eq!(arrow(Stop::Swatches(10), VK_DOWN, false, (0.0, 0.0, 0.0)), None, "the bottom row stays");
        assert_eq!(arrow(Stop::Swatches(4), VK_UP, false, (0.0, 0.0, 0.0)), Some(Arrow::Swatch(1)));
        assert_eq!(arrow(Stop::Done, VK_LEFT, false, (0.0, 0.0, 0.0)), None);
        assert_eq!(key_does(VK_RETURN, Stop::Ring, false), KeyDoes::Close(How::Keep(true)), "Enter on the ring keeps");
        assert_eq!(key_does(VK_RETURN, Stop::Swatches(3), false), KeyDoes::Close(How::Keep(true)), "Enter never picks");
        assert_eq!(key_does(VK_RETURN, Stop::Cancel, false), KeyDoes::Close(How::Undo), "Enter on Cancel undoes");
        assert_eq!(key_does(VK_SPACE, Stop::Done, false), KeyDoes::Close(How::Keep(true)));
        assert_eq!(key_does(VK_SPACE, Stop::Swatches(3), false), KeyDoes::Pick(3));
        assert_eq!(key_does(VK_ESCAPE, Stop::Square, false), KeyDoes::Close(How::Undo));
        assert_eq!(key_does(VK_RETURN, Stop::Ring, true), KeyDoes::Nothing, "a held Enter acts once");
        assert_eq!(key_does(VK_ESCAPE, Stop::Ring, true), KeyDoes::Nothing);
        // Tab wraps through five stops; back on the swatches, the color shown, else the last.
        assert_eq!(tab(Stop::Cancel, false, 0x123456, 4), Stop::Swatches(4));
        assert_eq!(tab(Stop::Cancel, false, ui::PRESETS[7], 4), Stop::Swatches(7));
        assert_eq!(tab(Stop::Swatches(7), true, 0, 7), Stop::Cancel);
        assert_eq!(tab(Stop::Ring, false, 0, 0), Stop::Square);
    }

    // AC-8: the duplicate line names the lowest other workspace in use with the same or a very
    // close color, counted as it shows (its default with no key), and never one you don't have.
    #[test]
    fn the_duplicate_line_names_the_lowest_workspace_in_use() {
        let mut ws = ui::WS;
        assert_eq!(dup_of(0x79A4F3, 1, &ws, 4), Some((0, true)), "workspace 1's own blue");
        assert_eq!(dup_of(0x7AA5F2, 1, &ws, 4), Some((0, false)), "a step off is very close");
        assert_eq!(dup_of(0x92BF4F, 1, &ws, 9), None, "lime is nobody's");
        assert_eq!(dup_of(0xDDB43C, 1, &ws, 4), None, "its own color is no duplicate");
        ws[2] = 0x92BF4F;
        ws[5] = 0x92BF4F;
        assert_eq!(dup_of(0x92BF4F, 1, &ws, 9), Some((2, true)), "the lowest");
        assert_eq!(dup_of(0xF97676, 1, &ws, 8), None, "workspace 9 isn't in use");
    }

    // Back's key given to another action asks first and names back by its row, like every clash,
    // so a recorded key never silently turns back off (spec 0024 AC-6).
    #[test]
    fn a_key_back_has_gets_the_clash_prompt() {
        let _one = SETTINGS.lock();
        settings::use_for_test("workspaces = 4\n");
        let combo = settings::Combo { mods: settings::WIN, vk: 0x08 };
        let Plan::Ask(p) = plan_combo("shortcuts.finder", Some(0), combo) else { panic!("took back's key without asking") };
        assert_eq!(p.title, "Win Backspace is already in use");
        assert!(p.sub.contains("“Back to the last workspace” uses it"), "{}", p.sub);
    }

    // AC-1: a click on the part of the last tile's × box inside the tile opens the picker; only
    // the part outside the tile removes a workspace. And the tiles are no keyboard stop of their own.
    #[test]
    fn a_click_inside_a_tile_never_removes_a_workspace() {
        let _one = SETTINGS.lock();
        settings::use_for_test("workspaces = 4\n");
        let mut win = fake_win(ui::LIGHT, 1.0);
        win.hover = Some(Ctl::WsTile(0, 3));
        let out = measure(&win);
        take(&mut win, out);
        let tile = win.geo.tiles.iter().find(|(d, i, _)| d == "DISPLAY1" && *i == 3).unwrap().2;
        let corner = (tile.0 + tile.2 - 2.0, tile.1 + 2.0); // inside the tile, inside the × box
        let beyond = (tile.0 + tile.2 + 4.0, tile.1 - 4.0); // in the × box, past the tile
        assert_eq!(hit_at(&win, corner), Some(Ctl::WsTile(0, 3)));
        assert_eq!(hit_at(&win, beyond), Some(Ctl::TileRemove));
        assert!(stops(&win, false).iter().any(|(c, _)| *c == Ctl::Tiles));
        assert!(!stops(&win, false).iter().any(|(c, _)| matches!(c, Ctl::WsTile(..) | Ctl::ShowTile(..))));
    }

    // A click on a line of an open menu picks that line: the menu's own hit, which keeps a
    // click between lines from reaching the page under it, must never swallow its lines. It
    // did, for the Names chooser (spec 0023 AC-12) and the gesture menu alike.
    #[test]
    fn a_click_on_a_menu_line_lands_on_that_line() {
        let _one = SETTINGS.lock();
        settings::use_for_test("workspaces = 4
");
        let mut win = fake_win(ui::LIGHT, 1.0);
        let out = measure(&win);
        take(&mut win, out);
        let mid = |a: Bx| (a.0 + a.2 / 2.0, a.1 + a.3 / 2.0);
        let open = |win: &mut Win, ctl: Ctl, sec: Section| {
            let top = win.tops.iter().find(|t| t.0 == Some(sec)).unwrap().1;
            win.scroll = top;
            let out = measure(win);
            take(win, out);
            win.hits.iter().find(|h| h.ctl == ctl).map(|h| h.at).unwrap()
        };

        let at = open(&mut win, Ctl::NameScreens, Section::Workspaces);
        win.chooser = Some((Ctl::NameScreens, at, 0));
        let out = measure(&win);
        take(&mut win, out);
        let lines: Vec<(Ctl, Bx)> = win.hits.iter().filter(|h| matches!(h.ctl, Ctl::ScreenItem(i) if i != usize::MAX)).map(|h| (h.ctl, h.at)).collect();
        assert!(lines.len() >= 3, "All screens and the two fake screens");
        for (c, a) in lines {
            assert_eq!(hit_at(&win, mid(a)), Some(c));
        }
        win.chooser = None;

        let at = open(&mut win, Ctl::Picker(2), Section::Touchpad);
        win.menu = Some(Menu { row: 2, times: 1, family: None, at, focus: 0 });
        let out = measure(&win);
        take(&mut win, out);
        let lines: Vec<(Ctl, Bx)> = win.hits.iter().filter(|h| matches!(h.ctl, Ctl::Item(i) if i != usize::MAX)).map(|h| (h.ctl, h.at)).collect();
        assert!(!lines.is_empty());
        for (c, a) in lines {
            assert_eq!(hit_at(&win, mid(a)), Some(c));
        }
    }

    // AC-2: with 2 workspaces, the Overview picture's tiles 3 and 4 are empty slots that open
    // nothing; 1 and 2 open the picker.
    #[test]
    fn overview_tiles_past_your_count_open_nothing() {
        let _one = SETTINGS.lock();
        settings::use_for_test("workspaces = 2\n");
        let mut win = fake_win(ui::LIGHT, 1.0);
        let out = measure(&win);
        take(&mut win, out);
        let live: Vec<(usize, usize)> = win.hits.iter().filter_map(|h| match h.ctl {
            Ctl::ShowTile(r, i) => Some((r, i)),
            _ => None,
        }).collect();
        assert_eq!(live, [(0, 0), (0, 1), (1, 0), (1, 1)]);
        assert_eq!(win.geo.shows.len(), 4);
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
        let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5), ("rose", ui::tests::rose(true), 1.0), ("rose-dark", ui::tests::rose(false), 1.0)];
        // Name, section to scroll to, what to set up.
        type Setup = fn(&mut Win);
        // Spec 0023: names on every screen, and the laptop's own (renamed Left, Notes on 2, no name on 3).
        const NAMES: &str = "[names]
ws1 = \"Mail\"
ws2 = \"Code\"
ws4 = \"Research and reading\"
[names.screens.internal]
screen = \"Left\"
ws2 = \"Notes\"
ws3 = \"\"
";
        const DOWN_TO_NAMES: f32 = 380.0;
        // Spec 0026: the Wallpaper card as approved in design r3, with a screen of its own.
        const WALL: &str = "[wallpaper]
dim = 30
ws1 = \"forest.jpg\"
ws2 = \"desk.png\"
ws4 = \"dunes.jpg\"
[wallpaper.screens.\"DEL-A0F5-1234\"]
ws2 = \"city.jpg\"
ws4 = \"\"
";
        const DOWN_TO_WALL: f32 = 720.0;
        // Spec 0028: the slideshow rows of design r3 (frames k to p).
        const SLIDES: &str = "[wallpaper]
dim = 30
ws1 = \"forest.jpg\"
ws2 = \"beach\"
ws3 = [\"desk.png\", \"dunes.jpg\", \"forest.jpg\"]
ws4 = \"city nights\"
[wallpaper.intervals]
ws4 = 5
[wallpaper.screens.\"DEL-A0F5-1234\"]
ws4 = \"trips\"
";
        // What the backdrop shows now: each screen moves on its own.
        fn slides(w: &mut Win) {
            for (id, beach) in [("internal", "beach\\beach.jpg"), ("DEL-A0F5-1234", "beach\\lake at dawn.PNG")] {
                w.pics.wall_now.insert((id.into(), 1), beach.into());
                w.pics.wall_now.insert((id.into(), 2), "dunes.jpg".into());
                w.pics.wall_now.insert((id.into(), 3), "city nights\\city.jpg".into());
            }
            w.pics.wall_counts = vec![("beach".into(), 12), ("city nights".into(), 8), ("trips".into(), 0)];
        }
        const DIRS: [(&str, usize); 3] = [("beach", 12), ("city nights", 8), ("trips", 0)];
        const FILES: [&str; 5] = ["city.jpg", "desk.png", "dunes.jpg", "forest.jpg", "lake at dawn.PNG"];
        // The DELL turned on its side, so its previews are tall.
        fn portrait(w: &mut Win) {
            w.pics.view.present[1].full = RECT { left: 1440, top: 0, right: 2880, bottom: 2560 };
            w.wall_screen = Some("DEL-A0F5-1234".into());
        }
        // Spec 0027: four rules, as the approved design shows them.
        const APPS: &str = "[names]
ws1 = \"Mail\"
ws2 = \"Code\"
ws3 = \"Research\"
[apps]
\"slack.exe\" = { workspace = 3, screen = \"DEL-A0F5-1234\" }
\"olk.exe\" = { workspace = 1, screen = \"internal\" }
\"microsoft.windowscalculator_8wekyb3d8bbwe!app\" = { workspace = 4 }
\"spotify.exe\" = { workspace = 2 }
";
        let states: [(&str, Option<Section>, Setup); 71] = [
            ("wallpaper-alone-own", Some(Section::Workspaces), |w| {
                settings::use_for_test("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"desk.png\", \"dunes.jpg\"]\n[wallpaper.screens.internal]\nws2 = \"forest.jpg\"\n");
                w.pics.view.present.truncate(1);
                w.pics.view.waiting.clear();
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-unused", Some(Section::Workspaces), |w| {
                settings::use_for_test("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"desk.png\", \"dunes.jpg\"]\n[wallpaper.screens.internal]\nws2 = \"forest.jpg\"\n[wallpaper.screens.\"DEL-A0F5-1234\"]\nws1 = \"city.jpg\"\nws2 = \"city.jpg\"\n");
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides-end", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                w.focus = Some(Ctl::Stepper("wallpaper.interval"));
                w.scroll = DOWN_TO_WALL + 330.0;
            }),
            ("wallpaper-slides-folders", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                w.wall_files = FILES.map(String::from).to_vec();
                w.wall_dirs = DIRS.map(|(f, n)| (f.to_string(), n)).to_vec();
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides-tick", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                w.wall_files = FILES.map(String::from).to_vec();
                w.wall_dirs = DIRS.map(|(f, n)| (f.to_string(), n)).to_vec();
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides-screen", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                portrait(w);
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides-gone", Some(Section::Workspaces), |w| {
                settings::use_for_test(SLIDES);
                slides(w);
                w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
                w.wall_screen = Some("LG-2".into());
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-slides-cant", Some(Section::Workspaces), |w| {
                settings::use_for_test("workspaces = 5
[wallpaper]
dim = 30
ws1 = \"beach\"
ws2 = \"trips\"
ws3 = \"scans\"
ws4 = [\"a.jpg\", \"b.jpg\", \"c.jpg\"]
ws5 = [\"old 1.png\", \"old 2.png\"]
[wallpaper.intervals]
ws3 = 10
");
                use crate::wallpaper::Problem;
                w.pics.wall_problems = vec![("beach".into(), Problem::Missing), ("trips".into(), Problem::Empty), ("scans".into(), Problem::Unreadable), ("a.jpg/b.jpg/c.jpg".into(), Problem::Missing), ("old 1.png/old 2.png".into(), Problem::Unreadable)];
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-screen", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                portrait(w);
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-menu", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.wall_files = FILES.map(String::from).to_vec();
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-menu-screen", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                portrait(w);
                w.wall_files = FILES.map(String::from).to_vec();
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-empty", Some(Section::Workspaces), |w| w.scroll = DOWN_TO_WALL),
            ("wallpaper-many", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                let many = ["alps.jpg", "aurora.png", "beach 2.jpg", "canyon.jpg", "city.jpg", "desk.png", "dunes.jpg", "fjord.jpg", "forest.jpg", "glacier.png", "harbor.jpg", "lake at dawn.PNG", "meadow.jpg", "moon.png"];
                w.wall_files = many.map(String::from).to_vec();
                w.wall_top = 3;
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-cant-show", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.pics.wall_problems = vec![("forest.jpg".into(), crate::wallpaper::Problem::Missing), ("desk.png".into(), crate::wallpaper::Problem::Undecodable)];
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-cant-draw", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.pics.wall_failed = true;
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-fit", Some(Section::Workspaces), |w| {
                settings::use_for_test("fastest = true
[wallpaper]
fit = \"center\"
transition = \"fade\"
ws1 = \"forest.jpg\"
ws2 = \"desk.png\"
");
                w.focus = Some(Ctl::Stepper("wallpaper.dim"));
                w.scroll = DOWN_TO_WALL + 120.0;
            }),
            ("wallpaper-chooser", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
                w.scroll = DOWN_TO_WALL;
            }),
            ("wallpaper-one", Some(Section::Workspaces), |w| {
                settings::use_for_test(WALL);
                w.pics.screens.truncate(1);
                w.pics.view.present.truncate(1);
                w.pics.view.names.truncate(1);
                w.scroll = DOWN_TO_WALL - 200.0;
            }),
            ("apps-add-many", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, false);
                let more: Vec<crate::apps::Seen> = (1..=27).map(|i| crate::apps::Seen { key: format!("app{i}.exe"), name: format!("App number {i}"), ..w.pics.apps[1].clone() }).collect();
                w.pics.apps.extend(more);
            }),
            ("apps", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, false);
            }),
            ("apps-notes", Some(Section::Apps), |w| {
                settings::use_for_test("[apps]
\"spotify.exe\" = { workspace = 6 }
\"ms-teams.exe\" = { workspace = 2, screen = \"LG-2\" }
\"notepad.exe\" = { workspace = 1, screen = \"GSM-5B7F-12345\" }
\"olk.exe\" = { workspace = 1, screen = \"internal\" }
");
                fake_apps(w, false);
                w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
                w.hover = Some(Ctl::AppX(3));
            }),
            ("apps-add", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, false);
            }),
            ("apps-add-none", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, true);
            }),
            ("apps-ws", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, false);
            }),
            ("apps-screen", Some(Section::Apps), |w| {
                settings::use_for_test(APPS);
                fake_apps(w, false);
                w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
            }),
            ("apps-empty", Some(Section::Apps), |w| fake_apps(w, false)),
            ("colors", Some(Section::Colors), |_| {}),
            ("colors-own", Some(Section::Colors), |w| {
                settings::use_for_test("[colors]\naccent = \"#E0457B\"\nws2 = \"#FFF4B0\"\nws3 = \"blue\"\n");
                w.focus = Some(Ctl::Hex(ACCENT));
                w.hexes = vec![Hex { key: ACCENT, digits: "2E8B5".into(), caret: 5, anchor: 5, line: None }];
                w.own = Some(0x2E8B57);
            }),
            ("place", Some(Section::Indicator), |w| {
                settings::use_for_test("[indicator]\nspot = \"top-right\"\n");
                w.scroll = 470.0;
                w.focus = Some(Ctl::Picture);
            }),
            ("place-screen", Some(Section::Indicator), |w| {
                settings::use_for_test("[indicator]\nspot = \"top-right\"\n[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"custom\"\nx = 70\ny = 120\n");
                w.screen = Some("DEL-A0F5-1234".into());
                w.scroll = 470.0;
            }),
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
            ("attention", Some(Section::Attention), |_| {}),
            ("attention-own", Some(Section::Attention), |w| {
                settings::use_for_test("[attention]\ntext = \"{{AppName}} has something for you\"\n[indicator]\nstyle = \"dots\"\n");
                w.focus = Some(Ctl::Words);
            }),
            ("attention-off", Some(Section::Attention), |_| settings::use_for_test("[attention]\nenabled = false\n")),
            ("overview", Some(Section::Overview), |w| w.focus = Some(Ctl::Stepper("overview.fade_ms"))),
            ("motion", Some(Section::Motion), |w| w.focus = Some(Ctl::Toggle("fastest"))),
            ("touchpad", Some(Section::Touchpad), |_| {}),
            // The color picker (spec 0021): the tile and picture cues, then the card in its states.
            ("tile-hover", None, |w| {
                w.hover = Some(Ctl::WsTile(1, 2));
                w.kb = false;
            }),
            ("overview-slots", Some(Section::Overview), |w| {
                settings::use_for_test("workspaces = 3\n");
                w.hover = Some(Ctl::ShowTile(0, 1));
                w.kb = false;
            }),
            ("pick-tile", None, |w| {
                w.pick = Some(picking("colors.ws2", Anchor::Tile("DISPLAY2".into(), 1), 0xDDB43C));
                w.kb = false;
            }),
            ("pick-swatch", Some(Section::Colors), |w| {
                // Workspace 3 moved to workspace 1's blue: the header shows start and now, and the duplicate line says so.
                let mut p = picking("colors.ws3", Anchor::Swatch("colors.ws3"), 0x79A4F3);
                (p.focus, p.from) = (Stop::Swatches(0), Some(Ctl::Swatch("colors.ws3")));
                w.pick = Some(p);
                w.focus = Some(Ctl::Swatch("colors.ws3"));
            }),
            ("pick-close", Some(Section::Colors), |w| {
                let mut p = picking("colors.ws3", Anchor::Swatch("colors.ws3"), 0x7AA5F2);
                p.focus = Stop::Square;
                w.pick = Some(p);
            }),
            ("pick-accent", Some(Section::Colors), |w| {
                settings::use_for_test("[colors]\naccent = \"#E0457B\"\n");
                let mut p = picking(ACCENT, Anchor::Swatch(ACCENT), 0x2E8B57);
                p.focus = Stop::Done;
                w.pick = Some(p);
            }),
            ("pick-failed", Some(Section::Colors), |w| {
                let mut p = picking("colors.ws2", Anchor::Swatch("colors.ws2"), 0xDDB43C);
                (p.failed, p.focus) = (Some("os error 32".into()), Stop::Cancel);
                w.pick = Some(p);
            }),
            ("pick-notes", None, |w| {
                // Both notes in the smallest window: the card opens over them, beside the tiles.
                w.vh = MIN_H;
                w.error = Some(("Workspace 4 color".into(), "os error 32".into()));
                w.edited = Some(("14:02".into(), vec!["Indicator stays: 550 → 800 ms".into()]));
                w.pick = Some(picking("colors.ws1", Anchor::Tile("DISPLAY1".into(), 0), 0x79A4F3));
                w.kb = false;
            }),
            ("pick-ring", None, |w| {
                let mut p = picking("colors.ws4", Anchor::Tile("DISPLAY2".into(), 3), 0x2E8B57);
                p.focus = Stop::Ring;
                w.pick = Some(p);
            }),
            ("pick-preset", None, |w| {
                let mut p = picking("colors.ws2", Anchor::Tile("DISPLAY2".into(), 1), 0x92BF4F);
                (p.focus, p.last_swatch) = (Stop::Swatches(10), 10);
                w.pick = Some(p);
                w.hover = Some(Ctl::Preset(4));
            }),
            ("pick-overview", Some(Section::Overview), |w| {
                w.pick = Some(picking("colors.ws2", Anchor::Show("DISPLAY1".into(), 1), 0xDDB43C));
                w.kb = false;
            }),
            // Spec 0023: the Names card in each of its states, then the Indicator's.
            ("names", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-empty", Some(Section::Workspaces), |w| w.scroll = DOWN_TO_NAMES),
            ("names-screen", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.pics.view.present[0].name = "Left".into();
                w.names_screen = Some("internal".into());
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-follow", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.pics.view.present[0].name = "Left".into();
                (w.names_screen, w.focus) = (Some("internal".into()), Some(Ctl::Name(Nm::Ws(0))));
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-own-emptied", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.pics.view.present[0].name = "Left".into();
                (w.names_screen, w.focus, w.typed) = (Some("internal".into()), Some(Ctl::Name(Nm::Ws(1))), Some(String::new()));
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-all-emptied", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                (w.focus, w.typed) = (Some(Ctl::Name(Nm::Ws(0))), Some(String::new()));
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-typing", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                (w.focus, w.typed) = (Some(Ctl::Name(Nm::Ws(2))), Some("Ideas 💡".into()));
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-failed", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.unsaved = vec![(None, Nm::Ws(1), "Codex".into())];
                w.error = Some(("Workspace 2 name".into(), "os error 32".into()));
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-one", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                w.pics.screens.truncate(1);
                w.pics.view.present.truncate(1);
                w.pics.view.names.truncate(1);
                w.scroll = DOWN_TO_NAMES - 120.0;
            }),
            ("names-menu", Some(Section::Workspaces), |w| {
                settings::use_for_test("[names.screens.internal]
screen = \"Desk\"
[names.screens.\"DEL-A0F5-1234\"]
screen = \"Desk\"
");
                for p in w.pics.view.present.iter_mut() {
                    p.name = "Desk".into();
                }
                w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
                w.scroll = DOWN_TO_NAMES;
            }),
            ("names-readonly", Some(Section::Workspaces), |w| {
                settings::use_for_test(NAMES);
                settings::broken_for_test("expected `=`, found newline at line 3 column 9");
                w.names_screen = Some("internal".into());
                w.scroll = DOWN_TO_NAMES;
            }),
            ("indicator-names", Some(Section::Indicator), |_| settings::use_for_test(NAMES)),
            ("indicator-no-screen", Some(Section::Indicator), |w| {
                settings::use_for_test(&format!("{NAMES}[indicator]
screen_name = false
"));
                w.scroll = 200.0;
            }),
            ("indicator-dots", Some(Section::Indicator), |w| {
                settings::use_for_test("[indicator]
style = \"dots\"
");
                w.scroll = 200.0;
            }),
            ("indicator-off", Some(Section::Indicator), |w| {
                settings::use_for_test("[indicator]
enabled = false
");
                w.scroll = 200.0;
            }),
            ("attention-names", Some(Section::Attention), |_| settings::use_for_test(NAMES)),
        ];
        for (tname, theme, scale) in &runs {
            for (sname, sec, setup) in &states {
                // The picker opens in neither high contrast nor (for brevity) the rose accent.
                if sname.starts_with("pick") && (theme.hc || tname.starts_with("rose")) {
                    continue;
                }
                let mut win = fake_win(*theme, *scale);
                settings::use_for_test("");
                setup(&mut win);
                fake_wall(&win);
                let out = measure(&win);
                if let Some(top) = out.tops.iter().find(|(s, _)| *s == *sec).map(|(_, t)| *t) {
                    win.scroll = (top - 8.0 + win.scroll).clamp(0.0, (out.height - (win.vh - out.fixed)).max(0.0));
                }
                let out = measure(&win);
        take(&mut win, out);
                if *sname == "menu"
                    && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::Picker(2)).map(|h| h.at)
                {
                    win.menu = Some(Menu { row: 2, times: 1, family: None, at, focus: 0 });
                }
                if *sname == "names-menu"
                    && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::NameScreens).map(|h| h.at)
                {
                    win.chooser = Some((Ctl::NameScreens, at, 2));
                }
                let apps_menu = match *sname {
                    "apps-add" | "apps-add-none" | "apps-add-many" => Some((Ctl::AppAdd, 0)),
                    "apps-ws" => Some((Ctl::AppWs(0), 1)),
                    "apps-screen" => Some((Ctl::AppScreen(0), 3)),
                    "wallpaper-menu" | "wallpaper-many" => Some((Ctl::WallPick(0), 5)),
                    "wallpaper-menu-screen" => Some((Ctl::WallPick(1), 1)),
                    "wallpaper-empty" => Some((Ctl::WallPick(0), 1)),
                    "wallpaper-chooser" => Some((Ctl::WallScreens, 2)),
                    "wallpaper-slides-folders" => Some((Ctl::WallPick(1), 7)),
                    "wallpaper-slides-tick" => Some((Ctl::WallPick(2), 5)),
                    _ => None,
                };
                if let Some((from, focus)) = apps_menu
                    && let Some(at) = win.hits.iter().find(|h| h.ctl == from).map(|h| h.at)
                {
                    win.chooser = Some((from, at, focus));
                }
                // An open picker: placed as on open, the page scrolled at once, the preview in place.
                if let Some((anchor, levels, beside)) = win.pick.as_ref().and_then(|p| levels_of(&p.anchor, &win.geo).map(|(a, l)| (a, l, !matches!(p.anchor, Anchor::Swatch(_))))) {
                    let (at, d) = place(&room_of(&win), &levels, anchor, beside, (PICK_W, PICK_H), true);
                    win.scroll += d;
                    let p = win.pick.as_mut().unwrap();
                    p.placed = Some(at);
                    let shown = ws_index(p.key).map_or(theme.b.acc, |i| theme.b.ws[i]);
                    p.preview = (shown != p.color).then(|| ui::theme_preview(!theme.dark, ui::tests::plain(), ws_index(p.key), p.color));
                    let out = measure(&win);
        take(&mut win, out);
                }
                let (pw, ph) = ((W * scale).round() as i32, (win.vh * scale).round() as i32);
                let t = painted(&win);
                ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                    let mut c = Canvas::new(dc, true, *scale, t, &win.fonts);
                    c.boxed((0.0, 0.0, W + 1.0, win.vh + 1.0), 0.0, t.b.bg, None);
                    compose(&mut c, &win);
                });
            }
        }
    }
}
