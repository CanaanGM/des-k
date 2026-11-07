// settings.toml in %LOCALAPPDATA%\des-k (docs/specs/0001-settings-file/index.md), read at start
// and again whenever the settings page saves it or its write time changes (spec 0012).
// Not next to the exe: the installed exe lives in Program Files, where you cannot write (spec 0008).
// A bad value resets just that setting. A file that is not TOML resets all of them at start,
// but keeps the settings in effect on a live reload. des-k always starts.
use std::cell::{Cell, RefCell};
use std::fmt;
use std::io::Write;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::SystemTime;
use toml::{Table, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

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
    pub workspaces: usize,
    pub copilot_as_right_ctrl: bool,
    pub follow_activation: bool,
    pub fastest: bool, // every des-k fade and move instant (spec 0017)
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
    pub finder: Vec<Combo>, // opens the window finder (spec 0007)
    pub overview: Vec<Combo>, // opens the workspace overview (spec 0010)
    pub pin: Vec<Combo>,      // keeps the focused window on every workspace (spec 0014)
    pub indicator_enabled: bool,
    pub indicator_style: Style,
    pub hold_ms: u32,
    pub fade_ms: u32,
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
            workspaces: 4,
            copilot_as_right_ctrl: true,
            follow_activation: true,
            fastest: false,
            previous: vec![Combo { mods: WIN | CTRL, vk: VK_LEFT.0 }],
            next: vec![Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 }],
            send_previous: vec![Combo { mods: WIN | CTRL | SHIFT, vk: VK_LEFT.0 }],
            send_next: vec![Combo { mods: WIN | CTRL | SHIFT, vk: VK_RIGHT.0 }],
            send_previous_stay: vec![Combo { mods: WIN | CTRL | ALT, vk: VK_LEFT.0 }],
            send_next_stay: vec![Combo { mods: WIN | CTRL | ALT, vk: VK_RIGHT.0 }],
            jump: Some(WIN),
            send_to: Some(WIN | SHIFT),
            send_to_stay: Some(WIN | CTRL),
            finder: vec![Combo { mods: WIN, vk: b'F' as u16 }],
            overview: vec![Combo { mods: WIN, vk: VK_TAB.0 }],
            pin: vec![Combo { mods: WIN | CTRL, vk: b'U' as u16 }],
            indicator_enabled: true,
            indicator_style: Style::Card,
            hold_ms: 550,
            fade_ms: 200,
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
        }
    }
}

/// Written verbatim when the file is missing. Must parse to `Settings::default()` (tested).
const DEFAULT_FILE: &str = r#"# des-k settings. A saved change applies within two seconds; [restore] acts at the next start.

workspaces = 4              # 2 to 9, every screen
copilot_as_right_ctrl = true
follow_activation = true    # switch to a hidden window's workspace when it comes to the front
fastest = false             # true: every des-k fade and move is instant

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
# Search every window on every screen and workspace; Enter goes there.
finder = ["Win+F"]
# Every workspace of every screen at once, like Task View (which it replaces on Win+Tab).
overview = ["Win+Tab"]
# Keep the focused window on every workspace of its screen; press again to undo.
pin = ["Win+Ctrl+U"]

[indicator]
enabled = true
style = "card"              # "card" or "dots"
hold_ms = 550               # 0 to 5000
fade_ms = 200               # 0 to 2000

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
# Actions: previous, next, send_previous, send_next, send_previous_stay, send_next_stay,
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
"#;

const TOP_KEYS: &[&str] = &["workspaces", "copilot_as_right_ctrl", "follow_activation", "fastest", "shortcuts", "indicator", "restore", "overview", "touchpad", "gesture"];
const SHORTCUT_KEYS: &[&str] = &["previous", "next", "send_previous", "send_next", "send_previous_stay", "send_next_stay", "jump", "send_to", "send_to_stay", "finder", "overview", "pin"];
const INDICATOR_KEYS: &[&str] = &["enabled", "style", "hold_ms", "fade_ms"];
const RESTORE_KEYS: &[&str] = &["enabled", "minutes"];
const OVERVIEW_KEYS: &[&str] = &["animation", "labels", "live", "fade_ms"];
const TOUCHPAD_KEYS: &[&str] = &["enabled", "swipe_mm", "twice_ms"];
const GESTURE_KEYS: &[&str] = &["fingers", "swipe", "tap", "times", "do"];

/// One problem found in the file, in the log's words. `key` is like "indicator.hold_ms", or
/// empty when the problem names no single setting.
#[derive(PartialEq, Debug)]
pub struct Problem {
    pub key: String,
    pub message: String,
}

impl Problem {
    fn new(message: String) -> Self {
        let key = message.split_once(": ").map_or("", |(k, _)| k);
        let key = if key.contains(' ') { "" } else { key };
        Problem { key: key.to_string(), message }
    }
}

struct Loaded {
    settings: Settings,
    problems: Vec<Problem>,
}

// Swapped whole on each reload. The old one is leaked, never freed, so every `&'static` a
// reader on any thread still holds stays valid (a few KB per save). Null until load().
static CURRENT: AtomicPtr<Loaded> = AtomicPtr::new(std::ptr::null_mut());
// What get() gives before load(), kept apart so an early get() never waits on load().
static DEFAULTS: LazyLock<Settings> = LazyLock::new(Settings::default);

thread_local! {
    // The file's write time when des-k last read it. Main thread only (spec 0012, file watch).
    static SEEN: Cell<Option<SystemTime>> = const { Cell::new(None) };
    // Why the file could not be read or is not TOML, until a read succeeds (AC-14). The page
    // reads it to go read only, at start and after a live reload alike.
    static PARSE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The last read or parse error, while the file is still broken. Main thread.
pub fn parse_error() -> Option<String> {
    PARSE_ERROR.with_borrow(Clone::clone)
}

fn current() -> Option<&'static Loaded> {
    // SAFETY: CURRENT is null or a leaked Box that is never freed.
    unsafe { CURRENT.load(Ordering::Acquire).as_ref() }
}

pub fn get() -> &'static Settings {
    current().map_or(&DEFAULTS, |l| &l.settings)
}

/// What is wrong in the file right now, for the settings page.
pub fn problems() -> &'static [Problem] {
    current().map_or(&[], |l| &l.problems)
}

fn store(settings: Settings, problems: Vec<Problem>) {
    CURRENT.store(Box::into_raw(Box::new(Loaded { settings, problems })), Ordering::Release);
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// settings.toml in the data folder, shared by the installed and the dev copy.
pub fn path() -> std::path::PathBuf {
    crate::ws::data_dir().join("settings.toml")
}

/// Read the file (writing the default one if missing) and log every problem. Main thread, at start.
pub fn load() {
    let path = path();
    SEEN.set(modified(&path));
    let (settings, problems) = match std::fs::read_to_string(&path) {
        Ok(text) => {
            PARSE_ERROR.set(text.parse::<Table>().err().map(|e| not_toml(&e.to_string(), e.message())));
            let (settings, warnings) = parse(&text);
            for w in &warnings {
                eprintln!("settings: {w}");
            }
            (settings, warnings.into_iter().map(Problem::new).collect())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // create_new: never overwrite a file that appeared meanwhile.
            let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut f| f.write_all(DEFAULT_FILE.as_bytes()));
            match written {
                Ok(()) => println!("settings: wrote defaults to {}", path.display()),
                Err(e) => eprintln!("settings: cannot write {} ({e}), using all defaults", path.display()),
            }
            SEEN.set(modified(&path));
            (Settings::default(), Vec::new())
        }
        Err(e) => {
            PARSE_ERROR.set(Some(format!("cannot read {} ({e})", path.display())));
            eprintln!("settings: cannot read {} ({e}), using all defaults", path.display());
            (Settings::default(), Vec::new())
        }
    };
    println!("settings: {}: {settings}", path.display());
    store(settings, problems);
}

/// Who changed the file, for the log (spec 0012 AC-15).
#[derive(Clone, Copy)]
pub enum Source {
    Page,
    File,
}

/// Main loop, every second: reload when the file's write time changed (AC-5). A missing
/// file is skipped, and read again once it is back.
pub fn watch() {
    if let Some(t) = modified(&path())
        && SEEN.get() != Some(t)
    {
        let _ = reload(Source::File);
    }
}

/// Read the file again and put it in effect for every reader at once. A file that is not
/// TOML keeps the settings in effect: a typo mid edit must not remap every shortcut (AC-14).
/// Returns the settings that changed. Main thread.
pub fn reload(source: Source) -> Result<Vec<Change>, String> {
    let path = path();
    SEEN.set(modified(&path)); // before reading, so a save during the read is seen next tick
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {} ({e})", path.display()));
    let (settings, warnings) = match text.and_then(|t| reparse(&t)) {
        Ok(got) => got,
        Err(why) => {
            eprintln!("settings: {why}, keeping the settings in effect");
            PARSE_ERROR.set(Some(why.clone()));
            crate::settings_page::refresh();
            return Err(why);
        }
    };
    PARSE_ERROR.set(None);
    let old = get();
    // Only new problems are logged, so one bad value is not logged again on every save.
    let before = problems();
    for w in warnings.iter().filter(|w| !before.iter().any(|p| &p.message == *w)) {
        eprintln!("settings: {w}");
    }
    let problems: Vec<Problem> = warnings.into_iter().map(Problem::new).collect();
    let changes = diff(old, &settings, source);
    if !changes.is_empty() || problems != before {
        store(settings, problems);
    }
    let moves = apply(old, &changes);
    if matches!(source, Source::File) && !changes.is_empty() {
        crate::settings_page::hand_edit(&changes, &moves);
    }
    crate::settings_page::refresh();
    Ok(changes)
}

/// One changed setting, as the log and the page's banner name it (AC-15, AC-20).
#[derive(Debug, PartialEq)]
pub struct Change {
    pub key: &'static str,
    pub was: String,
    pub now: String,
}

/// Logs one line per changed setting (AC-15) and returns them.
fn diff(old: &Settings, new: &Settings, source: Source) -> Vec<Change> {
    let from = match source {
        Source::Page => "page",
        Source::File => "file",
    };
    let mut changes = Vec::new();
    for ((key, was), (_, now)) in old.fields().into_iter().zip(new.fields()) {
        if was != now {
            println!("settings: {key} {was} -> {now} ({from})");
            changes.push(Change { key, was, now });
        }
    }
    changes
}

/// The side effects of `changes`, once the new settings are in effect. Most readers call
/// get() each time they act and need nothing. Returns the workspace move lines (AC-6).
fn apply(old: &Settings, changes: &[Change]) -> Vec<String> {
    if changes.is_empty() {
        return vec![];
    }
    let new = get();
    let has = |k: &str| changes.iter().any(|c| c.key == k);
    // Its tiles are drawn for the old settings.
    if crate::overview::is_open() {
        crate::overview::close(true);
    }
    let mut moves = vec![];
    if new.workspaces != old.workspaces {
        moves = crate::ws::resize(new.workspaces);
        for line in &moves {
            println!("settings: workspaces {} -> {}: {line}", old.workspaces, new.workspaces);
        }
    }
    if has("touchpad.enabled") && new.touchpad_enabled {
        crate::touchpad::start();
    }
    if has("gestures") || has("touchpad.twice_ms") {
        crate::touchpad::drop_pending();
    }
    if has("fastest") {
        crate::ui::transitions_all();
    }
    crate::welcome::refresh();
    moves
}

/// A change the settings page makes to the file (spec 0012).
pub enum Edit {
    /// A key like "indicator.enabled" or "workspaces", and its new value.
    Value(&'static str, toml_edit::Value),
    /// A shortcut list, like "shortcuts.finder"; empty is off.
    Combos(&'static str, Vec<Combo>),
    /// A digit set, like "shortcuts.jump", and its modifiers; None is off.
    Digits(&'static str, Option<u8>),
    /// Every [[gesture]] block, rewritten as this list (AC-12).
    Gestures(Vec<Gesture>),
    /// Two edits in one write, so a clash is never saved halfway (AC-9).
    Pair(Box<Edit>, Box<Edit>),
}

/// Changes one value in the file and keeps every other byte, then reloads (AC-3). The new
/// file is swapped in whole by one rename, so a crash never leaves half of it (AC-17).
pub fn write(edit: Edit) -> Result<(), String> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_FILE.to_string(),
        Err(e) => return Err(format!("cannot read {} ({e})", path.display())),
    };
    let new = edit_text(&text, &edit)?;
    let tmp = path.with_extension("toml.tmp");
    if let Err(e) = crate::ws::write_new(&tmp, &new).and_then(|()| std::fs::rename(&tmp, &path)) {
        let why = format!("cannot save {} ({e})", path.display());
        eprintln!("settings: {why}");
        return Err(why);
    }
    reload(Source::Page).map(|_| ())
}

/// Pure, so the tests can run it: the file's text with the edit made and nothing else changed.
fn edit_text(text: &str, edit: &Edit) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| not_toml(&e.to_string(), e.message()))?;
    edit_doc(&mut doc, edit)?;
    Ok(doc.to_string())
}

fn edit_doc(doc: &mut toml_edit::DocumentMut, edit: &Edit) -> Result<(), String> {
    match edit {
        Edit::Value(name, value) => set_value(doc, name, value.clone()),
        Edit::Combos(name, list) => {
            let mut items: toml_edit::Array = list.iter().map(|c| c.to_string()).collect();
            items.fmt();
            set_value(doc, name, items.into())
        }
        Edit::Digits(name, mods) => set_value(doc, name, mods.map_or(String::new(), |m| mods_text(Some(m))).into()),
        Edit::Gestures(list) => {
            set_gestures(doc, list);
            Ok(())
        }
        Edit::Pair(a, b) => edit_doc(doc, a).and_then(|()| edit_doc(doc, b)),
    }
}

/// The [[gesture]] blocks as `list`, one block per rule. The comment above the first block
/// stays; comments inside the blocks go (spec 0012 AC-12).
fn set_gestures(doc: &mut toml_edit::DocumentMut, list: &[Gesture]) {
    let first = doc.get("gesture").and_then(toml_edit::Item::as_array_of_tables).and_then(|a| a.get(0)).and_then(|t| t.decor().prefix().cloned());
    let mut blocks = toml_edit::ArrayOfTables::new();
    for (i, g) in list.iter().enumerate() {
        let mut t = toml_edit::Table::new();
        t.insert("fingers", toml_edit::value(g.fingers as i64));
        match g.motion {
            Motion::Tap => t.insert("tap", toml_edit::value(true)),
            m => t.insert("swipe", toml_edit::value(m.word())),
        };
        if g.times == 2 {
            t.insert("times", toml_edit::value(2));
        }
        t.insert("do", toml_edit::value(g.action.to_string()));
        match (i, &first) {
            (0, Some(prefix)) => t.decor_mut().set_prefix(prefix.clone()),
            _ => t.decor_mut().set_prefix("\n"),
        }
        blocks.push(t);
    }
    doc.insert("gesture", toml_edit::Item::ArrayOfTables(blocks));
}

fn set_value(doc: &mut toml_edit::DocumentMut, name: &str, value: toml_edit::Value) -> Result<(), String> {
    let (section, key) = match name.split_once('.') {
        Some((section, key)) => (Some(section), key),
        None => (None, name),
    };
    let table = match section {
        None => doc.as_table_mut(),
        Some(section) => doc.entry(section).or_insert(toml_edit::table()).as_table_mut().ok_or_else(|| format!("{section}: expected a [{section}] section"))?,
    };
    let mut value = value;
    match table.get_mut(key) {
        // get_mut, not insert, which would reformat the key. The old value's decor holds the
        // spacing and the comment after it.
        Some(item) => {
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = toml_edit::Item::Value(value);
        }
        None => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
    Ok(())
}

/// For the settings page's tests: the default file with one value changed.
#[cfg(test)]
pub fn edit_for_test(key: &'static str, value: toml_edit::Value) -> String {
    edit_text(DEFAULT_FILE, &Edit::Value(key, value)).unwrap()
}

#[cfg(test)]
pub fn parse_for_test(text: &str) -> (Settings, Vec<String>) {
    parse(text)
}

/// Like parse(), but a file that is not TOML is an error instead of all defaults.
fn reparse(text: &str) -> Result<(Settings, Vec<String>), String> {
    match text.parse::<Table>() {
        Ok(_) => Ok(parse(text)),
        Err(e) => Err(not_toml(&e.to_string(), e.message())),
    }
}

/// The first line of a parser's error carries the line and column; its message says what.
fn not_toml(full: &str, message: &str) -> String {
    format!("not valid TOML ({}: {message})", full.lines().next().unwrap_or(""))
}

/// Pure, so the tests can run it: the settings plus one message per problem found.
fn parse(text: &str) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut warn = Vec::new();
    let top: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            warn.push(format!("{}, using all defaults", not_toml(&e.to_string(), e.message())));
            return (s, warn);
        }
    };
    unknown_keys(&top, "", TOP_KEYS, &mut warn);
    s.workspaces = int(&top, "workspaces", "workspaces", 2, 9, s.workspaces as i64, &mut warn) as usize;
    s.copilot_as_right_ctrl = boolean(&top, "copilot_as_right_ctrl", "copilot_as_right_ctrl", s.copilot_as_right_ctrl, &mut warn);
    s.follow_activation = boolean(&top, "follow_activation", "follow_activation", s.follow_activation, &mut warn);
    s.fastest = boolean(&top, "fastest", "fastest", s.fastest, &mut warn);

    if let Some(t) = section(&top, "shortcuts", &mut warn) {
        unknown_keys(t, "shortcuts.", SHORTCUT_KEYS, &mut warn);
        let previous = combos(t, "previous", "shortcuts.previous", &s.previous, &mut warn);
        let next = combos(t, "next", "shortcuts.next", &s.next, &mut warn);
        // Checked after refilling empty lists, so a clash with a default is caught too.
        if let Some(c) = previous.iter().find(|c| next.contains(c)) {
            warn.push(format!("shortcuts: {c} is in both previous and next, using defaults for both"));
        } else {
            (s.previous, s.next) = (previous, next);
        }
        // Switch keys always win: a send list (or the finder, the overview, pin) sharing a combo with
        // them, or with an earlier list still on, is turned off (an empty list never matches)
        // (specs 0004, 0007, 0010, 0014).
        let mut taken = vec![("previous", s.previous.clone()), ("next", s.next.clone())];
        let sends = [
            ("send_previous", &mut s.send_previous),
            ("send_next", &mut s.send_next),
            ("send_previous_stay", &mut s.send_previous_stay),
            ("send_next_stay", &mut s.send_next_stay),
            ("finder", &mut s.finder),
            ("overview", &mut s.overview),
            ("pin", &mut s.pin),
        ];
        for (key, list) in sends {
            let got = combos(t, key, &format!("shortcuts.{key}"), list, &mut warn);
            match got.iter().find_map(|c| Some((c, taken.iter().find(|(_, l)| l.contains(c))?.0))) {
                Some((c, other)) => {
                    warn.push(format!("shortcuts: {c} is in both {other} and {key}, {key} turned off"));
                    list.clear();
                }
                None => {
                    taken.push((key, got.clone()));
                    *list = got;
                }
            }
        }
        // Digit sets (spec 0005): one with the same modifiers as an earlier set is turned off.
        let mut sets: Vec<(&str, u8)> = Vec::new();
        for (key, set) in [("jump", &mut s.jump), ("send_to", &mut s.send_to), ("send_to_stay", &mut s.send_to_stay)] {
            *set = mods(t, key, *set, &mut warn);
            let Some(m) = *set else { continue };
            match sets.iter().find(|(_, other)| *other == m) {
                Some((other, _)) => {
                    warn.push(format!("shortcuts: {} is in both {other} and {key}, {key} turned off", mods_text(*set)));
                    *set = None;
                }
                None => sets.push((key, m)),
            }
        }
        // A list on one of a set's digits is valid: the hook tries the lists first, so the
        // list keeps that digit and the set the other eight (the page says so, spec 0012 AC-8).
    }

    if let Some(t) = section(&top, "indicator", &mut warn) {
        unknown_keys(t, "indicator.", INDICATOR_KEYS, &mut warn);
        s.indicator_enabled = boolean(t, "enabled", "indicator.enabled", s.indicator_enabled, &mut warn);
        s.indicator_style = choice(t, "style", "indicator.style", STYLES, s.indicator_style, &mut warn);
        s.hold_ms = int(t, "hold_ms", "indicator.hold_ms", 0, 5000, s.hold_ms as i64, &mut warn) as u32;
        s.fade_ms = int(t, "fade_ms", "indicator.fade_ms", 0, 2000, s.fade_ms as i64, &mut warn) as u32;
    }

    if let Some(t) = section(&top, "restore", &mut warn) {
        unknown_keys(t, "restore.", RESTORE_KEYS, &mut warn);
        s.restore_enabled = boolean(t, "enabled", "restore.enabled", s.restore_enabled, &mut warn);
        s.restore_minutes = int(t, "minutes", "restore.minutes", 1, 30, s.restore_minutes as i64, &mut warn) as u32;
    }

    if let Some(t) = section(&top, "overview", &mut warn) {
        unknown_keys(t, "overview.", OVERVIEW_KEYS, &mut warn);
        s.overview_animation = choice(t, "animation", "overview.animation", ANIMATIONS, s.overview_animation, &mut warn);
        s.overview_labels = choice(t, "labels", "overview.labels", LABELS, s.overview_labels, &mut warn);
        s.overview_live = boolean(t, "live", "overview.live", s.overview_live, &mut warn);
        s.overview_fade_ms = int(t, "fade_ms", "overview.fade_ms", 0, 1000, s.overview_fade_ms as i64, &mut warn) as u32;
    }

    if let Some(t) = section(&top, "touchpad", &mut warn) {
        unknown_keys(t, "touchpad.", TOUCHPAD_KEYS, &mut warn);
        s.touchpad_enabled = boolean(t, "enabled", "touchpad.enabled", s.touchpad_enabled, &mut warn);
        s.swipe_mm = int(t, "swipe_mm", "touchpad.swipe_mm", 5, 50, s.swipe_mm as i64, &mut warn) as u32;
        s.twice_ms = int(t, "twice_ms", "touchpad.twice_ms", 100, 1000, s.twice_ms as i64, &mut warn) as u32;
    }

    if let Some(v) = top.get("gesture")
        && let Some(list) = gestures(v, &mut warn)
    {
        s.gestures = list;
    }
    (s, warn)
}

impl Settings {
    /// The once and twice rules for a gesture, as indexes into `gestures`.
    fn rules(&self, fingers: usize, motion: Motion) -> (Option<usize>, Option<usize>) {
        let rule = |times| self.gestures.iter().position(|g| g.fingers as usize == fingers && g.motion == motion && g.times == times);
        (rule(1), rule(2))
    }
}

/// The touchpad thread asks this for each gesture it recognizes (spec 0011).
pub fn rules_for(fingers: usize, motion: Motion) -> (Option<usize>, Option<usize>) {
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
pub const LISTS: [&str; 9] = ["previous", "next", "send_previous", "send_next", "send_previous_stay", "send_next_stay", "finder", "overview", "pin"];
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
            "send_previous" => &self.send_previous,
            "send_next" => &self.send_next,
            "send_previous_stay" => &self.send_previous_stay,
            "send_next_stay" => &self.send_next_stay,
            "finder" => &self.finder,
            "overview" => &self.overview,
            "pin" => &self.pin,
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

/// The [[gesture]] blocks, or None to keep the default map. A bad rule costs only itself;
/// writing any rule replaces the whole default map (spec 0011 AC-3, AC-4).
fn gestures(v: &Value, warn: &mut Vec<String>) -> Option<Vec<Gesture>> {
    let Some(items) = v.as_array().filter(|items| items.iter().all(Value::is_table)) else {
        warn.push(format!("gesture: expected [[gesture]] blocks, got a {}, using the default gestures", v.type_str()));
        return None;
    };
    if items.is_empty() {
        return None; // no rule written
    }
    let mut out: Vec<Gesture> = Vec::new();
    for (i, t) in items.iter().filter_map(Value::as_table).enumerate() {
        let name = format!("gesture rule {}", i + 1);
        let g = match gesture(t) {
            Ok(g) => g,
            Err(why) => {
                warn.push(format!("{name}: {why}, skipped"));
                continue;
            }
        };
        if out.iter().any(|o| (o.fingers, o.motion, o.times) == (g.fingers, g.motion, g.times)) {
            warn.push(format!("{name}: {g} is already set by an earlier rule, skipped"));
            continue;
        }
        if let Action::Combo(c) = g.action
            && (c == (Combo { mods: WIN, vk: b'L' as u16 }) || c == (Combo { mods: CTRL | ALT, vk: VK_DELETE.0 }))
        {
            warn.push(format!("{name}: Windows never lets a program type {c}, so it does nothing"));
        }
        out.push(g);
    }
    Some(out)
}

fn gesture(t: &Table) -> Result<Gesture, String> {
    if let Some(key) = t.keys().find(|k| !GESTURE_KEYS.contains(&k.as_str())) {
        return Err(format!("unknown field {key}"));
    }
    let fingers = match t.get("fingers") {
        Some(Value::Integer(n @ 3..=4)) => *n as u8,
        Some(v) => return Err(format!("fingers = {v}, not 3 or 4")),
        None => return Err("no fingers".into()),
    };
    let motion = match (t.get("swipe"), t.get("tap")) {
        (Some(Value::String(w)), None) => match SWIPES.iter().find(|(s, _)| s.eq_ignore_ascii_case(w)) {
            Some((_, m)) => *m,
            None => return Err(format!("swipe = \"{w}\", not \"left\", \"right\", \"up\" or \"down\"")),
        },
        (Some(v), None) => return Err(format!("swipe = {v}, not \"left\", \"right\", \"up\" or \"down\"")),
        (None, Some(Value::Boolean(true))) => Motion::Tap,
        (None, Some(v)) => return Err(format!("tap = {v}, only tap = true works")),
        (Some(_), Some(_)) => return Err("both swipe and tap".into()),
        (None, None) => return Err("neither swipe nor tap".into()),
    };
    let times = match t.get("times") {
        None => 1,
        Some(Value::Integer(n @ 1..=2)) => *n as u8,
        Some(v) => return Err(format!("times = {v}, not 1 or 2")),
    };
    let action = match t.get("do") {
        Some(Value::String(text)) => action(text)?,
        Some(v) => return Err(format!("do = {v}, not text")),
        None => return Err("no do".into()),
    };
    Ok(Gesture { fingers, motion, times, action })
}

/// An action name, any case, or else a key combo like the [shortcuts] ones.
fn action(text: &str) -> Result<Action, String> {
    let name = text.trim().to_ascii_lowercase();
    if let Some((_, a)) = ACTIONS.iter().find(|(n, _)| *n == name) {
        return Ok(*a);
    }
    let digit = |prefix: &str| name.strip_prefix(prefix)?.parse::<usize>().ok().filter(|n| (1..=9).contains(n)).map(|n| n - 1);
    if let Some(i) = digit("send_to_stay_") {
        return Ok(Action::SendTo(i, false));
    }
    if let Some(i) = digit("send_to_") {
        return Ok(Action::SendTo(i, true));
    }
    if let Some(i) = digit("jump_") {
        return Ok(Action::Jump(i));
    }
    parse_combo(text).map(Action::Combo).map_err(|why| format!("do = \"{text}\" is not an action name, and as a key combo it {why}"))
}

/// One word out of `options`, any case. A bad value keeps `default`.
fn choice<T: Copy + PartialEq>(t: &Table, key: &str, name: &str, options: &[(&str, T)], default: T, warn: &mut Vec<String>) -> T {
    let words = || options.iter().map(|(w, _)| format!("\"{w}\"")).collect::<Vec<_>>().join(", ");
    let fallback = options.iter().find(|(_, v)| *v == default).map_or("", |(w, _)| w);
    match t.get(key) {
        None => default,
        Some(Value::String(text)) => match options.iter().find(|(w, _)| w.eq_ignore_ascii_case(text)) {
            Some((_, v)) => *v,
            None => {
                warn.push(format!("{name}: \"{text}\" is not one of {}, using default \"{fallback}\"", words()));
                default
            }
        },
        Some(v) => {
            warn.push(format!("{name}: expected one of {}, got a {}, using default \"{fallback}\"", words(), v.type_str()));
            default
        }
    }
}

fn unknown_keys(t: &Table, prefix: &str, known: &[&str], warn: &mut Vec<String>) {
    for key in t.keys().filter(|k| !known.contains(&k.as_str())) {
        warn.push(format!("unknown key {prefix}{key}, ignored"));
    }
}

fn section<'a>(t: &'a Table, key: &str, warn: &mut Vec<String>) -> Option<&'a Table> {
    match t.get(key)? {
        Value::Table(t) => Some(t),
        v => {
            warn.push(format!("{key}: expected a [{key}] section, got a {}, using defaults for it", v.type_str()));
            None
        }
    }
}

fn int(t: &Table, key: &str, name: &str, lo: i64, hi: i64, default: i64, warn: &mut Vec<String>) -> i64 {
    match t.get(key) {
        None => default,
        Some(Value::Integer(n)) if (lo..=hi).contains(n) => *n,
        Some(Value::Integer(n)) => {
            warn.push(format!("{name}: {n} is not between {lo} and {hi}, using default {default}"));
            default
        }
        Some(v) => {
            warn.push(format!("{name}: expected a whole number, got a {}, using default {default}", v.type_str()));
            default
        }
    }
}

fn boolean(t: &Table, key: &str, name: &str, default: bool, warn: &mut Vec<String>) -> bool {
    match t.get(key) {
        None => default,
        Some(Value::Boolean(b)) => *b,
        Some(v) => {
            warn.push(format!("{name}: expected true or false, got a {}, using default {default}", v.type_str()));
            default
        }
    }
}

/// Bad entries are dropped one by one; a list left empty by that falls back to `default`.
/// A list written empty, `[]`, is off: the settings page writes it when you remove the last
/// combo (spec 0012 AC-8).
fn combos(t: &Table, key: &str, name: &str, default: &[Combo], warn: &mut Vec<String>) -> Vec<Combo> {
    let fallback = || default.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
    let items = match t.get(key) {
        None => return default.to_vec(),
        Some(Value::Array(items)) => items,
        Some(v) => {
            warn.push(format!("{name}: expected a list like [\"Win+Ctrl+Left\"], got a {}, using default [{}]", v.type_str(), fallback()));
            return default.to_vec();
        }
    };
    let mut out = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            warn.push(format!("{name}: entry {item} is not text, dropped"));
            continue;
        };
        match parse_combo(text) {
            Ok(c) if !out.contains(&c) => out.push(c),
            Ok(_) => {} // listed twice: harmless
            Err(why) => warn.push(format!("{name}: \"{text}\" {why}, dropped")),
        }
    }
    if out.is_empty() && !items.is_empty() {
        warn.push(format!("{name}: no valid entry left, using default [{}]", fallback()));
        return default.to_vec();
    }
    out
}

/// A digit set: text like "Win+Ctrl", or "" for off. A bad value keeps `default`.
fn mods(t: &Table, key: &str, default: Option<u8>, warn: &mut Vec<String>) -> Option<u8> {
    let text = match t.get(key) {
        None => return default,
        Some(Value::String(text)) => text,
        Some(v) => {
            warn.push(format!("shortcuts.{key}: expected text like \"Win+Ctrl\", got a {}, using {}", v.type_str(), mods_text(default)));
            return default;
        }
    };
    match parse_mods(text) {
        Ok(m) => m,
        Err(why) => {
            warn.push(format!("shortcuts.{key}: \"{text}\" {why}, using {}", mods_text(default)));
            default
        }
    }
}

/// Modifiers only, joined by `+`, any case. Shift alone would eat typed digits and symbols.
fn parse_mods(text: &str) -> Result<Option<u8>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut mods = 0u8;
    for part in text.split('+').map(str::trim) {
        let bit = mod_bit(part);
        if bit == 0 {
            return Err(format!("has \"{part}\", which is not Win, Ctrl, Alt or Shift"));
        }
        if mods & bit != 0 {
            return Err(format!("repeats {part}"));
        }
        mods |= bit;
    }
    if mods & (WIN | CTRL | ALT) == 0 {
        return Err("needs Win, Ctrl or Alt".into());
    }
    Ok(Some(mods))
}

pub fn mods_text(mods: Option<u8>) -> String {
    let Some(mods) = mods else { return "off".into() };
    let names = [(WIN, "Win"), (CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")];
    names.iter().filter(|(bit, _)| mods & bit != 0).map(|(_, name)| *name).collect::<Vec<_>>().join("+")
}

fn mod_bit(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "win" => WIN,
        "ctrl" => CTRL,
        "alt" => ALT,
        "shift" => SHIFT,
        _ => 0,
    }
}

/// "Win+Ctrl+Left": parts joined by `+`, any order, any case, at least one modifier.
fn parse_combo(text: &str) -> Result<Combo, String> {
    let (mut mods, mut vk) = (0u8, None);
    for part in text.split('+').map(str::trim) {
        let bit = mod_bit(part);
        if bit != 0 {
            if mods & bit != 0 {
                return Err(format!("repeats {part}"));
            }
            mods |= bit;
        } else if vk.is_some() {
            return Err("has more than one key".into());
        } else {
            vk = Some(vk_of(part).ok_or_else(|| format!("has \"{part}\", which is not a key des-k knows"))?);
        }
    }
    let vk = vk.ok_or("has no key")?;
    if mods == 0 {
        return Err("needs at least one of Win, Ctrl, Alt, Shift".into());
    }
    Ok(Combo { mods, vk })
}

const NAMED_KEYS: &[(&str, VIRTUAL_KEY)] = &[
    ("Left", VK_LEFT), ("Right", VK_RIGHT), ("Up", VK_UP), ("Down", VK_DOWN),
    ("Home", VK_HOME), ("End", VK_END), ("PageUp", VK_PRIOR), ("PageDown", VK_NEXT),
    ("Insert", VK_INSERT), ("Delete", VK_DELETE), ("Space", VK_SPACE), ("Enter", VK_RETURN),
    ("Tab", VK_TAB), ("Escape", VK_ESCAPE),
];

/// Named keys, A to Z, 0 to 9 (their vk is the ASCII code), F1 to F24.
fn vk_of(name: &str) -> Option<u16> {
    if let Some((_, vk)) = NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(vk.0);
    }
    let up = name.to_ascii_uppercase();
    match up.as_bytes() {
        [c @ (b'A'..=b'Z' | b'0'..=b'9')] => Some(*c as u16),
        [b'F', _, ..] => up[1..].parse::<u16>().ok().filter(|n| (1..=24).contains(n)).map(|n| VK_F1.0 + n - 1),
        _ => None,
    }
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
    /// Every setting as (name, value) in file order: the start log line, and the change log
    /// that compares two of these (spec 0012 AC-15).
    fn fields(&self) -> Vec<(&'static str, String)> {
        let list = |l: &[Combo]| format!("[{}]", l.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "));
        let word = |w: Option<&str>| w.unwrap_or("").to_string();
        let gestures = self.gestures.iter().map(|g| format!("{g}: {}", g.action)).collect::<Vec<_>>().join(", ");
        vec![
            ("workspaces", self.workspaces.to_string()),
            ("copilot_as_right_ctrl", self.copilot_as_right_ctrl.to_string()),
            ("follow_activation", self.follow_activation.to_string()),
            ("fastest", self.fastest.to_string()),
            ("shortcuts.previous", list(&self.previous)),
            ("shortcuts.next", list(&self.next)),
            ("shortcuts.send_previous", list(&self.send_previous)),
            ("shortcuts.send_next", list(&self.send_next)),
            ("shortcuts.send_previous_stay", list(&self.send_previous_stay)),
            ("shortcuts.send_next_stay", list(&self.send_next_stay)),
            ("shortcuts.jump", mods_text(self.jump).to_string()),
            ("shortcuts.send_to", mods_text(self.send_to).to_string()),
            ("shortcuts.send_to_stay", mods_text(self.send_to_stay).to_string()),
            ("shortcuts.finder", list(&self.finder)),
            ("shortcuts.overview", list(&self.overview)),
            ("shortcuts.pin", list(&self.pin)),
            ("indicator.enabled", self.indicator_enabled.to_string()),
            ("indicator.style", word(STYLES.iter().find(|(_, v)| *v == self.indicator_style).map(|(w, _)| *w))),
            ("indicator.hold_ms", self.hold_ms.to_string()),
            ("indicator.fade_ms", self.fade_ms.to_string()),
            ("restore.enabled", self.restore_enabled.to_string()),
            ("restore.minutes", self.restore_minutes.to_string()),
            ("overview.animation", word(ANIMATIONS.iter().find(|(_, v)| *v == self.overview_animation).map(|(w, _)| *w))),
            ("overview.labels", word(LABELS.iter().find(|(_, v)| *v == self.overview_labels).map(|(w, _)| *w))),
            ("overview.live", self.overview_live.to_string()),
            ("overview.fade_ms", self.overview_fade_ms.to_string()),
            ("touchpad.enabled", self.touchpad_enabled.to_string()),
            ("touchpad.swipe_mm", self.swipe_mm.to_string()),
            ("touchpad.twice_ms", self.twice_ms.to_string()),
            ("gestures", format!("[{gestures}]")),
        ]
    }
}

impl fmt::Display for Settings {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let fields: Vec<String> = self.fields().into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        write!(f, "{}", fields.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The file des-k writes for you and the values it falls back to are two copies of the
    // defaults. If they drift, a fresh install behaves differently from a broken file.
    #[test]
    fn default_file_is_the_defaults() {
        let (s, warn) = parse(DEFAULT_FILE);
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s, Settings::default());
        // parse() starts from the defaults, so also prove every key is spelled out in the file.
        let top: Table = DEFAULT_FILE.parse().unwrap();
        let (sc, ind, res) = (top["shortcuts"].as_table().unwrap(), top["indicator"].as_table().unwrap(), top["restore"].as_table().unwrap());
        assert!(TOP_KEYS.iter().all(|k| top.contains_key(*k)));
        assert!(SHORTCUT_KEYS.iter().all(|k| sc.contains_key(*k)));
        assert!(INDICATOR_KEYS.iter().all(|k| ind.contains_key(*k)));
        assert!(RESTORE_KEYS.iter().all(|k| res.contains_key(*k)));
        assert!(OVERVIEW_KEYS.iter().all(|k| top["overview"].as_table().unwrap().contains_key(*k)));
        assert!(TOUCHPAD_KEYS.iter().all(|k| top["touchpad"].as_table().unwrap().contains_key(*k)));
    }

    // "none" is how you get a spread with no text at all, so it must stick; a typo costs only
    // the labels, back to titles on hover.
    #[test]
    fn a_bad_overview_labels_only_resets_itself() {
        let (s, warn) = parse("[overview]\nlabels = \"None\"\n");
        assert_eq!(s.overview_labels, Labels::None);
        assert!(warn.is_empty(), "{warn:?}");
        let (s, warn) = parse("workspaces = 6\n[overview]\nlabels = \"sometimes\"\n");
        assert_eq!((s.workspaces, s.overview_labels), (6, Labels::Hover));
        assert_eq!(warn, ["overview.labels: \"sometimes\" is not one of \"hover\", \"always\", \"none\", using default \"hover\""]);
    }

    // live = false is how you stop the spread moving as you aim (AC-10), so it must stick; a
    // bad value costs only that key, back to live.
    #[test]
    fn a_bad_overview_live_only_resets_itself() {
        let (s, warn) = parse("[overview]\nlive = false\n");
        assert!(!s.overview_live);
        assert!(warn.is_empty(), "{warn:?}");
        let (s, warn) = parse("[overview]\nlabels = \"none\"\nlive = \"no\"\n");
        assert_eq!((s.overview_labels, s.overview_live), (Labels::None, true));
        assert_eq!(warn.len(), 1, "{warn:?}");
        assert!(warn[0].starts_with("overview.live:"), "{warn:?}");
    }

    // The indicator style and `fastest` are read like every key: a bad value resets only itself
    // (spec 0017), so a typo never turns the card into the dots or the fades off.
    #[test]
    fn style_and_fastest_reset_only_themselves() {
        let (s, warn) = parse("fastest = true\n[indicator]\nstyle = \"Dots\"\n");
        assert_eq!((s.fastest, s.indicator_style), (true, Style::Dots));
        assert!(warn.is_empty(), "{warn:?}");
        let (s, warn) = parse("fastest = \"yes\"\n[indicator]\nstyle = \"pill\"\nhold_ms = 900\n");
        assert_eq!((s.fastest, s.indicator_style, s.hold_ms), (false, Style::Card, 900));
        assert_eq!(warn.len(), 2, "{warn:?}");
        assert!(warn[1].starts_with("indicator.style: \"pill\" is not one of \"card\", \"dots\""), "{warn:?}");
    }

    // The overview's fade time is yours to shorten (0 is instant) but never past a second; a
    // bad value goes back to 250, and only it, so a typo never costs the animation itself
    // (spec 0017 AC-7).
    #[test]
    fn a_bad_overview_fade_time_only_resets_itself() {
        assert_eq!(parse("").0.overview_fade_ms, 250);
        let (s, warn) = parse("[overview]\nfade_ms = 0\n");
        assert_eq!(s.overview_fade_ms, 0);
        assert!(warn.is_empty(), "{warn:?}");
        let (s, warn) = parse("[overview]\nanimation = \"fade\"\nfade_ms = 1500\n");
        assert_eq!((s.overview_animation, s.overview_fade_ms), (Animation::Fade, 250));
        assert_eq!(warn.len(), 1, "{warn:?}");
        assert!(warn[0].starts_with("overview.fade_ms:"), "{warn:?}");
    }

    // Off must stay the default (the overview opens at once unless you ask), a chosen style
    // must stick, and a typo, or the "zoom" an older file may still hold, costs only the
    // animation, never the keys next to it (AC-11, AC-14).
    #[test]
    fn a_bad_overview_animation_only_resets_itself() {
        assert_eq!(parse("").0.overview_animation, Animation::Off);
        let (s, warn) = parse("[overview]\nanimation = \"Fade\"\n");
        assert_eq!(s.overview_animation, Animation::Fade);
        assert!(warn.is_empty(), "{warn:?}");
        let (s, warn) = parse("[overview]\nanimation = \"zoom\"\nlive = false\n");
        assert_eq!((s.overview_animation, s.overview_live), (Animation::Off, false));
        assert_eq!(warn, ["overview.animation: \"zoom\" is not one of \"off\", \"fade\", using default \"off\""]);
    }

    // minutes = 0 would end the restore before any app reopens; it must not also switch restore off.
    #[test]
    fn a_bad_restore_minutes_only_resets_itself() {
        let (s, warn) = parse("[restore]\nenabled = false\nminutes = 0\n");
        assert!(!s.restore_enabled);
        assert_eq!(s.restore_minutes, 3);
        assert_eq!(warn.len(), 1, "{warn:?}");
    }

    // The whole point of reading key by key: a typo costs one setting, never the rest.
    #[test]
    fn a_bad_value_only_resets_itself() {
        let text = "workspaces = 12\nfollow_activation = \"yes\"\ntypo = 1\n[shortcuts]\nprevious = [\"alt+CTRL+left\", \"Ctrl+Banana\", 7]\n[indicator]\nhold_ms = \"long\"\nfade_ms = 0\n";
        let (s, warn) = parse(text);
        assert_eq!(s.workspaces, 4);
        // A typo must not silently switch follow off: on is the safe side, typing never goes invisible.
        assert!(s.follow_activation);
        assert_eq!(s.previous, [Combo { mods: ALT | CTRL, vk: VK_LEFT.0 }]);
        assert_eq!(s.hold_ms, 550);
        assert_eq!(s.fade_ms, 0);
        assert_eq!(warn.len(), 6, "{warn:?}"); // workspaces, follow_activation, typo, Banana, 7, hold_ms
    }

    // follow_activation = false is the escape from an app that keeps stealing focus, so it must stick.
    #[test]
    fn follow_activation_can_be_turned_off() {
        let (s, warn) = parse("follow_activation = false\n");
        assert!(!s.follow_activation);
        assert!(warn.is_empty(), "{warn:?}");
    }

    // One combo in both lists would make a key do two things; neither list can be trusted.
    #[test]
    fn a_clash_resets_both_lists() {
        let (s, warn) = parse("[shortcuts]\nprevious = [\"Win+Ctrl+Right\"]\n");
        assert_eq!((s.previous, s.next), (Settings::default().previous, Settings::default().next));
        assert_eq!(warn.len(), 1, "{warn:?}");
    }

    // Win+Ctrl+Left/Right and the swipe must keep switching, whatever the send lists say.
    #[test]
    fn a_send_list_clashing_with_a_switch_list_is_turned_off() {
        let (s, warn) = parse("[shortcuts]
send_next_stay = [\"Win+Ctrl+Right\"]
");
        let d = Settings::default();
        assert!(s.send_next_stay.is_empty());
        assert_eq!((&s.previous, &s.next), (&d.previous, &d.next));
        assert_eq!((&s.send_previous, &s.send_next, &s.send_previous_stay), (&d.send_previous, &d.send_next, &d.send_previous_stay));
        assert_eq!(warn, ["shortcuts: Win+Ctrl+Right is in both next and send_next_stay, send_next_stay turned off"]);
    }

    // Moving a switch shortcut onto a default send combo must not make one key do two things.
    #[test]
    fn a_switch_list_beats_a_default_send_list() {
        let (s, warn) = parse("[shortcuts]
previous = [\"Win+Ctrl+Shift+Left\"]
");
        assert_eq!(s.previous, [Combo { mods: WIN | CTRL | SHIFT, vk: VK_LEFT.0 }]);
        assert!(s.send_previous.is_empty());
        assert_eq!(warn.len(), 1, "{warn:?}");
    }

    // A stay combo that is also a follow combo would send and switch at once; the earlier list keeps it.
    #[test]
    fn a_stay_list_clashing_with_a_follow_list_is_turned_off() {
        let (s, warn) = parse("[shortcuts]
send_previous_stay = [\"Win+Ctrl+Shift+Right\"]
");
        assert_eq!(s.send_next, Settings::default().send_next);
        assert!(s.send_previous_stay.is_empty());
        assert_eq!(warn.len(), 1, "{warn:?}");
    }

    // A typo in one send list costs that list only, back to its default.
    #[test]
    fn a_bad_send_entry_only_resets_its_list() {
        let (s, warn) = parse("[shortcuts]
send_next = [\"Win+Banana\"]
send_previous = [\"Win+Alt+N\"]
");
        let d = Settings::default();
        assert_eq!(s.send_next, d.send_next);
        assert_eq!(s.send_previous, [Combo { mods: WIN | ALT, vk: b'N' as u16 }]);
        assert_eq!((&s.send_previous_stay, &s.send_next_stay), (&d.send_previous_stay, &d.send_next_stay));
        assert_eq!(warn.len(), 2, "{warn:?}"); // Banana dropped, then no valid entry left
    }

    // "" is how you give Win+N back to the taskbar, so it must turn only that set off.
    #[test]
    fn an_empty_digit_set_is_off() {
        let (s, warn) = parse("[shortcuts]\njump = \"\"\n");
        let d = Settings::default();
        assert_eq!(s.jump, None);
        assert_eq!((s.send_to, s.send_to_stay), (d.send_to, d.send_to_stay));
        assert!(warn.is_empty(), "{warn:?}");
    }

    // Shift alone would swallow every typed ! @ #, so it is refused; a typo costs only its own key.
    #[test]
    fn a_bad_digit_set_only_resets_itself() {
        let (s, warn) = parse("[shortcuts]\njump = \"Shift\"\nsend_to = \"Win+Hyper\"\nsend_to_stay = \"alt+WIN\"\n");
        let d = Settings::default();
        assert_eq!((s.jump, s.send_to), (d.jump, d.send_to));
        assert_eq!(s.send_to_stay, Some(WIN | ALT));
        assert_eq!(warn.len(), 2, "{warn:?}");
    }

    // One modifier set for two actions would make a digit both jump and send; the earlier set keeps it.
    #[test]
    fn a_digit_set_clashing_with_an_earlier_one_is_turned_off() {
        let (s, warn) = parse("[shortcuts]\nsend_to_stay = \"win\"\n");
        assert_eq!(s.jump, Some(WIN));
        assert_eq!(s.send_to_stay, None);
        assert_eq!(warn, ["shortcuts: Win is in both jump and send_to_stay, send_to_stay turned off"]);
    }

    // An explicit list on one digit must not cost the set its other eight digits. It is a
    // setting the page makes on purpose (Move it here), so it is not logged as a problem.
    #[test]
    fn a_list_on_a_digit_only_takes_that_digit() {
        let (s, warn) = parse("[shortcuts]\nnext = [\"Win+Ctrl+2\"]\n");
        assert_eq!(s.next, [Combo { mods: WIN | CTRL, vk: b'2' as u16 }]);
        assert_eq!(s.send_to_stay, Some(WIN | CTRL));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s.skips("send_to_stay"), [('2', "next")]);
    }

    // Removing the last combo on the page writes [], and that must turn the shortcut off, not
    // bring the default back. A list whose every entry is bad still falls back (spec 0001 AC-6).
    #[test]
    fn an_empty_list_is_off() {
        let (s, warn) = parse("[shortcuts]\nfinder = []\nprevious = []\n");
        assert!(s.finder.is_empty() && s.previous.is_empty());
        assert_eq!(s.next, Settings::default().next);
        assert!(warn.is_empty(), "{warn:?}");
    }

    // Win+Ctrl+Right must keep switching (and the swipe with it) even if the finder is set onto it.
    #[test]
    fn a_finder_combo_clashing_with_a_switch_list_turns_the_finder_off() {
        let (s, warn) = parse("[shortcuts]
finder = [\"Win+Ctrl+Right\"]
");
        assert_eq!(s.next, Settings::default().next);
        assert!(s.finder.is_empty());
        assert_eq!(warn, ["shortcuts: Win+Ctrl+Right is in both next and finder, finder turned off"]);
    }

    // The hook tries the lists first, so a finder on Win+3 really does take 3 from jump; the log must say so.
    #[test]
    fn a_finder_on_a_digit_takes_that_digit_from_its_set() {
        let (s, warn) = parse("[shortcuts]
finder = [\"Win+3\"]
");
        assert_eq!(s.finder, [Combo { mods: WIN, vk: b'3' as u16 }]);
        assert_eq!(s.jump, Some(WIN));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s.skips("jump"), [('3', "finder")]);
    }

    // Win+F must keep opening the finder if the overview is set onto it: the earlier list keeps
    // the combo, so one key never opens both.
    #[test]
    fn an_overview_combo_clashing_with_the_finder_turns_the_overview_off() {
        let (s, warn) = parse("[shortcuts]
overview = [\"Win+F\"]
");
        assert_eq!(s.finder, Settings::default().finder);
        assert!(s.overview.is_empty());
        assert_eq!(warn, ["shortcuts: Win+F is in both finder and overview, overview turned off"]);
    }

    // The gestures you get without writing a rule are spec 0011 AC-2, spelled out here on
    // purpose, apart from default_gestures(), so a slip in that table fails this test.
    #[test]
    fn the_default_gestures_are_ac_2() {
        let (s, warn) = parse("");
        assert!(warn.is_empty(), "{warn:?}");
        let got: Vec<String> = s.gestures.iter().map(|g| format!("{g}: {}", g.action)).collect();
        assert_eq!(
            got,
            [
                "4 left: next", "4 right: previous", "4 up: overview", "4 down: show_desktop", "3 left: app_next",
                "3 right: app_previous", "3 up: overview", "3 down: show_desktop", "3 tap: search", "4 tap: notifications",
            ]
        );
    }

    // Writing one rule means you chose your own map: none of the defaults may still act.
    #[test]
    fn a_written_rule_replaces_the_whole_default_map() {
        let (s, warn) = parse("[[gesture]]\nfingers = 3\ntap = true\ndo = \"Play_Pause\"\n");
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s.gestures, [Gesture { fingers: 3, motion: Motion::Tap, times: 1, action: action("play_pause").unwrap() }]);
        assert_eq!(s.rules(4, Motion::Left), (None, None));
    }

    // AC-4: one typo must never cost the other gestures; each bad rule says why it was skipped.
    #[test]
    fn a_bad_rule_only_skips_itself() {
        let text = r#"
[[gesture]]
fingers = 5
swipe = "left"
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
do = "explode"
[[gesture]]
fingers = 4
swipe = "left"
tap = true
do = "next"
[[gesture]]
fingers = 4
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
times = 3
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
do = "next"
speed = 2
[[gesture]]
fingers = 3
tap = false
do = "search"
[[gesture]]
fingers = 4
swipe = "sideways"
do = "next"
[[gesture]]
fingers = 4
swipe = "right"
do = "previous"
"#;
        let (s, warn) = parse(text);
        assert_eq!(s.gestures, [Gesture { fingers: 4, motion: Motion::Right, times: 1, action: Action::Switch(-1) }]);
        assert_eq!(warn.len(), 8, "{warn:?}");
        for (i, w) in warn.iter().enumerate() {
            assert!(w.starts_with(&format!("gesture rule {}: ", i + 1)) && w.ends_with(", skipped"), "{w}");
        }
        assert!(warn[1].contains("\"explode\""), "{}", warn[1]);
        assert!(warn[5].contains("speed"), "{}", warn[5]);
    }

    // Two rules for one gesture would make it do two things; the first one keeps it.
    #[test]
    fn a_repeated_gesture_keeps_the_first_rule() {
        let (s, warn) = parse("[[gesture]]\nfingers = 4\nswipe = \"up\"\ndo = \"finder\"\n[[gesture]]\nfingers = 4\nswipe = \"UP\"\ndo = \"overview\"\n");
        assert_eq!(s.gestures.len(), 1);
        assert_eq!(s.gestures[0].action, Action::Finder);
        assert_eq!(warn, ["gesture rule 2: 4 up is already set by an earlier rule, skipped"]);
    }

    // A combo is how you reach any key Windows or an app has; an action name always wins over
    // one, so "next" can never be read as something else.
    #[test]
    fn do_takes_a_name_or_a_combo() {
        assert_eq!(action("NEXT"), Ok(Action::Switch(1)));
        assert_eq!(action("send_to_stay_3"), Ok(Action::SendTo(2, false)));
        assert_eq!(action("send_to_9"), Ok(Action::SendTo(8, true)));
        assert_eq!(action("jump_1"), Ok(Action::Jump(0)));
        assert_eq!(action("Win+Ctrl+Right"), Ok(Action::Combo(Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 })));
        for bad in ["jump_0", "jump_10", "Left", "explode"] {
            assert!(action(bad).is_err(), "{bad}");
        }
        // Every name prints back as itself, so the log shows what you wrote.
        for (name, a) in ACTIONS {
            assert_eq!(a.to_string(), *name);
        }
        // The welcome screen shows these words; a name with no label would show raw there.
        for (name, _) in ACTIONS {
            assert!(ACTION_LABELS.iter().any(|(n, _)| n == name), "{name} has no label");
        }
        assert_eq!(Action::SendTo(2, false).label(), "send window to 3, stay");
        assert_eq!(Action::Combo(Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 }).label(), "Win+Ctrl+Right");
        let (_, warn) = parse("[[gesture]]\nfingers = 4\ntap = true\ndo = \"Win+L\"\n");
        assert_eq!(warn, ["gesture rule 1: Windows never lets a program type Win+L, so it does nothing"]);
    }

    // AC-5: the reader must find both rules of a gesture, so it knows to wait for the second go.
    #[test]
    fn a_twice_rule_is_kept_beside_the_once_rule() {
        let (s, warn) = parse("[[gesture]]\nfingers = 4\nswipe = \"left\"\ndo = \"next\"\n[[gesture]]\nfingers = 4\nswipe = \"left\"\ntimes = 2\ndo = \"jump_1\"\n[[gesture]]\nfingers = 3\ntap = true\ntimes = 2\ndo = \"finder\"\n");
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s.rules(4, Motion::Left), (Some(0), Some(1)));
        assert_eq!(s.rules(3, Motion::Tap), (None, Some(2)));
        assert_eq!(s.rules(4, Motion::Right), (None, None));
    }

    // A bad [touchpad] value costs that key only; a gesture list that is not [[gesture]] blocks
    // falls back to the default map, never to no gestures at all.
    #[test]
    fn a_bad_touchpad_value_only_resets_itself() {
        let (s, warn) = parse("gesture = 4\n[touchpad]\nenabled = true\nswipe_mm = 60\ntwice_ms = 500\n");
        let d = Settings::default();
        assert!(s.touchpad_enabled);
        assert_eq!((s.swipe_mm, s.twice_ms), (15, 500));
        assert_eq!(s.gestures, d.gestures);
        assert_eq!(warn.len(), 2, "{warn:?}");
    }

    #[test]
    fn combo_text_round_trips() {
        for text in ["Win+Ctrl+Left", "Ctrl+Shift+A", "Alt+F24", "Win+7", "Ctrl+PageDown"] {
            assert_eq!(parse_combo(text).unwrap().to_string(), text);
        }
        for bad in ["Left", "Win+Win+Left", "Win+A+B", "Win+", "Win+F25", "Win+Ctrl"] {
            assert!(parse_combo(bad).is_err(), "{bad}");
        }
    }

    // The page and your hand edits share one file, so a click must change only its value and
    // keep every comment, both yours and the ones that explain each setting (spec 0012 AC-3).
    #[test]
    fn a_page_edit_changes_only_that_value() {
        let edit = |name, value| edit_text(DEFAULT_FILE, &Edit::Value(name, value)).unwrap();
        let off = edit("indicator.enabled", toml_edit::Value::from(false));
        assert_eq!(off, DEFAULT_FILE.replacen("[indicator]\nenabled = true", "[indicator]\nenabled = false", 1));
        assert!(!parse(&off).0.indicator_enabled);
        let hold = edit("indicator.hold_ms", toml_edit::Value::from(600));
        assert_eq!(hold, DEFAULT_FILE.replacen("hold_ms = 550               # 0 to 5000", "hold_ms = 600               # 0 to 5000", 1));
        let top = edit("workspaces", toml_edit::Value::from(6));
        assert_eq!(top, DEFAULT_FILE.replacen("workspaces = 4              # 2 to 9", "workspaces = 6              # 2 to 9", 1));
        // A key the file left out is added, and nothing else moves.
        let added = edit_text("[indicator]\nenabled = true # mine\n", &Edit::Value("indicator.hold_ms", toml_edit::Value::from(100))).unwrap();
        assert_eq!(added, "[indicator]\nenabled = true # mine\nhold_ms = 100\n");
    }

    // A file you are halfway through typing must not become all defaults on a live reload:
    // that would remap every shortcut mid edit (AC-14). At start, defaults stay the fallback.
    #[test]
    fn a_broken_file_is_an_error_on_reload_not_defaults() {
        let why = reparse("workspaces = \n").unwrap_err();
        assert!(why.starts_with("not valid TOML (") && why.contains("line 1"), "{why}");
        assert!(edit_text("workspaces = \n", &Edit::Value("workspaces", toml_edit::Value::from(3))).is_err());
        let (s, warn) = parse("workspaces = \n");
        assert_eq!((s, warn.len()), (Settings::default(), 1));
        assert_eq!(reparse("workspaces = 6\n").unwrap().0.workspaces, 6);
    }

    // The page finds a setting's note by its key, so the key must come from the log words.
    #[test]
    fn a_problem_knows_its_setting() {
        let (_, warn) = parse("typo = 1\n[indicator]\nhold_ms = \"x\"\n");
        let keys: Vec<String> = warn.into_iter().map(|w| Problem::new(w).key).collect();
        assert_eq!(keys, ["", "indicator.hold_ms"]);
    }

    // A recorded combo replaces only its own line, keeping the column the file lined up.
    #[test]
    fn a_combo_edit_changes_only_that_list() {
        let c = |mods, vk: u8| Combo { mods, vk: vk as u16 };
        let text = edit_text(DEFAULT_FILE, &Edit::Combos("shortcuts.finder", vec![c(WIN, b'F'), c(WIN | ALT, b'G')])).unwrap();
        assert_eq!(text, DEFAULT_FILE.replacen("finder = [\"Win+F\"]", "finder = [\"Win+F\", \"Win+Alt+G\"]", 1));
        assert_eq!(parse(&text).0.finder, [c(WIN, b'F'), c(WIN | ALT, b'G')]);
        let off = edit_text(DEFAULT_FILE, &Edit::Digits("shortcuts.jump", None)).unwrap();
        assert_eq!(off, DEFAULT_FILE.replacen("jump         = \"Win\"", "jump         = \"\"", 1));
        let on = edit_text(&off, &Edit::Digits("shortcuts.jump", Some(WIN | ALT))).unwrap();
        assert_eq!(parse(&on).0.jump, Some(WIN | ALT));
    }

    // Move it here saves both entries in one write, so the file never holds the clash.
    #[test]
    fn a_pair_writes_both_entries() {
        let next = Combo { mods: WIN, vk: b'F' as u16 };
        let pair = Edit::Pair(Box::new(Edit::Combos("shortcuts.next", vec![next])), Box::new(Edit::Combos("shortcuts.finder", vec![])));
        let (s, warn) = parse(&edit_text(DEFAULT_FILE, &pair).unwrap());
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!((s.next, s.finder), (vec![next], vec![]));
    }

    // The page rewrites every block from its 10 rows. Written over the default file with the
    // default map, nothing may change, so the comment above the blocks and their layout stay.
    #[test]
    fn the_gesture_rewrite_keeps_the_file() {
        let d = Settings::default();
        let map = full_map(|f, m, t| d.rule(f, m, t));
        assert_eq!(map, d.gestures);
        assert_eq!(edit_text(DEFAULT_FILE, &Edit::Gestures(map)).unwrap(), DEFAULT_FILE);
    }

    // A twice rule and a key combo survive the rewrite, and a row the file had no rule for is
    // written as "none" (AC-12).
    #[test]
    fn the_gesture_rewrite_parses_back() {
        let (hand, _) = parse("[[gesture]]\nfingers = 3\ntap = true\ndo = \"search\"\n");
        let combo = Action::Combo(Combo { mods: CTRL | SHIFT, vk: b'T' as u16 });
        let map = full_map(|f, m, t| match (f, m, t) {
            (4, Motion::Left, 2) => Some(combo),
            _ => hand.rule(f, m, t),
        });
        let text = edit_text("# mine\nworkspaces = 3\n", &Edit::Gestures(map.clone())).unwrap();
        assert!(text.starts_with("# mine\nworkspaces = 3\n"), "{text}");
        let (s, warn) = parse(&text);
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(s.gestures, map);
        assert_eq!(s.rule(3, Motion::Tap, 1), Some(action("search").unwrap()));
        assert_eq!(s.rule(4, Motion::Left, 1), Some(Action::Nothing));
        assert_eq!(s.rule(4, Motion::Left, 2), Some(combo));
        assert_eq!(s.gestures.len(), 11);
    }

    // The page's clash scan must agree with the hook: a list beats a set, a set only loses
    // the one digit, and two sets with the same keys would make a digit do two things.
    #[test]
    fn clashes_are_found_like_the_hook_finds_combos() {
        let d = Settings::default();
        let c = |mods, vk: u8| Combo { mods, vk: vk as u16 };
        assert_eq!(d.combo_owner("next", c(WIN, b'F')), Some(Owner::List("finder")));
        assert_eq!(d.combo_owner("finder", c(WIN, b'F')), None); // its own
        assert_eq!(d.combo_owner("finder", c(WIN, b'3')), Some(Owner::Set("jump")));
        assert_eq!(d.combo_owner("finder", c(WIN, b'0')), None); // no workspace 0
        assert_eq!(d.combo_owner("finder", c(WIN | ALT, b'3')), None);
        assert_eq!(d.set_owner("send_to_stay", WIN), Some("jump"));
        assert_eq!(d.set_owner("jump", WIN), None);
        assert_eq!(d.set_owner("jump", WIN | ALT), None);
        assert!(d.skips("jump").is_empty());
    }

    // The recorder refuses keys the file can't name, so every key it takes must round trip.
    #[test]
    fn every_savable_key_round_trips() {
        for vk in (0..=255u16).filter(|vk| savable(*vk)) {
            let combo = Combo { mods: CTRL, vk };
            assert_eq!(parse_combo(&combo.to_string()), Ok(combo), "vk {vk:#x}");
        }
        assert!(!savable(VK_NUMPAD3.0) && !savable(VK_OEM_1.0) && !savable(VK_VOLUME_UP.0) && !savable(VK_LWIN.0));
    }

    // The change log compares these one by one, so each must name its setting once.
    #[test]
    fn every_setting_is_logged_by_name() {
        let names: Vec<&str> = Settings::default().fields().into_iter().map(|(k, _)| k).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!((names.len(), unique.len()), (30, 30));
    }
}
