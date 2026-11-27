// The hints window's rows and their words: what it reads from settings.toml and Windows' touchpad,
// key caps, action names, the touchpad column's words, and what a recorded key would do.
use super::*;

/// One keyboard row: what it does, and each combo as key caps (none: No key).
#[derive(Clone, PartialEq, Debug)]
pub(super) struct KeyRow {
    pub(super) title: String,
    pub(super) keys: Vec<Vec<String>>,
}

/// What one gesture does: an action's name, Nothing, or a key combo it types.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Does {
    Name(String),
    Nothing,
    Types(Vec<String>),
}

#[derive(Clone, PartialEq, Debug)]
pub(super) struct Gact {
    pub(super) motion: Motion,
    pub(super) once: Does,
    pub(super) twice: Option<String>, // "Twice: Find a window"
}

/// What the touchpad runs now (spec 0011's states, AC-HNT-5).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Pad {
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
pub(super) struct View {
    pub(super) around: Vec<KeyRow>,
    pub(super) windows: Vec<KeyRow>,
    pub(super) copilot: bool,
    pub(super) rule: &'static str, // which screen keys act on (spec 0038 AC-11)
    pub(super) cards: Vec<(u8, Vec<Gact>)>, // fingers, then left, right, up, down, tap
    pub(super) pad: Pad,
    pub(super) key: Vec<Combo>,
}

// ---- What it reads ----

pub(crate) fn caps(c: &Combo) -> Vec<String> {
    c.to_string().split('+').map(String::from).collect()
}

pub(crate) fn mods_caps(mods: u8) -> Vec<String> {
    settings::mods_text(Some(mods)).split('+').map(String::from).collect()
}

pub(super) fn list(l: &[Combo]) -> Vec<Vec<String>> {
    l.iter().map(caps).collect()
}

pub(super) fn digits(set: Option<u8>) -> Vec<Vec<String>> {
    set.map(|m| mods_caps(m).into_iter().chain(["1–9".to_string()]).collect()).into_iter().collect()
}

/// An arrow pair is one ← → row only when both sides hold the same modifiers with Left and
/// Right; otherwise two rows, next first, each with its own keys or No key.
pub(super) fn pair(previous: &[Combo], next: &[Combo], (both, prev_title, next_title): (&str, &str, &str)) -> Vec<KeyRow> {
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
        Action::Back => "Back to the last workspace".into(),
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

pub(super) fn does(a: Option<Action>) -> Does {
    match a {
        None | Some(Action::Nothing) => Does::Nothing,
        Some(Action::Combo(c)) => Does::Types(caps(&c)),
        Some(a) => Does::Name(name(a)),
    }
}

/// Windows' touchpad state as the touchpad column words it.
pub(super) fn pad(st: &State, back: Option<GaveBack>) -> Pad {
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

pub(super) fn read(s: &Settings, st: &State, back: Option<GaveBack>) -> View {
    let row = |title: &str, keys| KeyRow { title: title.into(), keys };
    let mut around = pair(&s.previous, &s.next, ("Switch workspace", "Previous workspace", "Next workspace"));
    around.extend([row("Go to workspace 1–9", digits(s.jump)), row("Back to the last workspace", list(&s.back)), row("Open overview", list(&s.overview)), row("Find a window", list(&s.finder)), row("Pause des-k", list(&s.pause))]);
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
    View { around, windows, copilot: s.copilot_as_right_ctrl, rule: rule_line(s.act_on), cards, pad: pad(st, back), key: s.hints.clone() }
}

/// The line saying which screen keys act on (spec 0038 AC-11).
pub(super) fn rule_line(act_on: settings::ActOn) -> &'static str {
    match act_on {
        settings::ActOn::Mouse => "Keys act on the screen under the mouse.",
        settings::ActOn::Focus => "Keys act on the focused window's screen; swipes on the screen under the pointer.",
    }
}

pub(super) fn read_now() -> View {
    read(&settings::get(), &setup::state(), crate::welcome::gave_back())
}

/// "three-finger swipes" from setup's "3-finger swipes".
pub(super) fn spelled(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}

/// A note's icon, title, line, and whether it warns.
pub(super) type Words = (&'static str, String, String, bool);

/// The touchpad column's status, its note and its note's button.
pub(super) fn pad_words(p: Pad) -> (&'static str, Option<Words>, Option<&'static str>) {
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
pub(super) fn owner_name(list: &str) -> (&'static str, &'static str) {
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
        "pause" => ("Pause des-k", "shortcuts.pause"),
        "jump" => ("Go to workspace 1–9", "shortcuts.jump"),
        "back" => ("Back to the last workspace", "shortcuts.back"),
        "send_to" => ("Send window to 1–9", "shortcuts.send_to"),
        "send_to_stay" => ("Send window to 1–9 and stay", "shortcuts.send_to_stay"),
        _ => ("", ""),
    }
}

/// A recorded key: saved at once, or the clash prompt when another shortcut has it (AC-HNT-4,
/// the settings page's rules, spec 0012 AC-9). None: it is already the key.
pub(super) fn recorded_rec(s: &Settings, combo: Combo) -> Option<Rec> {
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

#[cfg(test)]
mod tests;
