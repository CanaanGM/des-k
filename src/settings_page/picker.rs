// The color picker's state, keys and writes (spec 0021).
pub(super) mod draw;

use super::*;
use crate::settings_page::cards::colors::{color_edit, color_title, path_of, ws_index};
use crate::settings_page::input::keys::{hit_at, point};
use draw::{Place, part_at, wheel_at};

/// The color picker while it is open (spec 0021): one key, the color shown, and what it saved.
pub(super) struct ColorPick {
    pub(super) key: &'static str,      // ACCENT or one of WS_KEYS
    pub(super) from: Option<Ctl>,      // the swatch that opened it, for focus on close; None for a tile
    pub(super) anchor: Anchor,         // what was clicked
    pub(super) before: Option<toml_edit::Value>, // the key's raw value in the file when it opened; None: no key
    pub(super) start: u32,             // the color shown when it opened, for the header's start chip and hex
    pub(super) file: u32,              // the color the file is known to hold: the start, then each write that worked
    pub(super) color: u32,             // the color shown: the source of truth
    pub(super) hsv: ui::color::Hsv,           // where the ring and square sit
    pub(super) preview: Option<Theme>, // the page's theme with `color` in place, while the page shows otherwise
    pub(super) written: Option<u32>,   // the value of the write in flight only: set before the call, cleared after
    pub(super) failed: Option<String>, // Windows' words from the last failed write, while Not saved shows
    pub(super) saved: bool,            // a write from this picker worked
    pub(super) held_edit: Option<(String, Vec<String>)>, // a hand edit's note that arrived while open
    pub(super) placed: Option<Place>,  // the card's window position and the keep set it was placed with
    pub(super) scroll0: f32,           // the page's scroll when it opened, for the way back (AC-14)
    pub(super) guard: Option<Guard>,   // the opening click: presses close to it are ignored for a moment (AC-15)
    pub(super) focus: Stop,
    pub(super) last_swatch: usize,     // the swatch Tab comes back to
    pub(super) drag: Option<Part>,     // Ring or Square while the mouse is down
    pub(super) held: Option<VIRTUAL_KEY>, // the arrow held down: its key up saves
}

/// What the picker was opened from, found again in `Geo` after every layout.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Anchor {
    Tile(String, usize), // a Workspaces tile: its screen's device and its workspace
    Show(String, usize), // an Overview picture tile
    Swatch(&'static str),
}

/// The picker's keyboard stops, in Tab order (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Stop {
    Swatches(usize),
    Ring,
    Square,
    Done,
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Part {
    Ring,
    Square,
}

/// The opening click: where its button went down, in screen pixels, and when presses near it
/// count again.
#[derive(Clone, Copy, Debug)]
pub(super) struct Guard {
    at: (i32, i32),
    until: Instant,
}

/// The page scrolling to `to` on its own (AC-14): to make room for the picker (`opening`: the
/// card shows when it ends) or back to where it was.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ease {
    from: f32,
    to: f32,
    start: Instant,
    ms: u32,
    opening: bool,
}

/// Where the picker's anchors and what it keeps clear were drawn, in window dip.
#[derive(Clone, Default, Debug)]
pub(super) struct Geo {
    pub(super) rows: Vec<(String, Bx)>,             // each plugged in screen's tile row, by device
    pub(super) tiles: Vec<(String, usize, Bx)>,     // each of their tiles
    pub(super) band: Option<Bx>,                    // the Overview picture
    pub(super) groups: Vec<(String, Bx)>,           // a screen's four tiles in it
    pub(super) shows: Vec<(String, usize, Bx)>,     // its live tiles
    pub(super) swatches: Vec<(&'static str, Bx)>,
    pub(super) legend: Option<Bx>,                  // the Colors sample's legend, kept clear for Your accent
}

// ---- The color picker (spec 0021, drawn to the approved design pass, handoff-4) ----

pub(super) const PICK_W: f32 = 388.0; // the card, in dip
pub(super) const PICK_H: f32 = 326.0;
const NOT_SAVED: &str = "Not saved: couldn't update the file.";
pub(super) const EASE_TIMER: usize = 1;
const EASE_MS: u32 = 150;
const MARGIN: f32 = 8.0; // the card's gap from what it sits by, and from the window's and notes' edges
const OUTSET: f32 = 4.0; // a tile's in view ring and hover edge reach this far outside it

thread_local! {
    // The open picker's ring at one scale, and its square at one scale and hue: built once,
    // drawn on every paint, dropped when it closes.
    pub(super) static RING: RefCell<Option<(u32, ui::canvas::Picture)>> = const { RefCell::new(None) };
    pub(super) static SQUARE: RefCell<Option<(u32, u32, ui::canvas::Picture)>> = const { RefCell::new(None) };
}

/// AC-8's line: the lowest other workspace in use whose color is `color` (true) or very close to
/// it (false), each counted with the color it shows.
fn dup_of(color: u32, me: usize, ws: &[u32; 9], n: usize) -> Option<(usize, bool)> {
    (0..n.min(9)).filter(|&j| j != me).find_map(|j| match ws[j] {
        c if c == color => Some((j, true)),
        c if ui::color::distance(c, color) < 10.0 => Some((j, false)),
        _ => None,
    })
}

/// Scrolls the page to `to`, eased out over 150 ms, or at once with Skip animations (AC-14).
/// `opening`: the picker's card shows when it ends. The caller lays the page out.
fn ease_to(to: f32, opening: bool) {
    let ms = ui::motion_ms(EASE_MS);
    let h = set(|w| {
        let to = to.clamp(0.0, (w.height - (w.vh - w.fixed)).max(0.0));
        if ms == 0 || (to - w.scroll).abs() < 0.5 {
            w.scroll = to;
            return None;
        }
        w.ease = Some(Ease { from: w.scroll, to, start: Instant::now(), ms, opening });
        Some(w.hwnd)
    })
    .flatten();
    if let Some(h) = h {
        unsafe { SetTimer(Some(h), EASE_TIMER, 16, None) };
    }
}

/// One frame of the ease: its progress clamped to 0..1 and the scroll to the page's limits.
pub(super) fn ease_step() {
    let done = set(|w| {
        let Some(e) = w.ease else { return true };
        let k = (e.start.elapsed().as_secs_f32() * 1000.0 / e.ms as f32).clamp(0.0, 1.0);
        let max = (w.height - (w.vh - w.fixed)).max(0.0);
        w.scroll = (e.from + (e.to - e.from) * (1.0 - (1.0 - k).powi(3))).clamp(0.0, max);
        k >= 1.0
    });
    match done {
        Some(true) => {
            if let Some(h) = hwnd() {
                unsafe { let _ = KillTimer(Some(h), EASE_TIMER); }
            }
            ease_finish();
        }
        Some(false) => {
            lay();
            repaint();
        }
        None => {}
    }
}

/// Ends the ease at once, at its end: any press, key or wheel turn during it, a resize, a close.
pub(super) fn ease_finish() {
    let Some(h) = set(|w| w.ease.take().map(|e| {
        w.scroll = e.to;
        w.hwnd
    }))
    .flatten() else {
        return;
    };
    unsafe { let _ = KillTimer(Some(h), EASE_TIMER); }
    layout();
}

/// The color the file gives `key` now; None: the accent follows Windows.
pub(super) fn file_color(s: &Settings, key: &str) -> Option<u32> {
    match ws_index(key) {
        Some(i) => Some(s.ws_colors[i].unwrap_or(ui::WS[i])),
        None => match s.accent {
            settings::Accent::Rgb(c) => Some(c),
            settings::Accent::Windows => None,
        },
    }
}

/// A reload brought a value for the picker's key that it did not write: neither what the file
/// is known to hold nor the write in flight. Windows' accent always counts (AC-12).
fn outside(now: Option<u32>, file: u32, written: Option<u32>) -> bool {
    now.is_none_or(|c| c != file && Some(c) != written)
}

/// The open picker must close, writing nothing (AC-10, AC-12): high contrast or read only came
/// on, its workspace went, or its key changed outside it.
pub(super) fn must_close(w: &Win) -> bool {
    let Some(p) = &w.pick else { return false };
    let s = settings::get();
    w.theme.hc || settings::store::parse_error().is_some() || ws_index(p.key).is_some_and(|i| i >= s.workspaces) || outside(file_color(&s, p.key), p.file, p.written)
}

/// The page's theme with `color` for `key`, while the page doesn't show that color already.
pub(super) fn preview_for(t: Theme, key: &str, color: u32) -> Option<Theme> {
    let shown = ws_index(key).map_or(t.b.acc, |i| t.b.ws[i]);
    (!t.hc && shown != color).then(|| {
        let l = ui::look();
        ui::theme_preview(l.apps_light, l, ws_index(key), color)
    })
}

/// Shows `color` on the card and the page, the ring and square at `hsv`. A pick, a drag or an
/// arrow ends Not saved, even one that writes nothing (AC-13).
fn show_color(w: &mut Win, color: u32, hsv: ui::color::Hsv) {
    let theme = w.theme;
    if let Some(p) = w.pick.as_mut() {
        (p.color, p.hsv, p.failed) = (color, hsv, None);
        p.preview = preview_for(theme, p.key, color);
    }
}

/// Opens the color picker for `key` (AC-1 to AC-3): nothing in high contrast or read only, for
/// a workspace you don't have, or for the accent while it follows Windows. Opened by a click,
/// it ignores presses near that click for a moment (AC-15); from the keyboard it doesn't.
pub(super) fn pick_open(key: &'static str, anchor: Anchor, from: Option<Ctl>) {
    ease_finish();
    let s = settings::get();
    let Some(color) = file_color(&s, key) else { return };
    if ws_index(key).is_some_and(|i| i >= s.workspaces) || settings::store::parse_error().is_some() {
        return;
    }
    // What the file holds now, after any hex box save, for Esc and Cancel to put back.
    let before = settings::edit::raw_at(&path_of(key));
    let wait = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
    let _ = set(|w| {
        if w.theme.hc || w.pick.is_some() {
            return;
        }
        let preset = ui::PRESETS.iter().position(|&c| c == color);
        w.pick = Some(ColorPick {
            key,
            from,
            anchor,
            before,
            start: color,
            file: color,
            color,
            hsv: ui::color::hsv_of(color, (0.0, 0.0, 0.0)),
            preview: None,
            written: None,
            failed: None,
            saved: false,
            held_edit: None,
            placed: None,
            scroll0: w.scroll,
            guard: (!w.kb).then(|| Guard { at: w.down_at, until: Instant::now() + wait }),
            focus: preset.map_or(Stop::Ring, Stop::Swatches),
            last_swatch: preset.unwrap_or(0),
            drag: None,
            held: None,
        });
    });
    layout();
}

/// Windows' words from a failed write, "cannot save … (os error 32)": the part in brackets.
pub(super) fn windows_words(why: &str) -> String {
    why.rsplit_once(" (").map_or(why.to_string(), |(_, w)| w.trim_end_matches(')').to_string())
}

/// One picker write, `value` being the color it leaves the key at. `written` is set before the
/// call: the write reloads before it returns, and the reload must not take it for an outside
/// edit. A failure shows Not saved and puts the file's color back on the card and the page
/// (AC-13); the page's own notes wait for the close.
fn pick_write(edit: Edit, value: u32) -> bool {
    if set(|w| w.pick.as_mut().map(|p| p.written = Some(value))).flatten().is_none() {
        return false;
    }
    let result = settings::edit::write(edit);
    let _ = set(|w| {
        let theme = w.theme;
        // The reload inside the write may have closed it.
        let Some(p) = w.pick.as_mut() else { return };
        p.written = None;
        match &result {
            Ok(()) => (p.file, p.saved, p.failed) = (value, true, None),
            Err(why) => {
                (p.failed, p.color) = (Some(windows_words(why)), p.file);
                p.hsv = ui::color::hsv_of(p.file, p.hsv);
            }
        }
        p.preview = preview_for(theme, p.key, p.color);
    });
    result.is_ok()
}

/// Saves the color shown unless the file holds it already (AC-5): one write per release, key
/// up, swatch or close, never one per move.
fn pick_save() {
    if let Some((key, color)) = get(|w| w.pick.as_ref().filter(|p| p.color != p.file).map(|p| (p.key, p.color))).flatten() {
        pick_write(color_edit(key, color), color);
    }
    layout();
}

/// What Esc and Cancel write (AC-6): the key exactly as the file had it when the picker opened
/// (no key then: take it out again), never through `color_edit`, and nothing when the file
/// holds that already.
fn undo_edit(key: &str, before: Option<&toml_edit::Value>, now: Option<&toml_edit::Value>) -> Option<Edit> {
    let bare = |v: &toml_edit::Value| {
        let mut v = v.clone();
        v.decor_mut().clear();
        v.to_string()
    };
    if before.map(bare) == now.map(bare) {
        return None;
    }
    Some(match before {
        Some(v) => Edit::At(path_of(key), v.clone()),
        None => Edit::Remove(path_of(key)),
    })
}

/// How the picker closes (AC-6): keeping (from the card: Done, Enter, Space on Done; or not: a
/// click outside, a resize, the page losing focus), undoing (Esc, Cancel), or writing nothing
/// (high contrast, read only, the count, an outside edit, what you clicked gone).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum How {
    Keep(bool), // true: from the card
    Undo,
    Quiet,
}

/// The page's couldn't save note once the picker closes (AC-13): shown while Not saved still
/// shows and the close is no undo, gone after a save in it worked, else as it was.
fn note_after(failed: Option<&String>, saved: bool, how: How, title: String, was: Option<(String, String)>) -> Option<(String, String)> {
    match failed {
        Some(why) if how != How::Undo => Some((title, why.clone())),
        _ if saved => None,
        _ => was,
    }
}

/// Every close goes through here: the write it needs, then the page's notes catch up, the page
/// eases back (closes from the card only) and focus goes back to the swatch that opened it.
/// Never call it inside `get` or `set`: it writes, and the write reloads.
pub(super) fn pick_close(how: How) {
    ease_finish();
    let Some((h, drag, key, color, file, start, before)) = set(|w| {
        let p = w.pick.as_mut()?;
        p.held = None;
        Some((w.hwnd, p.drag.take(), p.key, p.color, p.file, p.start, p.before.clone()))
    })
    .flatten() else {
        return;
    };
    if drag.is_some() {
        unsafe { let _ = ReleaseCapture(); }
    }
    let wrote = match how {
        How::Keep(_) if color != file => pick_write(color_edit(key, color), color),
        How::Undo => match undo_edit(key, before.as_ref(), settings::edit::raw_at(&path_of(key)).as_ref()) {
            Some(edit) => pick_write(edit, start),
            None => true,
        },
        _ => true,
    };
    if !wrote {
        return layout(); // stays open with Not saved
    }
    let fixed0 = get(|w| w.fixed).unwrap_or(0.0);
    let Some(p) = set(|w| w.pick.take()).flatten() else { return layout() };
    RING.set(None);
    SQUARE.set(None);
    let _ = set(|w| {
        w.error = note_after(p.failed.as_ref(), p.saved, how, color_title(p.key), w.error.take());
        if p.held_edit.is_some() {
            w.edited = p.held_edit.clone();
        }
        if p.from.is_some() {
            w.focus = p.from; // never reveal(): the way back is the only scroll on close
        }
        (w.hover, w.press) = (None, None);
    });
    // The notes changed height: the page scrolls by as much, so what was under the pointer stays.
    lay();
    let dt = get(|w| w.fixed).unwrap_or(fixed0) - fixed0;
    if dt != 0.0 {
        let _ = set(|w| w.scroll += dt);
        lay();
    }
    if matches!(how, How::Keep(true) | How::Undo) {
        ease_to(p.scroll0 + dt, false);
    }
    layout();
    if let Some(tree) = get(|w| w.tree.clone()) {
        uia::focus_moved(h, &tree, WHO);
    }
}

/// A press the opening click's guard drops (AC-15): inside the double click rectangle around
/// where that click went down, before the double click time has passed since the picker opened.
fn guarded(g: Option<Guard>, at: (i32, i32), now: Instant, (cx, cy): (i32, i32)) -> bool {
    g.is_some_and(|g| now < g.until && (at.0 - g.at.0).abs() * 2 <= cx && (at.1 - g.at.1).abs() * 2 <= cy)
}

/// A press while the picker is open: dropped near the opening click; outside the card it
/// closes and keeps, and does nothing else (AC-6); on the ring or square it picks at once and
/// drags; on a preset or a button it acts when the button comes up, as the page's controls do.
pub(super) fn pick_down(h: HWND, lp: LPARAM) {
    let rect = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
    let Some((pt, near, inside)) = get(|w| {
        let p = w.pick.as_ref()?;
        let pt = point(lp, w.scale);
        let inside = p.placed.is_none_or(|at| pt.0 >= at.x && pt.0 < at.x + PICK_W && pt.1 >= at.y && pt.1 < at.y + PICK_H);
        Some((pt, guarded(p.guard, w.down_at, Instant::now(), rect), inside))
    })
    .flatten() else {
        return;
    };
    if near {
        return;
    }
    if !inside {
        return pick_close(How::Keep(false));
    }
    unsafe { let _ = SetCapture(h); }
    let drag = set(|w| {
        w.kb = false;
        let at = w.pick.as_ref()?.placed?;
        let ((cx, cy), sq) = wheel_at(at, w.scale);
        let Some(part) = part_at((pt.0 - cx, pt.1 - cy), sq.2 / 2.0) else {
            w.press = hit_at(w, pt);
            return None;
        };
        let p = w.pick.as_mut()?;
        p.drag = Some(part);
        p.focus = if part == Part::Ring { Stop::Ring } else { Stop::Square };
        Some(part)
    })
    .flatten();
    match drag {
        Some(part) => pick_move(part, pt),
        None => layout(),
    }
}

/// The ring or square under the mouse: shows that color at once; letting go saves it.
pub(super) fn pick_move(part: Part, pt: (f32, f32)) {
    let _ = set(|w| {
        let Some((at, (h, s, v))) = w.pick.as_ref().and_then(|p| Some((p.placed?, p.hsv))) else { return };
        let ((cx, cy), sq) = wheel_at(at, w.scale);
        let hsv = match part {
            Part::Ring => ((pt.0 - cx).atan2(-(pt.1 - cy)).to_degrees().rem_euclid(360.0), s, v),
            Part::Square => (h, ((pt.0 - sq.0) / sq.2).clamp(0.0, 1.0), (1.0 - (pt.1 - sq.1) / sq.3).clamp(0.0, 1.0)),
        };
        show_color(w, ui::color::rgb_of(hsv), hsv);
    });
    layout();
}

/// The mouse let go of the ring or square, or lost the capture: saves what it shows, once.
/// Letting go calls ReleaseCapture, which comes here first, so the release finds nothing to do.
pub(super) fn finish_drag() {
    if set(|w| w.pick.as_mut().and_then(|p| p.drag.take())).flatten().is_some() {
        pick_save();
    }
}

/// A preset picked by a click or Space: shown and saved at once (AC-5).
pub(super) fn pick_preset(i: usize) {
    let _ = set(|w| {
        let Some(p) = w.pick.as_mut() else { return };
        (p.focus, p.last_swatch) = (Stop::Swatches(i), i);
        let (c, hsv) = (ui::PRESETS[i], ui::color::hsv_of(ui::PRESETS[i], p.hsv));
        show_color(w, c, hsv);
    });
    pick_save();
}

/// What an arrow does on a picker stop (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Arrow {
    Swatch(usize),
    Hsv(ui::color::Hsv),
}

/// On the swatches the arrows move and pick, 3 to a row, stopping at the first and last; on
/// the ring Left and Right turn the hue 1 degree (Shift: 10), wrapping; in the square the
/// arrows change saturation or value by 1% (Shift: 10%), clamped. None: nothing.
fn arrow(focus: Stop, vk: VIRTUAL_KEY, shift: bool, (h, s, v): ui::color::Hsv) -> Option<Arrow> {
    let step = if shift { 10.0 } else { 1.0 };
    match focus {
        Stop::Swatches(i) => {
            let n = match vk {
                VK_LEFT => i.checked_sub(1)?,
                VK_RIGHT => i + 1,
                VK_UP => i.checked_sub(3)?,
                VK_DOWN => i + 3,
                _ => return None,
            };
            (n < ui::PRESETS.len()).then_some(Arrow::Swatch(n))
        }
        Stop::Ring => match vk {
            VK_LEFT => Some(Arrow::Hsv(((h.round() - step).rem_euclid(360.0), s, v))),
            VK_RIGHT => Some(Arrow::Hsv(((h.round() + step).rem_euclid(360.0), s, v))),
            _ => None,
        },
        Stop::Square => {
            let d = step / 100.0;
            Some(Arrow::Hsv(match vk {
                VK_LEFT => (h, (s - d).max(0.0), v),
                VK_RIGHT => (h, (s + d).min(1.0), v),
                VK_UP => (h, s, (v + d).min(1.0)),
                VK_DOWN => (h, s, (v - d).max(0.0)),
                _ => return None,
            }))
        }
        Stop::Done | Stop::Cancel => None,
    }
}

/// Tab and Shift+Tab: five stops, wrapping inside the picker; coming back to the swatches lands
/// on the one equal to the color shown, else the last one focused.
fn tab(focus: Stop, back: bool, color: u32, last: usize) -> Stop {
    const ORDER: [Stop; 5] = [Stop::Swatches(0), Stop::Ring, Stop::Square, Stop::Done, Stop::Cancel];
    let i = ORDER.iter().position(|s| std::mem::discriminant(s) == std::mem::discriminant(&focus)).unwrap_or(0);
    match ORDER[(i + if back { 4 } else { 1 }) % 5] {
        Stop::Swatches(_) => Stop::Swatches(ui::PRESETS.iter().position(|&c| c == color).unwrap_or(last)),
        s => s,
    }
}

/// What a key down does in the picker, apart from moving (AC-9).
#[derive(Clone, Copy, PartialEq, Debug)]
enum KeyDoes {
    Close(How),
    Pick(usize),
    Move, // Tab or an arrow
    Nothing,
}

/// Esc anywhere, and Enter or Space on Cancel, undo; Enter from every other stop keeps and
/// closes, and never picks; Space on Done keeps, on a swatch picks it. Held Enter, Space and
/// Esc act once.
fn key_does(vk: VIRTUAL_KEY, focus: Stop, repeat: bool) -> KeyDoes {
    match (vk, focus) {
        (VK_RETURN | VK_SPACE | VK_ESCAPE, _) if repeat => KeyDoes::Nothing,
        (VK_ESCAPE, _) | (VK_RETURN | VK_SPACE, Stop::Cancel) => KeyDoes::Close(How::Undo),
        (VK_RETURN, _) | (VK_SPACE, Stop::Done) => KeyDoes::Close(How::Keep(true)),
        (VK_SPACE, Stop::Swatches(i)) => KeyDoes::Pick(i),
        (VK_TAB | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN, _) => KeyDoes::Move,
        _ => KeyDoes::Nothing,
    }
}

/// The picker's keys, taken before the page's, as the open menu's are (AC-9).
pub(super) fn pick_key(vk: VIRTUAL_KEY, repeat: bool) {
    let Some(focus) = set(|w| {
        w.kb = true;
        w.pick.as_ref().map(|p| p.focus)
    })
    .flatten() else {
        return;
    };
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match key_does(vk, focus, repeat) {
        KeyDoes::Close(how) => pick_close(how),
        KeyDoes::Pick(i) => pick_preset(i),
        KeyDoes::Nothing => repaint(),
        KeyDoes::Move if vk == VK_TAB => {
            let _ = set(|w| {
                if let Some(p) = w.pick.as_mut() {
                    p.focus = tab(p.focus, shift, p.color, p.last_swatch);
                    if let Stop::Swatches(i) = p.focus {
                        p.last_swatch = i;
                    }
                }
            });
            layout();
        }
        KeyDoes::Move => {
            // An arrow: shown at once; the key up saves.
            let _ = set(|w| {
                let Some(p) = w.pick.as_mut() else { return };
                let (c, hsv) = match arrow(p.focus, vk, shift, p.hsv) {
                    Some(Arrow::Swatch(n)) => {
                        (p.focus, p.last_swatch) = (Stop::Swatches(n), n);
                        (ui::PRESETS[n], ui::color::hsv_of(ui::PRESETS[n], p.hsv))
                    }
                    Some(Arrow::Hsv(hsv)) => (ui::color::rgb_of(hsv), hsv),
                    None => return,
                };
                p.held = Some(vk);
                show_color(w, c, hsv);
            });
            layout();
        }
    }
}

/// A key up: the held arrow's saves what it showed (AC-9). The key that opened the picker
/// comes up here too, and does nothing.
pub(super) fn pick_key_up(vk: VIRTUAL_KEY) {
    let up = set(|w| w.pick.as_mut().is_some_and(|p| p.held == Some(vk) && p.held.take().is_some()));
    if up == Some(true) {
        pick_save();
    }
}

/// The page lost the keyboard during a held arrow: saves what it showed, as its key up would.
pub(super) fn pick_lost_keys() {
    if set(|w| w.pick.as_mut().is_some_and(|p| p.held.take().is_some())) == Some(true) {
        pick_save();
    }
}

/// A click opens the picker over this control (spec 0021 AC-1): the hand shows there.
pub(super) fn opens(w: &Win, ctl: Option<Ctl>) -> bool {
    !w.theme.hc && settings::store::parse_error().is_none() && w.rec.is_none() && matches!(ctl, Some(Ctl::WsTile(..) | Ctl::ShowTile(..) | Ctl::Swatch(_)))
}

#[cfg(test)]
mod tests;
