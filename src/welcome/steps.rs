// The first run's steps and their plans: the step list, step 1's tries, step 2's rows and what
// recording a key or choosing des-k or Windows writes, the saves, and going from step to step.
use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Step {
    Try,
    Keys,
    Behavior,
    Pad,
    Done,
}

impl Step {
    pub(super) fn title(self) -> &'static str {
        match self {
            Step::Try => "Every screen gets its own workspaces",
            Step::Keys => "Shortcuts",
            Step::Behavior => "Behavior",
            Step::Pad => "Touchpad",
            Step::Done => "des-k is ready",
        }
    }

    pub(super) fn height(self) -> f32 {
        if matches!(self, Step::Try | Step::Behavior) { SHORT } else { TALL }
    }

    pub(super) fn index(self) -> usize {
        self as usize
    }
}

/// Step 1's rows: what to try, the key it shows, what to do, in the design's words.
pub(super) const TRIES: [(&str, &str, &str); 3] = [
    ("Switch workspace", "shortcuts.next", "Move your mouse to a screen, then press the keys."),
    ("Open overview", "shortcuts.overview", "Every workspace of every screen at once. Esc closes it."),
    ("Find a window", "shortcuts.finder", "Type part of a window's name, then Enter goes to it."),
];

/// A row of step 2: the des-k action that takes a Windows key, the [shortcuts] keys behind it
/// (two for a ← → pair, none for the Copilot key), Windows' key, and what Windows does with it.
pub(super) struct Take {
    pub(super) keys: &'static [&'static str],
    pub(super) title: &'static str,
    win: &'static str,
    does: &'static str,
}

pub(super) const TAKES: [Take; 7] = [
    Take { keys: &["shortcuts.previous", "shortcuts.next"], title: "Switch workspace", win: "Win Ctrl ← →", does: "switches desktops" },
    Take { keys: &["shortcuts.overview"], title: "Open overview", win: "Win Tab", does: "opens Task View" },
    Take { keys: &["shortcuts.finder"], title: "Find a window", win: "Win F", does: "opens Feedback Hub" },
    Take { keys: &["shortcuts.jump"], title: "Go to workspace 1–9", win: "Win 1–9", does: "opens taskbar app 1–9" },
    Take { keys: &["shortcuts.send_to"], title: "Send window to 1–9", win: "Win Shift 1–9", does: "new window of taskbar app 1–9" },
    Take { keys: &["shortcuts.send_to_stay"], title: "Send window to 1–9 and stay", win: "Win Ctrl 1–9", does: "last window of taskbar app 1–9" },
    Take { keys: &[], title: "Copilot key as Right Ctrl", win: "Copilot", does: "Copilot, or what you set" },
];

/// Step 2's More rows: keys des-k has that Windows doesn't.
pub(super) const MORE_KEYS: [(&str, &[&str]); 3] = [
    ("Send window to next or previous", &["shortcuts.send_previous", "shortcuts.send_next"]),
    ("Send window to next or previous and stay", &["shortcuts.send_previous_stay", "shortcuts.send_next_stay"]),
    ("Pin window to all workspaces", &["shortcuts.pin"]),
];

/// What the recorder is recording for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Rec {
    List(&'static str, Option<usize>), // a list, and the key it replaces (None: a new one)
    Digits(&'static str),
    Pair(&'static str, &'static str), // ← and → together (AC-FR-4)
    Gesture(usize, u8),               // a GESTURE_ROWS row and its times: Key combo…
}

/// A key field you click to record: a step 2 row, a More row, or the hints key on Done.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Field {
    Take(usize),
    More(usize),
    Hints,
}

/// The steps shown: no Touchpad step without a precision touchpad (AC-FR-5).
pub(super) fn steps(w: &Win) -> Vec<Step> {
    let pad = w.pics.pad.is_none_or(|p| p.pad);
    [Step::Try, Step::Keys, Step::Behavior, Step::Pad, Step::Done].into_iter().filter(|s| pad || *s != Step::Pad).collect()
}

// ---- Keys ----

/// An action's keys as words: "Win Ctrl →".
pub(super) fn words(caps: &[String]) -> String {
    sp::rows::spelled(caps)
}

/// The key a row's field shows: Windows' own first. A ← → pair shows as one key when both
/// sides hold the same modifiers.
pub(super) fn row_caps(s: &Settings, keys: &[&'static str]) -> Option<Vec<String>> {
    let first = |k: &str| {
        let sh = sp::rows::shortcut(k)?;
        let i = *sp::rows::drawn_order(s, sh).first()?;
        sp::rows::keys_of(s, k).get(i).cloned()
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
pub(super) enum Held {
    Desk,                       // des-k has it, for this action
    Own(Option<&'static str>),  // given back, and the action has your own key (taken from that action)
    Bare,                       // given back, no key
    Took(&'static str),         // another des-k action holds it
}

pub(super) fn held(s: &Settings, keys: &[&'static str]) -> Held {
    let shs: Vec<&sp::rows::Short> = keys.iter().filter_map(|k| sp::rows::shortcut(k)).collect();
    if !shs.is_empty() && shs.iter().all(|sh| sp::rows::has_win(s, sh)) {
        return Held::Desk;
    }
    if let Some(o) = shs.iter().find_map(|sh| sp::rows::took(s, sh)) {
        return Held::Took(o);
    }
    if row_caps(s, keys).is_none() {
        return Held::Bare;
    }
    // Your key is another action's Windows key: say which.
    let from = keys.first().and_then(|k| {
        let mine = sp::rows::shortcut(k)?;
        sp::rows::MOVE.iter().chain(sp::rows::SEND).filter(|o| o.key != mine.key).find_map(|o| match o.win?.0 {
            WinKey::Digits(m) if s.set(sp::rows::name_of(mine.key)) == Some(m) => Some(o.title),
            WinKey::Combo(c) if s.list(sp::rows::name_of(mine.key)).first() == Some(&c) => Some(o.title),
            _ => None,
        })
    });
    Held::Own(from)
}

/// A row's caption: whose key it is now and what Windows does with it.
pub(super) fn take_caption(s: &Settings, i: usize) -> String {
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
pub(super) fn plan_pair(p: &'static str, n: &'static str, mods: u8, replace: bool, title: &'static str) -> Plan {
    let s = settings::get();
    let mut edits = vec![];
    let mut clash: Option<&'static str> = None;
    let mut same = true;
    for (key, vk) in [(p, VK_LEFT.0), (n, VK_RIGHT.0)] {
        let name = sp::rows::name_of(key);
        let combo = Combo { mods, vk };
        let mut mine = s.list(name).to_vec();
        let at = if replace { sp::rows::shortcut(key).and_then(|sh| sp::rows::drawn_order(&s, sh).first().copied()) } else { None };
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
            && other != sp::rows::name_of(p)
            && other != sp::rows::name_of(n)
        {
            let (other_key, other_title) = sp::rows::entry(other);
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
            let mut held = crate::hints::rows::mods_caps(mods);
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
pub(super) fn plan_side(i: usize, desk: bool) -> Plan {
    let t = &TAKES[i];
    match t.keys {
        [k] => sp::recording::plan_give(k, desk),
        [p, n] if desk => {
            let s = settings::get();
            if held(&s, t.keys) == Held::Desk || matches!(held(&s, t.keys), Held::Took(_)) {
                return Plan::Nothing;
            }
            plan_pair(p, n, WIN | CTRL, false, t.title)
        }
        [p, n] => match (sp::recording::plan_give(p, false), sp::recording::plan_give(n, false)) {
            (Plan::Commit(a, t), Plan::Commit(b, _)) => Plan::Commit(Edit::Pair(Box::new(a), Box::new(b)), t),
            (Plan::Commit(a, t), _) | (_, Plan::Commit(a, t)) => Plan::Commit(a, t),
            _ => Plan::Nothing,
        },
        _ => Plan::Nothing,
    }
}

/// What a recording is for, from a field.
pub(super) fn rec_for(f: Field) -> Option<Rec> {
    let s = settings::get();
    let keys: &[&'static str] = match f {
        Field::Take(i) => TAKES[i].keys,
        Field::More(i) => MORE_KEYS[i].1,
        Field::Hints => &["shortcuts.hints"],
    };
    Some(match keys {
        [p, n] => Rec::Pair(p, n),
        [k] if sp::rows::is_digits(k) => Rec::Digits(k),
        [k] => Rec::List(k, sp::rows::shortcut(k).and_then(|sh| sp::rows::drawn_order(&s, sh).first().copied())),
        _ => return None,
    })
}

pub(super) fn field_of(rec: Rec) -> Option<Field> {
    let key = match rec {
        Rec::List(k, _) | Rec::Digits(k) | Rec::Pair(k, _) => k,
        Rec::Gesture(..) => return None,
    };
    field_of_key(key)
}

pub(super) fn field_of_key(key: &str) -> Option<Field> {
    if key == "shortcuts.hints" {
        return Some(Field::Hints);
    }
    if let Some(i) = TAKES.iter().position(|t| t.keys.contains(&key)) {
        return Some(Field::Take(i));
    }
    MORE_KEYS.iter().position(|(_, k)| k.contains(&key)).map(Field::More)
}

pub(super) fn title_of(f: Field) -> &'static str {
    match f {
        Field::Take(i) => TAKES[i].title,
        Field::More(i) => MORE_KEYS[i].0,
        Field::Hints => "Shortcut hints key",
    }
}

/// The recording's note: what to press.
pub(super) fn rec_note(rec: Rec) -> (String, String) {
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

/// Writes one value; a failure shows what stays and why until the next good save.
pub(super) fn save(key: &'static str, v: i64) {
    let Some(row) = sp::rows::find_row(key) else { return };
    commit(Edit::Value(key, sp::rows::file_value(row, v)), row.title);
}

pub(super) fn commit(edit: Edit, title: &str) {
    match settings::edit::write(edit) {
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

pub(super) fn run(plan: Plan) {
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

/// Goes to a step: the window grows or shrinks from its top edge, Next has the focus.
pub(super) fn go(step: Step) {
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
pub(super) fn client_height(h: HWND, ch: f32) {
    let (ww, wh) = size(h, W, ch);
    unsafe { let _ = SetWindowPos(h, None, 0, 0, ww, wh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE); }
}

pub(super) fn grow_tick(h: HWND) {
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

pub(super) fn next_step(back: bool) -> Option<Step> {
    get(|w| {
        let list = steps(w);
        let i = list.iter().position(|s| *s == w.step)?;
        if back { i.checked_sub(1).map(|j| list[j]) } else { list.get(i + 1).copied() }
    })
    .flatten()
}

#[cfg(test)]
mod tests;
