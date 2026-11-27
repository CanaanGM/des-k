// The page's rows and words: its sections and cards, each setting's row, value and steps, the
// shortcut lists and their key words, and the problem titles (spec 0012, spec 0017).
use super::*;
use crate::settings_page::cards::colors::{ACCENT, WS_KEYS};
use crate::settings_page::cards::place::is_place;

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

pub(super) const SECTIONS: [Section; 10] = [Section::Workspaces, Section::Apps, Section::Shortcuts, Section::Indicator, Section::Attention, Section::Restore, Section::Overview, Section::Colors, Section::Motion, Section::Touchpad];

impl Section {
    pub(super) fn name(self) -> &'static str {
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

    pub(super) fn icon(self) -> &'static str {
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

    pub(super) fn line(self) -> &'static str {
        match self {
            Section::Workspaces => "Every screen has its own workspaces, and can have its own count of them.",
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
pub(super) fn home(key: &str) -> Option<Section> {
    Some(match key.split('.').next().unwrap_or("") {
        "workspaces" | "workspaces_per_screen" | "follow_activation" | "act_on" | "jump_back" | "names" | "wallpaper" => Section::Workspaces,
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
const ACT_ON_WORDS: &[(&str, &str)] = &[("mouse", "Screen under the mouse"), ("focus", "Focused window's screen")];
/// The Keys act on row's caption, here and in the first run (spec 0038 AC-10).
pub(crate) const ACT_ON_CAP: &str = "Which screen switch, jump, back, the finder and the hints window act on. Swipes always act on the screen under the pointer.";
const FIT_WORDS: &[(&str, &str)] = &[("fill", "Fill"), ("fit", "Fit"), ("stretch", "Stretch"), ("center", "Center")];
const TRANSITION_WORDS: &[(&str, &str)] = &[("instant", "Instant"), ("fade", "Fade")];
/// A slideshow's minutes per picture (spec 0028 AC-11): up to 45 min, then hours up to 24.
pub(super) const LADDER: &[i64] = &[1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 360, 480, 720, 1440];
/// A stepper's unit for minutes shown in words, as `minutes` writes them.
pub(super) const MINUTES: &str = "minutes";

const fn row(key: &'static str, title: &'static str, cap: &'static str, kind: Kind) -> Row {
    Row { key, title, cap, kind }
}

const fn step(lo: i64, hi: i64, by: i64, unit: &'static str) -> Kind {
    Kind::Step { lo, hi, by, unit }
}

/// Every plain setting.
pub(crate) const ROWS: &[Row] = &[
    row("follow_activation", "Follow a window", "When a hidden window comes to the front (an app reuses it, a dialog pops up), go to its workspace.", Kind::Toggle),
    row("act_on", "Keys act on", ACT_ON_CAP, Kind::Choice(ACT_ON_WORDS)),
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

pub(crate) fn find_row(key: &str) -> Option<&'static Row> {
    ROWS.iter().chain([&WORKSPACES]).find(|r| r.key == key)
}

pub(super) fn word<T: Copy + PartialEq>(list: &[(&'static str, T)], v: T) -> &'static str {
    list.iter().find(|(_, x)| *x == v).map_or("", |(w, _)| w)
}

/// A row's value as a number: on and off as 1 and 0, a choice as its index.
pub(crate) fn value(s: &Settings, key: &str) -> i64 {
    let index = |w: &str, words: &[(&str, &str)]| words.iter().position(|(x, _)| *x == w).unwrap_or(0) as i64;
    match key {
        "workspaces" => s.shared_count as i64,
        "follow_activation" => s.follow_activation as i64,
        "act_on" => index(word(settings::ACT_ON, s.act_on), ACT_ON_WORDS),
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

pub(crate) const MOVE: &[Short] = &[
    short("shortcuts.previous", "Previous workspace", Some((combo(WIN | CTRL, 0x25), "switches desktops"))),
    short("shortcuts.next", "Next workspace", Some((combo(WIN | CTRL, 0x27), "switches desktops"))),
    short("shortcuts.jump", "Go to workspace 1–9", Some((WinKey::Digits(WIN), "opens taskbar app 1–9"))),
    short("shortcuts.back", "Back to the last workspace", None),
    short("shortcuts.overview", "Open overview", Some((combo(WIN, 0x09), "opens Task View"))),
    short("shortcuts.finder", "Find a window", Some((combo(WIN, 0x46), "opens Feedback Hub"))),
    short("shortcuts.hints", "Open shortcut hints", None),
    short("shortcuts.pause", "Pause des-k", None),
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

pub(super) fn win_caps(k: WinKey) -> Vec<String> {
    match k {
        WinKey::Combo(c) => crate::hints::rows::caps(&c),
        WinKey::Digits(m) => digit_caps(m),
    }
}

pub(crate) fn digit_caps(mods: u8) -> Vec<String> {
    crate::hints::rows::mods_caps(mods).into_iter().chain(["1–9".to_string()]).collect()
}

/// An action's keys as their caps, in the file's order, each with its place in the file.
pub(crate) fn keys_of(s: &Settings, key: &str) -> Vec<Vec<String>> {
    let name = name_of(key);
    if is_digits(key) {
        return s.set(name).map(digit_caps).into_iter().collect();
    }
    s.list(name).iter().map(crate::hints::rows::caps).collect()
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
pub(super) fn said(s: &Settings, key: &str) -> String {
    let all: Vec<String> = keys_of(s, key).iter().map(|k| spelled(k)).collect();
    if all.is_empty() { "no key".into() } else { all.join(" or ") }
}

pub(super) fn same_keys(a: &Settings, b: &Settings, key: &str) -> bool {
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
pub(super) fn title_of(key: &str) -> Option<&'static str> {
    find_row(key).map(|r| r.title).or_else(|| shortcut(key).map(|s| s.title))
}

/// A log line's combo in words: "Win+F" → "Win F", "Win+Alt" (a digit set) → "Win Alt 1–9".
pub(super) fn combo_words(text: &str, set: bool) -> String {
    let caps: Vec<String> = text.split('+').map(String::from).collect();
    let mut w = spelled(&caps);
    if set {
        w += " 1–9";
    }
    w
}

/// A plain title made from a problem's log line (AC-SET-5, amends spec 0001); the line itself
/// shows under it.
pub(super) fn title(p: &Problem, s: &Settings) -> String {
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
pub(super) fn fix_line(key: &str, s: &Settings) -> String {
    let Some(r) = find_row(key) else { return "Change it here to fix the file.".into() };
    let d = shown(r, value(&DEFAULTS, key));
    match r.kind {
        Kind::Toggle | Kind::Pair(..) => format!("Turn it on or off here, or Reset writes {} to the file.", if value(&DEFAULTS, key) != 0 { "true" } else { "false" }),
        _ if !unused(s, key).is_empty() => format!("Reset puts {d} back in the file."),
        _ => format!("Change it here, or Reset puts {d} back in the file."),
    }
}

/// A problem a row of Colors or Where it shows shows itself, so no section note repeats it.
pub(super) fn shown_in_row(key: &str) -> bool {
    key == ACCENT || WS_KEYS.contains(&key) || is_place(key) || (key.starts_with("indicator.screens.\"") && key.matches('"').count() == 2)
}

#[cfg(test)]
mod tests;
