// The first run (spec 0017, 0017-first-run.md; replaces spec 0011's welcome screen where they
// differ): five steps in one window, Try it, Shortcuts, Behavior, Touchpad and Done, where every
// setting it shows changes on the spot through settings::write and is in effect at once. Drawn
// to the approved design (pdesk-design restart/first-run-r11.html, with the More rows of
// first-run-r15.html): Windows' title bar, a thin progress line, the step, and a footer band.
// One drawn window with its own focus list and no child controls; the gesture picker's menu is
// drawn in it too. Tab visits every control, so a stepper's − and + are two stops. Lives on the
// main thread: its keys reach wndproc through main's loop. Sizes are in dip, scaled to the
// screen's dpi when drawn. It keeps no copy of a value: it draws what settings::get() holds.
use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
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

use crate::settings::{self, ALT, Action, CTRL, Combo, Edit, Motion, Owner, SHIFT, Settings, WIN};
use crate::settings_page::{self as sp, Does, Kind, Menu, Plan, Prompt, WinKey};
use crate::setup::{self, GaveBack, Row as PadRow, State as PadState, Wrote};
use crate::ui::{self, Bx, Canvas, F, Fonts, Theme, buffered, dark_title, mix, op, theme};
use crate::uia::{self, Node, Tree};

const CLASS: PCWSTR = w!("des-k-welcome");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);
const WHO: &str = "des-k first run";

// Layout, in dip. Two client heights only (600 and 680 with Windows' title bar), so Next moves
// the window's bottom edge at most once.
const W: f32 = 640.0;
const SHORT: f32 = 568.0;
const TALL: f32 = 648.0;
const GROW_MS: u32 = 150;
const GROW_TIMER: usize = 1;
const X0: f32 = 32.0; // the step's left edge
const FOOT: f32 = 68.0; // the footer band
const ROW: f32 = 52.0; // a row's least height
const KEY_W: f32 = 160.0; // a shortcut row's key column

const I_OK: &str = "\u{E73E}";
const I_WARN: &str = "\u{E7BA}";
const I_INFO: &str = "\u{E946}";
const I_KEY: &str = "\u{E765}";
const I_ADD: &str = "\u{E710}";
const I_MINUS: &str = "\u{E738}";
const I_PEN: &str = "\u{E70F}";
const I_DOWN: &str = "\u{E70D}";
const I_UP: &str = "\u{E70E}";
const I_MORE: &str = "\u{E76C}";
const I_SETUP: &str = "\u{E713}";

thread_local! {
    static FIRST: RefCell<Option<Win>> = const { RefCell::new(None) };
    /// The last give back's result: kept until des-k exits or a later give back replaces it; a
    /// give back that worked also goes once Windows' gestures read off again (AC-FR-5).
    static LAST: Cell<Option<GaveBack>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Try,
    Keys,
    Behavior,
    Pad,
    Done,
}

impl Step {
    fn title(self) -> &'static str {
        match self {
            Step::Try => "Every screen gets its own workspaces",
            Step::Keys => "Shortcuts",
            Step::Behavior => "Behavior",
            Step::Pad => "Touchpad",
            Step::Done => "des-k is ready",
        }
    }

    fn height(self) -> f32 {
        if matches!(self, Step::Try | Step::Behavior) { SHORT } else { TALL }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What step 1 asks you to try; main tells the first run when you did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Try {
    Switch,
    Overview,
    Find,
}

/// Step 1's rows: what to try, the key it shows, what to do, in the design's words.
const TRIES: [(&str, &str, &str); 3] = [
    ("Switch workspace", "shortcuts.next", "Move your mouse to a screen, then press the keys."),
    ("Open overview", "shortcuts.overview", "Every workspace of every screen at once. Esc closes it."),
    ("Find a window", "shortcuts.finder", "Type part of a window's name, then Enter goes to it."),
];

/// A row of step 2: the des-k action that takes a Windows key, the [shortcuts] keys behind it
/// (two for a ← → pair, none for the Copilot key), Windows' key, and what Windows does with it.
struct Take {
    keys: &'static [&'static str],
    title: &'static str,
    win: &'static str,
    does: &'static str,
}

const TAKES: [Take; 7] = [
    Take { keys: &["shortcuts.previous", "shortcuts.next"], title: "Switch workspace", win: "Win Ctrl ← →", does: "switches desktops" },
    Take { keys: &["shortcuts.overview"], title: "Open overview", win: "Win Tab", does: "opens Task View" },
    Take { keys: &["shortcuts.finder"], title: "Find a window", win: "Win F", does: "opens Feedback Hub" },
    Take { keys: &["shortcuts.jump"], title: "Go to workspace 1–9", win: "Win 1–9", does: "opens taskbar app 1–9" },
    Take { keys: &["shortcuts.send_to"], title: "Send window to 1–9", win: "Win Shift 1–9", does: "new window of taskbar app 1–9" },
    Take { keys: &["shortcuts.send_to_stay"], title: "Send window to 1–9 and stay", win: "Win Ctrl 1–9", does: "last window of taskbar app 1–9" },
    Take { keys: &[], title: "Copilot key as Right Ctrl", win: "Copilot", does: "Copilot, or what you set" },
];

/// Step 2's More rows: keys des-k has that Windows doesn't.
const MORE_KEYS: [(&str, &[&str]); 3] = [
    ("Send window to next or previous", &["shortcuts.send_previous", "shortcuts.send_next"]),
    ("Send window to next or previous and stay", &["shortcuts.send_previous_stay", "shortcuts.send_next_stay"]),
    ("Pin window to all workspaces", &["shortcuts.pin"]),
];

/// What the recorder is recording for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rec {
    List(&'static str, Option<usize>), // a list, and the key it replaces (None: a new one)
    Digits(&'static str),
    Pair(&'static str, &'static str), // ← and → together (AC-FR-4)
    Gesture(usize, u8),               // a GESTURE_ROWS row and its times: Key combo…
}

/// A key field you click to record: a step 2 row, a More row, or the hints key on Done.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Field {
    Take(usize),
    More(usize),
    Hints,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pad {
    Windows, // Set up in, or Open, Windows Settings
    ReadOn,
    GiveBack, // Give back, or Try again
}

/// Something you can click, focus from the keyboard, or both.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Next,
    Back,
    Skip,
    Field(Field),
    Side(usize),         // focus only: a step 2 row's des-k or Windows, arrows move it
    SideAt(usize, bool), // des-k (true) or Windows, clicked
    More,
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Pick(&'static str), // focus only: a choice, arrows move it
    Choice(&'static str, usize),
    Picker(usize),
    Item(usize),
    Pad(Pad),
    Show, // Shortcut hints
    Cancel,
    Move,
}

impl Ctl {
    /// The keyboard stop this control belongs to.
    fn stop(self) -> Ctl {
        match self {
            Ctl::Choice(k, _) => Ctl::Pick(k),
            Ctl::SideAt(i, _) => Ctl::Side(i),
            c => c,
        }
    }

    /// Enter presses it; on anything else Enter presses Next (AC-FR-6).
    fn button(self) -> bool {
        !matches!(self, Ctl::Toggle(_) | Ctl::Pick(_) | Ctl::Side(_))
    }
}

/// Where a control sits, in window dip. Group: 0 above the step, 1 the step (clipped to its
/// view), 2 the footer, 3 the open menu.
#[derive(Clone, Copy)]
struct Hit {
    ctl: Ctl,
    at: Bx,
    group: u8,
}

/// What the screens picture and the touchpad step draw, copied out of ws and Windows.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::ScreenInfo>,
    view: crate::ws::View,
    pad: Option<PadState>,
    mouse: String, // the device under the mouse
}

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    step: Step,
    more: [bool; 5], // per step: More open
    tried: [bool; 3],
    switched: Option<(String, usize, bool)>, // the screen that switched, to which workspace, the only screen
    scroll: f32,
    to_end: bool, // the next layout scrolls to the end (More just opened)
    content: f32,
    mid: (f32, f32), // the step's view: its top and bottom
    hits: Vec<Hit>,
    focus: Option<Ctl>,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    vw: f32,
    vh: f32,
    cap: f32,                              // the most client height the screen has room for
    grow: Option<(Instant, f32, f32, u32)>, // started, from, to, ms
    rec: Option<Rec>,
    refused: Option<(String, String)>,
    prompt: Option<Prompt>,
    menu: Option<Menu>,
    error: Option<(String, String)>, // the last save that failed: what, and Windows' words
    pics: Pics,
    tree: Tree,
}

// Never hold the borrow across a call that comes back here: settings::write reloads, and the
// reload calls refresh().
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&FIRST, "first run get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&FIRST, "first run set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

/// The steps shown: no Touchpad step without a precision touchpad (AC-FR-5).
fn steps(w: &Win) -> Vec<Step> {
    let pad = w.pics.pad.is_none_or(|p| p.pad);
    [Step::Try, Step::Keys, Step::Behavior, Step::Pad, Step::Done].into_iter().filter(|s| pad || *s != Step::Pad).collect()
}

// ---- Keys ----

/// An action's keys as words: "Win Ctrl →".
fn words(caps: &[String]) -> String {
    sp::spelled(caps)
}

/// The key a row's field shows: Windows' own first. A ← → pair shows as one key when both
/// sides hold the same modifiers.
fn row_caps(s: &Settings, keys: &[&'static str]) -> Option<Vec<String>> {
    let first = |k: &str| {
        let sh = sp::shortcut(k)?;
        let i = *sp::drawn_order(s, sh).first()?;
        sp::keys_of(s, k).get(i).cloned()
    };
    match keys {
        [k] => first(k),
        [p, n] => match (first(p), first(n)) {
            (Some(a), Some(z)) if a.last().map(String::as_str) == Some("Left") && z.last().map(String::as_str) == Some("Right") && a[..a.len() - 1] == z[..z.len() - 1] => {
                let mut caps = a[..a.len() - 1].to_vec();
                caps.extend(["Left".to_string(), "Right".to_string()]);
                Some(caps)
            }
            (a, z) => a.or(z),
        },
        _ => None,
    }
}

/// Where a step 2 row's Windows key is now.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Held {
    Desk,                       // des-k has it, for this action
    Own(Option<&'static str>),  // given back, and the action has your own key (taken from that action)
    Bare,                       // given back, no key
    Took(&'static str),         // another des-k action holds it
}

fn held(s: &Settings, keys: &[&'static str]) -> Held {
    let shs: Vec<&sp::Short> = keys.iter().filter_map(|k| sp::shortcut(k)).collect();
    if !shs.is_empty() && shs.iter().all(|sh| sp::has_win(s, sh)) {
        return Held::Desk;
    }
    if let Some(o) = shs.iter().find_map(|sh| sp::took(s, sh)) {
        return Held::Took(o);
    }
    if row_caps(s, keys).is_none() {
        return Held::Bare;
    }
    // Your key is another action's Windows key: say which.
    let from = keys.first().and_then(|k| {
        let mine = sp::shortcut(k)?;
        sp::MOVE.iter().chain(sp::SEND).filter(|o| o.key != mine.key).find_map(|o| match o.win?.0 {
            WinKey::Digits(m) if s.set(sp::name_of(mine.key)) == Some(m) => Some(o.title),
            WinKey::Combo(c) if s.list(sp::name_of(mine.key)).first() == Some(&c) => Some(o.title),
            _ => None,
        })
    });
    Held::Own(from)
}

/// A row's caption: whose key it is now and what Windows does with it.
fn take_caption(s: &Settings, i: usize) -> String {
    let t = &TAKES[i];
    if t.keys.is_empty() {
        return if s.copilot_as_right_ctrl { format!("Windows: {}", t.does) } else { "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl.".into() };
    }
    match held(s, t.keys) {
        Held::Desk => format!("Windows: {}", t.does),
        Held::Bare => format!("No key. Windows keeps {}: {}.", t.win, t.does),
        Held::Own(None) => format!("Your key. Windows keeps {}: {}.", t.win, t.does),
        Held::Own(Some(o)) => format!("Your key, taken from {o}. Windows keeps {}: {}.", t.win, t.does),
        Held::Took(o) => {
            let does = o.strip_prefix("Go to ").map_or_else(|| format!("does “{o}”"), |r| format!("goes to {r}"));
            let mine = if row_caps(s, t.keys).is_some() { "Your key" } else { "No key" };
            format!("{mine}. {} now {does}.", t.win)
        }
    }
}

/// Both sides of a ← → pair get the held modifiers; another action that had either arrow
/// loses it, after you say so (AC-FR-4). `replace`: the row's shown key, else a new one.
fn plan_pair(p: &'static str, n: &'static str, mods: u8, replace: bool, title: &'static str) -> Plan {
    let s = settings::get();
    let mut edits = vec![];
    let mut clash: Option<&'static str> = None;
    let mut same = true;
    for (key, vk) in [(p, VK_LEFT.0), (n, VK_RIGHT.0)] {
        let name = sp::name_of(key);
        let combo = Combo { mods, vk };
        let mut mine = s.list(name).to_vec();
        let at = if replace { sp::shortcut(key).and_then(|sh| sp::drawn_order(s, sh).first().copied()) } else { None };
        match at {
            Some(i) if i < mine.len() => mine[i] = combo,
            _ => mine.push(combo),
        }
        let mut seen = vec![];
        mine.retain(|c| !seen.contains(c) && {
            seen.push(*c);
            true
        });
        same &= mine == s.list(name);
        edits.push(Edit::Combos(key, mine));
        if let Some(Owner::List(other)) = s.combo_owner(name, combo)
            && other != sp::name_of(p)
            && other != sp::name_of(n)
        {
            let (other_key, other_title) = sp::entry(other);
            let rest: Vec<Combo> = s.list(other).iter().filter(|c| **c != combo).copied().collect();
            edits.push(Edit::Combos(other_key, rest));
            clash.get_or_insert(other_title);
        }
    }
    if same {
        return Plan::Nothing;
    }
    let edit = edits.into_iter().reduce(|a, b| Edit::Pair(Box::new(a), Box::new(b))).expect("two edits");
    match clash {
        None => Plan::Commit(edit, title),
        Some(o) => {
            let mut held = crate::hints::mods_caps(mods);
            held.extend(["Left".to_string(), "Right".to_string()]);
            Plan::Ask(Prompt {
                key: p,
                title: format!("{} is already in use", words(&held)),
                held,
                sub: format!("“{o}” uses it. Move it here and “{o}” loses this key. Cancel changes nothing."),
                edit,
            })
        }
    }
}

/// des-k or Windows for a step 2 row: a pair moves both arrows at once.
fn plan_side(i: usize, desk: bool) -> Plan {
    let t = &TAKES[i];
    match t.keys {
        [k] => sp::plan_give(k, desk),
        [p, n] if desk => {
            let s = settings::get();
            if held(s, t.keys) == Held::Desk || matches!(held(s, t.keys), Held::Took(_)) {
                return Plan::Nothing;
            }
            plan_pair(p, n, WIN | CTRL, false, t.title)
        }
        [p, n] => match (sp::plan_give(p, false), sp::plan_give(n, false)) {
            (Plan::Commit(a, t), Plan::Commit(b, _)) => Plan::Commit(Edit::Pair(Box::new(a), Box::new(b)), t),
            (Plan::Commit(a, t), _) | (_, Plan::Commit(a, t)) => Plan::Commit(a, t),
            _ => Plan::Nothing,
        },
        _ => Plan::Nothing,
    }
}

/// What a recording is for, from a field.
fn rec_for(f: Field) -> Option<Rec> {
    let s = settings::get();
    let keys: &[&'static str] = match f {
        Field::Take(i) => TAKES[i].keys,
        Field::More(i) => MORE_KEYS[i].1,
        Field::Hints => &["shortcuts.hints"],
    };
    Some(match keys {
        [p, n] => Rec::Pair(p, n),
        [k] if sp::is_digits(k) => Rec::Digits(k),
        [k] => Rec::List(k, sp::shortcut(k).and_then(|sh| sp::drawn_order(s, sh).first().copied())),
        _ => return None,
    })
}

fn field_of(rec: Rec) -> Option<Field> {
    let key = match rec {
        Rec::List(k, _) | Rec::Digits(k) | Rec::Pair(k, _) => k,
        Rec::Gesture(..) => return None,
    };
    field_of_key(key)
}

fn field_of_key(key: &str) -> Option<Field> {
    if key == "shortcuts.hints" {
        return Some(Field::Hints);
    }
    if let Some(i) = TAKES.iter().position(|t| t.keys.contains(&key)) {
        return Some(Field::Take(i));
    }
    MORE_KEYS.iter().position(|(_, k)| k.contains(&key)).map(Field::More)
}

fn title_of(f: Field) -> &'static str {
    match f {
        Field::Take(i) => TAKES[i].title,
        Field::More(i) => MORE_KEYS[i].0,
        Field::Hints => "Shortcut hints key",
    }
}

/// The recording's note: what to press.
fn rec_note(rec: Rec) -> (String, String) {
    match rec {
        Rec::Digits(k) => {
            let does = match k {
                "shortcuts.jump" => "go to workspace 1",
                "shortcuts.send_to" => "send the window to workspace 1",
                _ => "send the window to workspace 1 and stay",
            };
            ("Hold the keys you want, then press any digit".into(), format!("Win Alt 1 will {does}, Win Alt 2 to 2, and so on. Esc alone cancels."))
        }
        Rec::Pair(..) => ("Hold the keys you want, then press ← or →".into(), "Both arrows get them: ← for previous, → for next. Esc alone cancels.".into()),
        Rec::List(..) => ("Press the new key".into(), "Hold Win, Ctrl, Alt or Shift with a key. Esc alone cancels.".into()),
        Rec::Gesture(..) => ("Press the keys this tap should send".into(), "Any combo works, even one of des-k's own. Esc alone or a click elsewhere cancels.".into()),
    }
}

// ---- The touchpad's state, in the first run's words (spec 0011's states) ----

#[derive(Clone, Copy, PartialEq, Debug)]
enum Chip {
    Ok,
    Off,
    On,
    Err,
    Back,
    Part,
    Not,
}

/// One of spec 0011's touchpad states as step 4 draws it.
struct PadView {
    sub: String,
    note: Option<NoteSpec>,
    head: &'static str,
    chips: [Chip; 4],
    give: bool,  // the Give back row shows
    quit: bool,  // its caption adds "Quit does the same."
    ready: bool, // des-k runs the gestures now
    done_line: String, // Done's touchpad row
    done_button: bool, // and its Open Windows Settings
}

fn pad_view(st: &PadState, back: Option<GaveBack>) -> PadView {
    const LOG: &str = "Details: tray icon › Open log.";
    let note = |tone, icon, title: &str, line: String, buttons: Vec<(Ctl, &'static str)>| Some(NoteSpec { tone, icon, title: title.into(), lines: vec![P::T(line)], buttons });
    let windows = |label| vec![(Ctl::Pad(Pad::Windows), label)];
    let rows = st.rows.map(|r| match r {
        PadRow::Nothing => Chip::Ok,
        PadRow::StillOn => Chip::On,
        PadRow::CantRead => Chip::Err,
    });
    let mut v = PadView { sub: String::new(), note: None, head: "des-k would run", chips: rows, give: false, quit: false, ready: false, done_line: String::new(), done_button: false };
    if let Some(b) = back.filter(|b| b.failed()) {
        let none = b.wrote.iter().all(|w| *w == Wrote::NotOne);
        v.chips = b.wrote.map(|w| match w {
            Wrote::All => Chip::Back,
            Wrote::Part => Chip::Part,
            Wrote::NotOne => Chip::Not,
        });
        let (title, sub, line) = if none {
            ("Give back failed", "Giving the gestures back failed.", "None went back.")
        } else {
            ("Give back partly failed", "Giving the gestures back partly failed.", "Some went back, some didn't.")
        };
        v.sub = sub.into();
        v.head = if none { "des-k runs" } else { "des-k would run" };
        v.note = note(Tone::Warn, I_WARN, title, format!("{line} des-k keeps your old values to try again. Windows Settings is open. {LOG}"), vec![(Ctl::Pad(Pad::GiveBack), "Try again")]);
        v.ready = none && st.off;
        v.done_line = format!("{title}. Try again on the Touchpad step.");
        return v;
    }
    if let Some(b) = back.filter(|_| !st.off) {
        v.chips = [Chip::Back; 4];
        v.sub = "You gave the gestures back.".into();
        v.note = if b.cleared {
            note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", "des-k stopped reading them. Windows Settings is open: set them there to use them now.".into(), vec![])
        } else {
            note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", format!("Nothing is wrong now, but des-k couldn't delete its copy of your old values. Give back again to clean it up. {LOG}"), vec![])
        };
        v.give = st.saved;
        v.done_line = "Windows gets its gestures back at your next sign-in.".into();
        return v;
    }
    v.give = st.saved;
    if !st.enabled {
        v.sub = "des-k's touchpad reading is off.".into();
        if st.off {
            v.chips = [Chip::Off; 4];
            v.quit = true;
            v.note = Some(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "No 3- or 4-finger gesture works now".into(),
                lines: vec![P::T("Windows' own gestures are off too. Turn reading on, or turn them back on in Windows Settings.".into())],
                buttons: vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on"), (Ctl::Pad(Pad::Windows), "Open Windows Settings")],
            });
        } else {
            v.note = note(Tone::Info, I_INFO, "Windows runs its own 3- and 4-finger gestures", "des-k runs none of these until reading is on.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on")]);
        }
        v.done_line = "Touchpad reading is off.".into();
        return v;
    }
    if let Some(i) = st.rows.iter().position(|r| *r == PadRow::CantRead) {
        let row = setup::ROWS[i].0;
        let one = row.trim_end_matches('s');
        v.head = "des-k will run";
        v.sub = "des-k runs no gestures for now.".into();
        v.note = note(Tone::Warn, I_WARN, &format!("Can't read Windows' {one} setting"), format!("Until des-k can read it, it runs no gestures, and the ones you turned off don't work. Set {row} again in Windows Settings. {LOG}"), windows("Open Windows Settings"));
        v.done_line = format!("Not ready: des-k can't read Windows' {one} setting.");
        v.done_button = true;
        return v;
    }
    let on: Vec<usize> = (0..4).filter(|&i| st.rows[i] == PadRow::StillOn).collect();
    let off = 4 - on.len();
    match on.len() {
        0 => {
            v.head = "des-k runs";
            v.ready = true;
            v.quit = st.saved;
            v.sub = if st.saved {
                "All four are off in Windows. des-k runs these, also in admin windows.".into()
            } else {
                "All four are off in Windows. des-k runs these, also in admin windows. You turned them off yourself, so Quit has nothing to give back.".into()
            };
            v.done_line = "des-k runs your gestures.".into();
        }
        4 => {
            v.head = "des-k will run";
            v.sub = "des-k runs your 3- and 4-finger gestures once Windows' own are off.".into();
            v.note = note(Tone::Info, I_SETUP, "Turn off four gestures in Windows", "Settings › Touchpad: under Three-finger and Four-finger gestures, set Swipes and Taps to “Nothing”.".into(), windows("Set up in Windows Settings"));
            v.done_line = "Not ready: none of the four are off in Windows yet.".into();
            v.done_button = true;
        }
        _ => {
            v.head = "des-k will run";
            v.sub = format!("{off} of 4 done. des-k starts once all four are off in Windows. Until then, the {off} you turned off don't work.");
            let names: Vec<String> = on.iter().map(|&i| spelled_row(setup::ROWS[i].0)).collect();
            let mut line = names.join(", ");
            if let Some(c) = line.get(..1) {
                line = c.to_uppercase() + &line[1..];
            }
            v.note = note(Tone::Info, I_SETUP, "Still on in Windows", line + ".", windows("Open Windows Settings"));
            v.done_line = format!("Not ready: {off} of 4 are off in Windows. Until all four are, those {off} don't work.");
            v.done_button = true;
        }
    }
    v
}

/// "3-finger swipes" → "three-finger swipes", as a sentence has it.
fn spelled_row(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}

// ---- Drawing ----

/// A piece of a caption: words, or words in the text color (a reason).
enum P {
    T(String),
    Why(String),
    Icon(&'static str, u32), // a status glyph in its color
}

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Info,
    Warn,
    Rec,
}

struct NoteSpec {
    tone: Tone,
    icon: &'static str,
    title: String,
    lines: Vec<P>,
    buttons: Vec<(Ctl, &'static str)>,
}

/// What one pass of compose found.
struct Out {
    hits: Vec<Hit>,
    content: f32,
    mid: (f32, f32),
    said: Vec<(Ctl, Node)>, // what Narrator reads for each stop, in order
    texts: Vec<Node>,       // and the words around them: progress, heading, the tries
}

/// One pass over the window, drawing or only measuring.
struct Ctx<'a, 'f> {
    c: &'a mut Canvas<'f>,
    w: &'a Win,
    s: &'static Settings,
    cw: f32,
    group: u8,
    out: Out,
}

impl Ctx<'_, '_> {
    fn hit(&mut self, ctl: Ctl, at: Bx) {
        let group = if matches!(ctl, Ctl::Item(_)) { 3 } else { self.group };
        self.out.hits.push(Hit { ctl, at, group });
    }

    /// Lays `f` out without drawing it or keeping its controls.
    fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32 {
        let draw = std::mem::replace(&mut self.c.draw, false);
        let (hits, said, texts) = (self.out.hits.len(), self.out.said.len(), self.out.texts.len());
        let h = f(self);
        self.out.hits.truncate(hits);
        self.out.said.truncate(said);
        self.out.texts.truncate(texts);
        self.c.draw = draw;
        h
    }

    fn say(&mut self, ctl: Ctl, role: UIA_CONTROLTYPE_ID, name: &str, help: &str, greyed: bool, state: (Option<bool>, Option<String>)) {
        if self.out.said.iter().any(|(c, _)| *c == ctl) {
            return;
        }
        let node = Node { role, name: name.to_string(), help: help.to_string(), greyed, focusable: true, toggled: state.0, value: state.1, ..Node::default() };
        self.out.said.push((ctl, node));
    }

    fn text_node(&mut self, role: UIA_CONTROLTYPE_ID, name: &str, value: Option<String>) {
        if self.c.draw || self.out.texts.iter().any(|n| n.name == name) {
            return;
        }
        self.out.texts.push(Node { role, name: name.to_string(), value, ..Node::default() });
    }

    fn focused(&self, ctl: Ctl) -> bool {
        self.w.kb && self.w.focus == Some(ctl)
    }

    fn hovered(&self, ctl: Ctl) -> bool {
        self.w.hover == Some(ctl)
    }

    /// The focus ring: 2 dip in the text color, 2 dip out (AC-FR-6).
    fn ring(&self, (x, y, w, h): Bx, r: f32, ctl: Ctl) {
        if self.focused(ctl) {
            self.c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), r + 3.0, 2.0, op(self.c.t.b.tx));
        }
    }

    fn icon(&self, x: f32, y: f32, lh: f32, icon: &str, f: F, color: u32) {
        self.c.text(x, y, lh, icon, f, color);
    }

    fn btn_w(&self, label: &str) -> f32 {
        self.c.width(label, F::Body) + 30.0
    }

    /// A button, 32 dip: the purple one is Next. Returns its width.
    fn button(&mut self, ctl: Ctl, x: f32, y: f32, label: &str, primary: bool) -> f32 {
        let b = self.c.t.b;
        let w = self.btn_w(label);
        let hot = self.hovered(ctl);
        if primary {
            self.c.fill_rr((x, y, w, 32.0), 6.0, op(if hot && !self.c.t.hc { mix(b.acc, b.tx, 0.12) } else { b.acc }));
            self.c.text(x + 15.0, y, 32.0, label, F::Body, b.on_acc);
        } else {
            self.c.boxed((x, y, w, 32.0), 6.0, if hot { mix(b.card, b.tx, 0.05) } else { b.card }, Some(b.line));
            self.c.text(x + 15.0, y, 32.0, label, F::Body, b.tx);
        }
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0));
        self.say(ctl, UIA_ButtonControlTypeId, label, "", false, (None, None));
        w
    }

    fn switch_w(&self, on: bool) -> f32 {
        self.c.width(if on { "On" } else { "Off" }, F::Body) + 10.0 + 40.0
    }

    /// The word and the switch, 40 × 20, the switch ringed itself.
    fn switch(&mut self, key: &'static str, x: f32, y: f32, on: bool, title: &str, help: &str) {
        let b = self.c.t.b;
        let ctl = Ctl::Toggle(key);
        let label = if on { "On" } else { "Off" };
        let lw = self.c.width(label, F::Body);
        self.c.text(x, y, 20.0, label, F::Body, b.tx2);
        let sx = x + lw + 10.0;
        let track = (sx, y, 40.0, 20.0);
        if on {
            self.c.fill_rr(track, 10.0, op(b.acc));
            self.c.circle(sx + 30.0, y + 10.0, 6.0, Some(op(b.on_acc)), None);
        } else {
            let fill = if self.hovered(ctl) { mix(b.card, b.tx, 0.06) } else { b.card };
            self.c.boxed(track, 10.0, fill, Some(b.tx2));
            self.c.circle(sx + 10.0, y + 10.0, 6.0, Some(op(b.tx2)), None);
        }
        self.ring(track, 10.0, ctl);
        self.hit(ctl, (x, y - 6.0, lw + 50.0, 32.0));
        self.say(ctl, UIA_CheckBoxControlTypeId, title, help, false, (Some(on), None));
    }

    fn stepper_w(&self, v: i64, unit: &str) -> f32 {
        let u = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        64.0 + (self.c.width(&v.to_string(), F::Body) + self.c.width(&u, F::Small) + 8.0).max(56.0)
    }

    /// − value +: two stops, each ringed alone. `na`: greyed, kept but not in use; − and +
    /// do nothing but stay stops, so you hear why.
    #[allow(clippy::too_many_arguments)]
    fn stepper(&mut self, key: &'static str, x: f32, y: f32, (v, unit): (i64, &str), (down, up): (bool, bool), na: bool, title: &str, help: &str) {
        let b = self.c.t.b;
        let num = v.to_string();
        let u = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        let (nw, uw) = (self.c.width(&num, F::Body), self.c.width(&u, F::Small));
        let w = 64.0 + (nw + uw + 8.0).max(56.0);
        self.c.boxed((x, y, w, 32.0), 6.0, b.card, Some(if na && self.c.t.hc { b.tx3 } else { b.line }));
        let mid = w - 64.0;
        let tx = x + 32.0 + (mid - nw - uw) / 2.0;
        self.c.text(tx, y, 32.0, &num, F::Body, if na { b.tx3 } else { b.tx });
        self.c.text(tx + nw, y, 32.0, &u, F::Small, if na { b.tx3 } else { b.tx2 });
        let shown = format!("{v}{u}");
        for (ctl, ix, icon, live, what) in [(Ctl::Minus(key), x, I_MINUS, down, "Less"), (Ctl::Plus(key), x + w - 32.0, I_ADD, up, "More")] {
            let live = live && !na;
            if self.hovered(ctl) && live {
                self.c.fill_rr((ix + 4.0, y + 4.0, 24.0, 24.0), 4.0, op(mix(b.card, b.tx, 0.08)));
            }
            let iw = self.c.width(icon, F::IconTiny);
            self.icon(ix + (32.0 - iw) / 2.0, y, 32.0, icon, F::IconTiny, if live { b.tx } else { b.tx3 });
            if self.focused(ctl) {
                self.c.stroke_rr((ix + 1.0, y + 1.0, 30.0, 30.0), 5.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (ix, y, 32.0, 32.0));
            self.say(ctl, UIA_ButtonControlTypeId, &format!("{what}: {title}"), help, na, (None, Some(shown.clone())));
        }
    }

    fn seg_w(&self, words: &[&str]) -> f32 {
        4.0 + words.iter().map(|l| self.c.width(l, F::Small) + 20.0).sum::<f32>() + 2.0 * (words.len() as f32 - 1.0)
    }

    /// Choice buttons in one track, 28 dip; the chosen one raised. One stop, arrows move it.
    #[allow(clippy::too_many_arguments)]
    fn seg(&mut self, x: f32, y: f32, words: &[&str], sel: usize, na: bool, ctl: impl Fn(usize) -> Ctl, stop: Ctl, name: &str, help: &str) {
        let b = self.c.t.b;
        let w = self.seg_w(words);
        self.c.boxed((x, y, w, 28.0), 6.0, b.track, Some(b.line));
        let mut ox = x + 2.0;
        for (i, label) in words.iter().enumerate() {
            let iw = self.c.width(label, F::Small) + 20.0;
            let bx = (ox, y + 2.0, iw, 24.0);
            let chosen = sel == i && !na;
            let fg = if chosen {
                if self.c.t.hc {
                    self.c.fill_rr(bx, 4.0, op(b.hi));
                    b.on_acc
                } else {
                    self.c.fill_rr((bx.0 - 1.0, bx.1 - 1.0, bx.2 + 2.0, bx.3 + 2.0), 5.0, op(b.line));
                    self.c.fill_rr(bx, 4.0, op(b.hi));
                    b.tx
                }
            } else {
                if self.hovered(ctl(i)) && !na {
                    self.c.fill_rr(bx, 4.0, op(mix(b.track, b.tx, 0.06)));
                }
                if na { b.tx3 } else { b.tx2 }
            };
            self.c.text(ox + 10.0, y + 2.0, 24.0, label, if chosen { F::SmallBold } else { F::Small }, fg);
            self.hit(ctl(i), bx);
            ox += iw + 2.0;
        }
        self.ring((x, y, w, 28.0), 6.0, stop);
        self.say(stop, UIA_ComboBoxControlTypeId, name, help, na, (None, Some(words.get(sel).copied().unwrap_or("").to_string())));
    }

    fn caption(&mut self, x: f32, y: f32, w: f32, pieces: &[P]) -> f32 {
        let b = self.c.t.b;
        let runs: Vec<(&str, F, u32)> = pieces
            .iter()
            .map(|p| match p {
                P::T(s) => (s.as_str(), F::Small, b.tx2),
                P::Why(s) => (s.as_str(), F::Small, b.tx),
                P::Icon(s, col) => (*s, F::IconSmall, *col),
            })
            .collect();
        self.c.para(x, y, w, 16.0, &runs)
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

    /// The tint of a row that is recording or warns.
    fn tint(&self, y: f32, h: f32) {
        let b = self.c.t.b;
        if !self.c.t.hc {
            self.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, h), 0.0, op(mix(b.card, b.acc, 0.06)));
        }
    }

    /// One row in a card: name and caption on the left, the control on the right, both
    /// centred; under them, a note if one is given. Returns its height.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, y: f32, title: &str, cap: &[P], (cw, ch): (f32, f32), ctl: impl Fn(&mut Self, f32, f32), note: Option<&NoteSpec>, tinted: bool) -> f32 {
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let lw = if cw > 0.0 { w - cw - 16.0 } else { w };
        let lh = self.label_h(lw, title, cap);
        let top = lh.max(ch).max(ROW - 16.0);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.measured(|c| c.in_note(x, 0.0, w, n));
        }
        h += 8.0;
        if tinted {
            self.tint(y, h);
        }
        ctl(self, x + w - cw, y + 8.0 + (top - ch) / 2.0);
        self.label(x, y + 8.0 + (top - lh) / 2.0, lw, title, cap);
        if let Some(n) = note {
            self.in_note(x, y + 8.0 + top + 6.0, w, n);
        }
        h
    }

    fn sep(&self, y: f32) {
        self.c.rule(X0 + 1.0, y, self.cw - 2.0, op(self.c.t.b.line));
    }

    /// A card around `body`, which draws from its top and returns its height.
    fn card(&mut self, y: f32, body: impl Fn(&mut Self, f32) -> f32) -> f32 {
        let h = self.measured(|c| body(c, y));
        let b = self.c.t.b;
        self.c.boxed((X0, y, self.cw, h), 8.0, b.card, Some(b.line));
        body(self, y);
        // The edge again, over rows that tint up to it.
        self.c.stroke_rr((X0 + 0.5, y + 0.5, self.cw - 1.0, h - 1.0), 8.0, 1.0, op(b.line));
        h
    }

    /// The More row: a chevron, and what it opens.
    fn more(&mut self, y: f32, what: &str) -> f32 {
        let b = self.c.t.b;
        let open = self.w.more[self.w.step.index()];
        let icon = if open { I_UP } else { I_DOWN };
        let iw = self.c.width(icon, F::IconSmall);
        let h = self.row(y, "More", &[P::T(what.into())], (iw, 20.0), |c, x, y| c.icon(x, y, 20.0, icon, F::IconSmall, b.tx2), None, false);
        if self.hovered(Ctl::More) {
            self.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, h - 1.0), 0.0, ui::alpha(b.tx, 0.04));
        }
        let at = (X0 + 4.0, y + 2.0, self.cw - 8.0, h - 4.0);
        self.ring(at, 6.0, Ctl::More);
        self.hit(Ctl::More, (X0, y, self.cw, h));
        self.say(Ctl::More, UIA_ButtonControlTypeId, &format!("More: {what}"), "", false, (None, Some(if open { "open" } else { "closed" }.into())));
        h
    }

    fn tone_color(&self, tone: Tone) -> u32 {
        let b = self.c.t.b;
        match tone {
            Tone::Info => b.tx2,
            Tone::Warn => b.warn,
            Tone::Rec => b.acc,
        }
    }

    /// A note's text and buttons, from `x`; returns its height. Buttons sit on the right when
    /// they leave the text room, else under it.
    fn note_body(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec) -> f32 {
        let col = self.tone_color(n.tone);
        let b = self.c.t.b;
        self.icon(x, y + 2.0, 16.0, n.icon, F::Icon, col);
        let bw: f32 = n.buttons.iter().map(|(_, l)| self.btn_w(l) + 8.0).sum();
        let beside = bw > 0.0 && bw < w * 0.42;
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
                bx += self.button(*ctl, bx, by, label, false) + 8.0;
            }
            h = if beside { h.max(32.0) } else { h + 8.0 + 32.0 };
        }
        h
    }

    /// A note on its own: card colored, with its edge.
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
        let mut dx = 0.0;
        while dx < w {
            self.c.fill_rr((x + dx, y, 3.0f32.min(w - dx), 1.0), 0.0, op(b.line));
            dx += 6.0;
        }
        10.0 + self.note_body(x, y + 10.0, w, n) + 2.0
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
        caps.iter()
            .enumerate()
            .map(|(i, k)| {
                let (label, f) = match k.as_str() {
                    "Left" | "Right" | "Up" | "Down" => ("\u{E72B}", F::IconTiny),
                    k => (k, F::Small),
                };
                (self.c.width(label, f) + 2.0 * (22.0 / 2.0 - 5.0) + 2.0).max(22.0) + if i > 0 { 4.0 } else { 0.0 }
            })
            .sum()
    }

    /// A key field, 32 dip: its key and the pen, "Set key", or recording. `fill`: its width
    /// when it fills a column. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn key_field(&mut self, f: Field, x: f32, y: f32, fill: Option<f32>, caps: Option<&[String]>, rec: Option<(&[String], &str)>, help: &str) -> f32 {
        let b = self.c.t.b;
        let ctl = Ctl::Field(f);
        let pen_w = self.c.width(I_PEN, F::IconSmall);
        let set_w = self.c.width("Set key", F::Body);
        let w = fill.unwrap_or_else(|| match (rec, caps) {
            (Some((held, ghost)), _) => 10.0 + if held.is_empty() { self.c.width(ghost, F::Small) + 12.0 } else { self.caps_w(held) },
            (None, Some(k)) => 5.0 + self.caps_w(k) + 6.0 + pen_w + 4.0,
            (None, None) => 10.0 + 10.0 + 6.0 + set_w + 12.0,
        });
        let hot = self.hovered(ctl);
        match rec {
            Some((held, ghost)) => {
                self.c.fill_rr((x, y, w, 32.0), 6.0, op(b.acc));
                self.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, 28.0), 4.0, op(b.card));
                if held.is_empty() {
                    let gw = self.c.width(ghost, F::Small) + 12.0;
                    self.c.stroke_dash((x + 5.0, y + 5.0, gw, 22.0), 4.0, 1.0, op(b.tx3), DashStyleDash);
                    self.c.text(x + 11.0, y + 5.0, 22.0, ghost, F::Small, b.tx3);
                } else {
                    self.caps(x + 5.0, y + 5.0, held, true);
                }
            }
            None => {
                self.c.boxed((x, y, w, 32.0), 6.0, if hot { mix(b.track, b.tx, 0.05) } else { b.track }, Some(b.line));
                match caps {
                    Some(k) => {
                        self.caps(x + 5.0, y + 5.0, k, false);
                        self.icon(x + w - 4.0 - pen_w, y, 32.0, I_PEN, F::IconSmall, b.tx3);
                    }
                    None => {
                        self.icon(x + 10.0, y, 32.0, I_ADD, F::IconTiny, b.tx2);
                        self.c.text(x + 26.0, y, 32.0, "Set key", F::Body, b.tx);
                    }
                }
            }
        }
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0));
        let value = if rec.is_some() { "recording".to_string() } else { caps.map_or("no key".into(), words) };
        self.say(ctl, UIA_ButtonControlTypeId, &format!("{}: key", title_of(f)), help, false, (None, Some(value)));
        w
    }

    // ---- Above the step: progress, heading, the fixed note ----

    /// Returns the bottom of what sits above the step's view.
    fn head(&mut self) -> f32 {
        let t = self.c.t;
        let b = t.b;
        let list = steps(self.w);
        let n = list.len();
        let j = list.iter().position(|s| *s == self.w.step).unwrap_or(0);
        let frac = (j + 1) as f32 / n as f32;
        let bw = self.w.vw - 2.0 * X0;
        if t.hc {
            self.c.boxed((X0, 0.0, bw, 6.0), 3.0, b.bg, Some(b.tx));
            self.c.fill_rr((X0 + 1.0, 1.0, (bw - 2.0) * frac, 4.0), 2.0, op(b.acc));
        } else {
            self.c.fill_rr((X0, 0.0, bw, 3.0), 1.5, op(b.line));
            self.c.fill_rr((X0, 0.0, bw * frac, 3.0), 1.5, op(b.acc));
        }
        let eye = format!("Step {} of {n}", j + 1);
        self.text_node(UIA_ProgressBarControlTypeId, &eye, Some(format!("{} of {n}", j + 1)));
        self.c.text(X0, 19.0, 16.0, &eye, F::Small, b.tx2);
        let title = self.w.step.title();
        self.text_node(UIA_HeaderControlTypeId, title, None);
        self.c.text(X0, 37.0, 32.0, title, F::Heading, b.tx);
        let mut y = 69.0;
        let sub = self.sub();
        if !sub.is_empty() {
            self.text_node(UIA_TextControlTypeId, &sub, None);
            y += 4.0 + self.c.para(X0, 73.0, self.cw, 20.0, &[(&sub, F::Body, b.tx2)]);
        }
        if let Some(n) = self.fixed() {
            y += 12.0;
            y += self.note(X0, y, self.cw, &n);
        }
        y
    }

    fn sub(&self) -> String {
        let w = self.w;
        match w.step {
            Step::Try if w.pics.screens.len() > 1 => "Switch the screen under your mouse. Your other screens stay as they are.".into(),
            Step::Try => "Switch workspaces on your screen. Plug in another, and it gets its own.".into(),
            Step::Keys => "des-k's default shortcuts. Each one overrides a Windows shortcut. Turn any off, or click a key to change it.".into(),
            Step::Behavior => "des-k's defaults. Change any now or later.".into(),
            Step::Pad => self.pad().sub,
            Step::Done => "Everything is saved. Open this again any time: des-k settings › Welcome.".into(),
        }
    }

    fn pad(&self) -> PadView {
        let st = self.w.pics.pad.unwrap_or(PadState { pad: true, enabled: self.s.touchpad_enabled, rows: [PadRow::StillOn; 4], off: false, saved: false });
        pad_view(&st, LAST.get())
    }

    /// The note that never scrolls away: a failed save, then the touchpad's, or a gesture
    /// recording's.
    fn fixed(&self) -> Option<NoteSpec> {
        if let Some((what, why)) = &self.w.error {
            return Some(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: format!("Couldn't save “{what}”"),
                lines: vec![P::T(format!("Your change isn't saved or in use. Close settings.toml in any program that has it open, then try again. Windows said: {why}"))],
                buttons: vec![],
            });
        }
        if self.w.step != Step::Pad {
            return None;
        }
        if let Some(Rec::Gesture(..)) = self.w.rec {
            let (title, line) = rec_note(self.w.rec?);
            let lines = match &self.w.refused {
                Some((t, l)) => return Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: t.clone(), lines: vec![P::T(l.clone())], buttons: vec![(Ctl::Cancel, "Cancel")] }),
                None => vec![P::T(line)],
            };
            return Some(NoteSpec { tone: Tone::Rec, icon: I_KEY, title, lines, buttons: vec![(Ctl::Cancel, "Cancel")] });
        }
        self.pad().note
    }

    // ---- Step 1: try it ----

    fn try_it(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let mut h = self.screens(y) + 12.0;
        h += self.card(y + h, |c, y| c.tries(y));
        if self.w.tried.iter().all(|t| *t) {
            h += 12.0;
            h += self.c.para(X0, y + h, self.cw, 16.0, &[("That's des-k. Next: your keys.", F::Small, b.tx2)]);
        }
        h
    }

    /// Your screens, each in its workspace's color with its name and number, a window where
    /// that workspace has one, and the pointer on the screen under the mouse.
    fn screens(&mut self, y: f32) -> f32 {
        let list = self.w.pics.screens.clone();
        let (wd, ht) = (576.0, 176.0);
        if list.is_empty() {
            return ht;
        }
        let b = self.c.t.b;
        let bx = list.iter().map(|s| s.rect.left).min().unwrap_or(0) as f32;
        let by = list.iter().map(|s| s.rect.top).min().unwrap_or(0) as f32;
        let bw = list.iter().map(|s| s.rect.right).max().unwrap_or(1) as f32 - bx;
        let bh = list.iter().map(|s| s.rect.bottom).max().unwrap_or(1) as f32 - by;
        let sc = ((wd - 4.0) / bw).min((ht - 18.0) / bh).min(0.09);
        let ox = X0 + (self.cw - wd) / 2.0 + (wd - bw * sc) / 2.0;
        let oy = y + (ht - (bh * sc + 16.0)) / 2.0;
        let mut said = vec![];
        for s in &list {
            let x = ox + (s.rect.left as f32 - bx) * sc + 3.0;
            let yy = oy + (s.rect.top as f32 - by) * sc + 3.0;
            let (w, h) = ((s.rect.right - s.rect.left) as f32 * sc - 6.0, (s.rect.bottom - s.rect.top) as f32 * sc - 6.0);
            let below = list.iter().any(|o| o.device != s.device && (o.rect.top - s.rect.bottom).abs() < 2 && o.rect.left < s.rect.right && o.rect.right > s.rect.left);
            if !below {
                if s.name == "Built-in display" {
                    self.c.polygon(&[(x - 6.0, yy + h), (x + w + 6.0, yy + h), (x + w + 3.0, yy + h + 4.0), (x - 3.0, yy + h + 4.0)], op(b.stand), (op(b.stand), 0.5));
                } else {
                    let cx = x + w / 2.0;
                    self.c.polygon(&[(cx - 8.0, yy + h), (cx + 8.0, yy + h), (cx + 11.0, yy + h + 9.0), (cx - 11.0, yy + h + 9.0)], op(b.stand), (op(b.stand), 0.5));
                    self.c.fill_rr((cx - 22.0, yy + h + 9.0, 44.0, 3.0), 1.5, op(b.stand));
                }
            }
            let a = self.w.pics.view.active.iter().find(|(d, _)| *d == s.device).map_or(0, |(_, a)| *a);
            self.c.fill_rr((x, yy, w, h), 6.0, op(b.bez));
            self.c.fill_rr((x + 2.0, yy + 2.0, w - 4.0, h - 4.0), 4.0, op(b.ws[a.min(8)]));
            let name = self.w.pics.view.names.iter().find(|(d, _)| *d == s.device).map_or_else(|| s.name.clone(), |(_, n)| n.clone());
            said.push(format!("{name} on workspace {}", a + 1));
            // The name in lines that fit, when the face has room for them and the number.
            let mut lines: Vec<String> = vec![];
            for word in name.split(' ') {
                match lines.last_mut() {
                    Some(l) if self.c.width(&format!("{l} {word}"), F::Small) <= w - 16.0 => *l = format!("{l} {word}"),
                    _ => lines.push(word.to_string()),
                }
            }
            let roomy = lines.len() as f32 * 15.0 + 30.0 <= h && lines.iter().all(|l| self.c.width(l, F::Small) <= w - 16.0);
            if roomy {
                for (k, l) in lines.iter().enumerate() {
                    self.c.text(x + 9.0, yy + 7.0 + k as f32 * 15.0, 15.0, l, F::Small, b.on_ws);
                }
            }
            let wy = yy + 12.0 + if roomy { lines.len() as f32 * 15.0 } else { 0.0 } + 8.0;
            let wh = h - (wy - yy) - 30.0;
            let busy = self.w.pics.view.windows.iter().any(|(d, ws, _)| *d == s.device && *ws == a);
            if busy && w > 100.0 && wh >= 18.0 {
                self.c.fill_rr((x + w * 0.42, wy, w * 0.48, wh), 3.0, ui::alpha(0xFFFFFF, 0.85));
            }
            let num = (a + 1).to_string();
            if roomy {
                self.c.text(x + 9.0, yy + h - 34.0, 28.0, &num, F::Title, b.on_ws);
            } else {
                let nw = self.c.width(&num, F::Title);
                self.c.text(x + (w - nw) / 2.0, yy + (h - 28.0) / 2.0, 28.0, &num, F::Title, b.on_ws);
            }
            if s.device == self.w.pics.mouse {
                let (px, py) = (x + w - 28.0, yy + h - 30.0);
                let pts = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.0, 19.8), (9.8, 18.5), (7.0, 12.1), (12.6, 12.1)].map(|(a, z)| (px + a, py + z));
                self.c.polygon(&pts, op(0xFFFFFF), (op(0x111111), 1.1));
            }
        }
        self.text_node(UIA_ImageControlTypeId, &said.join(", "), None);
        ht
    }

    fn tries(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let s = self.s;
        let tried = self.w.tried;
        let now = tried.iter().position(|t| !*t);
        let mut h = 0.0;
        for (i, (title, key, how)) in TRIES.iter().enumerate() {
            if i > 0 {
                self.sep(y + h);
            }
            let done = tried[i];
            let cap = if now == Some(i) {
                how.to_string()
            } else if done && i == 0 {
                self.switched()
            } else {
                String::new()
            };
            let caps = sp::shortcut(key).and_then(|sh| sp::drawn_order(s, sh).first().and_then(|&k| sp::keys_of(s, key).get(k).cloned()));
            let kw = caps.as_ref().map_or_else(|| self.c.width("No key", F::Small), |c| self.caps_w(c));
            let lx = X0 + 16.0 + 20.0 + 12.0;
            let lw = self.cw - 32.0 - 32.0 - kw - 12.0;
            let cap_p: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.clone())] };
            let lh = self.measured(|c| c.try_label(lx, 0.0, lw, title, &cap_p, false));
            let top = lh.max(20.0).max(ROW - 16.0);
            let two = !cap.is_empty();
            let row_y = y + h + 8.0;
            let cy = if two { row_y } else { row_y + (top - 20.0) / 2.0 };
            let cx = X0 + 16.0;
            if done {
                self.c.circle(cx + 10.0, cy + 10.0, 10.0, Some(op(b.acc)), None);
                let iw = self.c.width(I_OK, F::IconTiny);
                self.icon(cx + 10.0 - iw / 2.0, cy, 20.0, I_OK, F::IconTiny, b.on_acc);
            } else {
                let now = now == Some(i);
                self.c.circle(cx + 10.0, cy + 10.0, if now { 9.25 } else { 9.5 }, None, Some((op(if now { b.acc } else { b.tx3 }), if now { 1.5 } else { 1.0 })));
                let d = (i + 1).to_string();
                let dw = self.c.width(&d, F::SmallBold);
                self.c.text(cx + 10.0 - dw / 2.0, cy, 20.0, &d, F::SmallBold, if now { b.tx } else { b.tx2 });
            }
            self.try_label(lx, if two { row_y } else { row_y + (top - lh) / 2.0 }, lw, title, &cap_p, done || now == Some(i));
            let ky = if two { row_y - 1.0 } else { row_y + (top - 22.0) / 2.0 };
            let kx = X0 + self.cw - 16.0 - kw;
            match &caps {
                Some(c) => {
                    self.caps(kx, ky, c, done && i == 0 && tried == [true, false, false]);
                }
                None => self.c.text(kx, ky, 22.0, "No key", F::Small, b.tx2),
            }
            let state = if done { "done" } else if now == Some(i) { "now" } else { "to do" };
            let spoken = format!("{}. {title}, {}, {state}. {cap}", i + 1, caps.as_ref().map_or("no key".into(), |c| words(c)));
            self.text_node(UIA_ListItemControlTypeId, spoken.trim_end(), None);
            h += 8.0 + top + 8.0;
        }
        h
    }

    fn try_label(&mut self, x: f32, y: f32, w: f32, title: &str, cap: &[P], strong: bool) -> f32 {
        let b = self.c.t.b;
        let mut h = self.c.para(x, y, w, 20.0, &[(title, F::Bold, if strong { b.tx } else { b.tx2 })]);
        if !cap.is_empty() {
            h += self.caption(x, y + h, w, cap);
        }
        h
    }

    /// The switch row's result.
    fn switched(&self) -> String {
        let back = row_caps(self.s, &["shortcuts.previous"]).map(|c| words(&c));
        let undo = |who: &str| match &back {
            Some(k) => format!("{who} hidden, not closed: {k} brings them back."),
            None => format!("{who} hidden, not closed."),
        };
        match &self.w.switched {
            Some((_, to, true)) => format!("Switched to workspace {}. {}", to + 1, undo("Your windows are")),
            Some((name, to, false)) => format!("Only the {name} switched, to workspace {}. {}", to + 1, undo("Its windows are")),
            None => "Switched.".into(),
        }
    }

    // ---- Step 2: shortcuts ----

    fn keys(&mut self, y: f32) -> f32 {
        self.card(y, |c, y| {
            let b = c.c.t.b;
            let head = "Override Windows?";
            let col3 = c.c.width(head, F::SmallBold).max(c.seg_w(&["Yes", "No"]));
            c.c.text(X0 + 16.0, y + 8.0, 16.0, "Key · click to change", F::SmallBold, b.tx2);
            c.c.text(X0 + 16.0 + KEY_W + 16.0, y + 8.0, 16.0, "Action", F::SmallBold, b.tx2);
            c.c.text(X0 + c.cw - 16.0 - col3, y + 8.0, 16.0, head, F::SmallBold, b.tx2);
            let mut h = 32.0;
            for i in 0..TAKES.len() {
                c.sep(y + h);
                h += c.take_row(y + h, i, col3);
            }
            c.sep(y + h);
            h += c.more(y + h, "Send and pin keys");
            if c.w.more[Step::Keys.index()] {
                for i in 0..MORE_KEYS.len() {
                    c.sep(y + h);
                    h += c.more_key(y + h, i);
                }
            }
            h
        })
    }

    /// The note under a key field: recording, a refused key, or a clash.
    fn key_note(&self, f: Field) -> Option<(NoteSpec, Vec<String>)> {
        if let Some(p) = &self.w.prompt
            && field_of_key(p.key) == Some(f)
        {
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: p.title.clone(), lines: vec![P::T(p.sub.clone())], buttons: vec![(Ctl::Move, "Move it here"), (Ctl::Cancel, "Cancel")] };
            return Some((n, p.held.clone()));
        }
        let rec = self.w.rec.filter(|r| field_of(*r) == Some(f))?;
        let n = match &self.w.refused {
            Some((t, l)) => NoteSpec { tone: Tone::Warn, icon: I_WARN, title: t.clone(), lines: vec![P::T(l.clone())], buttons: vec![(Ctl::Cancel, "Cancel")] },
            None => {
                let (t, l) = rec_note(rec);
                NoteSpec { tone: Tone::Rec, icon: I_KEY, title: t, lines: vec![P::T(l)], buttons: vec![(Ctl::Cancel, "Cancel")] }
            }
        };
        Some((n, vec![]))
    }

    fn ghost(f: Field) -> &'static str {
        match f {
            Field::Take(3..=5) => "1–9",
            Field::Take(0) | Field::More(0 | 1) => "← →",
            _ => "key",
        }
    }

    fn take_row(&mut self, y: f32, i: usize, col3: f32) -> f32 {
        let b = self.c.t.b;
        let s = self.s;
        let t = &TAKES[i];
        let f = Field::Take(i);
        let cap = take_caption(s, i);
        let x = X0 + 16.0;
        let lx = x + KEY_W + 16.0;
        let lw = X0 + self.cw - 16.0 - col3 - 16.0 - lx;
        let lh = self.label_h(lw, t.title, &[P::T(cap.clone())]);
        let top = lh.max(32.0).max(ROW - 16.0);
        let note = self.key_note(f);
        let mut h = 8.0 + top;
        if let Some((n, _)) = &note {
            let nw = self.cw - 32.0;
            h += 6.0 + self.measured(|c| c.in_note(x, 0.0, nw, n));
        }
        h += 8.0;
        if note.is_some() {
            self.tint(y, h);
        }
        let fy = y + 8.0 + (top - 32.0) / 2.0;
        if t.keys.is_empty() {
            // The Copilot key: no key to record.
            let caps = vec!["Copilot".to_string()];
            self.c.boxed((x, fy, KEY_W, 32.0), 6.0, b.track, Some(b.line));
            self.caps(x + 5.0, fy + 5.0, &caps, false);
        } else {
            let caps = row_caps(s, t.keys);
            let rec = note.as_ref().map(|(_, held)| (held.as_slice(), Self::ghost(f)));
            self.key_field(f, x, fy, Some(KEY_W), caps.as_deref(), rec, &cap);
        }
        // Yes or No, greyed while another action holds the key.
        let (sel, na) = if t.keys.is_empty() {
            (if s.copilot_as_right_ctrl { 0 } else { 1 }, false)
        } else {
            match held(s, t.keys) {
                Held::Desk => (0, false),
                Held::Took(_) => (0, true),
                _ => (1, false),
            }
        };
        let sw = self.seg_w(&["Yes", "No"]);
        let name = format!("{}: override Windows?", t.title);
        self.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &["Yes", "No"], sel, na, |k| Ctl::SideAt(i, k == 0), Ctl::Side(i), &name, &cap);
        self.label(lx, y + 8.0 + (top - lh) / 2.0, lw, t.title, &[P::T(cap.clone())]);
        if let Some((n, _)) = &note {
            self.in_note(x, y + 8.0 + top + 6.0, self.cw - 32.0, n);
        }
        h
    }

    fn more_key(&mut self, y: f32, i: usize) -> f32 {
        let (title, keys) = MORE_KEYS[i];
        let f = Field::More(i);
        let caps = row_caps(self.s, keys);
        let note = self.key_note(f);
        let rec = note.as_ref().map(|(_, held)| (held.clone(), Self::ghost(f)));
        let fw = self.measured(|c| c.key_field(f, 0.0, 0.0, None, caps.as_deref(), rec.as_ref().map(|(h, g)| (h.as_slice(), *g)), ""));
        let n = note.map(|(n, _)| n);
        self.row(y, title, &[], (fw, 32.0), |c, x, y| {
            c.key_field(f, x, y, None, caps.as_deref(), rec.as_ref().map(|(h, g)| (h.as_slice(), *g)), "");
        }, n.as_ref(), n.is_some())
    }

    // ---- Step 3: behavior ----

    fn behavior(&mut self, y: f32) -> f32 {
        self.card(y, |c, y| {
            let s = c.s;
            let n = s.workspaces as i64;
            let warn = n > 2 && (c.focused(Ctl::Minus("workspaces")) || c.hovered(Ctl::Minus("workspaces")));
            let note = warn.then(|| c.lowering());
            let cap = format!("Every screen gets workspaces 1 to {n}.");
            let sw = c.stepper_w(n, "");
            let mut h = c.row(y, "Workspaces per screen", &[P::T(cap.clone())], (sw, 32.0), |c, x, y| {
                c.stepper("workspaces", x, y, (n, ""), (n > 2, n < 9), false, "Workspaces per screen", &cap);
            }, note.as_ref(), warn);
            for (key, title, cap) in [
                ("follow_activation", "Follow a window", "When a hidden window comes to the front (an app reuses it, a dialog pops up), go to its workspace."),
                ("indicator.enabled", "Workspace indicator", "Shows the workspace for a moment after a switch, send or pin."),
                ("restore.enabled", "Restore after restart", "Reopened windows go back to their workspaces. Applies from des-k's next start."),
            ] {
                c.sep(y + h);
                h += c.toggle_row(y + h, key, title, cap);
            }
            c.sep(y + h);
            h += c.more(y + h, "Animations, indicator style and timing, restore time, overview");
            if c.w.more[Step::Behavior.index()] {
                let rows: [(&'static str, &str, &str); 9] = [
                    ("fastest", "Skip animations", "Every des-k fade and move happens at once, and the overview leaves out its window shadows. Windows stops animating the settings, welcome and hints windows as they open and close. Windows' own Animation effects setting doesn't change des-k."),
                    ("indicator.style", "Indicator style", "The number and a bar per workspace, with the screen's name when you have two or more."),
                    ("indicator.hold_ms", "Indicator stays", "How long it shows. 0 to 5000 ms."),
                    ("indicator.fade_ms", "Indicator fades out", "How long it takes to go. 0 to 2000 ms."),
                    ("restore.minutes", "Restore runs for", "Apps you open later stay on the workspace you're on."),
                    ("overview.labels", "Window names in overview", ""),
                    ("overview.animation", "Overview animation", ""),
                    ("overview.fade_ms", "Overview fades for", "How long the fade in and the fade out each take. 0 to 1000 ms."),
                    ("overview.live", "Live overview", "Off: shows windows as they were when the overview opened."),
                ];
                for (key, title, cap) in rows {
                    c.sep(y + h);
                    h += c.setting(y + h, key, title, cap);
                }
            }
            h
        })
    }

    /// What lowering the count moves, before you press − (as the settings page warns).
    fn lowering(&self) -> NoteSpec {
        let n = self.s.workspaces;
        let list = self.w.pics.view.removals.first().cloned().unwrap_or_default();
        let win = |k: usize| match k {
            0 => "none".to_string(),
            1 => "1 window".to_string(),
            k => format!("{k} windows"),
        };
        let each: Vec<String> = list.iter().filter(|m| !m.unplugged || m.windows > 0).map(|m| format!("{}{}: {}", m.name, if m.unplugged { " (not plugged in)" } else { "" }, win(m.windows))).collect();
        let what = if each.is_empty() { String::new() } else { format!(" ({})", each.join("; ")) };
        NoteSpec {
            tone: Tone::Warn,
            icon: I_WARN,
            title: format!("Lowering to {} removes workspace {n} from every screen", n - 1),
            lines: vec![P::T(format!("Its windows move to workspace {}{what}. A screen showing workspace {n} switches to {}. Raising it again won't move them back.", n - 1, n - 1))],
            buttons: vec![],
        }
    }

    fn toggle_row(&mut self, y: f32, key: &'static str, title: &str, cap: &str) -> f32 {
        let on = sp::value(self.s, key) != 0;
        let sw = self.switch_w(on);
        let caps: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.into())] };
        self.row(y, title, &caps, (sw, 20.0), |c, x, y| c.switch(key, x, y, on, title, cap), None, false)
    }

    /// A More row: a switch, a number or a choice, greyed with every reason when not in use.
    fn setting(&mut self, y: f32, key: &'static str, title: &str, cap: &str) -> f32 {
        let Some(r) = sp::find_row(key) else { return 0.0 };
        let v = sp::value(self.s, key);
        let why = sp::unused(self.s, key);
        let na = !why.is_empty() && matches!(r.kind, Kind::Step { .. });
        let mut caps: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.into())] };
        if na {
            let reasons: Vec<&str> = why.iter().map(|w| match w {
                sp::Why::AnimationNone => "Overview animation is None",
                sp::Why::Skip => "Skip animations is on",
                sp::Why::IndicatorOff => "the indicator is off",
            }).collect();
            caps.push(P::Why(format!(" Not in use: {}.", reasons.join(", and "))));
        }
        let help: String = caps.iter().map(|p| match p {
            P::T(s) | P::Why(s) => s.as_str(),
            P::Icon(..) => "",
        }).collect();
        match r.kind {
            Kind::Toggle => self.toggle_row(y, key, title, cap),
            Kind::Step { lo, hi, unit, .. } => {
                let sw = self.stepper_w(v, unit);
                self.row(y, title, &caps, (sw, 32.0), |c, x, y| c.stepper(key, x, y, (v, unit), (v > lo, v < hi), na, title, &help), None, false)
            }
            Kind::Choice(_) | Kind::Pair(..) => {
                let list = words_of(r);
                let sel = if matches!(r.kind, Kind::Pair(..)) { (v == 0) as usize } else { v as usize };
                let sw = self.seg_w(&list);
                self.row(y, title, &caps, (sw, 28.0), |c, x, y| c.seg(x, y, &list, sel, false, |i| Ctl::Choice(key, i), Ctl::Pick(key), title, &help), None, false)
            }
        }
    }

    // ---- Step 4: touchpad ----

    fn touchpad(&mut self, y: f32) -> f32 {
        let pv = self.pad();
        let mut h = 0.0;
        for (g, fingers) in [(0usize, 3u8), (1, 4)] {
            if g > 0 {
                h += 12.0;
            }
            let cap = format!("{} · click an action to change it", pv.head);
            let chips = pv.chips;
            h += self.card(y + h, |c, y| {
                let mut h = c.header(y, fingers, &cap);
                for (k, tap) in [(0usize, false), (1, true)] {
                    if k > 0 {
                        c.sep(y + h);
                    }
                    h += c.gesture_row(y + h, fingers, tap, chips[g * 2 + k]);
                }
                h
            });
        }
        h += 12.0;
        let (give, quit) = (pv.give, pv.quit);
        h += self.card(y + h, |c, y| {
            let mut h = 0.0;
            if give {
                let cap = format!("Undo: Windows uses its own gestures again from your next sign-in.{}", if quit { " Quit does the same." } else { "" });
                let bw = c.btn_w("Give back");
                h += c.row(y, "Give back to Windows", &[P::T(cap)], (bw, 32.0), |c, x, y| {
                    c.button(Ctl::Pad(Pad::GiveBack), x, y, "Give back", false);
                }, None, false);
                c.sep(y + h);
            }
            h += c.more(y + h, "Reading, swipe distance, wait for a second gesture");
            if c.w.more[Step::Pad.index()] {
                for (key, title, cap) in [
                    ("touchpad.enabled", "Read touchpad gestures", "Also in admin windows."),
                    ("touchpad.swipe_mm", "Swipe distance", "How far fingers move before a swipe counts."),
                    ("touchpad.twice_ms", "Wait for a second gesture", "Only for gestures with a twice action."),
                ] {
                    c.sep(y + h);
                    h += c.setting(y + h, key, title, cap);
                }
            }
            h
        });
        h
    }

    /// A card's tinted header: the fingers' circle, its title, a caption on the right.
    fn header(&mut self, y: f32, n: u8, cap: &str) -> f32 {
        let b = self.c.t.b;
        let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc, 0.09) };
        self.c.fill_rr((X0 + 1.0, y + 1.0, self.cw - 2.0, 36.0), 7.0, op(tint));
        self.c.fill_rr((X0 + 1.0, y + 20.0, self.cw - 2.0, 17.0), 0.0, op(tint));
        self.c.rule(X0 + 1.0, y + 37.0, self.cw - 2.0, op(b.line));
        let x = X0 + 16.0;
        self.c.circle(x + 11.0, y + 19.0, 11.0, Some(op(b.acc)), None);
        let d = n.to_string();
        let dw = self.c.width(&d, F::SmallBold);
        self.c.text(x + 11.0 - dw / 2.0, y + 8.0, 22.0, &d, F::SmallBold, b.on_acc);
        let title = format!("{n} fingers");
        self.c.text(x + 32.0, y + 8.0, 22.0, &title, F::Bold, b.tx);
        let cw = self.c.width(cap, F::Small);
        self.c.text(X0 + self.cw - 16.0 - cw, y + 8.0, 22.0, cap, F::Small, b.tx2);
        self.text_node(UIA_HeaderControlTypeId, &format!("{title}: {cap}"), None);
        38.0
    }

    fn gesture_row(&mut self, y: f32, fingers: u8, tap: bool, chip: Chip) -> f32 {
        let b = self.c.t.b;
        let motions: &[Motion] = if tap { &[Motion::Tap] } else { &[Motion::Left, Motion::Right, Motion::Up, Motion::Down] };
        let (x, px) = (X0 + 16.0, X0 + 16.0 + 116.0 + 16.0);
        let pw_all = X0 + self.cw - 16.0 - px;
        let cols = if tap { 1 } else { 2 };
        let pw = (pw_all - 8.0 * (cols as f32 - 1.0)) / cols as f32;
        let rows: Vec<usize> = motions.iter().filter_map(|m| settings::GESTURE_ROWS.iter().position(|r| *r == (fingers, *m))).collect();
        let mut ly = 0.0;
        for line in rows.chunks(cols) {
            let lh = line.iter().map(|&row| self.measured(|c| c.picker(row, 0.0, 0.0, pw))).fold(0.0, f32::max);
            for (k, &row) in line.iter().enumerate() {
                self.picker(row, px + k as f32 * (pw + 8.0), y + 8.0 + ly, pw);
            }
            ly += lh + 8.0;
        }
        let ph: f32 = ly - 8.0;
        self.c.text(x, y + 14.0, 20.0, if tap { "Tap" } else { "Swipe" }, F::Bold, b.tx);
        let (icon, text, col) = match chip {
            Chip::Ok => (Some(I_OK), "Off in Windows", b.good),
            Chip::Off => (None, "Off in Windows", b.tx2),
            Chip::On => (None, "Windows uses it", b.tx2),
            Chip::Err => (Some(I_WARN), "Can't read it", b.warn),
            Chip::Back => (None, "Back at next sign-in", b.tx2),
            Chip::Part => (Some(I_WARN), "Partly written back", b.warn),
            Chip::Not => (Some(I_WARN), "Not written back", b.warn),
        };
        let mut cx = x;
        if let Some(i) = icon {
            self.icon(cx, y + 36.0, 16.0, i, F::IconSmall, col);
            cx += 16.0;
        }
        self.c.text(cx, y + 36.0, 16.0, text, F::Small, b.tx2);
        let gesture = if tap { "tap" } else { "swipes" };
        self.text_node(UIA_TextControlTypeId, &format!("{fingers}-finger {gesture}: {text}"), None);
        // Its name and Windows' state for it, on the left, need 44 dip.
        8.0 + ph.max(44.0) + 8.0
    }

    /// A gesture picker: its direction mark, what it does, its twice action, ▾.
    fn picker(&mut self, row: usize, x: f32, y: f32, w: f32) -> f32 {
        let t = self.c.t;
        let b = t.b;
        let (fingers, motion) = settings::GESTURE_ROWS[row];
        let s = self.s;
        let once = s.rule(fingers, motion, 1);
        let twice = s.rule(fingers, motion, 2);
        let ctl = Ctl::Picker(row);
        let open = self.w.menu.is_some_and(|m| m.row == row);
        let waiting = self.w.rec == Some(Rec::Gesture(row, 1)) || self.w.rec == Some(Rec::Gesture(row, 2));
        let text = sp::once_label(once.unwrap_or(Action::Nothing));
        let sub = if twice.is_some() { format!("Twice: {}", sp::twice_label(twice)) } else { String::new() };
        let h = if sub.is_empty() { 32.0 } else { 40.0 };
        if waiting {
            // Key combo…: the field records, held keys and a ghost.
            self.c.fill_rr((x, y, w, h), 6.0, op(b.acc));
            self.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(b.card));
        } else if open {
            self.c.fill_rr((x, y, w, h), 6.0, op(b.acc));
            self.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(b.card));
        } else {
            self.c.boxed((x, y, w, h), 6.0, if self.hovered(ctl) { mix(b.track, b.tx, 0.05) } else { b.track }, Some(b.line));
        }
        let glyph = match motion {
            Motion::Left => "\u{E72B}",
            Motion::Right => "\u{E72A}",
            Motion::Up => "\u{E74A}",
            Motion::Down => "\u{E74B}",
            Motion::Tap => "",
        };
        let my = y + (h - 22.0) / 2.0;
        if t.hc {
            self.c.stroke_rr((x + 6.5, my + 0.5, 21.0, 21.0), 4.0, 1.0, op(b.acc));
        } else {
            self.c.fill_rr((x + 6.0, my, 22.0, 22.0), 4.0, op(b.held));
        }
        let mark = if t.dark && !t.hc { b.link } else { b.acc };
        if glyph.is_empty() {
            self.c.circle(x + 17.0, my + 11.0, 3.0, Some(op(mark)), None);
        } else {
            let gw = self.c.width(glyph, F::IconTiny);
            self.icon(x + 6.0 + (22.0 - gw) / 2.0, my, 22.0, glyph, F::IconTiny, mark);
        }
        let lx = x + 36.0;
        let lw = w - 36.0 - 26.0;
        if waiting {
            let ghost = "key";
            let gw = self.c.width(ghost, F::Small) + 12.0;
            self.c.stroke_dash((lx, my, gw, 22.0), 4.0, 1.0, op(b.tx3), DashStyleDash);
            self.c.text(lx + 6.0, my, 22.0, ghost, F::Small, b.tx3);
        } else if sub.is_empty() {
            let shown = self.c.fit(&text, F::Body, lw);
            self.c.text(lx, y, 32.0, &shown, F::Body, b.tx);
        } else {
            let shown = self.c.fit(&text, F::Body, lw);
            self.c.text(lx, y + 2.0, 20.0, &shown, F::Body, b.tx);
            let s2 = self.c.fit(&sub, F::Small, lw);
            self.c.text(lx, y + 21.0, 16.0, &s2, F::Small, b.tx2);
        }
        if !waiting {
            let cw = self.c.width(I_DOWN, F::IconTiny);
            self.icon(x + w - 10.0 - cw, y, h, I_DOWN, F::IconTiny, b.tx2);
        }
        self.ring((x, y, w, h), 6.0, ctl);
        self.hit(ctl, (x, y, w, h));
        let dir = match motion {
            Motion::Left => "swipe left",
            Motion::Right => "swipe right",
            Motion::Up => "swipe up",
            Motion::Down => "swipe down",
            Motion::Tap => "tap",
        };
        self.say(ctl, UIA_ComboBoxControlTypeId, &format!("{fingers} fingers, {dir}"), &sub, false, (None, Some(text)));
        h
    }

    /// The open menu, over everything: its lines in two columns and a footer, the chosen one
    /// checked, the default marked, kept inside the window.
    fn menu(&mut self) {
        let Some(m) = self.w.menu else { return };
        let t = self.c.t;
        let b = t.b;
        let lines = sp::menu_lines(&m, self.s);
        let (fingers, motion) = settings::GESTURE_ROWS[m.row];
        let dir = if motion == Motion::Tap { "tap".to_string() } else { format!("swipe {}", motion.word()) };
        let head = match m.family {
            Some(f) => sp::FAMILIES[f].0.to_string(),
            None if m.times == 2 => format!("{fingers} fingers, {dir}, done twice"),
            None => format!("{fingers} fingers, {dir}"),
        };
        let small = m.family.is_some() || m.times == 2;
        let mw = if small { 272.0 } else { 480.0 };
        // des-k's column is the wider one, as its names are longer.
        let colw = if small { mw - 8.0 } else { 252.0 };
        let colw1 = mw - 8.0 - 4.0 - colw;
        let n0 = lines.iter().filter(|l| l.col == 0).count();
        let n1 = lines.iter().filter(|l| l.col == 1).count();
        let rows = n0.max(n1) as f32;
        let feet = lines.iter().any(|l| l.col == 2) as u8 as f32 + lines.iter().any(|l| l.col == 3) as u8 as f32;
        let cols_head = if m.family.is_none() && m.times == 1 { 24.0 } else { 0.0 };
        let mh = 4.0 + 24.0 + cols_head + rows * 28.0 + feet * (9.0 + 28.0) + 4.0;
        let chosen = lines.iter().position(|l| l.on && l.col < 2);
        let off = chosen.map_or(0.0, |i| {
            let l = &lines[i];
            let k = lines.iter().take(i).filter(|x| x.col == l.col).count();
            4.0 + 24.0 + cols_head + k as f32 * 28.0
        });
        let my = (m.at.1 - off).clamp(8.0, (self.w.vh - mh - 8.0).max(8.0));
        let mx = m.at.0.min(self.w.vw - mw - 16.0).max(8.0);
        self.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::alpha(0, 0.08));
        }
        self.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.hit(Ctl::Item(usize::MAX), (mx, my, mw, mh));
        self.c.text(mx + 14.0, my + 4.0, 24.0, &head, F::SmallBold, b.tx2);
        let mut ys = [my + 28.0; 2];
        if cols_head > 0.0 {
            self.c.text(mx + 14.0, my + 28.0, 24.0, "des-k", F::SmallBold, b.tx2);
            self.c.text(mx + 4.0 + colw + 4.0 + 10.0, my + 28.0, 24.0, "Windows and media", F::SmallBold, b.tx2);
            ys = [my + 52.0; 2];
        }
        let foot_y = my + 4.0 + 24.0 + cols_head + rows * 28.0;
        let mut fx = 0.0;
        let mut drew_rule = [false; 2];
        for (i, l) in lines.iter().enumerate() {
            let (x, y, w) = match l.col {
                0 | 1 => {
                    let c = if small { 0 } else { l.col as usize };
                    let y = ys[c];
                    ys[c] += 28.0;
                    (mx + 4.0 + c as f32 * (colw + 4.0), y, if c == 0 { colw } else { colw1 })
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
                    let w = if small { mw - 8.0 } else { (mw - 8.0 - 4.0 * (per - 1.0)) / per };
                    let x = mx + 4.0 + if small { 0.0 } else { fx };
                    fx += w + 4.0;
                    (x, y + if small { (lines.iter().take(i).filter(|x| x.col == c).count()) as f32 * 28.0 } else { 0.0 }, w)
                }
            };
            let ctl = Ctl::Item(i);
            let hot = self.hovered(ctl) || (self.w.kb && m.focus == i);
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
            let more = matches!(l.does, Does::Family(_) | Does::Twice);
            let room = w - 30.0 - 6.0 - if l.default { 52.0 } else { 0.0 } - if more { 16.0 } else { 0.0 };
            let shown = self.c.fit(&l.text, F::Body, room);
            self.c.text(x + 30.0, y, 28.0, &shown, F::Body, ink);
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
            if self.w.kb && m.focus == i {
                self.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (x, y, w, 28.0));
        }
    }

    // ---- Step 5: done ----

    fn done(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let s = self.s;
        let pad = self.w.pics.pad.is_none_or(|p| p.pad);
        let pv = self.pad();
        let mut h = self.card(y, |c, y| {
            let mut h = 0.0;
            let tp = if pad {
                let (icon, col) = if pv.ready { (I_OK, b.good) } else { (I_WARN, b.warn) };
                vec![P::Icon(icon, col), P::T(format!(" {}", pv.done_line))]
            } else {
                vec![P::T("No precision touchpad found. Plug one in any time and set it up in des-k settings › Touchpad.".into())]
            };
            let button = pad && pv.done_button;
            let bw = if button { c.btn_w("Open Windows Settings") } else { 0.0 };
            h += c.row(y, "Touchpad", &tp, (bw, 32.0), |c, x, y| {
                if button {
                    c.button(Ctl::Pad(Pad::Windows), x, y, "Open Windows Settings", false);
                }
            }, None, false);
            c.sep(y + h);
            let cap = "Every key and gesture. Right-click the des-k icon by the clock, then Shortcut hints. Don't see it? Click ^ first.";
            let bw = c.btn_w("Show");
            h += c.row(y + h, "Shortcut hints", &[P::T(cap.into())], (bw, 32.0), |c, x, y| {
                c.button(Ctl::Show, x, y, "Show", false);
            }, None, false);
            c.sep(y + h);
            h += c.hints_row(y + h);
            h
        });
        h += 16.0;
        h += self.c.para(X0, y + h, self.cw, 20.0, &[("Your main shortcuts", F::Bold, b.tx)]) + 8.0;
        h += self.card(y + h, |c, y| {
            let main: [(&str, &[&'static str]); 4] = [
                ("Switch workspace", &["shortcuts.previous", "shortcuts.next"]),
                ("Send window to next or previous", &["shortcuts.send_previous", "shortcuts.send_next"]),
                ("Open overview", &["shortcuts.overview"]),
                ("Find a window", &["shortcuts.finder"]),
            ];
            let mut h = 0.0;
            for (i, (title, keys)) in main.iter().enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                let caps = row_caps(s, keys);
                let kw = caps.as_ref().map_or_else(|| c.c.width("No key", F::Small), |k| c.caps_w(k));
                let top = 28.0;
                let ry = y + h + 6.0;
                c.c.text(X0 + 16.0, ry, top, title, F::Body, b.tx);
                match &caps {
                    Some(k) => {
                        c.caps(X0 + c.cw - 16.0 - kw, ry + 3.0, k, false);
                    }
                    None => c.c.text(X0 + c.cw - 16.0 - kw, ry, top, "No key", F::Small, b.tx2),
                }
                c.text_node(UIA_ListItemControlTypeId, &format!("{title}: {}", caps.as_ref().map_or("no key".into(), |k| words(k))), None);
                h += 40.0;
            }
            c.sep(y + h);
            let line = taken_line(s);
            let lh = c.c.para(X0 + 16.0, y + h + 6.0, c.cw - 32.0, 16.0, &[(&line, F::Small, b.tx2)]);
            c.text_node(UIA_TextControlTypeId, &line, None);
            h + 6.0 + lh.max(28.0) + 6.0
        });
        h
    }

    /// The hints key: Set key, the key and Change, or recording it.
    fn hints_row(&mut self, y: f32) -> f32 {
        let s = self.s;
        let f = Field::Hints;
        let caps = row_caps(s, &["shortcuts.hints"]);
        let note = self.key_note(f);
        let cap = match (&note, &caps) {
            (Some(_), _) => vec![],
            (None, Some(_)) => vec![P::T("Opens the hints on the screen under your mouse.".into())],
            (None, None) => vec![P::T("Not set.".into())],
        };
        let rec = note.as_ref().map(|(_, held)| held.clone());
        let n = note.map(|(n, _)| n);
        let (cw, ch) = match (&rec, &caps) {
            (Some(held), _) => (self.measured(|c| c.key_field(f, 0.0, 0.0, None, None, Some((held.as_slice(), "key")), "")), 32.0),
            (None, Some(k)) => (self.caps_w(k) + 8.0 + self.btn_w("Change"), 32.0),
            (None, None) => (self.btn_w("Set key"), 32.0),
        };
        self.row(y, "Shortcut hints key", &cap, (cw, ch), |c, x, y| match (&rec, &caps) {
            (Some(held), _) => {
                c.key_field(f, x, y, None, None, Some((held.as_slice(), "key")), "");
            }
            (None, Some(k)) => {
                let kw = c.caps(x, y + 5.0, k, false);
                c.button(Ctl::Field(f), x + kw + 8.0, y, "Change", false);
            }
            (None, None) => {
                c.button(Ctl::Field(f), x, y, "Set key", false);
            }
        }, n.as_ref(), n.is_some())
    }

    // ---- The footer ----

    fn footer(&mut self) {
        let b = self.c.t.b;
        let (vw, vh) = (self.w.vw, self.w.vh);
        let y = vh - FOOT;
        self.c.fill_rr((0.0, y, vw, FOOT), 0.0, op(b.band));
        self.c.rule(0.0, y, vw, op(b.line));
        let by = y + 16.0;
        let mut x = X0;
        let step = self.w.step;
        if step == Step::Try && !self.w.tried.iter().all(|t| *t) {
            x += self.button(Ctl::Skip, x, by, "Skip", false) + 8.0;
        }
        if step != Step::Try {
            self.button(Ctl::Back, x, by, "Back", false);
        }
        let next = if step == Step::Done { "Done" } else { "Next" };
        let nw = self.btn_w(next);
        self.button(Ctl::Next, vw - X0 - nw, by, next, true);
    }
}

/// "Taken over from Windows: …", from live settings.
fn taken_line(s: &Settings) -> String {
    let mut keys: Vec<String> = vec![];
    let has = |k: &str| sp::shortcut(k).is_some_and(|sh| sp::has_win(s, sh));
    for (k, w) in [("shortcuts.overview", "Win Tab"), ("shortcuts.finder", "Win F")] {
        if has(k) {
            keys.push(w.into());
        }
    }
    if has("shortcuts.previous") && has("shortcuts.next") {
        keys.push("Win Ctrl ← →".into());
    } else if has("shortcuts.previous") {
        keys.push("Win Ctrl ←".into());
    } else if has("shortcuts.next") {
        keys.push("Win Ctrl →".into());
    }
    match (has("shortcuts.jump"), has("shortcuts.send_to"), has("shortcuts.send_to_stay")) {
        (true, true, true) => keys.push("Win 1–9 (also with Shift or Ctrl)".into()),
        (j, sh, c) => {
            for (on, w) in [(j, "Win 1–9"), (sh, "Win Shift 1–9"), (c, "Win Ctrl 1–9")] {
                if on {
                    keys.push(w.into());
                }
            }
        }
    }
    if s.copilot_as_right_ctrl {
        keys.push("the Copilot key".into());
    }
    match keys.len() {
        0 => "des-k takes no keys from Windows. Change any in des-k settings › Shortcuts.".into(),
        1 => format!("Taken over from Windows: {}. Change any in des-k settings › Shortcuts.", keys[0]),
        n => format!("Taken over from Windows: {} and {}. Change any in des-k settings › Shortcuts.", keys[..n - 1].join(", "), keys[n - 1]),
    }
}

/// A choice row's words, as the first run shows them.
fn words_of(r: &sp::Row) -> Vec<&'static str> {
    match r.kind {
        Kind::Choice(list) => list.iter().map(|(_, l)| *l).collect(),
        Kind::Pair(on, off) => vec![on, off],
        _ => vec![],
    }
}

/// The whole window, back to front: the step, then what sits above and below it, the scroll
/// thumb and the open menu.
fn compose(c: &mut Canvas, w: &Win) -> Out {
    let s = settings::get();
    let cw = w.vw - 2.0 * X0;
    let mut x = Ctx { c, w, s, cw, group: 0, out: Out { hits: vec![], content: 0.0, mid: (0.0, 0.0), said: vec![], texts: vec![] } };
    let b = x.c.t.b;
    let head = x.measured(|c| c.head());
    let (top, bottom) = (head + 16.0, w.vh - FOOT - 8.0);
    x.out.mid = (top, bottom);
    x.group = 1;
    let y0 = top - w.scroll;
    let content = match w.step {
        Step::Try => x.try_it(y0),
        Step::Keys => x.keys(y0),
        Step::Behavior => x.behavior(y0),
        Step::Pad => x.touchpad(y0),
        Step::Done => x.done(y0),
    };
    x.out.content = content;
    // The head and the footer cover the step scrolled under them; a line marks a cut edge.
    x.c.fill_rr((0.0, 0.0, w.vw, top), 0.0, op(b.bg));
    x.c.fill_rr((0.0, bottom, w.vw, w.vh - bottom), 0.0, op(b.bg));
    // Drawn after the step, it still comes first for Narrator and the Tab order.
    x.group = 0;
    let (t0, s0) = (x.out.texts.len(), x.out.said.len());
    x.head();
    let k = x.out.texts.len() - t0;
    x.out.texts.rotate_right(k);
    let k = x.out.said.len() - s0;
    x.out.said.rotate_right(k);
    let view = bottom - top;
    if w.scroll > 0.5 {
        x.c.rule(X0 - 1.0, top, cw + 2.0, op(b.line));
    }
    if w.scroll < content - view - 1.0 {
        x.c.rule(X0 - 1.0, bottom - 1.0, cw + 2.0, op(b.line));
    }
    x.group = 2;
    x.footer();
    if let Some((ty, th)) = thumb(w.scroll, content, view, top) {
        x.c.fill_rr((w.vw - 5.0, ty, 2.0, th), 1.0, op(b.tx3));
    }
    x.group = 3;
    x.menu();
    // The menu's hits first.
    x.out.hits.sort_by_key(|h| std::cmp::Reverse(h.group == 3));
    x.out
}

/// The thumb's top and height, when the step holds more than its view.
fn thumb(scroll: f32, content: f32, view: f32, top: f32) -> Option<(f32, f32)> {
    if content - view <= 1.0 {
        return None;
    }
    let th = ((view - 8.0) * view / content).max(24.0);
    let ty = top + 4.0 + (view - 8.0 - th) * (scroll / (content - view).max(1.0));
    Some((ty, th))
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the window out again for its scroll, keeps the focus on a stop that is still there,
/// and gives Narrator the step as painted.
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
            let view = out.mid.1 - out.mid.0;
            let max = (out.content - view).max(0.0);
            let scroll = if std::mem::take(&mut w.to_end) { max } else { w.scroll.clamp(0.0, max) };
            let moved = scroll != w.scroll;
            w.scroll = scroll;
            w.content = out.content;
            w.mid = out.mid;
            w.hits = out.hits;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                w.focus = Some(Ctl::Next);
            }
            let s = w.scale;
            let px = |b: Bx| RECT { left: (b.0 * s) as i32, top: (b.1 * s) as i32, right: ((b.0 + b.2) * s) as i32, bottom: ((b.1 + b.3) * s) as i32 };
            let mut kids: Vec<Node> = out.texts;
            kids.extend(out.said.into_iter().map(|(ctl, mut n)| {
                if let Some(h) = w.hits.iter().find(|h| h.ctl.stop() == ctl) {
                    n.rect = px(h.at);
                }
                n.focused = w.focus == Some(ctl);
                n
            }));
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

/// Reads des-k and Windows again and redraws, if the first run is open: after every settings
/// reload and on WM_TOUCHPADS_CHANGED.
pub fn refresh() {
    if hwnd().is_some() {
        update_pictures();
        layout();
    }
}

/// Main loop, every second and on a display change: the screens and the touchpad follow you.
pub fn tick() {
    if let Some(h) = hwnd()
        && !unsafe { IsIconic(h) }.as_bool()
        && update_pictures()
    {
        layout();
    }
}

/// Takes a new copy of the pictures' data; true when it changed.
fn update_pictures() -> bool {
    let n = settings::get().workspaces;
    let Some(view) = crate::ws::view(&[n.saturating_sub(1).max(2)]) else { return false };
    let st = setup::state();
    // A give back that worked is over once Windows' gestures read off again; a failed one
    // stays until a later give back replaces it (AC-FR-5).
    if st.off && LAST.get().is_some_and(|b| !b.failed()) {
        LAST.set(None);
    }
    let new = Pics { screens: crate::ws::screen_info(), view, pad: Some(st), mouse: crate::ws::under_mouse().1 };
    set(|w| {
        let changed = w.pics != new;
        w.pics = new;
        changed
    }) == Some(true)
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

/// Scrolls the focused stop into view, and tells Narrator.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().filter(|h| Some(h.ctl.stop()) == w.focus && h.group == 1).map(|h| h.at).next()?;
        let (top, bottom) = (h.1 - 12.0, h.1 + h.3 + 12.0);
        if top < w.mid.0 {
            Some(top - w.mid.0)
        } else if bottom > w.mid.1 {
            Some(bottom - w.mid.1)
        } else {
            None
        }
    })
    .flatten();
    match dy {
        Some(dy) => scroll_by(dy),
        None => layout(),
    }
    if let Some((h, tree)) = get(|w| (w.hwnd, w.tree.clone())) {
        uia::focus_moved(h, &tree, WHO);
    }
}

/// Writes one value; a failure shows what stays and why until the next good save.
fn save(key: &'static str, v: i64) {
    let Some(row) = sp::find_row(key) else { return };
    commit(Edit::Value(key, sp::file_value(row, v)), row.title);
}

fn commit(edit: Edit, title: &str) {
    match settings::write(edit) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
        }
        Err(why) => {
            let said = why.rsplit_once(" (").map_or(why.clone(), |(_, w)| w.trim_end_matches(')').to_string());
            let _ = set(|w| w.error = Some((title.to_string(), said)));
        }
    }
    refresh();
}

fn run(plan: Plan) {
    match plan {
        Plan::Nothing => {}
        Plan::Commit(edit, title) => commit(edit, title),
        Plan::Ask(p) => {
            let _ = set(|w| {
                w.prompt = Some(p);
                w.focus = Some(Ctl::Cancel);
            });
            layout();
            reveal();
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
        w.focus = Some(Ctl::Cancel);
    });
    // One recording at a time.
    crate::hints::cancel_recording();
    crate::settings_page::cancel_recording();
    if let Some(h) = hwnd() {
        crate::keys::record(h, true);
    }
    layout();
    reveal();
}

fn stop_rec() {
    if let Some(h) = hwnd() {
        crate::keys::record(h, false);
    }
    let _ = set(|w| {
        let f = w.rec.and_then(field_of);
        w.rec = None;
        w.refused = None;
        if w.focus == Some(Ctl::Cancel) {
            w.focus = Some(f.map_or(Ctl::Next, Ctl::Field));
        }
    });
    layout();
}

/// Another window starts recording, so a recording here stops.
pub fn cancel_recording() {
    if get(|w| w.rec.is_some()) == Some(true) {
        stop_rec();
    }
}

/// The window the hook is recording for is this one.
pub fn owns(h: HWND) -> bool {
    hwnd() == Some(h)
}

/// The hook's WM_RECORDED: the first key down that is not a modifier, and the modifiers held
/// with it. Every key but Esc alone is recorded (AC-FR-6).
pub fn recorded(vk: u16, mods: u8) {
    let Some(rec) = get(|w| w.rec).flatten() else { return };
    if vk == VK_ESCAPE.0 && mods == 0 {
        return stop_rec();
    }
    let refuse = |title: &str, sub: &str| {
        let _ = set(|w| w.refused = Some((title.into(), sub.into())));
        layout();
    };
    let title = |r: Rec| field_of(r).map_or("Gestures", title_of);
    match rec {
        Rec::Digits(key) => {
            let digit = (b'1' as u16..=b'9' as u16).contains(&vk) || (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&vk);
            if !digit {
                return refuse("Press a digit 1 to 9", "Hold the keys, then press any digit. Still recording; Esc alone cancels.");
            }
            if mods & (WIN | CTRL | ALT) == 0 {
                return refuse(if mods == SHIFT { "Shift alone can't be used" } else { "A digit needs Win, Ctrl or Alt" }, "Hold Win, Ctrl or Alt with the digit. Still recording.");
            }
            stop_rec();
            run(sp::plan_digits(key, mods));
        }
        Rec::Pair(p, n) => {
            if vk != VK_LEFT.0 && vk != VK_RIGHT.0 {
                return refuse("Press ← or →", "Hold the keys, then press either arrow. Still recording; Esc alone cancels.");
            }
            if mods == 0 {
                return refuse("A key needs Win, Ctrl, Alt or Shift", "Hold one of them with the arrow. Still recording; Esc alone cancels.");
            }
            stop_rec();
            run(plan_pair(p, n, mods, true, title(rec)));
        }
        Rec::List(..) | Rec::Gesture(..) => {
            if !settings::savable(vk) {
                return refuse("This key can't be saved", "Use a letter, a digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Still recording.");
            }
            if mods == 0 {
                return refuse("A key needs Win, Ctrl, Alt or Shift", "Hold one of them with the key. Still recording; Esc alone cancels.");
            }
            stop_rec();
            let combo = Combo { mods, vk };
            match rec {
                Rec::List(key, replace) => run(sp::plan_combo(key, replace, combo)),
                Rec::Gesture(row, times) => {
                    if let Some(e) = sp::gesture_edit(row, times, Some(Action::Combo(combo))) {
                        commit(e, "Gestures");
                    }
                }
                _ => {}
            }
        }
    }
}

/// Step 1: you did something it asks you to try (main and the touchpad call this).
pub(crate) fn tried(t: Try) {
    if hwnd().is_none() {
        return;
    }
    let switched = (t == Try::Switch).then(|| {
        let (_, dev) = crate::ws::under_mouse();
        let screens = crate::ws::screen_info();
        let view = crate::ws::view(&[]);
        let to = view.as_ref().and_then(|v| v.active.iter().find(|(d, _)| *d == dev).map(|(_, a)| *a)).unwrap_or(0);
        let name = view.as_ref().and_then(|v| v.names.iter().find(|(d, _)| *d == dev).map(|(_, n)| n.clone())).or_else(|| screens.iter().find(|s| s.device == dev).map(|s| s.name.clone())).unwrap_or_default();
        (name, to, screens.len() < 2)
    });
    let _ = set(|w| {
        w.tried[t as usize] = true;
        if let Some(sw) = switched {
            w.switched = Some(sw);
        }
    });
    update_pictures();
    layout();
}

/// Goes to a step: the window grows or shrinks from its top edge, Next has the focus.
fn go(step: Step) {
    let Some(h) = hwnd() else { return };
    if get(|w| w.rec.is_some()) == Some(true) {
        stop_rec();
    }
    let _ = set(|w| {
        w.step = step;
        w.scroll = 0.0;
        w.prompt = None;
        w.menu = None;
        w.refused = None;
        w.focus = Some(Ctl::Next);
    });
    let (from, to, cap) = get(|w| (w.vh, step.height(), w.cap)).unwrap_or((SHORT, SHORT, TALL));
    let to = to.min(cap);
    let ms = ui::motion_ms(GROW_MS);
    if (to - from).abs() < 0.5 || ms == 0 {
        client_height(h, to);
    } else {
        let _ = set(|w| w.grow = Some((Instant::now(), from, to, ms)));
        unsafe { SetTimer(Some(h), GROW_TIMER, 10, None) };
    }
    layout();
    reveal();
}

/// Sets the client height, keeping the top edge where it is.
fn client_height(h: HWND, ch: f32) {
    let (ww, wh) = size(h, W, ch);
    unsafe { let _ = SetWindowPos(h, None, 0, 0, ww, wh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE); }
}

fn grow_tick(h: HWND) {
    let Some((t0, from, to, ms)) = get(|w| w.grow).flatten() else {
        unsafe { let _ = KillTimer(Some(h), GROW_TIMER); }
        return;
    };
    let u = (t0.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0);
    let e = 1.0 - (1.0 - u).powi(3);
    client_height(h, from + (to - from) * e);
    if u >= 1.0 {
        let _ = set(|w| w.grow = None);
        unsafe { let _ = KillTimer(Some(h), GROW_TIMER); }
    }
}

fn next_step(back: bool) -> Option<Step> {
    get(|w| {
        let list = steps(w);
        let i = list.iter().position(|s| *s == w.step)?;
        if back { i.checked_sub(1).map(|j| list[j]) } else { list.get(i + 1).copied() }
    })
    .flatten()
}

fn step_key(key: &'static str, up: bool) {
    let s = settings::get();
    let Some(sp::Row { kind: Kind::Step { lo, hi, by, .. }, .. }) = sp::find_row(key) else { return };
    // Greyed: kept but not in use, so − and + do nothing.
    if !sp::unused(s, key).is_empty() {
        return;
    }
    let v = sp::value(s, key);
    let n = sp::stepped(v, *lo, *hi, *by, up);
    if n != v {
        save(key, n);
    }
}

fn open_menu(row: usize, times: u8, family: Option<usize>) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == Ctl::Picker(row)).map(|h| h.at)).flatten() else { return };
    let m = Menu { row, times, family, at, focus: 0 };
    let focus = sp::menu_lines(&m, settings::get()).iter().position(|l| l.on).unwrap_or(0);
    let _ = set(|w| w.menu = Some(Menu { focus, ..m }));
    layout();
}

fn close_menu() {
    let _ = set(|w| w.menu = None);
    layout();
}

fn choose(i: usize) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let lines = sp::menu_lines(&m, settings::get());
    let Some(l) = lines.get(i) else { return };
    match l.does {
        Does::Set(a) => {
            close_menu();
            if let Some(e) = sp::gesture_edit(m.row, m.times, a) {
                commit(e, "Gestures");
            }
        }
        Does::Combo => start_rec(Rec::Gesture(m.row, m.times)),
        Does::Family(f) => open_menu(m.row, m.times, Some(f)),
        Does::Twice => open_menu(m.row, 2, None),
    }
}

/// Give back, from here or the settings page: one result both screens show.
pub(crate) fn give() {
    if let Some(b) = setup::give_back() {
        LAST.set(Some(b));
    }
    refresh();
}

/// The last give back's result, while it lasts: the hints window and the settings page word
/// their touchpad state from it too.
pub(crate) fn gave_back() -> Option<GaveBack> {
    LAST.get()
}

fn act(ctl: Ctl) {
    let Some(h) = hwnd() else { return };
    let s = settings::get();
    match ctl {
        Ctl::Next => match next_step(false) {
            Some(st) => go(st),
            None => unsafe { let _ = DestroyWindow(h); },
        },
        Ctl::Back => {
            if let Some(st) = next_step(true) {
                go(st);
            }
        }
        Ctl::Skip => go(Step::Done),
        Ctl::Field(f) => {
            if let Some(r) = rec_for(f) {
                start_rec(r);
            }
        }
        Ctl::SideAt(6, desk) => {
            if s.copilot_as_right_ctrl != desk {
                commit(Edit::Value("copilot_as_right_ctrl", desk.into()), TAKES[6].title);
            }
        }
        Ctl::SideAt(i, desk) => run(plan_side(i, desk)),
        Ctl::Side(_) | Ctl::Pick(_) => {}
        Ctl::More => {
            let _ = set(|w| {
                let i = w.step.index();
                w.more[i] = !w.more[i];
                w.to_end = w.more[i]; // opens on its end, as the design scrolls it
            });
            layout();
        }
        Ctl::Toggle(k) => save(k, 1 - sp::value(s, k)),
        Ctl::Minus(k) => step_key(k, false),
        Ctl::Plus(k) => step_key(k, true),
        Ctl::Choice(k, i) => match sp::find_row(k).map(|r| r.kind) {
            Some(Kind::Pair(..)) => save(k, (i == 0) as i64),
            _ => save(k, i as i64),
        },
        Ctl::Picker(row) => open_menu(row, 1, None),
        Ctl::Item(usize::MAX) => {}
        Ctl::Item(i) => choose(i),
        Ctl::Pad(Pad::Windows) => {
            setup::use_gestures();
            refresh();
        }
        Ctl::Pad(Pad::ReadOn) => save("touchpad.enabled", 1),
        Ctl::Pad(Pad::GiveBack) => give(),
        Ctl::Show => crate::hints::show(),
        Ctl::Cancel => {
            if get(|w| w.rec.is_some()) == Some(true) {
                stop_rec();
            } else {
                let _ = set(|w| {
                    let f = w.prompt.take().and_then(|p| field_of_key(p.key));
                    w.focus = Some(f.map_or(Ctl::Next, Ctl::Field));
                });
                layout();
            }
        }
        Ctl::Move => {
            if let Some(p) = set(|w| w.prompt.take()).flatten() {
                let f = field_of_key(p.key);
                let _ = set(|w| w.focus = Some(f.map_or(Ctl::Next, Ctl::Field)));
                commit(p.edit, f.map_or("Shortcuts", title_of));
            }
        }
    }
}

/// Left and Right on a choice: it moves.
fn change(focus: Ctl, right: bool) {
    let s = settings::get();
    match focus {
        Ctl::Pick(k) => {
            let Some(r) = sp::find_row(k) else { return };
            let n = words_of(r).len();
            let i = sp::value(s, k) as usize;
            let i = match r.kind {
                Kind::Pair(..) => 1 - sp::value(s, k) as usize,
                _ => i,
            };
            let to = if right { (i + 1).min(n - 1) } else { i.saturating_sub(1) };
            if to != i {
                act(Ctl::Choice(k, to));
            }
        }
        Ctl::Side(i) => act(Ctl::SideAt(i, !right)),
        _ => return,
    }
    reveal();
}

/// The open menu's keys: Up and Down move, Enter or Space picks, Esc closes or goes back.
fn menu_key(vk: VIRTUAL_KEY) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let n = sp::menu_lines(&m, settings::get()).len();
    match vk {
        VK_ESCAPE if m.family.is_some() || m.times == 2 => open_menu(m.row, 1, None),
        VK_ESCAPE => close_menu(),
        VK_UP | VK_DOWN | VK_HOME | VK_END | VK_LEFT | VK_RIGHT => {
            let f = match vk {
                VK_UP | VK_LEFT => m.focus.saturating_sub(1),
                VK_DOWN | VK_RIGHT => (m.focus + 1).min(n - 1),
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

/// Every stop in Tab order: above the step, the step, the footer.
fn stops(w: &Win) -> Vec<Ctl> {
    let mut hits: Vec<&Hit> = w.hits.iter().filter(|h| h.group != 3).collect();
    hits.sort_by_key(|h| h.group);
    let mut out: Vec<Ctl> = vec![];
    for h in hits {
        let c = h.ctl.stop();
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    if get(|w| w.menu.is_some()) == Some(true) {
        return menu_key(vk);
    }
    let focus = get(|w| w.focus).flatten();
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match vk {
        VK_ESCAPE => {
            let layer = get(|w| w.rec.is_some() || w.prompt.is_some()) == Some(true);
            if layer {
                act(Ctl::Cancel);
                reveal();
            } else {
                unsafe { let _ = DestroyWindow(h); }
            }
        }
        VK_TAB => {
            let next = get(|w| {
                let list = stops(w);
                let i = w.focus.and_then(|f| list.iter().position(|c| *c == f));
                let n = list.len();
                if n == 0 {
                    return None;
                }
                Some(match (i, shift) {
                    (Some(i), false) => list[(i + 1) % n],
                    (Some(i), true) => list[(i + n - 1) % n],
                    (None, _) => list[0],
                })
            })
            .flatten();
            if let Some(c) = next {
                let _ = set(|w| w.focus = Some(c));
                reveal();
            }
        }
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
            if let Some(f) = focus {
                change(f, matches!(vk, VK_RIGHT | VK_DOWN));
            }
        }
        VK_RETURN => match focus {
            Some(f) if f.button() => {
                act(f);
                reveal();
            }
            _ => act(Ctl::Next),
        },
        VK_SPACE => {
            if let Some(f) = focus.filter(|f| !matches!(f, Ctl::Pick(_) | Ctl::Side(_))) {
                act(f);
                reveal();
            }
        }
        VK_PRIOR | VK_NEXT => {
            let page = get(|w| w.mid.1 - w.mid.0 - 40.0).unwrap_or(0.0);
            scroll_by(if vk == VK_PRIOR { -page } else { page });
        }
        _ => repaint(),
    }
}

fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. The step's controls count only inside its view; while the menu
/// is open, only its lines do.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let menu = w.menu.is_some();
    let shown = |h: &Hit| if menu { h.group == 3 } else { h.group != 1 || (y >= w.mid.0 && y < w.mid.1) };
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the first run on the screen under the mouse, or brings the open one to the front
/// (AC-FR-1).
pub fn show() {
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
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
        RegisterClassW(&wc); // fails harmlessly when it opens a second time
        let (mon, _) = crate::ws::under_mouse();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("des-k"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return warn!("first run: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        crate::ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        // The tallest step must fit under the title bar; on a short screen both heights cap.
        let (_, frame) = size(h, W, 0.0);
        let cap = (((work.bottom - work.top) - frame) as f32 / scale).min(TALL);
        FIRST.set(Some(Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            step: Step::Try,
            more: [false; 5],
            tried: [false; 3],
            switched: None,
            scroll: 0.0,
            to_end: false,
            content: 0.0,
            mid: (0.0, 0.0),
            hits: vec![],
            focus: Some(Ctl::Next),
            kb: false,
            hover: None,
            press: None,
            vw: W,
            vh: SHORT.min(cap),
            cap,
            grow: None,
            rec: None,
            refused: None,
            prompt: None,
            menu: None,
            error: None,
            pics: Pics::default(),
            tree: Arc::new(Mutex::new(Node::default())),
        }));
        let (ww, wh) = size(h, W, SHORT.min(cap));
        let (_, tall) = size(h, W, cap);
        // Placed so the tallest step fits below the same top edge.
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - tall) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        update_pictures();
        layout();
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
            WM_KEYDOWN => {
                on_key(h, VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            WM_TIMER if wp.0 == GROW_TIMER => {
                grow_tick(h);
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
                // A click while recording only cancels it.
                if get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                    return LRESULT(0);
                }
                if get(|w| w.menu.is_some() && hit_at(w, point(lp, w.scale)).is_none()) == Some(true) {
                    close_menu();
                    return LRESULT(0);
                }
                let _ = SetCapture(h);
                let _ = set(|w| {
                    w.kb = false;
                    w.press = hit_at(w, point(lp, w.scale));
                    if let Some(p) = w.press.filter(|p| !matches!(p, Ctl::Item(_))) {
                        w.focus = Some(p.stop());
                    }
                });
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if set(|w| {
                    let hover = hit_at(w, point(lp, w.scale));
                    std::mem::replace(&mut w.hover, hover) != hover
                }) == Some(true)
                {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    layout(); // pointing at − shows what lowering moves
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
                    let now = hit_at(w, point(lp, w.scale));
                    (was.is_some() && was == now).then_some(was).flatten()
                })
                .flatten();
                if let Some(c) = click {
                    act(c);
                }
                repaint();
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
            WM_ACTIVATE => {
                // Recording ends when the window loses focus, and an open menu closes.
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
                let _ = KillTimer(Some(h), GROW_TIMER);
                let _ = UiaReturnRawElementProvider(h, WPARAM(0), LPARAM(0), None);
                drop(FIRST.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let to = *(lp.0 as *const RECT);
                let scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                let vh = set(|w| {
                    w.scale = scale;
                    w.fonts = Fonts::new(scale);
                    w.vh
                })
                .unwrap_or(SHORT);
                let (ww, wh) = size(h, W, vh);
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
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

    // The Done step's last line names exactly the Windows keys des-k holds: a key given back
    // must leave the line, or you'd think Windows still lost it.
    #[test]
    fn the_taken_line_follows_what_des_k_holds() {
        let d = Settings::default();
        assert_eq!(taken_line(&d), "Taken over from Windows: Win Tab, Win F, Win Ctrl ← →, Win 1–9 (also with Shift or Ctrl) and the Copilot key. Change any in des-k settings › Shortcuts.");
        let s = Settings { finder: vec![], send_to: None, copilot_as_right_ctrl: false, ..Settings::default() };
        assert_eq!(taken_line(&s), "Taken over from Windows: Win Tab, Win Ctrl ← →, Win 1–9 and Win Ctrl 1–9. Change any in des-k settings › Shortcuts.");
        let none = Settings { finder: vec![], overview: vec![], previous: vec![], next: vec![], jump: None, send_to: None, send_to_stay: None, copilot_as_right_ctrl: false, ..Settings::default() };
        assert!(taken_line(&none).starts_with("des-k takes no keys"));
    }

    // A row's words must say where its Windows key went, so giving one back never leaves you
    // guessing whether Windows has it (AC-FR-4): des-k's, given back bare, your own key, taken.
    #[test]
    fn a_shortcut_row_says_where_its_windows_key_is() {
        let d = Settings::default();
        assert_eq!(held(&d, TAKES[2].keys), Held::Desk);
        let bare = Settings { finder: vec![], ..Settings::default() };
        assert_eq!(held(&bare, TAKES[2].keys), Held::Bare);
        let own = Settings { finder: vec![Combo { mods: WIN | ALT, vk: b'F' as u16 }], ..Settings::default() };
        assert_eq!(held(&own, TAKES[2].keys), Held::Own(None));
        // Go to workspace took Win Ctrl 1–9 from Send window to 1–9 and stay.
        let moved = Settings { jump: Some(WIN | CTRL), send_to_stay: None, ..Settings::default() };
        assert_eq!(held(&moved, TAKES[3].keys), Held::Own(Some("Send window to 1–9 and stay")));
        assert!(matches!(held(&moved, TAKES[5].keys), Held::Took("Go to workspace 1–9")));
        assert_eq!(take_caption(&moved, 5), "No key. Win Ctrl 1–9 now goes to workspace 1–9.");
        // The ← → pair is des-k's only while both arrows are.
        let half = Settings { next: vec![], ..Settings::default() };
        assert_ne!(held(&half, TAKES[0].keys), Held::Desk);
    }

    // A pair row shows one key only when both arrows share their modifiers, as you set them
    // together; otherwise it must not pretend they match.
    #[test]
    fn a_pair_shows_as_one_key_only_when_it_is_one() {
        let d = Settings::default();
        assert_eq!(row_caps(&d, TAKES[0].keys).map(|c| words(&c)), Some("Win Ctrl ← →".into()));
        let odd = Settings { next: vec![Combo { mods: WIN | ALT, vk: VK_RIGHT.0 }], ..Settings::default() };
        assert_eq!(row_caps(&odd, TAKES[0].keys).map(|c| words(&c)), Some("Win Ctrl ←".into()));
    }

    // spec 0011's states each get their own words, and Give back hides while a failed one
    // offers Try again (AC-FR-5).
    #[test]
    fn the_touchpad_states_word_each_case() {
        let (n, o) = (PadRow::Nothing, PadRow::StillOn);
        let st = |rows: [PadRow; 4], saved| PadState { pad: true, enabled: true, rows, off: rows == [n; 4], saved };
        assert!(pad_view(&st([o; 4], false), None).note.is_some_and(|x| x.title == "Turn off four gestures in Windows"));
        assert_eq!(pad_view(&st([o, n, o, n], true), None).sub, "2 of 4 done. des-k starts once all four are off in Windows. Until then, the 2 you turned off don't work.");
        assert!(pad_view(&st([n; 4], true), None).ready);
        let failed = GaveBack { wrote: [Wrote::NotOne; 4], cleared: false };
        let v = pad_view(&st([n; 4], true), Some(failed));
        assert!(!v.give && v.note.is_some_and(|x| x.buttons[0].1 == "Try again"));
    }

    // Not a check that can fail: draws the design file's states offscreen into
    // target/welcome/*.png, so the drawing can be compared with the approved design without
    // opening the window. Run with `cargo test render -- --ignored`.
    #[test]
    #[ignore]
    fn render_first_run() {
        assert!(ui::gdiplus());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("welcome");
        std::fs::create_dir_all(&dir).unwrap();
        let r = |l, t, rr, b| RECT { left: l, top: t, right: rr, bottom: b };
        let screens = vec![
            crate::ws::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 540, 1440, 1440), scale: 150 },
            crate::ws::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(1440, 0, 4000, 1440), scale: 150 },
        ];
        let view = crate::ws::View {
            active: vec![("DISPLAY1".into(), 0), ("DISPLAY2".into(), 0)],
            windows: vec![("DISPLAY1".into(), 0, r(0, 0, 9, 9)), ("DISPLAY2".into(), 0, r(0, 0, 9, 9)), ("DISPLAY2".into(), 3, r(0, 0, 9, 9))],
            names: vec![("DISPLAY1".into(), "Built-in display".into()), ("DISPLAY2".into(), "DELL U2720Q".into())],
            removals: vec![vec![
                crate::ws::Moved { name: "DELL U2720Q".into(), device: Some("DISPLAY2".into()), unplugged: false, windows: 1, from: vec![3] },
                crate::ws::Moved { name: "Built-in display".into(), device: Some("DISPLAY1".into()), unplugged: false, windows: 0, from: vec![] },
            ]],
            waiting: vec![],
        };
        let (n, o) = (PadRow::Nothing, PadRow::StillOn);
        let pad = |rows: [PadRow; 4], saved| Some(PadState { pad: true, enabled: true, rows, off: rows == [n; 4], saved });
        let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5)];
        type Setup = fn(&mut Win);
        let states: Vec<(&str, Setup)> = vec![
            ("f0-try", |_| {}),
            ("f1-switched", |w| {
                w.tried = [true, false, false];
                w.switched = Some(("DELL U2720Q".into(), 1, false));
                w.pics.view.active[1].1 = 1;
            }),
            ("f2-keys", |w| {
                w.step = Step::Keys;
                w.focus = Some(Ctl::Side(2));
            }),
            ("f3-behavior", |w| w.step = Step::Behavior),
            ("f4-pad-first", |w| {
                w.step = Step::Pad;
                w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn; 4], off: false, saved: false });
            }),
            ("f5-pad-saved", |w| {
                w.step = Step::Pad;
                w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], off: false, saved: true });
            }),
            ("f6-pad-all", |w| w.step = Step::Pad),
            ("f7-done", |w| {
                w.step = Step::Done;
                w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], off: false, saved: true });
            }),
            ("f8-keys-more", |w| {
                w.step = Step::Keys;
                w.more[1] = true;
                w.to_end = true;
            }),
            ("f10-keys-rec", |w| {
                w.step = Step::Keys;
                w.rec = Some(Rec::Digits("shortcuts.jump"));
                w.focus = Some(Ctl::Cancel);
            }),
            ("f12-keys-clash", |w| {
                w.step = Step::Keys;
                let held = vec!["Win".to_string(), "Ctrl".to_string(), "1–9".to_string()];
                let sub = "“Send window to 1–9 and stay” uses it. Move it here and that action has no key. Cancel changes nothing.".into();
                w.prompt = Some(Prompt { key: "shortcuts.jump", held, title: "Win Ctrl 1–9 is already in use".into(), sub, edit: Edit::Gestures(vec![]) });
                w.focus = Some(Ctl::Cancel);
            }),
            ("f14-behavior-more", |w| {
                w.step = Step::Behavior;
                w.more[2] = true;
                w.focus = Some(Ctl::Minus("workspaces"));
            }),
            ("r15-f0-behavior-more-end", |w| {
                w.step = Step::Behavior;
                w.more[2] = true;
                w.to_end = true;
            }),
            ("f15-try-all", |w| {
                w.tried = [true; 3];
                w.switched = Some(("DELL U2720Q".into(), 1, false));
            }),
            ("f24-pad-menu", |w| w.step = Step::Pad),
            ("f26-done-hints-rec", |w| {
                w.step = Step::Done;
                w.rec = Some(Rec::List("shortcuts.hints", None));
                w.focus = Some(Ctl::Cancel);
            }),
        ];
        for (tname, theme, scale) in &runs {
            for (sname, setup) in &states {
                let mut win = Win {
                    hwnd: HWND::default(), scale: *scale, theme: *theme, fonts: Fonts::new(*scale), step: Step::Try, more: [false; 5], tried: [false; 3], switched: None,
                    scroll: 0.0, to_end: false, content: 0.0, mid: (0.0, 0.0), hits: vec![], focus: Some(Ctl::Next), kb: true, hover: None, press: None, vw: W, vh: SHORT, cap: TALL,
                    grow: None, rec: None, refused: None, prompt: None, menu: None, error: None,
                    pics: Pics { screens: screens.clone(), view: view.clone(), pad: pad([n; 4], true), mouse: "DISPLAY2".into() },
                    tree: Arc::new(Mutex::new(Node::default())),
                };
                let _ = o;
                setup(&mut win);
                win.kb = win.focus != Some(Ctl::Next); // the ring shows after a key, so only where a state sets it
                win.vh = win.step.height();
                let measure = |win: &Win| unsafe {
                    let dc = GetDC(None);
                    let out = compose(&mut Canvas::new(dc, false, *scale, *theme, &win.fonts), win);
                    ReleaseDC(None, dc);
                    out
                };
                let out = measure(&win);
                let view_h = out.mid.1 - out.mid.0;
                if win.to_end {
                    win.scroll = (out.content - view_h).max(0.0);
                } else if let Some(at) = out.hits.iter().find(|h| Some(h.ctl.stop()) == win.focus && h.group == 1).map(|h| h.at)
                    && at.1 + at.3 + 12.0 > out.mid.1
                {
                    win.scroll = (at.1 + at.3 + 12.0 - out.mid.1).min((out.content - view_h).max(0.0));
                }
                win.hits = measure(&win).hits;
                if *sname == "f24-pad-menu"
                    && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::Picker(settings::GESTURE_ROWS.iter().position(|r| *r == (3, Motion::Up)).unwrap())).map(|h| h.at)
                {
                    let row = settings::GESTURE_ROWS.iter().position(|r| *r == (3, Motion::Up)).unwrap();
                    win.menu = Some(Menu { row, times: 1, family: None, at, focus: 0 });
                }
                let (pw, ph) = ((W * scale).round() as i32, (win.vh * scale).round() as i32);
                ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                    let mut c = Canvas::new(dc, true, *scale, *theme, &win.fonts);
                    c.boxed((0.0, 0.0, W + 1.0, win.vh + 1.0), 0.0, theme.b.bg, None);
                    compose(&mut c, &win);
                });
            }
        }
    }
}
