// The Shortcut hints window (spec 0017, 0017-shortcut-hints.md): every des-k shortcut and
// gesture as settings.toml has them now, drawn to the approved design (pdesk-design
// restart/hints-r3.html), with its own key set, changed or removed in the footer through the
// settings page's recorder rules. Opens from the tray menu or its key, centred on the screen
// under the mouse. Owned by the tray's hidden window, so it is never in the taskbar or Alt+Tab.
// Lives on the main thread; main's loop passes its messages through IsDialogMessageW
// (`key`), so Tab, Enter and Esc work. Sizes are in dip (1/96 inch).
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::{UIA_PaneControlTypeId, UIA_TextControlTypeId},
        Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::{GetActiveWindow, GetFocus, IsWindowEnabled, SetFocus, VIRTUAL_KEY, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT},
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings::{self, Action, Combo, Edit, Motion, Owner, Settings};
use crate::setup::{self, GaveBack, Row as PadRow, State};
use crate::ui::{self, Bx, Canvas, F, Theme, buffered, dark_title, mix, op};
use crate::uia::{self, Node, Tree};

const CLASS: PCWSTR = w!("des-k-hints");
const TITLE: &str = "Shortcut hints";
// No minimize box: with no taskbar button (AC-HNT-7) a minimized window would have nowhere to come back from.
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_CLIPCHILDREN.0);

// The buttons, created in Tab order (AC-HNT-6). Esc is IDCANCEL, never one of these.
const ID_SET: i32 = 101; // Set key, or Change
const ID_REMOVE: i32 = 102;
const ID_NOTE: i32 = 103; // the touchpad note's: Touchpad setup or Turn reading on
const ID_MOVE: i32 = 104; // the clash's Move it here
const ID_CANCEL: i32 = 105; // recording's and the clash's Cancel
const ID_SETTINGS: i32 = 106;
const IDS: [i32; 6] = [ID_SET, ID_REMOVE, ID_NOTE, ID_MOVE, ID_CANCEL, ID_SETTINGS];

// Sizes from the design: a 960 × 672 window, 640 of it under Windows' title bar.
const W: f32 = 960.0;
const H: f32 = 640.0;
const SIDE: f32 = 32.0;
const MID_TOP: f32 = 88.0; // where the scrolling middle starts, under the title and its line
const LEFT_W: f32 = 480.0; // the keyboard column; the touchpad column is the rest after 16
const RIGHT_X: f32 = SIDE + LEFT_W + 16.0;
const RIGHT_W: f32 = W - SIDE - RIGHT_X;
const HEAD: f32 = 37.0; // a card's tinted header, its line included
const KROW: f32 = 36.0;
const BTN_H: f32 = 32.0;
const CAP: f32 = 22.0;
const RING: f32 = 4.0; // a button's window reaches this far past its pill, for the focus ring
const MIN_H: f32 = 320.0; // the least client height, on the shortest screen

const COPILOT: &str = "The Copilot key works as Right Ctrl, so Win Ctrl keys work with it.";

/// One keyboard row: what it does, and each combo as key caps (none: No key).
#[derive(Clone, PartialEq, Debug)]
struct KeyRow {
    title: String,
    keys: Vec<Vec<String>>,
}

/// What one gesture does: an action's name, Nothing, or a key combo it types.
#[derive(Clone, PartialEq, Debug)]
enum Does {
    Name(String),
    Nothing,
    Types(Vec<String>),
}

#[derive(Clone, PartialEq, Debug)]
struct Gact {
    motion: Motion,
    once: Does,
    twice: Option<String>, // "Twice: Find a window"
}

/// What the touchpad runs now (spec 0011's states, AC-HNT-5).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pad {
    On,
    StillOn([bool; 4]), // per setup::ROWS
    CantRead(usize),
    GaveBack,
    Off,    // reading off, Windows' own off too
    OffWin, // reading off, Windows runs its own
    Missing, // no precision touchpad
}

/// What the window shows, read fresh from settings.toml and Windows.
#[derive(Clone, PartialEq, Debug)]
struct View {
    around: Vec<KeyRow>,
    windows: Vec<KeyRow>,
    copilot: bool,
    cards: Vec<(u8, Vec<Gact>)>, // fingers, then left, right, up, down, tap
    pad: Pad,
    key: Vec<Combo>,
}

/// The recorder in the footer (AC-HNT-4): nothing is saved until you choose.
#[derive(Clone, PartialEq, Debug)]
enum Rec {
    Idle,
    Recording(Option<(String, String)>), // a refused key: its title and line
    Clash { combo: Combo, title: String, sub: String, other: Option<(&'static str, Vec<Combo>)> },
}

/// A button as the layout placed it: its pill, in dip.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    id: i32,
    bx: Bx,
    back: u32, // the color behind it
    label: &'static str,
    scrolls: bool, // in the middle: clipped to it
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Fit {
    height: f32,
    content: f32, // the middle's whole height
    shown: f32,   // how much of it shows
}

struct Win {
    hwnd: HWND,
    dpi: u32,
    scale: f32,
    theme: Theme,
    view: View,
    rec: Rec,
    error: Option<String>, // a save that failed
    places: Vec<Place>,
    buttons: Vec<(i32, HWND)>,
    focus: Option<HWND>, // the focused button while the window is not active
    fit: Fit,
    scroll: f32,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
    static TREE: Tree = Tree::default();
}

// Never hold the borrow across a call that sends this window a message: a button redraws
// itself at once on focus, enable and text changes, and that comes back as WM_DRAWITEM.
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&WIN, "hints get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&WIN, "hints set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

// ---- What it reads ----

pub(crate) fn caps(c: &Combo) -> Vec<String> {
    c.to_string().split('+').map(String::from).collect()
}

pub(crate) fn mods_caps(mods: u8) -> Vec<String> {
    settings::mods_text(Some(mods)).split('+').map(String::from).collect()
}

fn list(l: &[Combo]) -> Vec<Vec<String>> {
    l.iter().map(caps).collect()
}

fn digits(set: Option<u8>) -> Vec<Vec<String>> {
    set.map(|m| mods_caps(m).into_iter().chain(["1–9".to_string()]).collect()).into_iter().collect()
}

/// An arrow pair is one ← → row only when both sides hold the same modifiers with Left and
/// Right; otherwise two rows, next first, each with its own keys or No key.
fn pair(previous: &[Combo], next: &[Combo], (both, prev_title, next_title): (&str, &str, &str)) -> Vec<KeyRow> {
    let joined = !previous.is_empty()
        && previous.len() == next.len()
        && previous.iter().zip(next).all(|(p, n)| p.mods == n.mods && p.vk == VK_LEFT.0 && n.vk == VK_RIGHT.0);
    if joined {
        let keys = previous.iter().map(|p| mods_caps(p.mods).into_iter().chain(["Left".to_string(), "Right".to_string()]).collect()).collect();
        return vec![KeyRow { title: both.into(), keys }];
    }
    vec![KeyRow { title: next_title.into(), keys: list(next) }, KeyRow { title: prev_title.into(), keys: list(previous) }]
}

/// An action as the hints name it, with the first run's full names.
pub(crate) fn name(a: Action) -> String {
    match a {
        Action::Switch(d) => if d < 0 { "Previous workspace" } else { "Next workspace" }.into(),
        Action::Send(d, follow) => format!("Send window to {} workspace{}", if d < 0 { "previous" } else { "next" }, if follow { "" } else { " and stay" }),
        Action::Jump(i) => format!("Go to workspace {}", i + 1),
        Action::SendTo(i, follow) => format!("Send window to {}{}", i + 1, if follow { "" } else { " and stay" }),
        Action::Finder => "Find a window".into(),
        Action::Pin => "Pin window to all workspaces".into(),
        Action::Nothing => "Nothing".into(),
        Action::Combo(c) => format!("Types {}", caps(&c).join(" ")),
        a => {
            let l = a.label();
            let mut c = l.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        }
    }
}

fn does(a: Option<Action>) -> Does {
    match a {
        None | Some(Action::Nothing) => Does::Nothing,
        Some(Action::Combo(c)) => Does::Types(caps(&c)),
        Some(a) => Does::Name(name(a)),
    }
}

/// Windows' touchpad state as the touchpad column words it.
fn pad(st: &State, back: Option<GaveBack>) -> Pad {
    match () {
        _ if !st.enabled && st.off => Pad::Off,
        _ if !st.enabled => Pad::OffWin,
        // Until Windows' gestures read off again, which ends the give back (as on the welcome screen).
        _ if back.is_some_and(|b| !b.failed()) && !st.off => Pad::GaveBack,
        _ if !st.pad => Pad::Missing,
        _ => match st.rows.iter().position(|r| *r == PadRow::CantRead) {
            Some(i) => Pad::CantRead(i),
            None if st.rows.contains(&PadRow::StillOn) => Pad::StillOn(st.rows.map(|r| r == PadRow::StillOn)),
            None => Pad::On,
        },
    }
}

fn read(s: &Settings, st: &State, back: Option<GaveBack>) -> View {
    let row = |title: &str, keys| KeyRow { title: title.into(), keys };
    let mut around = pair(&s.previous, &s.next, ("Switch workspace", "Previous workspace", "Next workspace"));
    around.extend([row("Go to workspace 1–9", digits(s.jump)), row("Open overview", list(&s.overview)), row("Find a window", list(&s.finder))]);
    let mut windows = pair(&s.send_previous, &s.send_next, ("Send window to next or previous", "Send window to previous workspace", "Send window to next workspace"));
    windows.extend(pair(&s.send_previous_stay, &s.send_next_stay, ("Send window to next or previous and stay", "Send window to previous workspace and stay", "Send window to next workspace and stay")));
    windows.extend([row("Send window to 1–9", digits(s.send_to)), row("Send window to 1–9 and stay", digits(s.send_to_stay)), row("Pin window to all workspaces", list(&s.pin))]);
    let cards = [3u8, 4]
        .into_iter()
        .map(|f| {
            let acts = [Motion::Left, Motion::Right, Motion::Up, Motion::Down, Motion::Tap]
                .into_iter()
                .map(|m| Gact {
                    motion: m,
                    once: does(s.rule(f, m, 1)),
                    twice: s.rule(f, m, 2).map(|a| format!("Twice: {}", if let Action::Combo(_) = a { name(a).replacen("Types", "types", 1) } else { name(a) })),
                })
                .collect();
            (f, acts)
        })
        .collect();
    View { around, windows, copilot: s.copilot_as_right_ctrl, cards, pad: pad(st, back), key: s.hints.clone() }
}

fn read_now() -> View {
    read(settings::get(), &setup::state(), crate::welcome::gave_back())
}

/// "three-finger swipes" from setup's "3-finger swipes".
fn spelled(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}

/// A note's icon, title, line, and whether it warns.
type Words = (&'static str, String, String, bool);

/// The touchpad column's status, its note and its note's button.
fn pad_words(p: Pad) -> (&'static str, Option<Words>, Option<&'static str>) {
    const SETUP: Option<&str> = Some("Touchpad setup");
    const READ: Option<&str> = Some("Turn reading on");
    match p {
        Pad::On => ("des-k runs these", None, None),
        Pad::StillOn(on) => {
            let n = on.iter().filter(|o| **o).count();
            let names: Vec<String> = setup::ROWS.iter().zip(on).filter(|(_, o)| *o).map(|((r, _), _)| spelled(r)).collect();
            let title = format!("{n} of 4 {} still on in Windows", if n == 1 { "is" } else { "are" });
            let rest = match 4 - n {
                0 => ".".to_string(),
                off => format!("; until then, the {off} you turned off don't work."),
            };
            let line = format!("Still on: {}. des-k runs these once all four are off{rest}", names.join(", "));
            ("Not running", Some(("\u{E7BA}", title, line, true)), SETUP)
        }
        Pad::CantRead(i) => {
            let row = setup::ROWS[i].0.trim_end_matches('s');
            let line = format!("It is the {row} setting. Until des-k can read it, it runs none of these, and the gestures you turned off don't work. Details: tray icon › Open log.");
            ("Not running", Some(("\u{E7BA}", "Can't read a Windows setting".into(), line, true)), SETUP)
        }
        Pad::GaveBack => {
            let line = "des-k stopped reading gestures, and Windows keeps its own off until you sign in again, so none work now. To use them now, set them in Windows Settings.";
            ("Not running", Some(("\u{E946}", "Given back to Windows at your next sign-in".into(), line.into(), false)), SETUP)
        }
        Pad::Off => ("Not running", Some(("\u{E7BA}", "Touchpad reading is off".into(), "Windows' own gestures are off too, so no 3- or 4-finger gesture works now.".into(), true)), READ),
        Pad::OffWin => ("Windows runs its own", Some(("\u{E946}", "Touchpad reading is off".into(), "Windows runs its own 3- and 4-finger gestures. des-k runs none of these.".into(), false)), READ),
        Pad::Missing => ("No touchpad", Some(("\u{E946}", "No precision touchpad found".into(), "Your gestures are kept. They run once one is plugged in and set up.".into(), false)), None),
    }
}

/// A shortcut list's or set's name as the rows have it, and its key in the file.
fn owner_name(list: &str) -> (&'static str, &'static str) {
    match list {
        "previous" => ("Previous workspace", "shortcuts.previous"),
        "next" => ("Next workspace", "shortcuts.next"),
        "send_previous" => ("Send window to previous workspace", "shortcuts.send_previous"),
        "send_next" => ("Send window to next workspace", "shortcuts.send_next"),
        "send_previous_stay" => ("Send window to previous workspace and stay", "shortcuts.send_previous_stay"),
        "send_next_stay" => ("Send window to next workspace and stay", "shortcuts.send_next_stay"),
        "finder" => ("Find a window", "shortcuts.finder"),
        "overview" => ("Open overview", "shortcuts.overview"),
        "pin" => ("Pin window to all workspaces", "shortcuts.pin"),
        "jump" => ("Go to workspace 1–9", "shortcuts.jump"),
        "send_to" => ("Send window to 1–9", "shortcuts.send_to"),
        "send_to_stay" => ("Send window to 1–9 and stay", "shortcuts.send_to_stay"),
        _ => ("", ""),
    }
}

/// A recorded key: saved at once, or the clash prompt when another shortcut has it (AC-HNT-4,
/// the settings page's rules, spec 0012 AC-9). None: it is already the key.
fn recorded_rec(s: &Settings, combo: Combo) -> Option<Rec> {
    if s.hints == [combo] {
        return None;
    }
    let said = caps(&combo).join(" ");
    Some(match s.combo_owner("hints", combo) {
        None => Rec::Idle,
        Some(Owner::List(other)) => {
            let (title, key) = owner_name(other);
            let rest = s.list(other).iter().filter(|c| **c != combo).copied().collect();
            Rec::Clash {
                combo,
                title: format!("{said} is already in use"),
                sub: format!("“{title}” uses it. Move it here and “{title}” loses this key. Cancel changes nothing."),
                other: Some((key, rest)),
            }
        }
        // The hook tries the lists first, so the set only loses this one digit (spec 0012 AC-8).
        Some(Owner::Set(set)) => {
            let title = owner_name(set).0;
            let d = combo.vk as u8 as char;
            Rec::Clash { combo, title: format!("{said} is part of “{title}”"), sub: format!("Move it here and “{title}” skips {d}. Cancel changes nothing."), other: None }
        }
    })
}

// ---- Drawing ----

/// Key caps for `combos`, each combo's caps 4 apart and "or" between combos: their width.
fn keys_w(c: &Canvas, combos: &[Vec<String>]) -> f32 {
    let m = c.measuring();
    let caps: f32 = combos.iter().map(|k| k.iter().map(|x| m.key_cap(0.0, 0.0, CAP, x, false) + 4.0).sum::<f32>() - 4.0).sum();
    caps + combos.len().saturating_sub(1) as f32 * (16.0 + c.width("or", F::Small))
}

/// Draws `combos` from x, centred on `cy`; returns where they end.
fn draw_keys(c: &Canvas, x: f32, cy: f32, combos: &[Vec<String>], held: bool) -> f32 {
    let mut x = x;
    for (i, combo) in combos.iter().enumerate() {
        if i > 0 {
            c.text(x + 8.0, cy - 8.0, 16.0, "or", F::Small, c.t.b.tx2);
            x += 16.0 + c.width("or", F::Small);
        }
        for (j, k) in combo.iter().enumerate() {
            if j > 0 {
                x += 4.0;
            }
            x += c.key_cap(x, cy - CAP / 2.0, CAP, k, held);
        }
    }
    x
}

/// A card's tinted header with its line under it; `n`: a finger count in a circle.
fn header(c: &Canvas, (x, y, w): (f32, f32, f32), title: &str, n: Option<u8>, dim: bool) -> f32 {
    let (t, b) = (c.t, c.t.b);
    let h = if n.is_some() { 39.0 } else { HEAD };
    let tint = match () {
        _ if t.hc => b.card,
        _ if dim => b.track,
        _ => mix(b.card, b.acc, 0.09),
    };
    c.fill_rr((x + 1.0, y + 1.0, w - 2.0, h - 1.0), 7.0, op(tint));
    c.fill_rr((x + 1.0, y + h / 2.0, w - 2.0, h / 2.0 - 1.0), 0.0, op(tint));
    c.rule(x, y + h - 1.0, w, op(b.line));
    let mut tx = x + 16.0;
    if let Some(n) = n {
        let fill = if dim { b.tx3 } else { b.acc };
        c.circle(tx + 11.0, y + h / 2.0, 11.0, Some(op(fill)), None);
        let s = n.to_string();
        c.text(tx + 11.0 - c.width(&s, F::SmallBold) / 2.0, y + (h - 22.0) / 2.0, 22.0, &s, F::SmallBold, b.on_acc);
        tx += 32.0;
    }
    c.text(tx, y, h - 1.0, title, F::Bold, if dim { b.tx3 } else { b.tx });
    h
}

/// A keyboard card: its header and one row per action. Returns where it ends.
fn key_card(c: &Canvas, x: f32, y: f32, title: &str, rows: &[KeyRow]) -> f32 {
    let b = c.t.b;
    let h = HEAD + rows.len() as f32 * KROW;
    c.boxed((x, y, LEFT_W, h), 8.0, b.card, Some(b.line));
    let mut ry = y + header(c, (x, y, LEFT_W), title, None, false);
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            c.rule(x + 1.0, ry, LEFT_W - 2.0, op(b.line));
        }
        let right = x + LEFT_W - 16.0;
        let kw = if r.keys.is_empty() { c.width("No key", F::Small) } else { keys_w(c, &r.keys) };
        if r.keys.is_empty() {
            c.text(right - kw, ry, KROW, "No key", F::Small, b.tx3);
        } else {
            draw_keys(c, right - kw, ry + KROW / 2.0, &r.keys, false);
        }
        c.text(x + 16.0, ry, KROW, &c.fit(&r.title, F::Body, LEFT_W - 48.0 - kw), F::Body, b.tx);
        ry += KROW;
    }
    ry
}

/// One gesture: its direction mark, then what it does and its twice line. Returns its height.
fn gact(c: &Canvas, x: f32, y: f32, w: f32, g: &Gact, dim: bool) -> f32 {
    let (t, b) = (c.t, c.t.b);
    let glyph = match g.motion {
        Motion::Left => "\u{E72B}",
        Motion::Right => "\u{E72A}",
        Motion::Up => "\u{E74A}",
        Motion::Down => "\u{E74B}",
        Motion::Tap => "\u{E815}",
    };
    let mark = if dim { b.tx3 } else { b.acc };
    match () {
        _ if t.hc => c.stroke_rr((x + 0.5, y + 0.5, 19.0, 19.0), 4.0, 1.0, op(mark)),
        _ if dim => c.fill_rr((x, y, 20.0, 20.0), 4.0, op(b.track)),
        _ => c.fill_rr((x, y, 20.0, 20.0), 4.0, op(b.held)),
    }
    c.text(x + (20.0 - c.width(glyph, F::IconTiny)) / 2.0, y, 20.0, glyph, F::IconTiny, mark);
    let (lx, lw) = (x + 26.0, w - 26.0);
    let ink = if dim { b.tx3 } else { b.tx };
    let mut h = 20.0;
    match &g.once {
        Does::Name(n) => c.text(lx, y, 20.0, &c.fit(n, F::Body, lw), F::Body, ink),
        Does::Nothing => c.text(lx, y, 20.0, "Nothing", F::Body, b.tx3),
        Does::Types(k) => {
            c.text(lx, y - 1.0, CAP, "Types", F::Body, ink);
            draw_keys(c, lx + c.width("Types", F::Body) + 6.0, y + 10.0, std::slice::from_ref(k), false);
            h = CAP;
        }
    }
    if let Some(tw) = &g.twice {
        c.text(lx, y + h, 16.0, &c.fit(tw, F::Small, lw), F::Small, if dim { b.tx3 } else { b.tx2 });
        h += 16.0;
    }
    h
}

/// A gesture card: 3 or 4 fingers, its swipes in two columns, then its tap. Returns where it ends.
fn gesture_card(c: &Canvas, x: f32, y: f32, fingers: u8, acts: &[Gact], dim: bool) -> f32 {
    let b = c.t.b;
    let col = (RIGHT_W - 32.0 - 60.0 - 12.0) / 2.0;
    let gx = x + 16.0 + 44.0 + 16.0;
    // Measured first: the card's edge goes under everything.
    let m = c.measuring();
    let row_h = |a: &Gact, b2: &Gact| gact(&m, 0.0, 0.0, col, a, dim).max(gact(&m, 0.0, 0.0, col, b2, dim));
    let (r1, r2) = (row_h(&acts[0], &acts[1]), row_h(&acts[2], &acts[3]));
    let swipe_h = 8.0 + r1 + 6.0 + r2 + 8.0;
    let tap_h = (8.0 + gact(&m, 0.0, 0.0, col, &acts[4], dim) + 8.0).max(40.0);
    let h = 39.0 + swipe_h + tap_h;
    c.boxed((x, y, RIGHT_W, h), 8.0, b.card, Some(b.line));
    let mut ry = y + header(c, (x, y, RIGHT_W), &format!("{fingers} fingers"), Some(fingers), dim);
    let label = if dim { b.tx3 } else { b.tx };
    c.text(x + 16.0, ry + 10.0, 20.0, "Swipe", F::Bold, label);
    gact(c, gx, ry + 8.0, col, &acts[0], dim);
    gact(c, gx + col + 12.0, ry + 8.0, col, &acts[1], dim);
    gact(c, gx, ry + 8.0 + r1 + 6.0, col, &acts[2], dim);
    gact(c, gx + col + 12.0, ry + 8.0 + r1 + 6.0, col, &acts[3], dim);
    ry += swipe_h;
    c.rule(x + 1.0, ry, RIGHT_W - 2.0, op(b.line));
    c.text(x + 16.0, ry, tap_h, "Tap", F::Bold, label);
    let th = gact(&m, 0.0, 0.0, col, &acts[4], dim);
    gact(c, gx, ry + (tap_h - th) / 2.0, col, &acts[4], dim);
    ry + tap_h
}

/// A button's pill width for its label.
fn btn_w(c: &Canvas, label: &str) -> f32 {
    c.width(label, F::Body) + 30.0
}

/// A note: an icon, its title and line, and its buttons on the right. Returns its height.
#[allow(clippy::too_many_arguments)]
fn note(c: &Canvas, places: &mut Vec<Place>, (x, y, w): (f32, f32, f32), (icon, color): (&str, u32), title: &str, line: &str, buttons: &[(i32, &'static str)], scrolls: bool) -> f32 {
    let b = c.t.b;
    let bw: f32 = buttons.iter().map(|(_, l)| btn_w(c, l) + 8.0).sum::<f32>();
    let lw = w - 24.0 - 28.0 - if bw > 0.0 { bw + 4.0 } else { 0.0 };
    let m = c.measuring();
    let text_h = m.para(0.0, 0.0, lw, 20.0, &[(title, F::Bold, 0)]) + m.para(0.0, 0.0, lw, 16.0, &[(line, F::Small, 0)]);
    let inner = text_h.max(if buttons.is_empty() { 0.0 } else { BTN_H });
    let h = 20.0 + inner;
    c.boxed((x, y, w, h), 6.0, b.card, Some(b.line));
    c.text(x + 12.0, y + 12.0, 16.0, icon, F::Icon, color);
    let ty = y + 10.0 + (inner - text_h) / 2.0;
    let th = c.para(x + 40.0, ty, lw, 20.0, &[(title, F::Bold, b.tx)]);
    c.para(x + 40.0, ty + th, lw, 16.0, &[(line, F::Small, b.tx2)]);
    let mut bx = x + w - 12.0;
    for &(id, label) in buttons.iter().rev() {
        let bw = btn_w(c, label);
        bx -= bw;
        places.push(Place { id, bx: (bx, y + (h - BTN_H) / 2.0, bw, BTN_H), back: b.card, label, scrolls });
        bx -= 8.0;
    }
    h
}

/// The scrolling middle, its top at `top`: the keyboard column and the touchpad column.
/// Returns its whole height, from `top` - 1 (its 1 dip inset) to its bottom padding.
fn middle(c: &Canvas, places: &mut Vec<Place>, v: &View, top: f32) -> f32 {
    let b = c.t.b;
    // The keyboard.
    c.text(SIDE, top, 20.0, "Keyboard", F::Bold, b.tx);
    let mut y = key_card(c, SIDE, top + 28.0, "Move around", &v.around) + 12.0;
    y = key_card(c, SIDE, y, "Move windows", &v.windows);
    if v.copilot {
        c.text(SIDE, y + 10.0, 16.0, COPILOT, F::Small, b.tx2);
        y += 26.0;
    }
    let left = y;
    // The touchpad.
    let (status, words, button) = pad_words(v.pad);
    let right = RIGHT_X + RIGHT_W;
    c.text(RIGHT_X, top, 20.0, "Touchpad", F::Bold, b.tx);
    let sw = c.width(status, F::Small);
    c.text(right - sw, top, 20.0, status, F::Small, b.tx2);
    if v.pad == Pad::On {
        c.text(right - sw - 16.0, top, 20.0, "\u{E73E}", F::IconSmall, b.good);
    }
    let mut y = top + 28.0;
    if let Some((icon, title, line, warn)) = &words {
        let ids: Vec<(i32, &'static str)> = button.map(|l| (ID_NOTE, l)).into_iter().collect();
        y += note(c, places, (RIGHT_X, y, RIGHT_W), (icon, if *warn { b.warn } else { b.tx2 }), title, line, &ids, true) + 12.0;
    }
    let dim = v.pad != Pad::On;
    for (i, (fingers, acts)) in v.cards.iter().enumerate() {
        if i > 0 {
            y += 12.0;
        }
        y = gesture_card(c, RIGHT_X, y, *fingers, acts, dim);
    }
    left.max(y) + 12.0 - (top - 1.0)
}

/// The footer's height: its line, padding, the recorder's note when one shows, the key row.
fn foot_h(c: &Canvas, rec: &Rec, error: Option<&str>) -> f32 {
    let mut scratch = vec![];
    1.0 + 16.0 + foot_note(&c.measuring(), &mut scratch, rec, error, 0.0).map_or(0.0, |h| h + 12.0) + BTN_H + 20.0
}

/// The recorder's note or a save error, at `y`: its height, or None when neither shows.
fn foot_note(c: &Canvas, places: &mut Vec<Place>, rec: &Rec, error: Option<&str>, y: f32) -> Option<f32> {
    let b = c.t.b;
    let at = (SIDE, y, W - 2.0 * SIDE);
    Some(match (rec, error) {
        (Rec::Recording(None), _) => note(c, places, at, ("\u{E765}", b.acc), "Press the new key", "Hold Win, Ctrl, Alt or Shift with a key. If Windows uses it, des-k takes it over. Esc alone cancels.", &[(ID_CANCEL, "Cancel")], false),
        (Rec::Recording(Some((t, l))), _) => note(c, places, at, ("\u{E7BA}", b.warn), t, l, &[(ID_CANCEL, "Cancel")], false),
        (Rec::Clash { title, sub, .. }, _) => note(c, places, at, ("\u{E7BA}", b.warn), title, sub, &[(ID_MOVE, "Move it here"), (ID_CANCEL, "Cancel")], false),
        (Rec::Idle, Some(why)) => note(c, places, at, ("\u{E7BA}", b.warn), "Not saved", why, &[], false),
        (Rec::Idle, None) => return None,
    })
}

/// The footer band at `top`: the recorder's note, then the hints key and Open settings.
fn footer(c: &Canvas, places: &mut Vec<Place>, v: &View, rec: &Rec, error: Option<&str>, top: f32, height: f32) {
    let (t, b) = (c.t, c.t.b);
    c.fill_rr((0.0, top, W, height - top), 0.0, op(if t.hc { b.bg } else { b.band }));
    c.rule(0.0, top, W, op(b.line));
    let mut y = top + 1.0 + 16.0;
    if let Some(h) = foot_note(c, places, rec, error, y) {
        y += h + 12.0;
    }
    let back = if t.hc { b.bg } else { b.band };
    let cy = y + BTN_H / 2.0;
    let mut x = SIDE;
    let button = |x: f32, id: i32, label: &'static str, places: &mut Vec<Place>| {
        let w = btn_w(c, label);
        places.push(Place { id, bx: (x, y, w, BTN_H), back, label, scrolls: false });
        x + w
    };
    match rec {
        Rec::Idle if v.key.is_empty() => {
            c.text(x, y, BTN_H, "Opens with: no key", F::Body, b.tx2);
            x += c.width("Opens with: no key", F::Body) + 8.0;
            button(x, ID_SET, "Set key", places);
        }
        Rec::Idle => {
            c.text(x, y, BTN_H, "Opens with", F::Body, b.tx2);
            x += c.width("Opens with", F::Body) + 8.0;
            x = draw_keys(c, x, cy, &list(&v.key), false) + 8.0;
            x = button(x, ID_SET, "Change", places) + 8.0;
            button(x, ID_REMOVE, "Remove", places);
        }
        // The field waits for the key, or holds the one that clashed.
        Rec::Recording(_) | Rec::Clash { .. } => {
            c.text(x, y, BTN_H, "Opens with", F::Body, b.tx2);
            x += c.width("Opens with", F::Body) + 8.0;
            let inner = match rec {
                Rec::Clash { combo, .. } => keys_w(c, &[caps(combo)]),
                _ => c.width("key", F::Small) + 14.0,
            };
            let field = (x, y, inner + 10.0, BTN_H);
            c.fill_rr(field, 6.0, op(if t.hc { b.bg } else { b.card }));
            c.stroke_rr((field.0 + 1.0, field.1 + 1.0, field.2 - 2.0, field.3 - 2.0), 5.0, 2.0, op(b.acc));
            match rec {
                Rec::Clash { combo, .. } => {
                    draw_keys(c, x + 5.0, cy, &[caps(combo)], true);
                }
                _ => {
                    let g = (x + 5.0, cy - CAP / 2.0, inner, CAP);
                    c.stroke_dash((g.0 + 0.5, g.1 + 0.5, g.2 - 1.0, g.3 - 1.0), 4.0, 1.0, op(b.tx3), windows::Win32::Graphics::GdiPlus::DashStyleDash);
                    c.text(g.0 + 7.0, g.1, CAP, "key", F::Small, b.tx3);
                }
            }
        }
    }
    let w = btn_w(c, "Open settings");
    places.push(Place { id: ID_SETTINGS, bx: (W - SIDE - w, y, w, BTN_H), back, label: "Open settings", scrolls: false });
}

/// The whole window, back to front, `height` dip tall, the middle scrolled by `scroll`.
fn compose(c: &Canvas, places: &mut Vec<Place>, v: &View, rec: &Rec, error: Option<&str>, height: f32, scroll: f32) -> Fit {
    let b = c.t.b;
    let foot = foot_h(c, rec, error);
    let shown = (height - foot - MID_TOP).max(0.0);
    let content = middle(&c.measuring(), &mut vec![], v, MID_TOP + 1.0);
    let scroll = scroll.clamp(0.0, (content - shown).max(0.0));
    c.fill_rr((0.0, 0.0, W, height), 0.0, op(b.bg));
    middle(c, places, v, MID_TOP + 1.0 - scroll);
    // The title area covers what scrolled above the middle; the footer what is below it.
    c.fill_rr((0.0, 0.0, W, MID_TOP), 0.0, op(b.bg));
    c.text(SIDE, 16.0, 32.0, TITLE, F::Heading, b.tx);
    c.text(SIDE, 52.0, 20.0, "Every des-k shortcut and gesture, as set now. Esc closes this.", F::Body, b.tx2);
    if scroll > 0.5 {
        c.rule(0.0, MID_TOP, W, op(b.line));
    }
    footer(c, places, v, rec, error, height - foot, height);
    if content > shown + 0.5 {
        let th = ((shown - 8.0) * shown / content).max(24.0);
        let ty = MID_TOP + 4.0 + scroll / (content - shown) * (shown - 8.0 - th);
        c.fill_rr((W - 5.0, ty, 2.0, th), 1.0, op(b.tx3));
    }
    Fit { height, content, shown }
}

/// One button, filling its window: the backdrop, the pill, the focus ring.
fn paint_button(c: &Canvas, p: &Place, (x, y, w, h): Bx, pressed: bool, focus: bool) {
    let b = c.t.b;
    let fill = if pressed { mix(b.card, b.tx, 0.12) } else { b.card };
    c.boxed((x, y, w, h), 6.0, if c.t.hc { b.bg } else { fill }, Some(b.line));
    let tw = c.width(p.label, F::Body);
    c.text(x + (w - tw) / 2.0, y, h, p.label, F::Body, b.tx);
    if focus {
        c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), 9.0, 2.0, op(b.tx));
    }
}

// ---- The window ----

impl Win {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn tree(&self) -> Node {
        let rect = |(x, y, w, h): Bx| RECT { left: self.px(x), top: self.px(y), right: self.px(x + w), bottom: self.px(y + h) };
        let text = |name: String, b: Bx| Node { role: UIA_TextControlTypeId, name, rect: rect(b), ..Node::default() };
        let keys = |k: &[Vec<String>]| if k.is_empty() { "No key".to_string() } else { k.iter().map(|c| c.join(" ")).collect::<Vec<_>>().join(", or ") };
        let mid = (0.0, MID_TOP, W, self.fit.shown);
        let mut kids = vec![text(TITLE.into(), (SIDE, 16.0, W - 2.0 * SIDE, 32.0))];
        for r in self.view.around.iter().chain(&self.view.windows) {
            kids.push(text(format!("{}: {}", r.title, keys(&r.keys)), (SIDE, MID_TOP, LEFT_W, self.fit.shown)));
        }
        if self.view.copilot {
            kids.push(text(COPILOT.into(), mid));
        }
        let (status, words, _) = pad_words(self.view.pad);
        kids.push(text(format!("Touchpad: {status}"), (RIGHT_X, MID_TOP, RIGHT_W, 20.0)));
        if let Some((_, title, line, _)) = words {
            kids.push(text(format!("{title}. {line}"), (RIGHT_X, MID_TOP, RIGHT_W, self.fit.shown)));
        }
        for (f, acts) in &self.view.cards {
            for g in acts {
                let what = match &g.once {
                    Does::Name(n) => n.clone(),
                    Does::Nothing => "Nothing".into(),
                    Does::Types(k) => format!("Types {}", k.join(" ")),
                };
                let how = if g.motion == Motion::Tap { "tap".to_string() } else { format!("swipe {}", g.motion.word()) };
                let twice = g.twice.as_ref().map_or(String::new(), |t| format!(", {}", t.to_lowercase()));
                kids.push(text(format!("{f} fingers, {how}: {what}{twice}"), (RIGHT_X, MID_TOP, RIGHT_W, self.fit.shown)));
            }
        }
        let opens = if self.view.key.is_empty() { "Opens with: no key".to_string() } else { format!("Opens with {}", keys(&list(&self.view.key))) };
        kids.push(text(opens, (SIDE, self.fit.height - BTN_H - 20.0, 400.0, BTN_H)));
        Node { role: UIA_PaneControlTypeId, name: TITLE.into(), rect: rect((0.0, 0.0, W, self.fit.height)), kids, ..Node::default() }
    }
}

/// The work area of the screen `at` is on, else the window's own screen.
fn work_area(h: HWND, at: Option<RECT>) -> Option<RECT> {
    unsafe {
        let mon = match at {
            Some(r) => MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST),
        };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(mon, &mut mi).as_bool().then_some(mi.rcWork)
    }
}

/// The client height, in dip, that fits under the title bar in `work` px of screen height.
fn client_cap(work: i32, dpi: u32, s: f32) -> f32 {
    let mut r = RECT::default();
    unsafe { let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi); }
    (work - (r.bottom - r.top)) as f32 / s
}

/// Lays the window out again and fits it and its buttons: 640 dip tall, or its screen's work
/// area when that is less (AC-HNT-1); `to` (WM_DPICHANGED's rectangle) moves it too.
fn relayout(to: Option<RECT>) {
    let Some((h, s, dpi)) = get(|w| (w.hwnd, w.scale, w.dpi)) else { return };
    let work = work_area(h, to);
    let cap = work.map_or(H, |r| client_cap(r.bottom - r.top, dpi, s));
    let height = H.min(cap).max(MIN_H);
    let Some((places, buttons, tree)) = set(|w| {
        let fonts = ui::fonts(w.scale);
        let mut places = vec![];
        unsafe {
            let dc = GetDC(None);
            let c = Canvas::new(dc, false, w.scale, w.theme, &fonts);
            w.fit = compose(&c, &mut places, &w.view, &w.rec, w.error.as_deref(), height, w.scroll);
            drop(c);
            ReleaseDC(None, dc);
        }
        w.scroll = w.scroll.clamp(0.0, (w.fit.content - w.fit.shown).max(0.0));
        w.places.clone_from(&places);
        (places, w.buttons.clone(), w.tree())
    }) else {
        return;
    };
    TREE.with(|t| {
        if let Ok(mut n) = t.try_lock() {
            *n = tree;
        }
    });
    let px = |v: f32| (v * s).round() as i32;
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: px(W), bottom: px(height) };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut now = RECT::default();
        let _ = GetWindowRect(h, &mut now);
        let (x, mut y) = to.map_or((now.left, now.top), |t| (t.left, t.top));
        if let Some(wk) = work.filter(|_| wh > now.bottom - now.top || to.is_some()) {
            y = y.min(wk.bottom - wh).max(wk.top);
        }
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        let (mid_top, mid_bottom) = (px(MID_TOP), px(MID_TOP) + get(|w| px(w.fit.shown)).unwrap_or(0));
        for (id, b) in &buttons {
            let Some(p) = places.iter().find(|p| p.id == *id) else {
                let _ = ShowWindow(*b, SW_HIDE);
                continue;
            };
            let (bx, by) = (px(p.bx.0 - RING), px(p.bx.1 - RING));
            let (bw, bh) = (px(p.bx.0 + p.bx.2 + RING) - bx, px(p.bx.1 + p.bx.3 + RING) - by);
            let _ = SetWindowTextW(*b, &HSTRING::from(p.label));
            let _ = MoveWindow(*b, bx, by, bw, bh, true);
            // A button in the middle shows only the part of it inside the middle.
            if p.scrolls {
                let (top, bottom) = ((mid_top - by).max(0), (mid_bottom - by).min(bh));
                if bottom <= top {
                    let _ = ShowWindow(*b, SW_HIDE);
                    continue;
                }
                let rgn = CreateRectRgn(0, top, bw, bottom);
                SetWindowRgn(*b, Some(rgn), true); // the window owns it now
            }
            let _ = ShowWindow(*b, SW_SHOWNA);
            let _ = InvalidateRect(Some(*b), None, false);
        }
        let _ = InvalidateRect(Some(h), None, false);
        // A layout that hid the focused button moves focus to the first one that shows.
        let usable = |b: HWND| buttons.iter().any(|(_, x)| *x == b) && IsWindowVisible(b).as_bool() && IsWindowEnabled(b).as_bool();
        let first = IDS.iter().copied().find(|id| places.iter().any(|p| p.id == *id && !p.scrolls));
        let fallback = buttons.iter().find(|(id, _)| Some(*id) == first).map(|(_, b)| *b);
        if let Some(d) = fallback {
            if GetActiveWindow() == h && !usable(GetFocus()) {
                let _ = SetFocus(Some(d));
            }
            let _ = set(|w| {
                if w.focus.is_some_and(|f| !usable(f)) {
                    w.focus = Some(d);
                }
            });
        }
    }
}

/// Scrolls the middle by `dy` dip.
fn scroll_by(dy: f32) {
    let moved = set(|w| {
        let s = (w.scroll + dy).clamp(0.0, (w.fit.content - w.fit.shown).max(0.0));
        (s != w.scroll).then(|| w.scroll = s)
    });
    if moved.flatten().is_some() {
        relayout(None);
    }
}

/// Settings or Windows' touchpad state changed: show it at once, if the window is open
/// (settings::reload, the touchpad thread, getting active).
pub fn refresh() {
    if hwnd().is_none() {
        return;
    }
    let view = read_now();
    if set(|w| std::mem::replace(&mut w.view, view.clone()) != view) == Some(true) {
        relayout(None);
    }
}

/// Main loop: the window's keys (Tab, Enter, Esc) through the dialog manager, and Page Up and
/// Page Down scrolling the middle.
pub fn key(msg: &MSG) -> bool {
    let Some(h) = hwnd() else { return false };
    unsafe {
        if msg.message == WM_KEYDOWN && (msg.hwnd == h || IsChild(h, msg.hwnd).as_bool()) {
            let page = get(|w| (w.fit.shown - 40.0).max(20.0)).unwrap_or(0.0);
            let dy = match VIRTUAL_KEY(msg.wParam.0 as u16) {
                VK_PRIOR => Some(-page),
                VK_NEXT => Some(page),
                _ => None,
            };
            if let Some(dy) = dy {
                scroll_by(dy);
                return true;
            }
        }
        let took = IsDialogMessageW(h, msg).as_bool();
        if took {
            reveal_focus();
        }
        took
    }
}

/// Tab moved focus to the touchpad note's button while it is scrolled out of view: scroll to it.
fn reveal_focus() {
    let focused = unsafe { GetFocus() };
    let dy = get(|w| {
        let note = w.buttons.iter().find(|(id, b)| *id == ID_NOTE && *b == focused)?;
        let p = w.places.iter().find(|p| p.id == note.0)?;
        let (top, bottom) = (MID_TOP + RING, MID_TOP + w.fit.shown - RING);
        if p.bx.1 < top {
            Some(p.bx.1 - top)
        } else if p.bx.1 + p.bx.3 > bottom {
            Some(p.bx.1 + p.bx.3 - bottom)
        } else {
            None
        }
    });
    if let Some(Some(dy)) = dy {
        scroll_by(dy);
    }
}

/// The screen under the mouse: its monitor and work area.
fn under_mouse() -> (HMONITOR, RECT) {
    let (mon, _) = crate::ws::under_mouse();
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { let _ = GetMonitorInfoW(mon, &mut mi); }
    (mon, mi.rcWork)
}

/// Centres the window in `work`.
fn centre(h: HWND, work: RECT) {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Moves the open window to the screen under the mouse, in front with focus.
fn bring(h: HWND) {
    let (mon, work) = under_mouse();
    unsafe {
        if MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) != mon {
            // Twice: the first move may change its dpi, and so its size.
            let _ = SetWindowPos(h, None, work.left, work.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            centre(h, work);
        }
        let _ = SetForegroundWindow(h);
    }
}

/// Tray menu › Shortcut hints: opens the window, or brings the open one here.
pub fn show() {
    match hwnd() {
        Some(h) => bring(h),
        None => open(),
    }
}

/// Its key (WM_HINTS): opens it; closes it while it is open on the screen under the mouse,
/// and moves it here while it is open on another screen (AC-HNT-2).
pub fn toggle() {
    let Some(h) = hwnd() else { return open() };
    let (mon, _) = under_mouse();
    if unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) } == mon {
        close(h);
    } else {
        bring(h);
    }
}

fn open() {
    ui::gdiplus();
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc); // fails harmlessly when the window opens a second time
        let (_, work) = under_mouse();
        // Owned by the tray's hidden window: never in the taskbar or Alt+Tab (AC-HNT-7).
        let owner = crate::tray::window();
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, &HSTRING::from(TITLE), STYLE, work.left, work.top, 100, 100, owner, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("hints: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let style = WINDOW_STYLE(WS_CHILD.0 | WS_TABSTOP.0 | BS_OWNERDRAW as u32);
        let make = |id: i32| {
            let menu = HMENU(id as isize as *mut core::ffi::c_void);
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!(""), style, 0, 0, 0, 0, Some(h), Some(menu), Some(inst.into()), None).ok().map(|b| (id, b))
        };
        let buttons: Vec<(i32, HWND)> = IDS.into_iter().filter_map(make).collect();
        let dpi = GetDpiForWindow(h);
        let theme = ui::theme();
        dark_title(h, &theme);
        ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        let view = read_now();
        WIN.with_borrow_mut(|w| *w = Some(Win { hwnd: h, dpi, scale: dpi as f32 / 96.0, theme, view, rec: Rec::Idle, error: None, places: vec![], buttons, focus: None, fit: Fit::default(), scroll: 0.0 }));
        relayout(None);
        centre(h, work);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
    println!("hints: opened");
}

/// Esc (with nothing to cancel), the title bar's X, or its key.
fn close(h: HWND) {
    unsafe { let _ = DestroyWindow(h); }
}

/// Starts recording a key for the window; the hook swallows every key until it stops.
fn start_rec() {
    let Some(h) = hwnd() else { return };
    crate::settings_page::cancel_recording(); // one recording at a time
    crate::welcome::cancel_recording();
    let _ = set(|w| {
        w.rec = Rec::Recording(None);
        w.error = None;
    });
    crate::keys::record(h, true);
    relayout(None);
    focus_button(ID_CANCEL);
}

/// Ends recording or the clash prompt, saving nothing.
fn stop_rec() {
    let Some(h) = hwnd() else { return };
    crate::keys::record(h, false);
    let _ = set(|w| w.rec = Rec::Idle);
    relayout(None);
    focus_button(ID_SET);
}

/// The settings page starts recording, so a recording here stops (AC-HNT-4).
pub fn cancel_recording() {
    if get(|w| w.rec != Rec::Idle) == Some(true) {
        stop_rec();
    }
}

fn focus_button(id: i32) {
    if let Some(Some(b)) = get(|w| w.buttons.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)) {
        unsafe { let _ = SetFocus(Some(b)); }
    }
}

/// Saves an edit; a failure shows in the footer until the next action.
fn save(edit: Edit) {
    let failed = settings::write(edit).err();
    let _ = set(|w| w.error = failed);
    refresh();
    relayout(None);
}

/// Whether the hints window is the one recording: main hands it WM_RECORDED.
pub fn owns(h: HWND) -> bool {
    hwnd() == Some(h)
}

/// The hook's WM_RECORDED while this window records: the settings page's rules (spec 0012
/// AC-7, AC-9). Main thread.
pub fn recorded(vk: u16, mods: u8) {
    if !matches!(get(|w| w.rec.clone()), Some(Rec::Recording(_))) {
        return;
    }
    if vk == VK_ESCAPE.0 && mods == 0 {
        return stop_rec();
    }
    let refuse = |title: &str, sub: &str| {
        let _ = set(|w| w.rec = Rec::Recording(Some((title.into(), sub.into()))));
        relayout(None);
    };
    if !settings::savable(vk) {
        return refuse("This key can't be saved; try another", "Use a letter, a number row digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Still recording.");
    }
    if mods == 0 {
        return refuse("Hold Win, Ctrl, Alt or Shift with the key.", "A combo needs at least one of them. Still recording; Esc alone cancels.");
    }
    let Some(h) = hwnd() else { return };
    crate::keys::record(h, false);
    let combo = Combo { mods, vk };
    match recorded_rec(settings::get(), combo) {
        None => stop_rec(),
        Some(Rec::Idle) => {
            let _ = set(|w| w.rec = Rec::Idle);
            save(Edit::Combos("shortcuts.hints", vec![combo]));
            focus_button(ID_SET);
        }
        Some(clash) => {
            let _ = set(|w| w.rec = clash);
            relayout(None);
            focus_button(ID_CANCEL); // as the settings page's prompt: Cancel has focus
        }
    }
}

/// Move it here: the key comes here and the other shortcut loses it, in one write.
fn move_here() {
    let Some(Rec::Clash { combo, other, .. }) = get(|w| w.rec.clone()) else { return };
    let mine = Edit::Combos("shortcuts.hints", vec![combo]);
    let edit = match other {
        Some((key, rest)) => Edit::Pair(Box::new(mine), Box::new(Edit::Combos(key, rest))),
        None => mine,
    };
    let _ = set(|w| w.rec = Rec::Idle);
    save(edit);
    focus_button(ID_SET);
}

fn on_command(h: HWND, id: i32) {
    match id {
        ID_SET => start_rec(),
        ID_REMOVE => save(Edit::Combos("shortcuts.hints", vec![])),
        ID_NOTE if matches!(get(|w| w.view.pad), Some(Pad::Off | Pad::OffWin)) => save(Edit::Value("touchpad.enabled", true.into())),
        ID_NOTE => crate::settings_page::open(Some(crate::settings_page::Section::Touchpad)),
        ID_MOVE => move_here(),
        ID_CANCEL => stop_rec(),
        ID_SETTINGS => crate::settings_page::open(Some(crate::settings_page::Section::Shortcuts)),
        // Esc: backs out of the clash prompt first, like a dialog; else closes.
        _ if id == IDCANCEL.0 => match get(|w| w.rec.clone()) {
            Some(Rec::Idle) | None => close(h),
            Some(_) => stop_rec(),
        },
        _ => {}
    }
}

fn on_draw_item(dis: &DRAWITEMSTRUCT) {
    let _ = get(|w| {
        let Some(p) = w.places.iter().find(|p| p.id == dis.CtlID as i32) else { return };
        let r = dis.rcItem;
        let (pw, ph) = (r.right - r.left, r.bottom - r.top);
        let state = dis.itemState;
        let fonts = ui::fonts(w.scale);
        buffered(dis.hDC, r.left, r.top, pw, ph, |dc| {
            let c = Canvas::new(dc, true, w.scale, w.theme, &fonts);
            let (bw, bh) = (pw as f32 / w.scale, ph as f32 / w.scale);
            c.fill_rr((0.0, 0.0, bw + 1.0, bh + 1.0), 0.0, op(p.back));
            let focus = state.0 & ODS_FOCUS.0 != 0 && state.0 & ODS_NOFOCUSRECT.0 == 0;
            paint_button(&c, p, (RING, RING, bw - 2.0 * RING, bh - 2.0 * RING), state.0 & ODS_SELECTED.0 != 0, focus);
        });
    });
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
                    let fonts = ui::fonts(w.scale);
                    buffered(dc, 0, 0, r.right, r.bottom, |mem| {
                        let c = Canvas::new(mem, true, w.scale, w.theme, &fonts);
                        compose(&c, &mut vec![], &w.view, &w.rec, w.error.as_deref(), w.fit.height, w.scroll);
                    });
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // A button with focus hands the wheel on to here.
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_DRAWITEM => {
                on_draw_item(&*(lp.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
            }
            WM_COMMAND if (wp.0 >> 16) & 0xFFFF == BN_CLICKED as usize => {
                on_command(h, (wp.0 & 0xFFFF) as i32);
                LRESULT(0)
            }
            // Enter presses the focused button, like any Windows dialog; with none, nothing.
            DM_GETDEFID => {
                let focused = GetFocus();
                let id = get(|w| w.buttons.iter().find(|(_, b)| *b == focused).map(|(id, _)| *id)).flatten();
                match id {
                    Some(id) => LRESULT(((DC_HASDEFID as isize) << 16) | id as isize),
                    None => LRESULT(0),
                }
            }
            WM_ACTIVATE => {
                if wp.0 & 0xFFFF == WA_INACTIVE as usize {
                    // Recording ends when the window loses focus, as on the settings page (spec 0012 AC-7).
                    if matches!(get(|w| w.rec.clone()), Some(Rec::Recording(_))) {
                        stop_rec();
                    }
                    let focused = GetFocus();
                    let _ = set(|w| w.focus = w.buttons.iter().any(|(_, b)| *b == focused).then_some(focused));
                } else if (wp.0 >> 16) & 0xFFFF == 0 {
                    refresh(); // back from Windows Settings: the touchpad may read differently
                    if let Some(Some(b)) = get(|w| w.focus) {
                        let _ = SetFocus(Some(b));
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                close(h);
                LRESULT(0)
            }
            WM_DESTROY => {
                crate::keys::record(h, false);
                drop(WIN.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_GETOBJECT => TREE.with(|t| uia::answer_tree(h, wp, lp, t)),
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                let to = *(lp.0 as *const RECT);
                let _ = set(|w| {
                    w.dpi = dpi;
                    w.scale = dpi as f32 / 96.0;
                });
                relayout(Some(to));
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
                let t = ui::theme();
                if set(|w| std::mem::replace(&mut w.theme, t) != t) == Some(true) {
                    dark_title(h, &t);
                    relayout(None);
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
    use crate::settings::{ALT, CTRL, Gesture, SHIFT, WIN};
    use crate::setup::Wrote;
    use crate::ui::{DARK, LIGHT};

    fn c(mods: u8, vk: u16) -> Combo {
        Combo { mods, vk }
    }

    fn st() -> State {
        State { pad: true, enabled: true, rows: [PadRow::Nothing; 4], off: true, saved: true }
    }

    /// The design's own setup (frame 4): two combos for switching, a split send pair, Win 1–9
    /// given back, Find a window on Win Alt F, a twice action, Nothing, a typed combo, a workspace.
    fn mine() -> Settings {
        let mut s = Settings::default();
        s.previous.push(c(CTRL | ALT, VK_LEFT.0));
        s.next.push(c(CTRL | ALT, VK_RIGHT.0));
        s.send_previous.clear();
        s.jump = None;
        s.finder = vec![c(WIN | ALT, b'F' as u16)];
        s.copilot_as_right_ctrl = false;
        s.hints = vec![c(WIN | CTRL, b'H' as u16)];
        let set = |s: &mut Settings, f, m, t, a| {
            s.gestures.retain(|g: &Gesture| (g.fingers, g.motion, g.times) != (f, m, t));
            s.gestures.push(Gesture { fingers: f, motion: m, times: t, action: a });
        };
        set(&mut s, 4, Motion::Up, 2, Action::Finder);
        set(&mut s, 3, Motion::Down, 1, Action::Nothing);
        set(&mut s, 3, Motion::Tap, 1, Action::Combo(c(CTRL | ALT, b'T' as u16)));
        set(&mut s, 4, Motion::Tap, 1, Action::Jump(2));
        s
    }

    // AC-HNT-3: every row must show what the file holds now: a pair with the same modifiers is
    // one row, a split pair is two (next first), an empty list says No key, and every combo
    // of a list shows, so the sheet never hides a key you set.
    #[test]
    fn the_rows_follow_the_settings() {
        let d = read(&Settings::default(), &st(), None);
        assert_eq!(d.around[0], KeyRow { title: "Switch workspace".into(), keys: vec![vec!["Win".into(), "Ctrl".into(), "Left".into(), "Right".into()]] });
        assert_eq!(d.around[1].keys, [["Win", "1–9"]]);
        assert_eq!(d.windows.len(), 5);
        let v = read(&mine(), &st(), None);
        assert_eq!(v.around[0].keys.len(), 2, "both switch combos");
        assert!(v.around[1].keys.is_empty(), "Win 1–9 given back: No key");
        assert_eq!(v.around[3].keys, [["Win", "Alt", "F"]]);
        let titles: Vec<&str> = v.windows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles[..2], ["Send window to next workspace", "Send window to previous workspace"]);
        assert!(v.windows[1].keys.is_empty());
        assert!(!v.copilot);
        assert_eq!(v.key, [c(WIN | CTRL, b'H' as u16)]);
    }

    // AC-HNT-3: a gesture shows its action, a twice action under it, Nothing, or the keys it types.
    #[test]
    fn the_gestures_say_what_each_one_does() {
        let v = read(&mine(), &st(), None);
        let (three, four) = (&v.cards[0].1, &v.cards[1].1);
        assert_eq!(three[0].once, Does::Name("Next app".into()));
        assert_eq!(three[3].once, Does::Nothing);
        assert_eq!(three[4].once, Does::Types(vec!["Ctrl".into(), "Alt".into(), "T".into()]));
        assert_eq!((four[2].once.clone(), four[2].twice.as_deref()), (Does::Name("Overview".into()), Some("Twice: Find a window")));
        assert_eq!(four[4].once, Does::Name("Go to workspace 3".into()));
    }

    // AC-HNT-5: each of spec 0011's touchpad states gets its own words, so you always know
    // why a gesture does nothing, and the button that fixes it.
    #[test]
    fn every_touchpad_state_says_why() {
        assert_eq!(pad(&st(), None), Pad::On);
        let wait = State { rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], ..st() };
        let (status, words, button) = pad_words(pad(&wait, None));
        let (_, title, line, warn) = words.unwrap();
        assert_eq!((status, title.as_str(), warn, button), ("Not running", "2 of 4 are still on in Windows", true, Some("Touchpad setup")));
        assert_eq!(line, "Still on: three-finger swipes, four-finger swipes. des-k runs these once all four are off; until then, the 2 you turned off don't work.");
        let cant = State { rows: [PadRow::Nothing, PadRow::CantRead, PadRow::Nothing, PadRow::Nothing], ..st() };
        assert!(pad_words(pad(&cant, None)).1.unwrap().2.starts_with("It is the 3-finger tap setting."));
        // A give back writes your values back, so Windows reads them on again until you sign in.
        let back = GaveBack { wrote: [Wrote::All; 4], cleared: true };
        let given = State { rows: [PadRow::StillOn; 4], off: false, saved: false, ..st() };
        assert_eq!(pad(&given, Some(back)), Pad::GaveBack);
        assert_eq!(pad(&st(), Some(back)), Pad::On, "Windows' gestures read off again: the give back is over");
        assert_eq!(pad(&State { enabled: false, ..st() }, None), Pad::Off);
        assert_eq!(pad(&State { enabled: false, off: false, ..st() }, None), Pad::OffWin);
        assert_eq!(pad_words(Pad::OffWin).2, Some("Turn reading on"));
        assert_eq!(pad(&State { pad: false, ..st() }, None), Pad::Missing);
        assert_eq!(pad_words(Pad::Missing).2, None);
    }

    // AC-HNT-4: a free key saves at once; one another shortcut has asks first and names it,
    // and Move it here takes it from that one only; a digit of a set only costs that digit.
    #[test]
    fn a_recorded_key_saves_or_asks_first() {
        let s = Settings::default();
        assert_eq!(recorded_rec(&s, c(WIN | CTRL, b'H' as u16)), Some(Rec::Idle));
        match recorded_rec(&s, c(WIN, b'F' as u16)) {
            Some(Rec::Clash { title, sub, other, .. }) => {
                assert_eq!(title, "Win F is already in use");
                assert_eq!(sub, "“Find a window” uses it. Move it here and “Find a window” loses this key. Cancel changes nothing.");
                assert_eq!(other, Some(("shortcuts.finder", vec![])));
            }
            r => panic!("{r:?}"),
        }
        match recorded_rec(&s, c(WIN, b'3' as u16)) {
            Some(Rec::Clash { sub, other: None, .. }) => assert!(sub.contains("skips 3"), "{sub}"),
            r => panic!("{r:?}"),
        }
        let set = Settings { hints: vec![c(WIN | SHIFT, b'H' as u16)], ..Settings::default() };
        assert_eq!(recorded_rec(&set, c(WIN | SHIFT, b'H' as u16)), None, "already its key");
    }

    /// A render state: its name, the settings, Windows' touchpad, a give back, the recorder, the height.
    type Shot<'a> = (&'a str, &'a Settings, State, Option<GaveBack>, Rec, f32);

    /// Draws the design's states to target/hints/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/hr3-* (spec 0017 AC-1, AC-HNT-1 to AC-HNT-5).
    #[test]
    #[ignore]
    fn render() {
        ui::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("hints");
        std::fs::create_dir_all(&dir).unwrap();
        let d = Settings::default();
        let keyed = Settings { hints: vec![c(WIN | CTRL, b'H' as u16)], ..Settings::default() };
        let rec = Rec::Recording(None);
        let clash = recorded_rec(&d, c(WIN, b'F' as u16)).unwrap();
        let rows = |r: [PadRow; 4]| State { rows: r, ..st() };
        let wait = rows([PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing]);
        let cant = rows([PadRow::Nothing, PadRow::CantRead, PadRow::Nothing, PadRow::Nothing]);
        let back = Some(GaveBack { wrote: [Wrote::All; 4], cleared: true });
        let my = mine();
        let states: Vec<Shot> = vec![
            ("defaults", &d, st(), None, Rec::Idle, H),
            ("key-set", &keyed, st(), None, Rec::Idle, H),
            ("recording", &d, st(), None, rec, H),
            ("clash", &d, st(), None, clash, H),
            ("mine", &my, st(), None, Rec::Idle, H),
            ("still-on", &d, wait, None, Rec::Idle, H),
            ("cant-read", &d, cant, None, Rec::Idle, H),
            ("given-back", &d, State { rows: [PadRow::StillOn; 4], off: false, saved: false, ..st() }, back, Rec::Idle, H),
            ("reading-off", &d, State { enabled: false, ..st() }, None, Rec::Idle, H),
            ("reading-off-windows", &d, State { enabled: false, off: false, ..st() }, None, Rec::Idle, H),
            ("no-touchpad", &d, State { pad: false, ..st() }, None, Rec::Idle, H),
            ("short", &d, st(), None, Rec::Idle, 528.0),
        ];
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let themes: [(&str, Theme, f32); 5] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5)];
        for (tname, t, scale) in themes {
            let fonts = ui::fonts(scale);
            for (name, s, state, back, rec, height) in &states {
                let v = read(s, state, *back);
                let (w, h) = ((W * scale).round() as i32, (height * scale).round() as i32);
                ui::tests::save_png(w, h, &dir.join(format!("{name}-{tname}.png")), |dc| {
                    let c = Canvas::new(dc, true, scale, t, &fonts);
                    let mut places = vec![];
                    compose(&c, &mut places, &v, rec, None, *height, 0.0);
                    // The buttons are windows of their own: drawn here where they sit.
                    for p in &places {
                        let focus = p.id == ID_CANCEL && matches!(rec, Rec::Clash { .. });
                        c.fill_rr((p.bx.0 - RING, p.bx.1 - RING, p.bx.2 + 2.0 * RING, p.bx.3 + 2.0 * RING), 0.0, op(p.back));
                        paint_button(&c, p, p.bx, false, focus);
                    }
                });
            }
        }
    }
}
