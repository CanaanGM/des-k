// The settings in force: the loaded copy readers take, reading the file at start, the 1 s
// watch and the reload after a save, and the change log lines (spec 0012, spec 0032).
use super::*;
use super::parse::{NAMES_KEYS, Problem, WALLPAPER_KEYS, not_toml, parse, reparse};
use arc_swap::ArcSwap;
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::sync::LazyLock;
use std::time::SystemTime;

pub(super) struct Loaded {
    pub(super) settings: Settings,
    pub(super) problems: Vec<Problem>,
    pub(super) version: u64, // 0 for the defaults, one more on each store
}

// Swapped whole on each reload, without a lock, so the key hook never waits (spec 0032). An old
// one is freed once the last handle to it lets go. The defaults until load().
pub(super) static CURRENT: LazyLock<ArcSwap<Loaded>> =
    LazyLock::new(|| ArcSwap::from_pointee(Loaded { settings: Settings::default(), problems: Vec::new(), version: 0 }));

/// The problems of the version in force when it was taken, held like `Current`.
pub(crate) struct Problems(Guard<Arc<Loaded>>);

impl Deref for Problems {
    type Target = [Problem];
    fn deref(&self) -> &[Problem] {
        &self.0.problems
    }
}

thread_local! {
    // The file's write time when des-k last read it. Main thread only (spec 0012, file watch).
    static SEEN: Cell<Option<SystemTime>> = const { Cell::new(None) };
    // Why the file could not be read or is not TOML, until a read succeeds (AC-14). The page
    // reads it to go read only, at start and after a live reload alike.
    static PARSE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The last read or parse error, while the file is still broken. Main thread.
pub(crate) fn parse_error() -> Option<String> {
    PARSE_ERROR.with_borrow(Clone::clone)
}

/// What is wrong in the file right now, for the settings page.
pub(crate) fn problems() -> Problems {
    Problems(CURRENT.load())
}

fn store(settings: Settings, problems: Vec<Problem>) {
    put(&CURRENT, settings, problems);
}

// The swap itself, apart from CURRENT so a test can count what it frees.
fn put(cell: &ArcSwap<Loaded>, settings: Settings, problems: Vec<Problem>) {
    let version = cell.load().version + 1; // only main stores, so nothing comes between
    cell.store(Arc::new(Loaded { settings, problems, version }));
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// settings.toml in the data folder, shared by the installed and the dev copy.
pub(crate) fn path() -> std::path::PathBuf {
    crate::ws::data_dir().join("settings.toml")
}

/// Read the file (writing the default one if missing) and log every problem. Main thread, at start.
pub(crate) fn load() {
    let path = path();
    SEEN.set(modified(&path));
    let (settings, problems) = match std::fs::read_to_string(&path) {
        Ok(text) => {
            PARSE_ERROR.set(text.parse::<Table>().err().map(|e| not_toml(&e.to_string(), e.message())));
            let (settings, warnings) = parse(&text);
            for w in &warnings {
                warn!("settings: {w}");
            }
            (settings, warnings.into_iter().map(Problem::new).collect())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // create_new: never overwrite a file that appeared meanwhile.
            let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut f| f.write_all(DEFAULT_FILE.as_bytes()));
            match written {
                Ok(()) => info!("settings: wrote defaults to {}", path.file_name().unwrap_or_default().display()),
                Err(e) => error!("settings: cannot write {} ({e}), using all defaults", path.file_name().unwrap_or_default().display()),
            }
            SEEN.set(modified(&path));
            (Settings::default(), Vec::new())
        }
        Err(e) => {
            PARSE_ERROR.set(Some(format!("cannot read {} ({e})", path.display())));
            warn!("settings: cannot read {} ({e}), using all defaults", path.file_name().unwrap_or_default().display());
            (Settings::default(), Vec::new())
        }
    };
    info!("settings: {}: {settings}", path.file_name().unwrap_or_default().display());
    store(settings, problems);
}

/// Who changed the file, for the log (spec 0012 AC-15).
#[derive(Clone, Copy)]
pub(super) enum Source {
    Page,
    File,
}

/// Main loop, every second: reload when the file's write time changed (AC-5). A missing
/// file is skipped, and read again once it is back.
pub(crate) fn watch() {
    if let Some(t) = modified(&path())
        && SEEN.get() != Some(t)
    {
        let _ = reload(Source::File);
    }
}

/// Read the file again and put it in effect for every reader at once. A file that is not
/// TOML keeps the settings in effect: a typo mid edit must not remap every shortcut (AC-14).
/// Returns the settings that changed. Main thread.
pub(super) fn reload(source: Source) -> Result<Vec<Change>, String> {
    let path = path();
    SEEN.set(modified(&path)); // before reading, so a save during the read is seen next tick
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {} ({e})", path.display()));
    let (settings, warnings) = match text.and_then(|t| reparse(&t)) {
        Ok(got) => got,
        Err(why) => {
            warn!("settings: {why}, keeping the settings in effect");
            PARSE_ERROR.set(Some(why.clone()));
            crate::settings_page::refresh();
            return Err(why);
        }
    };
    PARSE_ERROR.set(None);
    // An Arc, not a handle: it outlives the store below, through apply.
    let old_loaded = CURRENT.load_full();
    let old = &old_loaded.settings;
    // Only new problems are logged, so one bad value is not logged again on every save.
    let before = &old_loaded.problems;
    for w in warnings.iter().filter(|w| !before.iter().any(|p| &p.message == *w)) {
        warn!("settings: {w}");
    }
    let problems: Vec<Problem> = warnings.into_iter().map(Problem::new).collect();
    let changes = diff(old, &settings, source);
    if !changes.is_empty() || problems != *before {
        store(settings, problems);
        #[cfg(feature = "drill")]
        crate::drill::reloaded();
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
pub(crate) struct Change {
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
    // Names log their keys only (spec 0023 AC-15), so a new name for a key already set changes
    // no text here; compare the names themselves, or the new one is never stored.
    let renamed = old.names != new.names || old.names_screens != new.names_screens;
    let repictured = old.wallpaper != new.wallpaper || old.wallpaper_screens != new.wallpaper_screens; // the same, for picture file names (spec 0026 AC-16)
    for ((key, was), (_, now)) in old.fields().into_iter().zip(new.fields()) {
        if was != now || (key == "names" && renamed) || (key == "wallpaper" && repictured) {
            info!("settings: {key} {was} -> {now} ({from})");
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
            info!("settings: workspaces {} -> {}: {line}", old.workspaces, new.workspaces);
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
    if has("apps") {
        crate::apps::sync();
    }
    if has("attention.enabled") && !new.attention_enabled {
        crate::ws::forget_wants();
    }
    // Colors resolve in ui::theme_of on each paint; every window just needs to paint again.
    if changes.iter().any(|c| c.key.starts_with("colors.")) {
        crate::ui::repaint_all();
    }
    crate::welcome::refresh();
    crate::hints::refresh();
    crate::whats_new::refresh();
    moves
}

/// For the settings page's render test: what get() and problems() give from now on.
#[cfg(test)]
pub(crate) fn use_for_test(text: &str) {
    let (settings, warnings) = parse(text);
    store(settings, warnings.into_iter().map(Problem::new).collect());
    PARSE_ERROR.set(None); // as a good read does
}

/// For the settings page's renders: the file can't be read, so the page is read only.
#[cfg(test)]
pub(crate) fn broken_for_test(why: &str) {
    PARSE_ERROR.set(Some(why.to_string()));
}

impl Settings {
    /// Every setting as (name, value) in file order: the start log line, and the change log
    /// that compares two of these (spec 0012 AC-15).
    fn fields(&self) -> Vec<(&'static str, String)> {
        let list = |l: &[Combo]| format!("[{}]", l.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "));
        let word = |w: Option<&str>| w.unwrap_or("").to_string();
        let gestures = self.gestures.iter().map(|g| format!("{g}: {}", g.action)).collect::<Vec<_>>().join(", ");
        let mut fields = vec![
            ("workspaces", self.workspaces.to_string()),
            ("copilot_as_right_ctrl", self.copilot_as_right_ctrl.to_string()),
            ("follow_activation", self.follow_activation.to_string()),
            ("act_on", word(ACT_ON.iter().find(|(_, v)| *v == self.act_on).map(|(w, _)| *w))),
            ("jump_back", self.jump_back.to_string()),
            ("fastest", self.fastest.to_string()),
            ("first_run_seen", self.first_run_seen.map_or(String::new(), |b| b.to_string())),
            ("whats_new_seen", self.whats_new_seen.map_or(String::new(), |v| v.to_string())),
            ("shortcuts.previous", list(&self.previous)),
            ("shortcuts.next", list(&self.next)),
            ("shortcuts.send_previous", list(&self.send_previous)),
            ("shortcuts.send_next", list(&self.send_next)),
            ("shortcuts.send_previous_stay", list(&self.send_previous_stay)),
            ("shortcuts.send_next_stay", list(&self.send_next_stay)),
            ("shortcuts.jump", mods_text(self.jump).to_string()),
            ("shortcuts.send_to", mods_text(self.send_to).to_string()),
            ("shortcuts.send_to_stay", mods_text(self.send_to_stay).to_string()),
            ("shortcuts.back", list(&self.back)),
            ("shortcuts.finder", list(&self.finder)),
            ("shortcuts.overview", list(&self.overview)),
            ("shortcuts.pin", list(&self.pin)),
            ("shortcuts.hints", list(&self.hints)),
            ("indicator.enabled", self.indicator_enabled.to_string()),
            ("indicator.style", word(STYLES.iter().find(|(_, v)| *v == self.indicator_style).map(|(w, _)| *w))),
            ("indicator.screen_name", self.indicator_screen_name.to_string()),
            ("indicator.hold_ms", self.hold_ms.to_string()),
            ("indicator.fade_ms", self.fade_ms.to_string()),
            ("indicator.spot", word(SPOTS.iter().find(|(_, v)| *v == self.indicator_pos.spot).map(|(w, _)| *w))),
            ("indicator.gap", self.indicator_pos.gap.to_string()),
            ("indicator.x", self.indicator_pos.x.to_string()),
            ("indicator.y", self.indicator_pos.y.to_string()),
            ("indicator.screens", screens_text(&self.indicator_screens)),
            ("attention.enabled", self.attention_enabled.to_string()),
            // Quoted, so a line break in a hand edit keeps the log line one line.
            ("attention.text", format!("{:?}", self.attention_text)),
            ("colors.accent", match self.accent {
                Accent::Windows => "windows".to_string(),
                Accent::Rgb(c) => hex(c),
            }),
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
        ];
        // [colors] ws1 to ws9, named like the file's keys.
        const WS_NAMES: [&str; 9] = ["colors.ws1", "colors.ws2", "colors.ws3", "colors.ws4", "colors.ws5", "colors.ws6", "colors.ws7", "colors.ws8", "colors.ws9"];
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        let ws = WS_NAMES.iter().zip(self.ws_colors).map(|(k, c)| (*k, c.map_or(String::new(), hex)));
        fields.splice(at..at, ws);
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        fields.insert(at, ("names", names_text(&self.names, &self.names_screens)));
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        fields.insert(at, ("wallpaper", wallpaper_text(&self.wallpaper, &self.wallpaper_screens)));
        // These say nothing about you, so they log with their values (spec 0026 AC-16).
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        let look = [
            ("wallpaper.fit", word(FITS.iter().find(|(_, v)| *v == self.wallpaper_fit).map(|(w, _)| *w))),
            ("wallpaper.transition", word(TRANSITIONS.iter().find(|(_, v)| *v == self.wallpaper_fade).map(|(w, _)| *w))),
            ("wallpaper.dim", self.wallpaper_dim.to_string()),
            ("wallpaper.interval", self.wallpaper_interval.to_string()),
            ("wallpaper.shuffle", self.wallpaper_shuffle.to_string()),
        ];
        fields.splice(at..at, look);
        // A workspace's own interval, or the word default while it follows `interval` (spec 0028 AC-12).
        const INTERVAL_NAMES: [&str; 9] = [
            "wallpaper.intervals.ws1", "wallpaper.intervals.ws2", "wallpaper.intervals.ws3", "wallpaper.intervals.ws4", "wallpaper.intervals.ws5",
            "wallpaper.intervals.ws6", "wallpaper.intervals.ws7", "wallpaper.intervals.ws8", "wallpaper.intervals.ws9",
        ];
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        let own = INTERVAL_NAMES.iter().zip(self.wallpaper_intervals).map(|(k, n)| (*k, n.map_or("default".to_string(), |n| n.to_string())));
        fields.splice(at..at, own);
        let at = fields.iter().position(|(k, _)| *k == "restore.enabled").unwrap_or(fields.len());
        fields.insert(at, ("apps", apps_text(&self.apps)));
        fields
    }
}

/// Every screen's own place on one line, for the change log: the screen tables vary in number,
/// so they are one field.
fn screens_text(screens: &BTreeMap<String, PosOverride>) -> String {
    let one = |o: &PosOverride| {
        let spot = o.spot.and_then(|s| SPOTS.iter().find(|(_, v)| *v == s)).map(|(w, _)| format!("spot={w}"));
        let rest = [o.gap.map(|g| format!("gap={g}")), o.x.map(|x| format!("x={x}")), o.y.map(|y| format!("y={y}"))];
        std::iter::once(spot).chain(rest).flatten().collect::<Vec<_>>().join(" ")
    };
    let all: Vec<String> = screens.iter().map(|(id, o)| format!("{id}: {}", one(o))).collect();
    format!("[{}]", all.join(", "))
}

/// Which `[names]` keys are set, never their text (spec 0023 AC-15), like
/// `[ws1, ws2; screens: internal (screen, ws2)]`.
fn names_text(names: &[String; 9], screens: &BTreeMap<String, ScreenNames>) -> String {
    let all: Vec<&str> = (0..9).filter(|&i| !names[i].is_empty()).map(|i| NAMES_KEYS[i]).collect();
    let one = |n: &ScreenNames| {
        let keys: Vec<&str> = n.screen.iter().map(|_| "screen").chain((0..9).filter(|&i| n.ws[i].is_some()).map(|i| NAMES_KEYS[i])).collect();
        keys.join(", ")
    };
    let per: Vec<String> = screens.iter().map(|(id, n)| format!("{id} ({})", one(n))).collect();
    let mut parts = vec![];
    if !all.is_empty() {
        parts.push(all.join(", "));
    }
    if !per.is_empty() {
        parts.push(format!("screens: {}", per.join(", ")));
    }
    format!("[{}]", parts.join("; "))
}

/// Which `[wallpaper]` keys are set, never their file names (spec 0026 AC-16), like
/// `[ws1, ws2; screens: internal (ws2)]`.
fn wallpaper_text(pictures: &[Pick; 9], screens: &BTreeMap<String, [Option<Pick>; 9]>) -> String {
    let all: Vec<&str> = (0..9).filter(|&i| pictures[i] != Pick::None).map(|i| WALLPAPER_KEYS[i]).collect();
    let one = |ws: &[Option<Pick>; 9]| (0..9).filter(|&i| ws[i].is_some()).map(|i| WALLPAPER_KEYS[i]).collect::<Vec<_>>().join(", ");
    let per: Vec<String> = screens.iter().map(|(id, ws)| format!("{id} ({})", one(ws))).collect();
    let mut parts = vec![];
    if !all.is_empty() {
        parts.push(all.join(", "));
    }
    if !per.is_empty() {
        parts.push(format!("screens: {}", per.join(", ")));
    }
    format!("[{}]", parts.join("; "))
}

/// Every rule by key, workspace and screen id, never a path (spec 0027 AC-15), like
/// `[slack.exe: 1 on GSM-5B7F-12345, spotify.exe: 4]`.
fn apps_text(apps: &[(String, Rule)]) -> String {
    let one = |(k, r): &(String, Rule)| match &r.screen {
        Some(id) => format!("{k}: {} on {id}", r.workspace + 1),
        None => format!("{k}: {}", r.workspace + 1),
    };
    format!("[{}]", apps.iter().map(one).collect::<Vec<_>>().join(", "))
}

impl fmt::Display for Settings {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let fields: Vec<String> = self.fields().into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        write!(f, "{}", fields.join(" "))
    }
}

#[cfg(test)]
mod tests;
