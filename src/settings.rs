// settings.toml in %LOCALAPPDATA%\des-k (docs/specs/0001-settings-file/index.md), read at start
// and again whenever the settings page saves it or its write time changes (spec 0012).
// Not next to the exe: the installed exe lives in Program Files, where you cannot write (spec 0008).
// A bad value resets just that setting. A file that is not TOML resets all of them at start,
// but keeps the settings in effect on a live reload. des-k always starts.
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use arc_swap::Guard;
use std::ops::Deref;
use std::sync::Arc;
use toml::{Table, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub(crate) mod edit;
pub(crate) mod parse;
pub(crate) mod store;

use parse::{NAMED_KEYS, action};
use store::{CURRENT, Loaded};

pub const WIN: u8 = 1;
pub const CTRL: u8 = 2;
pub const ALT: u8 = 4;
pub const SHIFT: u8 = 8;

/// A shortcut: exactly these modifiers (either side of the keyboard) plus one key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    pub mods: u8,
    pub vk: u16,
}

/// What text the overview shows under the windows in its spread (spec 0010).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Labels {
    Hover,
    Always,
    None,
}

pub const LABELS: &[(&str, Labels)] = &[("hover", Labels::Hover), ("always", Labels::Always), ("none", Labels::None)];

/// How the overview opens and closes (spec 0010).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Animation {
    Off,
    Fade,
}

pub const ANIMATIONS: &[(&str, Animation)] = &[("off", Animation::Off), ("fade", Animation::Fade)];

/// How the indicator looks (spec 0017): the card, or the dots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Card,
    Dots,
}

pub const STYLES: &[(&str, Style)] = &[("card", Style::Card), ("dots", Style::Dots)];

/// Which screen a keyboard shortcut acts on (spec 0038): the one under the mouse, or the
/// focused window's. Swipes always use the pointer's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActOn {
    Mouse,
    Focus,
}

pub const ACT_ON: &[(&str, ActOn)] = &[("mouse", ActOn::Mouse), ("focus", ActOn::Focus)];

/// How des-k places a workspace's picture on a screen (spec 0026 AC-5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Fill,    // covers the screen, cropping the edges
    Whole,   // `fit`: the whole picture, bars on two sides
    Stretch, // fills exactly, shape distorted
    Center,  // actual size, bars or cropping around it
}

pub const FITS: &[(&str, Fit)] = &[("fill", Fit::Fill), ("fit", Fit::Whole), ("stretch", Fit::Stretch), ("center", Fit::Center)];
// `transition`: whether a picture change fades (spec 0026 AC-4).
const TRANSITIONS: &[(&str, bool)] = &[("instant", false), ("fade", true)];

/// Where the indicator sits on a screen's work area (spec 0020).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Spot {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    Custom,
}

pub const SPOTS: &[(&str, Spot)] = &[
    ("top-left", Spot::TopLeft),
    ("top", Spot::Top),
    ("top-right", Spot::TopRight),
    ("left", Spot::Left),
    ("center", Spot::Center),
    ("right", Spot::Right),
    ("bottom-left", Spot::BottomLeft),
    ("bottom", Spot::Bottom),
    ("bottom-right", Spot::BottomRight),
    ("custom", Spot::Custom),
];

/// The indicator's place: a spot, its gap from the edges in pixels at 100% scale, and with
/// `Custom` the card's center in percent of the work area (spec 0020 AC-7, AC-8).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pos {
    pub spot: Spot,
    pub gap: u32,
    pub x: f64,
    pub y: f64,
}

impl Default for Pos {
    fn default() -> Self {
        Pos { spot: Spot::Center, gap: 24, x: 50.0, y: 50.0 }
    }
}

/// One screen's `[indicator.screens."<id>"]`: a key left out comes from the default (AC-9).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PosOverride {
    pub spot: Option<Spot>,
    pub gap: Option<u32>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl PosOverride {
    pub fn over(&self, d: Pos) -> Pos {
        Pos { spot: self.spot.unwrap_or(d.spot), gap: self.gap.unwrap_or(d.gap), x: self.x.unwrap_or(d.x), y: self.y.unwrap_or(d.y) }
    }
}

/// des-k's accent: Windows' own, or a color you typed (spec 0020 AC-1, AC-2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Accent {
    Windows,
    Rgb(u32),
}

/// What three or four fingers did on the touchpad (spec 0011).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Tap,
}

const SWIPES: &[(&str, Motion)] = &[("left", Motion::Left), ("right", Motion::Right), ("up", Motion::Up), ("down", Motion::Down)];

impl Motion {
    pub fn word(self) -> &'static str {
        SWIPES.iter().find(|(_, m)| *m == self).map_or("tap", |(w, _)| w)
    }
}

/// What a gesture rule does (0011-gesture-map).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Switch(isize),       // -1 previous, +1 next
    Back,                // the workspace the screen showed before (spec 0024)
    Send(isize, bool),   // step, and follow
    Jump(usize),         // 0 based
    SendTo(usize, bool), // 0 based, and follow
    Finder,
    Overview,
    ShowDesktop,
    AppNext,
    AppPrevious,
    Pin,
    Keys(&'static [u16]), // typed as des-k's own input, straight to Windows
    MiddleClick,
    Nothing,
    Combo(Combo), // typed as if you pressed it, so des-k's own shortcuts see it
}

const ACTIONS: &[(&str, Action)] = &[
    ("previous", Action::Switch(-1)),
    ("next", Action::Switch(1)),
    ("back", Action::Back),
    ("send_previous", Action::Send(-1, true)),
    ("send_next", Action::Send(1, true)),
    ("send_previous_stay", Action::Send(-1, false)),
    ("send_next_stay", Action::Send(1, false)),
    ("finder", Action::Finder),
    ("overview", Action::Overview),
    ("show_desktop", Action::ShowDesktop),
    ("app_next", Action::AppNext),
    ("app_previous", Action::AppPrevious),
    ("pin", Action::Pin),
    ("search", Action::Keys(&[VK_LWIN.0, b'S' as u16])),
    ("notifications", Action::Keys(&[VK_LWIN.0, b'N' as u16])),
    ("middle_click", Action::MiddleClick),
    ("play_pause", Action::Keys(&[VK_MEDIA_PLAY_PAUSE.0])),
    ("next_track", Action::Keys(&[VK_MEDIA_NEXT_TRACK.0])),
    ("previous_track", Action::Keys(&[VK_MEDIA_PREV_TRACK.0])),
    ("volume_up", Action::Keys(&[VK_VOLUME_UP.0])),
    ("volume_down", Action::Keys(&[VK_VOLUME_DOWN.0])),
    ("mute", Action::Keys(&[VK_VOLUME_MUTE.0])),
    ("none", Action::Nothing),
];

/// What the welcome screen says each ACTIONS name does (0011-first-run-setup).
const ACTION_LABELS: &[(&str, &str)] = &[
    ("previous", "previous workspace"),
    ("next", "next workspace"),
    ("back", "last workspace"),
    ("send_previous", "send window to previous"),
    ("send_next", "send window to next"),
    ("send_previous_stay", "send window to previous, stay"),
    ("send_next_stay", "send window to next, stay"),
    ("finder", "finder"),
    ("overview", "overview"),
    ("show_desktop", "show desktop"),
    ("app_next", "next app"),
    ("app_previous", "previous app"),
    ("pin", "pin window to every workspace"),
    ("search", "search"),
    ("notifications", "notifications"),
    ("middle_click", "middle click"),
    ("play_pause", "play or pause"),
    ("next_track", "next track"),
    ("previous_track", "previous track"),
    ("volume_up", "volume up"),
    ("volume_down", "volume down"),
    ("mute", "mute"),
    ("none", "nothing"),
];

/// One `[[gesture]]` rule: this many fingers, this motion, done `times` in a row, runs `action`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gesture {
    pub fingers: u8,
    pub motion: Motion,
    pub times: u8,
    pub action: Action,
}

#[derive(PartialEq, Debug)]
pub struct Settings {
    // The file's `workspaces`: every screen's count unless `[workspaces_per_screen]` names it.
    // Never read it as a screen's count; ask `count_for` (spec 0040).
    pub shared_count: usize,
    // `[workspaces_per_screen]`'s good lines, by screen id, kept for screens not plugged in.
    pub count_by_screen: BTreeMap<String, usize>,
    pub copilot_as_right_ctrl: bool,
    pub follow_activation: bool,
    pub act_on: ActOn,
    pub jump_back: bool, // a jump to the shown workspace goes back to the last one (spec 0024)
    pub fastest: bool, // every des-k fade and move instant (spec 0017)
    // Read at start only (spec 0017 AC-9). None: the file lacks the key, as an upgrader's does.
    pub first_run_seen: Option<bool>,
    // The last release whose What's new you saw (spec 0029). None: missing, as an upgrader from
    // 0.8.0 or earlier has it, or not X.Y.Z (warned), both counting as older than every release.
    pub whats_new_seen: Option<crate::update::Version>,
    pub previous: Vec<Combo>,
    pub next: Vec<Combo>,
    pub send_previous: Vec<Combo>, // these two switch along with the window
    pub send_next: Vec<Combo>,
    pub send_previous_stay: Vec<Combo>, // these two leave the screen where it is
    pub send_next_stay: Vec<Combo>,
    // Modifier sets for the digits 1 to 9 (spec 0005); None = off.
    pub jump: Option<u8>,
    pub send_to: Option<u8>, // and follow
    pub send_to_stay: Option<u8>,
    pub back: Vec<Combo>,   // flips the screen to the workspace it showed before (spec 0024)
    pub finder: Vec<Combo>, // opens the window finder (spec 0007)
    pub overview: Vec<Combo>, // opens the workspace overview (spec 0010)
    pub pin: Vec<Combo>,      // keeps the focused window on every workspace (spec 0014)
    pub hints: Vec<Combo>,    // opens the Shortcut hints window (spec 0017); none by default
    pub pause: Vec<Combo>,    // pauses des-k, and resumes it (spec 0039); none by default
    pub indicator_enabled: bool,
    pub indicator_style: Style,
    pub indicator_pos: Pos,
    // By screen id (spec 0016); a table for a screen that isn't plugged in does nothing (AC-9).
    pub indicator_screens: BTreeMap<String, PosOverride>,
    pub hold_ms: u32,
    pub fade_ms: u32,
    // Spec 0022: a hidden window's flash shows on its screen, in your words, kept raw.
    pub attention_enabled: bool,
    pub attention_text: String,
    pub restore_enabled: bool,
    pub restore_minutes: u32,
    pub overview_animation: Animation,
    pub overview_labels: Labels,
    pub overview_live: bool, // false: the list is taken once, at open
    pub overview_fade_ms: u32, // with animation = "fade" (spec 0017 AC-7)
    // Spec 0011. Safe on by default: a gesture acts only while Windows' own gestures read off.
    pub touchpad_enabled: bool,
    pub swipe_mm: u32,
    pub twice_ms: u32,
    pub gestures: Vec<Gesture>,
    pub accent: Accent,
    pub ws_colors: [Option<u32>; 9], // None keeps look B's color (spec 0020 AC-4)
    // Spec 0023: shown on the card when two or more screens are plugged in.
    pub indicator_screen_name: bool,
    // Spec 0023. Read only through ws_name and screen_rename, which own the fallback rules.
    pub names: [String; 9], // every screen's; empty = no name
    pub names_screens: BTreeMap<String, ScreenNames>, // by screen id (spec 0016)
    // Spec 0027: rules for where new windows open, in file order, keys lowercased.
    pub apps: Vec<(String, Rule)>,
    // Spec 0041: apps des-k leaves alone, from `"<key>" = "alone"` in [apps], keys lowercased.
    // A key is a rule or alone, never both. Read through `ws::alone`.
    pub alone: HashSet<String>,
    // Spec 0026. Read only through wallpaper_for, which owns the fallback rules.
    pub wallpaper: [Pick; 9], // every screen's
    pub wallpaper_screens: BTreeMap<String, [Option<Pick>; 9]>, // by screen id; Some(Pick::None) = your Windows wallpaper there
    pub wallpaper_fit: Fit,
    pub wallpaper_fade: bool, // transition = "fade"; read through wallpaper_fades, which fastest overrides
    pub wallpaper_dim: u8,    // 0 to 80: percent darker, laid over at paint time (AC-18)
    // Spec 0028: minutes each slideshow picture shows; read through interval_for.
    pub wallpaper_interval: u16,
    pub wallpaper_intervals: [Option<u16>; 9],
    pub wallpaper_shuffle: bool,
}

/// A workspace's wallpaper value (spec 0028 AC-1, AC-14). Names are plain names in the
/// wallpapers folder, never paths.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Pick {
    #[default]
    None,             // "" or an empty set
    Picture(String),  // a file in the wallpapers folder
    Folder(String),   // a subfolder: a slideshow of its top level pictures
    Set(Vec<String>), // two or more pictures, no repeats, in name order: a slideshow of them
}

impl Pick {
    /// One text per value: `""` for none, a picture's or folder's name, or a set's names
    /// joined by `/`, which no name holds. A row's note is filed under it (`wallpaper::problem`).
    pub fn key(&self) -> String {
        match self {
            Pick::None => String::new(),
            Pick::Picture(f) | Pick::Folder(f) => f.clone(),
            Pick::Set(v) => v.join("/"),
        }
    }
}

/// One `[apps]` rule (spec 0027): where an app's new main windows open.
#[derive(Clone, PartialEq, Debug)]
pub struct Rule {
    pub workspace: usize,       // 0 based; past the count, the rule waits (AC-9)
    pub screen: Option<String>, // a screen id (spec 0016); None: the screen it opens on
}

/// One `[names.screens."<id>"]` table (spec 0023 AC-1, AC-11).
#[derive(PartialEq, Debug, Default)]
pub struct ScreenNames {
    pub screen: Option<String>,  // the rename; never empty
    pub ws: [Option<String>; 9], // Some("") = no name on this screen
}

/// The map used when settings.toml has no [[gesture]] rule (spec 0011 AC-2).
fn default_gestures() -> Vec<Gesture> {
    use Motion::*;
    [
        (4, Left, "next"),
        (4, Right, "previous"),
        (4, Up, "overview"),
        (4, Down, "show_desktop"),
        (3, Left, "app_next"),
        (3, Right, "app_previous"),
        (3, Up, "overview"),
        (3, Down, "show_desktop"),
        (3, Tap, "search"),
        (4, Tap, "notifications"),
    ]
    .map(|(fingers, motion, name)| Gesture { fingers, motion, times: 1, action: action(name).expect("a known action name") })
    .into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shared_count: 4,
            count_by_screen: BTreeMap::new(),
            copilot_as_right_ctrl: false, // a new install asks first (spec 0038 AC-1)
            follow_activation: true,
            act_on: ActOn::Mouse,
            jump_back: true,
            fastest: false,
            first_run_seen: Some(false),
            whats_new_seen: None,
            previous: vec![Combo { mods: WIN | CTRL, vk: VK_LEFT.0 }],
            next: vec![Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 }],
            send_previous: vec![Combo { mods: WIN | CTRL | SHIFT, vk: VK_LEFT.0 }],
            send_next: vec![Combo { mods: WIN | CTRL | SHIFT, vk: VK_RIGHT.0 }],
            send_previous_stay: vec![Combo { mods: WIN | CTRL | ALT, vk: VK_LEFT.0 }],
            send_next_stay: vec![Combo { mods: WIN | CTRL | ALT, vk: VK_RIGHT.0 }],
            jump: Some(WIN),
            send_to: Some(WIN | SHIFT),
            send_to_stay: Some(WIN | CTRL),
            back: vec![Combo { mods: WIN, vk: VK_BACK.0 }],
            finder: vec![Combo { mods: WIN, vk: b'F' as u16 }],
            overview: vec![Combo { mods: WIN, vk: VK_TAB.0 }],
            pin: vec![Combo { mods: WIN | CTRL, vk: b'U' as u16 }],
            hints: vec![],
            pause: vec![],
            indicator_enabled: true,
            indicator_style: Style::Card,
            indicator_pos: Pos::default(),
            indicator_screens: BTreeMap::new(),
            hold_ms: 550,
            fade_ms: 200,
            attention_enabled: true,
            attention_text: ATTENTION_TEXT.into(),
            restore_enabled: true,
            restore_minutes: 3,
            overview_animation: Animation::Off,
            overview_labels: Labels::Hover,
            overview_live: true,
            overview_fade_ms: 250,
            touchpad_enabled: true,
            swipe_mm: 15,
            twice_ms: 300,
            gestures: default_gestures(),
            accent: Accent::Windows,
            ws_colors: [None; 9],
            indicator_screen_name: true,
            names: Default::default(),
            names_screens: BTreeMap::new(),
            apps: Vec::new(),
            alone: HashSet::new(),
            wallpaper: Default::default(),
            wallpaper_screens: BTreeMap::new(),
            wallpaper_fit: Fit::Fill,
            wallpaper_fade: false,
            wallpaper_dim: 0,
            wallpaper_interval: 30,
            wallpaper_intervals: [None; 9],
            wallpaper_shuffle: false,
        }
    }
}

/// Written verbatim when the file is missing. Must parse to `Settings::default()` (tested).
const DEFAULT_FILE: &str = r##"# des-k settings. A saved change applies within two seconds; [restore] acts at the next start.

workspaces = 4              # 2 to 9, every screen without its own count below
copilot_as_right_ctrl = false # true: the Copilot key is Right Ctrl, so Win Ctrl keys work with it
follow_activation = true    # switch to a hidden window's workspace when it comes to the front
act_on = "mouse"            # which screen keys act on: "mouse" (under it) or "focus" (the focused window's)
jump_back = true            # Win+N on the workspace you're on goes back to the last one
fastest = false             # true: every des-k fade and move is instant
first_run_seen = false      # false: the first run opens at the next start

# A screen's own workspace count, by its id (the settings page writes these):
# [workspaces_per_screen]
# "internal" = 2

[shortcuts]
# Keep these two if you switch with the touchpad swipe.
previous = ["Win+Ctrl+Left"]
next     = ["Win+Ctrl+Right"]
# Send the focused window to the previous or next workspace (wraps around).
send_previous      = ["Win+Ctrl+Shift+Left"]   # and follow it there
send_next          = ["Win+Ctrl+Shift+Right"]
send_previous_stay = ["Win+Ctrl+Alt+Left"]     # and stay where you are
send_next_stay     = ["Win+Ctrl+Alt+Right"]
# Go straight to workspace N, or send the focused window there (N = 1 to 9, number row or numpad).
jump         = "Win"         # "" turns a set off
send_to      = "Win+Shift"   # and follow it there
send_to_stay = "Win+Ctrl"    # and stay where you are
# Flip this screen between its workspace and the one before.
back = ["Win+Backspace"]
# Search every window on every screen and workspace; Enter goes there.
finder = ["Win+F"]
# Every workspace of every screen at once, like Task View (which it replaces on Win+Tab).
overview = ["Win+Tab"]
# Keep the focused window on every workspace of its screen; press again to undo.
pin = ["Win+Ctrl+U"]
# Opens the Shortcut hints window: every des-k shortcut and gesture, as set now. [] is no key.
hints = []
# Pauses des-k: every hidden window comes back and Windows gets its own shortcuts back,
# until you press it again or pick Resume des-k in the tray menu. [] is no key.
pause = []

[indicator]
enabled = true
style = "card"              # "card" or "dots"
screen_name = true          # with two or more screens, the card names its screen
spot = "center"             # top-left, top, top-right, left, center, right, bottom-left, bottom, bottom-right or custom
gap = 24                    # 0 to 400: pixels from the screen's edges at 100% scale (center ignores it)
x = 50                      # 0 to 100, with spot = "custom": the card's center, in percent of the screen
y = 50
hold_ms = 550               # 0 to 5000
fade_ms = 200               # 0 to 2000
# A screen can have its own place: a table named by its id (spec 0016), with any of spot, gap, x, y.
# [indicator.screens."DEL-A1B2-12345"]
# spot = "top-right"

[attention]
enabled = true              # a window on a hidden workspace that flashes shows on its screen
text = "{{appname}} misses you"   # up to 100 characters; {{appname}} is the app's name

[colors]
accent = "windows"          # "windows" follows your Windows accent color, or your own "#RRGGBB"
# Your own color for a workspace, in light and dark alike; a workspace left out keeps des-k's:
# ws1 = "#79A4F3"

[names]
# A name for a workspace on every screen, up to 32 characters; a workspace left out shows its number:
# ws1 = "Mail"
# A screen's own table, named by its id, renames the screen and can name its workspaces its own way:
# [names.screens."DEL-A1B2-12345"]
# screen = "Left"
# ws2 = "Notes"             # "" is no name on this screen

[wallpaper]
fit = "fill"                # fill (covers the screen), fit (bars on two sides), stretch or center
transition = "instant"      # instant or fade (fastest = true keeps it instant)
dim = 0                     # 0 to 80: darkens des-k's pictures by this percent, so white icon labels stay readable
interval = 30               # minutes each slideshow picture shows, 1 to 1440
shuffle = false             # true: slideshows go in random order, else by file name
# A picture for a workspace on every screen: a file name in %LOCALAPPDATA%\des-k\wallpapers\
# (.jpg, .jpeg, .png, .bmp or .gif). A workspace left out shows your Windows wallpaper:
# ws1 = "forest.jpg"
# A subfolder of wallpapers\ shows its pictures one after another, and so does a list:
# ws2 = "beach"
# ws3 = ["dunes.jpg", "forest.jpg"]
# [wallpaper.intervals]     # a workspace's own interval, on every screen
# ws2 = 5
# A screen's own table, named by its id, overrides it there:
# [wallpaper.screens."DEL-A1B2-12345"]
# ws2 = "desk.png"          # "" shows your Windows wallpaper on this screen

[apps]
# One line per app. workspace is 1 to 9; screen is optional (a screen id, as in [names.screens]).
# The settings page's Apps section writes these for you.
# "slack.exe" = { workspace = 1, screen = "GSM-5B7F-12345" }
# "spotify.exe" = { workspace = 4 }
# "teams.exe" = "alone"     # des-k never hides, follows, places or lists its windows

[restore]
enabled = true              # after a reboot or sign in, hide reopened windows back into their workspaces
minutes = 3                 # how long after start to keep matching (1 to 30)

[overview]
animation = "off"           # "off" or "fade"
labels = "hover"            # "hover", "always" or "none"
live = true                 # false: the list is taken when the overview opens
fade_ms = 250               # 0 to 1000, with animation = "fade"

[touchpad]
# des-k reads three and four finger gestures itself, so they work over admin windows too.
# It acts only while Windows' own three and four finger gestures are all set to Nothing, so one
# gesture never does two things. Touchpad setup, on the settings page, helps you set them.
enabled = true
swipe_mm = 15               # how far the fingers travel before a swipe counts, in mm (5 to 50)
twice_ms = 300              # how long a gesture with a "times = 2" rule waits for its second go (100 to 1000)

# One block per gesture. fingers = 3 or 4; swipe = "left", "right", "up" or "down", or tap = true;
# times = 1 or 2 (optional); do = an action below, or a key combo like "Ctrl+Shift+T".
# Actions: previous, next, back, send_previous, send_next, send_previous_stay, send_next_stay,
# jump_1 to jump_9, send_to_1 to send_to_9, send_to_stay_1 to send_to_stay_9, finder, overview,
# show_desktop, app_next, app_previous, pin, search, notifications, middle_click, play_pause,
# next_track, previous_track, volume_up, volume_down, mute, none.
# Writing any [[gesture]] block replaces this whole list.
[[gesture]]
fingers = 4
swipe = "left"
do = "next"

[[gesture]]
fingers = 4
swipe = "right"
do = "previous"

[[gesture]]
fingers = 4
swipe = "up"
do = "overview"

[[gesture]]
fingers = 4
swipe = "down"
do = "show_desktop"

[[gesture]]
fingers = 3
swipe = "left"
do = "app_next"

[[gesture]]
fingers = 3
swipe = "right"
do = "app_previous"

[[gesture]]
fingers = 3
swipe = "up"
do = "overview"

[[gesture]]
fingers = 3
swipe = "down"
do = "show_desktop"

[[gesture]]
fingers = 3
tap = true
do = "search"

[[gesture]]
fingers = 4
tap = true
do = "notifications"
"##;


pub const ATTENTION_TEXT: &str = "{{appname}} misses you";
pub const ATTENTION_MAX: usize = 100; // in UTF-16 units, as the page's text box counts (spec 0022 AC-9)
pub const NAME_MAX: usize = 32; // in UTF-16 units, after cleanup (spec 0023 AC-2)
pub const APPS_MAX: usize = 64; // spec 0027 AC-10

/// The settings in force when it was taken: one whole version, whatever saves come after. Hold
/// it for one event or one paint, never in a struct or across a message pump; anything longer
/// lived would keep that copy, and slow every later read on its thread.
pub struct Current(Guard<Arc<Loaded>>);

impl Current {
    /// Changes on every reload that changed something, so a reader can tell it missed one.
    pub fn version(&self) -> u64 {
        self.0.version
    }
}

impl Deref for Current {
    type Target = Settings;
    fn deref(&self) -> &Settings {
        &self.0.settings
    }
}

pub fn get() -> Current {
    Current(CURRENT.load())
}

impl Settings {
    /// The once and twice rules for a gesture.
    fn rules(&self, fingers: usize, motion: Motion) -> (Option<Gesture>, Option<Gesture>) {
        let rule = |times| self.gestures.iter().find(|g| g.fingers as usize == fingers && g.motion == motion && g.times == times).copied();
        (rule(1), rule(2))
    }
}

/// The touchpad thread asks this once per gesture it recognizes (spec 0011) and keeps the rules
/// themselves, so a save before it posts never swaps one for another (spec 0032 AC-4).
pub fn rules_for(fingers: usize, motion: Motion) -> (Option<Gesture>, Option<Gesture>) {
    get().rules(fingers, motion)
}

/// The 10 gestures a touchpad makes, in the order the default file lists them.
pub const GESTURE_ROWS: [(u8, Motion); 10] = [
    (4, Motion::Left), (4, Motion::Right), (4, Motion::Up), (4, Motion::Down),
    (3, Motion::Left), (3, Motion::Right), (3, Motion::Up), (3, Motion::Down),
    (3, Motion::Tap), (4, Motion::Tap),
];

/// What the settings page writes for the touchpad: every row's once rule, "none" where there
/// is none, then its twice rule when it has one (spec 0012 AC-12). `rule(fingers, motion,
/// times)` gives a row's action.
pub fn full_map(rule: impl Fn(u8, Motion, u8) -> Option<Action>) -> Vec<Gesture> {
    let mut out = vec![];
    for (fingers, motion) in GESTURE_ROWS {
        out.push(Gesture { fingers, motion, times: 1, action: rule(fingers, motion, 1).unwrap_or(Action::Nothing) });
        if let Some(action) = rule(fingers, motion, 2) {
            out.push(Gesture { fingers, motion, times: 2, action });
        }
    }
    out
}

/// Every named action, in the order the file's comment lists them, for the gesture pickers.
pub fn actions() -> impl Iterator<Item = Action> {
    ACTIONS.iter().map(|(_, a)| *a)
}

/// A key a shortcut can be saved with: the parser's own names (`vk_of`).
pub fn savable(vk: u16) -> bool {
    NAMED_KEYS.iter().any(|(_, k)| k.0 == vk) || (b'A' as u16..=b'Z' as u16).contains(&vk) || (b'0' as u16..=b'9' as u16).contains(&vk) || (VK_F1.0..=VK_F24.0).contains(&vk)
}

/// The shortcut lists in the order the hook tries them, then the digit sets (keys::shortcut).
pub const LISTS: [&str; 12] = ["previous", "next", "back", "send_previous", "send_next", "send_previous_stay", "send_next_stay", "finder", "overview", "pin", "hints", "pause"];
pub const SETS: [&str; 3] = ["jump", "send_to", "send_to_stay"];

/// Who already uses a combo the page is about to save (spec 0012 AC-9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    List(&'static str),
    /// A digit set the combo is one digit of: the set would skip that digit.
    Set(&'static str),
}

impl Settings {
    /// A row's action for one go (`times` 1) or two.
    pub fn rule(&self, fingers: u8, motion: Motion, times: u8) -> Option<Action> {
        self.gestures.iter().find(|g| (g.fingers, g.motion, g.times) == (fingers, motion, times)).map(|g| g.action)
    }

    /// A shortcut list by its name in [shortcuts].
    pub fn list(&self, name: &str) -> &[Combo] {
        match name {
            "previous" => &self.previous,
            "next" => &self.next,
            "back" => &self.back,
            "send_previous" => &self.send_previous,
            "send_next" => &self.send_next,
            "send_previous_stay" => &self.send_previous_stay,
            "send_next_stay" => &self.send_next_stay,
            "finder" => &self.finder,
            "overview" => &self.overview,
            "pin" => &self.pin,
            "hints" => &self.hints,
            "pause" => &self.pause,
            _ => &[],
        }
    }

    /// A digit set by its name in [shortcuts].
    pub fn set(&self, name: &str) -> Option<u8> {
        match name {
            "jump" => self.jump,
            "send_to" => self.send_to,
            "send_to_stay" => self.send_to_stay,
            _ => None,
        }
    }

    /// Who else uses `combo`, for the list `me`: another list first, as the hook tries them,
    /// else a digit set it is one digit of.
    pub fn combo_owner(&self, me: &str, combo: Combo) -> Option<Owner> {
        if let Some(other) = LISTS.iter().find(|l| **l != me && self.list(l).contains(&combo)) {
            return Some(Owner::List(other));
        }
        let digit = (b'1' as u16..=b'9' as u16).contains(&combo.vk);
        SETS.iter().find(|s| digit && self.set(s) == Some(combo.mods)).map(|s| Owner::Set(s))
    }

    /// The other digit set with these modifiers, if one has them.
    pub fn set_owner(&self, me: &str, mods: u8) -> Option<&'static str> {
        SETS.iter().find(|s| **s != me && self.set(s) == Some(mods)).copied()
    }

    /// The digits a set skips because a list holds its modifiers plus that digit, each with
    /// that list, by digit (AC-8).
    pub fn skips(&self, set: &str) -> Vec<(char, &'static str)> {
        let Some(mods) = self.set(set) else { return vec![] };
        let mut out: Vec<(char, &'static str)> = LISTS
            .iter()
            .flat_map(|l| self.list(l).iter().filter(|c| c.mods == mods && (b'1' as u16..=b'9' as u16).contains(&c.vk)).map(move |c| (c.vk as u8 as char, *l)))
            .collect();
        out.sort();
        out
    }
}

/// The popup's words for an app: your `[attention] text` filled (spec 0022 AC-9).
pub fn attention_line(name: &str) -> String {
    fill(&get().attention_text, name)
}

/// `text` with every `{{appname}}`, in any letter case, made `name` ("An app" when empty), and
/// control characters (a newline) made spaces, since the popup's line has one paragraph.
pub fn fill(text: &str, name: &str) -> String {
    const TAG: &str = "{{appname}}";
    let name = if name.trim().is_empty() { "An app" } else { name };
    // ASCII lower casing keeps every byte where it was, so a match's place holds in `text`.
    let lower = text.to_ascii_lowercase();
    let (mut out, mut at) = (String::new(), 0);
    while let Some(i) = lower[at..].find(TAG) {
        out += &text[at..at + i];
        out += name;
        at += i + TAG.len();
    }
    out += &text[at..];
    out.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

pub fn mods_text(mods: Option<u8>) -> String {
    let Some(mods) = mods else { return "off".into() };
    let names = [(WIN, "Win"), (CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")];
    names.iter().filter(|(bit, _)| mods & bit != 0).map(|(_, name)| *name).collect::<Vec<_>>().join("+")
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (bit, name) in [(WIN, "Win"), (CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")] {
            if self.mods & bit != 0 {
                write!(f, "{name}+")?;
            }
        }
        match NAMED_KEYS.iter().find(|(_, vk)| vk.0 == self.vk) {
            Some((name, _)) => write!(f, "{name}"),
            None if (VK_F1.0..=VK_F24.0).contains(&self.vk) => write!(f, "F{}", self.vk - VK_F1.0 + 1),
            None => write!(f, "{}", self.vk as u8 as char),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some((name, _)) = ACTIONS.iter().find(|(_, a)| a == self) {
            return write!(f, "{name}");
        }
        match self {
            Action::Jump(i) => write!(f, "jump_{}", i + 1),
            Action::SendTo(i, true) => write!(f, "send_to_{}", i + 1),
            Action::SendTo(i, false) => write!(f, "send_to_stay_{}", i + 1),
            Action::Combo(c) => write!(f, "{c}"),
            _ => write!(f, "?"),
        }
    }
}

impl Action {
    /// The plain words the welcome screen shows for this action.
    pub fn label(&self) -> String {
        match self {
            Action::Jump(i) => format!("workspace {}", i + 1),
            Action::SendTo(i, true) => format!("send window to {}", i + 1),
            Action::SendTo(i, false) => format!("send window to {}, stay", i + 1),
            Action::Combo(c) => c.to_string(),
            _ => {
                let name = self.to_string();
                ACTION_LABELS.iter().find(|(n, _)| *n == name).map_or(name, |(_, l)| l.to_string())
            }
        }
    }
}

/// "4 left", "3 tap twice": the gesture, without its action.
impl fmt::Display for Gesture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}{}", self.fingers, self.motion.word(), if self.times == 2 { " twice" } else { "" })
    }
}

impl Settings {
    /// Screen `id`'s workspace count: its `[workspaces_per_screen]` line, else `workspaces`
    /// (spec 0040 AC-1).
    pub fn count_for(&self, id: &str) -> usize {
        self.count_by_screen.get(id).copied().unwrap_or(self.shared_count)
    }

    /// Whether screen `id` follows `workspaces`: it has no line, or one equal to it (AC-1).
    pub fn follows_shared(&self, id: &str) -> bool {
        self.count_by_screen.get(id).is_none_or(|&n| n == self.shared_count)
    }

    /// The counts as the start line says them: "4 workspaces each; internal 2" (AC-3).
    pub fn counts_said(&self) -> String {
        let lines: Vec<String> = self.count_by_screen.iter().map(|(id, n)| format!("{id} {n}")).collect();
        let each = format!("{} workspaces each", self.shared_count);
        if lines.is_empty() { each } else { format!("{each}; {}", lines.join(", ")) }
    }

    /// The highest count any screen can have, for lists that cover every screen (AC-11).
    pub fn most(&self) -> usize {
        self.count_by_screen.values().copied().fold(self.shared_count, usize::max)
    }

    /// The indicator's place on screen `id`: its own table's keys over the default (AC-9).
    pub fn indicator_pos_for(&self, id: &str) -> Pos {
        self.indicator_screens.get(id).map_or(self.indicator_pos, |o| o.over(self.indicator_pos))
    }

    /// Workspace `i`'s name (0 based) on screen `id`: the screen's own key, else `[names]`'s,
    /// else none; `""` is none, and so is a workspace past the count (spec 0023 AC-1, AC-3).
    pub fn ws_name(&self, id: &str, i: usize) -> Option<&str> {
        if i >= self.count_for(id) {
            return None;
        }
        let own = self.names_screens.get(id).and_then(|n| n.ws.get(i)?.as_deref());
        Some(own.unwrap_or(self.names.get(i)?)).filter(|n| !n.is_empty())
    }

    /// Workspace `i`'s wallpaper (0 based) on screen `id`: the screen's own key, else
    /// `[wallpaper]`'s, else none; `Pick::None` is none, and so is a workspace past the count
    /// (spec 0026 AC-1, AC-10).
    pub fn wallpaper_for(&self, id: &str, i: usize) -> Option<&Pick> {
        if i >= self.count_for(id) {
            return None;
        }
        let own = self.wallpaper_screens.get(id).and_then(|w| w.get(i)?.as_ref());
        Some(own.unwrap_or(self.wallpaper.get(i)?)).filter(|p| **p != Pick::None)
    }

    /// Minutes each picture of workspace `i`'s slideshow shows (spec 0028 AC-2).
    pub fn interval_for(&self, i: usize) -> u16 {
        self.wallpaper_intervals.get(i).copied().flatten().unwrap_or(self.wallpaper_interval)
    }

    /// Whether a picture change fades: `transition = "fade"`, unless `fastest` (spec 0026 AC-4).
    pub fn wallpaper_fades(&self) -> bool {
        self.wallpaper_fade && !self.fastest
    }

    /// Your name for screen `id`, if you renamed it (spec 0023 AC-11).
    pub fn screen_rename(&self, id: &str) -> Option<&str> {
        self.names_screens.get(id)?.screen.as_deref()
    }
}

/// `#RRGGBB`, upper case, as the settings page writes a color.
pub fn hex(c: u32) -> String {
    format!("#{c:06X}")
}

#[cfg(test)]
impl Settings {
    /// The one picture or folder workspace `i` shows on `id`, for tests that read one value.
    pub(crate) fn wall_name(&self, id: &str, i: usize) -> Option<&str> {
        match self.wallpaper_for(id, i)? {
            Pick::Picture(f) | Pick::Folder(f) => Some(f),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
