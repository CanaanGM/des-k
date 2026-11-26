// Reads settings.toml's text into `Settings`: each key checked, a bad value reset alone with a
// problem line, and the key names and limits the reader holds the file to (spec 0001).
use super::*;

const TOP_KEYS: &[&str] = &["workspaces", "copilot_as_right_ctrl", "follow_activation", "act_on", "jump_back", "fastest", "first_run_seen", "whats_new_seen", "shortcuts", "indicator", "attention", "colors", "names", "wallpaper", "apps", "restore", "overview", "touchpad", "gesture"];
const SHORTCUT_KEYS: &[&str] = &["previous", "next", "send_previous", "send_next", "send_previous_stay", "send_next_stay", "jump", "send_to", "send_to_stay", "back", "finder", "overview", "pin", "hints", "pause"];
const INDICATOR_KEYS: &[&str] = &["enabled", "style", "screen_name", "spot", "gap", "x", "y", "hold_ms", "fade_ms", "screens"];
const SCREEN_KEYS: &[&str] = &["spot", "gap", "x", "y"];
const ATTENTION_KEYS: &[&str] = &["enabled", "text"];
const COLORS_KEYS: &[&str] = &["accent", "ws1", "ws2", "ws3", "ws4", "ws5", "ws6", "ws7", "ws8", "ws9"];
pub(super) const NAMES_KEYS: &[&str] = &["ws1", "ws2", "ws3", "ws4", "ws5", "ws6", "ws7", "ws8", "ws9", "screens"];
const SCREEN_NAMES_KEYS: &[&str] = &["screen", "ws1", "ws2", "ws3", "ws4", "ws5", "ws6", "ws7", "ws8", "ws9"];
pub(super) const WALLPAPER_KEYS: &[&str] = &["ws1", "ws2", "ws3", "ws4", "ws5", "ws6", "ws7", "ws8", "ws9", "screens", "fit", "transition", "dim", "interval", "intervals", "shuffle"];
const INTERVAL_MAX: i64 = 1440; // minutes: a day (spec 0028 AC-2)
const PICTURE_MAX: usize = 255; // in UTF-16 units (spec 0026 AC-2)
const PICTURE_TYPES: &[&str] = &["jpg", "jpeg", "png", "bmp", "gif"];
const RULE_KEYS: &[&str] = &["workspace", "screen"];
const APP_KEY_MAX: usize = 260;
const RESTORE_KEYS: &[&str] = &["enabled", "minutes"];
const OVERVIEW_KEYS: &[&str] = &["animation", "labels", "live", "fade_ms"];
const TOUCHPAD_KEYS: &[&str] = &["enabled", "swipe_mm", "twice_ms"];
const GESTURE_KEYS: &[&str] = &["fingers", "swipe", "tap", "times", "do"];

/// One problem found in the file, in the log's words. `key` is like "indicator.hold_ms", or
/// empty when the problem names no single setting.
#[derive(PartialEq, Debug)]
pub(crate) struct Problem {
    pub key: String,
    pub message: String,
}

impl Problem {
    pub(super) fn new(message: String) -> Self {
        let key = message.split_once(": ").map_or("", |(k, _)| k);
        let key = if key.contains(' ') { "" } else { key };
        Problem { key: key.to_string(), message }
    }
}

#[cfg(test)]
pub(crate) fn parse_for_test(text: &str) -> (Settings, Vec<String>) {
    parse(text)
}

/// Like parse(), but a file that is not TOML is an error instead of all defaults.
pub(super) fn reparse(text: &str) -> Result<(Settings, Vec<String>), String> {
    match text.parse::<Table>() {
        Ok(_) => Ok(parse(text)),
        Err(e) => Err(not_toml(&e.to_string(), e.message())),
    }
}

/// The first line of a parser's error carries the line and column; its message says what.
pub(super) fn not_toml(full: &str, message: &str) -> String {
    format!("not valid TOML ({}: {message})", full.lines().next().unwrap_or(""))
}

/// Pure, so the tests can run it: the settings plus one message per problem found.
pub(super) fn parse(text: &str) -> (Settings, Vec<String>) {
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
    s.act_on = choice(&top, "act_on", "act_on", ACT_ON, s.act_on, &mut warn);
    s.jump_back = boolean(&top, "jump_back", "jump_back", s.jump_back, &mut warn);
    s.fastest = boolean(&top, "fastest", "fastest", s.fastest, &mut warn);
    s.first_run_seen = top.contains_key("first_run_seen").then(|| boolean(&top, "first_run_seen", "first_run_seen", false, &mut warn));
    // Only ever compared with des-k's own version, never shown or opened (spec 0029 AC-9).
    s.whats_new_seen = match top.get("whats_new_seen") {
        None => None,
        Some(Value::String(v)) => {
            let seen = crate::update::parse_version(v);
            if seen.is_none() {
                warn.push("whats_new_seen: not a version like \"0.9.0\", so What's new counts it as missing".into());
            }
            seen
        }
        Some(v) => {
            warn.push(format!("whats_new_seen: expected text like \"0.9.0\", got a {}, so What's new counts it as missing", v.type_str()));
            None
        }
    };

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
        // Switch keys always win: a send list (or back, the finder, the overview, pin, hints, pause) sharing
        // a combo with them, or with an earlier list still on, is turned off (an empty list never
        // matches) (specs 0004, 0007, 0010, 0014, 0017, 0024).
        let mut taken = vec![("previous", s.previous.clone()), ("next", s.next.clone())];
        let sends = [
            ("back", &mut s.back),
            ("send_previous", &mut s.send_previous),
            ("send_next", &mut s.send_next),
            ("send_previous_stay", &mut s.send_previous_stay),
            ("send_next_stay", &mut s.send_next_stay),
            ("finder", &mut s.finder),
            ("overview", &mut s.overview),
            ("pin", &mut s.pin),
            ("hints", &mut s.hints),
            ("pause", &mut s.pause),
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
        s.indicator_screen_name = boolean(t, "screen_name", "indicator.screen_name", s.indicator_screen_name, &mut warn);
        s.hold_ms = int(t, "hold_ms", "indicator.hold_ms", 0, 5000, s.hold_ms as i64, &mut warn) as u32;
        s.fade_ms = int(t, "fade_ms", "indicator.fade_ms", 0, 2000, s.fade_ms as i64, &mut warn) as u32;
        let d = Pos::default();
        s.indicator_pos = Pos {
            spot: choice(t, "spot", "indicator.spot", SPOTS, d.spot, &mut warn),
            gap: int(t, "gap", "indicator.gap", 0, 400, d.gap as i64, &mut warn) as u32,
            x: percent(t, "x", "indicator.x", d.x, &mut warn),
            y: percent(t, "y", "indicator.y", d.y, &mut warn),
        };
        s.indicator_screens = screens(t, s.indicator_pos, &mut warn);
    }

    if let Some(t) = section(&top, "attention", &mut warn) {
        unknown_keys(t, "attention.", ATTENTION_KEYS, &mut warn);
        s.attention_enabled = boolean(t, "enabled", "attention.enabled", s.attention_enabled, &mut warn);
        s.attention_text = attention_text(t, &mut warn);
    }

    if let Some(t) = section(&top, "colors", &mut warn) {
        unknown_keys(t, "colors.", COLORS_KEYS, &mut warn);
        s.accent = match t.get("accent") {
            Some(Value::String(w)) if w.eq_ignore_ascii_case("windows") => Accent::Windows,
            _ => color(t, "accent", "colors.accent", "\"windows\"", &mut warn).map_or(Accent::Windows, Accent::Rgb),
        };
        for (i, c) in s.ws_colors.iter_mut().enumerate() {
            let key = COLORS_KEYS[i + 1];
            *c = color(t, key, &format!("colors.{key}"), "des-k's color", &mut warn);
        }
    }

    if let Some(t) = section(&top, "names", &mut warn) {
        unknown_keys(t, "names.", NAMES_KEYS, &mut warn);
        for (i, n) in s.names.iter_mut().enumerate() {
            let key = NAMES_KEYS[i];
            *n = name(t, key, &format!("names.{key}"), &mut warn).unwrap_or_default();
        }
        s.names_screens = name_screens(t, &mut warn);
    }

    if let Some(t) = section(&top, "wallpaper", &mut warn) {
        unknown_keys(t, "wallpaper.", WALLPAPER_KEYS, &mut warn);
        for (i, p) in s.wallpaper.iter_mut().enumerate() {
            let key = WALLPAPER_KEYS[i];
            *p = pick(t, key, &format!("wallpaper.{key}"), &mut warn).unwrap_or_default();
        }
        s.wallpaper_screens = wallpaper_screens(t, &mut warn);
        s.wallpaper_fit = choice(t, "fit", "wallpaper.fit", FITS, s.wallpaper_fit, &mut warn);
        s.wallpaper_fade = choice(t, "transition", "wallpaper.transition", TRANSITIONS, s.wallpaper_fade, &mut warn);
        s.wallpaper_dim = int(t, "dim", "wallpaper.dim", 0, 80, 0, &mut warn) as u8;
        s.wallpaper_interval = int(t, "interval", "wallpaper.interval", 1, INTERVAL_MAX, 30, &mut warn) as u16;
        s.wallpaper_shuffle = boolean(t, "shuffle", "wallpaper.shuffle", false, &mut warn);
        if let Some(it) = t.get("intervals") {
            match it.as_table() {
                Some(it) => {
                    unknown_keys(it, "wallpaper.intervals.", &WALLPAPER_KEYS[..9], &mut warn);
                    for (i, key) in WALLPAPER_KEYS[..9].iter().enumerate() {
                        // A bad one goes back to following `interval` (AC-2).
                        let n = int(it, key, &format!("wallpaper.intervals.{key}"), 1, INTERVAL_MAX, 0, &mut warn);
                        s.wallpaper_intervals[i] = (n > 0).then_some(n as u16);
                    }
                }
                None => warn.push(format!("wallpaper.intervals: expected a [wallpaper.intervals] table, got a {}, ignored", it.type_str())),
            }
        }
    }

    if let Some(t) = section(&top, "apps", &mut warn) {
        s.apps = apps(t, &file_order(text, "apps"), &mut warn);
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
        return Err(format!("unknown field {}", key_shown(key)));
    }
    let fingers = match t.get("fingers") {
        Some(Value::Integer(n @ 3..=4)) => *n as u8,
        Some(v) => return Err(format!("fingers = {}, not 3 or 4", shown(v))),
        None => return Err("no fingers".into()),
    };
    let motion = match (t.get("swipe"), t.get("tap")) {
        (Some(Value::String(w)), None) => match SWIPES.iter().find(|(s, _)| s.eq_ignore_ascii_case(w)) {
            Some((_, m)) => *m,
            None => return Err(format!("swipe = {}, not \"left\", \"right\", \"up\" or \"down\"", quoted(w))),
        },
        (Some(v), None) => return Err(format!("swipe = {}, not \"left\", \"right\", \"up\" or \"down\"", shown(v))),
        (None, Some(Value::Boolean(true))) => Motion::Tap,
        (None, Some(v)) => return Err(format!("tap = {}, only tap = true works", shown(v))),
        (Some(_), Some(_)) => return Err("both swipe and tap".into()),
        (None, None) => return Err("neither swipe nor tap".into()),
    };
    let times = match t.get("times") {
        None => 1,
        Some(Value::Integer(n @ 1..=2)) => *n as u8,
        Some(v) => return Err(format!("times = {}, not 1 or 2", shown(v))),
    };
    let action = match t.get("do") {
        Some(Value::String(text)) => action(text)?,
        Some(v) => return Err(format!("do = {}, not text", shown(v))),
        None => return Err("no do".into()),
    };
    Ok(Gesture { fingers, motion, times, action })
}

/// An action name, any case, or else a key combo like the [shortcuts] ones.
pub(super) fn action(text: &str) -> Result<Action, String> {
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
    if has_path(text) {
        return Err(format!("do = {PATH_VALUE} is not an action name or a key combo"));
    }
    parse_combo(text).map(Action::Combo).map_err(|why| format!("do = \"{text}\" is not an action name, and as a key combo it {why}"))
}

/// What a warn line says in place of a value with a path in it. Repeating a typo helps you find
/// it, but a path can name your user folder, and a log is safe to share (SECURITY.md).
const PATH_VALUE: &str = "a value with a path in it";

fn has_path(text: &str) -> bool {
    text.contains(['\\', '/', ':'])
}

/// Text from the file, quoted for a warn line, unless it has a path in it.
fn quoted(text: &str) -> String {
    if has_path(text) { PATH_VALUE.to_string() } else { format!("\"{text}\"") }
}

/// A key des-k does not know, for a warn line, unless it has a path in it.
fn key_shown(key: &str) -> String {
    if has_path(key) { "a key with a path in it".to_string() } else { key.to_string() }
}

/// A value of the wrong type, as TOML writes it, for a warn line, unless it has a path in it.
fn shown(v: &Value) -> String {
    let text = v.to_string();
    if has_path(&text) { PATH_VALUE.to_string() } else { text }
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
                warn.push(format!("{name}: {} is not one of {}, using default \"{fallback}\"", quoted(text), words()));
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
        warn.push(format!("unknown key {prefix}{}, ignored", key_shown(key)));
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

/// `[attention] text`: any text up to `ATTENTION_MAX` UTF-16 units, kept as typed. The log
/// line never quotes it (spec 0022 AC-9).
fn attention_text(t: &Table, warn: &mut Vec<String>) -> String {
    let fail = |warn: &mut Vec<String>, why: String| {
        warn.push(format!("attention.text: {why}, using the default"));
        ATTENTION_TEXT.to_string()
    };
    match t.get("text") {
        None => ATTENTION_TEXT.into(),
        Some(Value::String(s)) if s.trim().is_empty() => fail(warn, "it is empty".into()),
        Some(Value::String(s)) if s.encode_utf16().count() > ATTENTION_MAX => fail(warn, format!("{} characters, the most is {ATTENTION_MAX}", s.encode_utf16().count())),
        Some(Value::String(s)) => s.clone(),
        Some(v) => fail(warn, format!("expected text, got a {}", v.type_str())),
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

/// A percent, 0 to 100; a whole number or a decimal (spec 0020 AC-8).
fn percent(t: &Table, key: &str, name: &str, default: f64, warn: &mut Vec<String>) -> f64 {
    let n = match t.get(key) {
        None => return default,
        Some(Value::Integer(n)) => *n as f64,
        Some(Value::Float(n)) => *n,
        Some(v) => {
            warn.push(format!("{name}: expected a number, got a {}, using default {default}", v.type_str()));
            return default;
        }
    };
    if (0.0..=100.0).contains(&n) {
        n
    } else {
        warn.push(format!("{name}: {n} is not between 0 and 100, using default {default}"));
        default
    }
}

/// `#RRGGBB` text as 0xRRGGBB, hex digits in either case; None when missing or bad (AC-11).
fn color(t: &Table, key: &str, name: &str, fallback: &str, warn: &mut Vec<String>) -> Option<u32> {
    let text = match t.get(key)? {
        Value::String(text) => text,
        v => {
            warn.push(format!("{name}: expected a \"#RRGGBB\" color, got a {}, using {fallback}", v.type_str()));
            return None;
        }
    };
    let rgb = text.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|h| u32::from_str_radix(h, 16).ok());
    if rgb.is_none() {
        warn.push(format!("{name}: {} is not a \"#RRGGBB\" color, using {fallback}", quoted(text)));
    }
    rgb
}

/// Runs one of the readers above on a screen table's key; None when the key is missing or bad,
/// so the default fills it in (spec 0020 AC-11).
fn opt<T>(t: &Table, key: &str, warn: &mut Vec<String>, read: impl FnOnce(&mut Vec<String>) -> T) -> Option<T> {
    if !t.contains_key(key) {
        return None;
    }
    let n = warn.len();
    let v = read(warn);
    (warn.len() == n).then_some(v)
}

/// The `[indicator.screens."<id>"]` tables, over the default place `d`.
fn screens(t: &Table, d: Pos, warn: &mut Vec<String>) -> BTreeMap<String, PosOverride> {
    let Some(v) = t.get("screens") else { return BTreeMap::new() };
    let Some(list) = v.as_table().filter(|l| l.values().all(Value::is_table)) else {
        let got = if v.is_table() { "table with plain values in it" } else { v.type_str() };
        warn.push(format!("indicator.screens: expected [indicator.screens.\"<screen id>\"] tables, got a {got}, ignored"));
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (id, st) in list.iter().filter_map(|(id, v)| Some((id, v.as_table()?))) {
        let name = |k: &str| format!("indicator.screens.\"{id}\".{k}");
        unknown_keys(st, &name(""), SCREEN_KEYS, warn);
        let o = PosOverride {
            spot: opt(st, "spot", warn, |w| choice(st, "spot", &name("spot"), SPOTS, d.spot, w)),
            gap: opt(st, "gap", warn, |w| int(st, "gap", &name("gap"), 0, 400, d.gap as i64, w) as u32),
            x: opt(st, "x", warn, |w| percent(st, "x", &name("x"), d.x, w)),
            y: opt(st, "y", warn, |w| percent(st, "y", &name("y"), d.y, w)),
        };
        out.insert(id.clone(), o);
    }
    out
}

/// A name or rename as spec 0023 AC-2 cleans it. None when missing or bad; the warn line
/// names the key, never the text, since a name can say anything about you (AC-15).
fn name(t: &Table, key: &str, full: &str, warn: &mut Vec<String>) -> Option<String> {
    let text = match t.get(key)? {
        Value::String(s) => clean_name(s),
        v => {
            warn.push(format!("{full}: expected text, got a {}, ignored", v.type_str()));
            return None;
        }
    };
    let n = text.encode_utf16().count();
    if n > NAME_MAX {
        warn.push(format!("{full}: {n} characters, the most is {NAME_MAX}, ignored"));
        return None;
    }
    Some(text)
}

/// Control characters (a newline, a tab) become spaces and both ends are trimmed, since a
/// name is drawn on one line (spec 0023 AC-2).
pub(crate) fn clean_name(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// A wallpaper value (spec 0028 AC-1, AC-14): a string is a picture by its extension, else a
/// subfolder; a list is a set of pictures. None when missing or bad; `""` is `Pick::None`. A
/// warn line names the key, never the value, since a file name can say anything.
fn pick(t: &Table, key: &str, full: &str, warn: &mut Vec<String>) -> Option<Pick> {
    match t.get(key)? {
        Value::String(name) if name.is_empty() => Some(Pick::None),
        Value::String(name) => match name_why(name) {
            Some(why) => {
                warn.push(format!("{full}: {why}, ignored"));
                None
            }
            None if is_picture_name(name) => Some(Pick::Picture(name.clone())),
            None => Some(Pick::Folder(name.clone())),
        },
        Value::Array(list) => {
            let mut names: Vec<String> = list.iter().filter_map(|v| v.as_str().filter(|n| is_picture_name(n)).map(str::to_string)).collect();
            let skipped = list.len() - names.len();
            if skipped > 0 {
                let entries = if skipped == 1 { "entry" } else { "entries" };
                warn.push(format!("{full}: {skipped} {entries} skipped, not a picture name"));
            }
            names.sort_by(|a, b| name_order(a, b));
            names.dedup();
            Some(match names.len() {
                0 => Pick::None,
                1 => Pick::Picture(names.remove(0)),
                _ => Pick::Set(names),
            })
        }
        v => {
            warn.push(format!("{full}: expected a picture, a folder or a list of pictures, got a {}, ignored", v.type_str()));
            None
        }
    }
}

/// Why `name` can't name anything in the wallpapers folder (spec 0026 AC-2), or None when it
/// can: the full path is always that folder joined with it.
fn name_why(name: &str) -> Option<String> {
    Some(if name.is_empty() || name == "." || name.contains(['\\', '/', ':']) || name.contains("..") {
        "not a plain file name in the wallpapers folder".to_string()
    } else if name.encode_utf16().count() > PICTURE_MAX {
        format!("{} characters, the most is {PICTURE_MAX}", name.encode_utf16().count())
    } else {
        return None;
    })
}

/// Whether `name` ends in a picture's extension (spec 0028 AC-1), any case.
fn picture_ext(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| PICTURE_TYPES.iter().any(|t| ext.eq_ignore_ascii_case(t)))
}

/// Slideshow order (spec 0028 AC-3): by name with case ignored, then by exact name, so
/// `A.jpg` and `a.jpg` keep a fixed order.
pub(crate) fn name_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// A subfolder value the settings page may list and write (spec 0028 AC-11): a name the
/// file's reader takes as a folder, so `trip.png` is not one.
pub(crate) fn is_folder_name(name: &str) -> bool {
    name_why(name).is_none() && !picture_ext(name)
}

/// A file in the wallpapers folder the settings page may list and write (spec 0026 AC-14):
/// one whose name the file's reader keeps.
pub(crate) fn is_picture_name(name: &str) -> bool {
    name_why(name).is_none() && picture_ext(name)
}

/// The `[names.screens."<id>"]` tables. A table for a screen not plugged in is kept (AC-3).
fn name_screens(t: &Table, warn: &mut Vec<String>) -> BTreeMap<String, ScreenNames> {
    let Some(v) = t.get("screens") else { return BTreeMap::new() };
    let Some(list) = v.as_table().filter(|l| l.values().all(Value::is_table)) else {
        let got = if v.is_table() { "table with plain values in it" } else { v.type_str() };
        warn.push(format!("names.screens: expected [names.screens.\"<screen id>\"] tables, got a {got}, ignored"));
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (id, st) in list.iter().filter_map(|(id, v)| Some((id, v.as_table()?))) {
        let full = |k: &str| format!("names.screens.\"{id}\".{k}");
        unknown_keys(st, &full(""), SCREEN_NAMES_KEYS, warn);
        let screen = name(st, "screen", &full("screen"), warn).filter(|n| !n.is_empty());
        let ws = std::array::from_fn(|i| name(st, NAMES_KEYS[i], &full(NAMES_KEYS[i]), warn));
        out.insert(id.clone(), ScreenNames { screen, ws });
    }
    out
}

/// The `[wallpaper.screens."<id>"]` tables (spec 0026 AC-1). A table for a screen not plugged
/// in is kept (AC-10).
fn wallpaper_screens(t: &Table, warn: &mut Vec<String>) -> BTreeMap<String, [Option<Pick>; 9]> {
    let Some(v) = t.get("screens") else { return BTreeMap::new() };
    let Some(list) = v.as_table().filter(|l| l.values().all(Value::is_table)) else {
        let got = if v.is_table() { "table with plain values in it" } else { v.type_str() };
        warn.push(format!("wallpaper.screens: expected [wallpaper.screens.\"<screen id>\"] tables, got a {got}, ignored"));
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (id, st) in list.iter().filter_map(|(id, v)| Some((id, v.as_table()?))) {
        let full = |k: &str| format!("wallpaper.screens.\"{id}\".{k}");
        unknown_keys(st, &full(""), &WALLPAPER_KEYS[..9], warn);
        out.insert(id.clone(), std::array::from_fn(|i| pick(st, WALLPAPER_KEYS[i], &full(WALLPAPER_KEYS[i]), warn)));
    }
    out
}

/// The `[apps]` rules (spec 0027 AC-10): a bad one is dropped alone, the first 64 kept, and of
/// two keys that differ only in case the first wins. A key is only ever compared, never opened
/// or run, and one with a path in it is never echoed: a log is safe to share.
fn apps(t: &Table, order: &[String], warn: &mut Vec<String>) -> Vec<(String, Rule)> {
    let mut out: Vec<(String, Rule)> = Vec::new();
    let mut dropped = 0;
    let rest = t.keys().filter(|k| !order.contains(k));
    for (key, v) in order.iter().chain(rest).filter_map(|k| t.get_key_value(k)) {
        let shown = if key.contains(['\\', '/', ':']) { "a key with a path in it".to_string() } else { format!("\"{key}\"") };
        let full = format!("apps.{shown}");
        if !app_key(key) {
            warn.push(format!("apps: {shown} is not an exe file name (like \"slack.exe\") or a Store app id, ignored"));
            continue;
        }
        let lower = key.to_lowercase();
        if out.iter().any(|(k, _)| *k == lower) {
            warn.push(format!("apps: {shown} differs from an earlier key only in case, ignored"));
            continue;
        }
        let rule = match v {
            Value::Table(r) => rule(r, &full),
            v => Err(format!("{full}: expected {{ workspace = N }}, got a {}, ignored", v.type_str())),
        };
        match rule {
            Err(why) => warn.push(why),
            Ok(_) if out.len() == APPS_MAX => dropped += 1,
            Ok(r) => out.push((lower, r)),
        }
    }
    if dropped > 0 {
        warn.push(format!("apps: more than {APPS_MAX} rules, the last {dropped} ignored"));
    }
    out
}

/// Table `name`'s keys as the file has them: toml's Table sorts its keys, and the first of two
/// rules that differ only in case wins (spec 0027 AC-10).
pub(super) fn file_order(text: &str, name: &str) -> Vec<String> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok();
    let t = doc.as_ref().and_then(|d| d.get(name)?.as_table_like());
    t.map_or_else(Vec::new, |t| t.iter().map(|(k, _)| k.to_string()).collect())
}

/// An exe file name (`slack.exe`) or a Store app id (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`).
fn app_key(key: &str) -> bool {
    let plain = !key.is_empty() && key.chars().count() <= APP_KEY_MAX && !key.contains(['\\', '/', ':']) && !key.chars().any(char::is_control);
    plain && (key.contains('!') || (key.len() > 4 && key.to_lowercase().ends_with(".exe")))
}

/// One rule's table; any bad part drops the whole rule, since half a rule would put windows
/// somewhere you did not ask for.
fn rule(t: &Table, full: &str) -> Result<Rule, String> {
    if let Some(k) = t.keys().find(|k| !RULE_KEYS.contains(&k.as_str())) {
        return Err(format!("{full}: unknown key {}, rule ignored", key_shown(k)));
    }
    let workspace = match t.get("workspace") {
        Some(Value::Integer(n)) if (1..=9).contains(n) => *n as usize - 1,
        Some(Value::Integer(n)) => return Err(format!("{full}.workspace: {n} is not 1 to 9, rule ignored")),
        Some(v) => return Err(format!("{full}.workspace: expected a whole number, got a {}, rule ignored", v.type_str())),
        None => return Err(format!("{full}: no workspace, rule ignored")),
    };
    let screen = match t.get("screen") {
        None => None,
        Some(Value::String(id)) if !id.trim().is_empty() => Some(id.clone()),
        Some(Value::String(_)) => return Err(format!("{full}.screen: empty, rule ignored")),
        Some(v) => return Err(format!("{full}.screen: expected a screen id, got a {}, rule ignored", v.type_str())),
    };
    Ok(Rule { workspace, screen })
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
            warn.push(format!("{name}: entry {} is not text, dropped", shown(item)));
            continue;
        };
        match parse_combo(text) {
            Ok(c) if !out.contains(&c) => out.push(c),
            Ok(_) => {} // listed twice: harmless
            Err(why) => warn.push(format!("{name}: {} {why}, dropped", quoted(text))),
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
            warn.push(format!("shortcuts.{key}: {} {why}, using {}", quoted(text), mods_text(default)));
            default
        }
    }
}

/// Modifiers only, joined by `+`, any case. Shift alone would eat typed digits and symbols.
fn parse_mods(text: &str) -> Result<Option<u8>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    if has_path(text) {
        return Err("is not Win, Ctrl, Alt or Shift".into());
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
    if has_path(text) {
        return Err("is not a key combo".into());
    }
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

pub(super) const NAMED_KEYS: &[(&str, VIRTUAL_KEY)] = &[
    ("Left", VK_LEFT), ("Right", VK_RIGHT), ("Up", VK_UP), ("Down", VK_DOWN),
    ("Home", VK_HOME), ("End", VK_END), ("PageUp", VK_PRIOR), ("PageDown", VK_NEXT),
    ("Insert", VK_INSERT), ("Delete", VK_DELETE), ("Space", VK_SPACE), ("Enter", VK_RETURN),
    ("Tab", VK_TAB), ("Escape", VK_ESCAPE), ("Backspace", VK_BACK),
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

#[cfg(test)]
mod tests;
