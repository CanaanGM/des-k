// What a recorded combo does on this page: a key row, the digits, or a gesture, and the clash
// prompt (spec 0012 AC-7, AC-12).
use super::*;
use super::menu::set_gesture;
use super::rows::{WinKey, digit_caps, entry, has_win, is_digits, name_of, shortcut, spelled, took};

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

/// Starts recording; the hook swallows every key until it stops.
pub(super) fn start_rec(rec: Rec) {
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
pub(super) fn stop_rec() {
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
pub(super) fn remove(key: &'static str, i: usize) {
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
pub(super) fn give_key(key: &'static str, desk: bool) {
    run(plan_give(key, desk));
}

pub(crate) fn plan_give(key: &'static str, desk: bool) -> Plan {
    let Some(sh) = shortcut(key) else { return Plan::Nothing };
    let Some((wk, _)) = sh.win else { return Plan::Nothing };
    let s = settings::get();
    if desk == has_win(&s, sh) || (desk && took(&s, sh).is_some()) {
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
pub(super) fn reset_keys(key: &'static str) {
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

#[cfg(test)]
mod tests;
