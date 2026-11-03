# 0012. Settings page

**Date**: 2026-11-01
**Updated**: 2026-11-01, after the approved design (r4): the open points A1 to A14 are settled in the ACs below, and AC-18 to AC-21 are new.
**Status**: In Progress

## Summary

des-k gets a Settings window where you change every setting by clicking: workspaces, shortcuts, the dots, restore, the overview and touchpad gestures. Each change is saved to `settings.toml` at once, only that value changes, every comment stays, and des-k uses it right away with no restart. des-k also watches the file, so a hand edit applies live too. This changes two rules from spec 0001: des-k now writes the file, and settings are no longer read only once.

## Requirements

**User stories**:
- As the des-k user, I want to change any setting by clicking, so I don't have to learn the file format.
- As the des-k user, I want a change to work the moment I make it, so I can try a value and feel it.
- As the des-k user, I want my hand comments and edits in `settings.toml` kept, so the page and the file live side by side.

**Acceptance criteria**:
- **AC-1**: Tray › **Settings** opens the page, centered in the work area of the screen under the mouse. Opening it again while it's open brings the open one to the front. The tray no longer has "Open settings"; the page has an **Open settings.toml** button (opened through explorer, `crate::tray::open`).
- **AC-2**: The page shows every setting the file holds, with its current value: `workspaces`, `copilot_as_right_ctrl`, `follow_activation`; the 11 `[shortcuts]` entries; `[indicator]` 3; `[restore]` 2; `[overview]` 3; `[touchpad]` 3; and the 10 gesture rows. A side list (Workspaces, Shortcuts, Indicator, Restore, Overview, Touchpad) jumps to each section; every section is also reachable by scrolling. Each side list item shows its count of problems. When a problem matches no setting on the page (an unknown key, or `gesture rule N`), a **Notes** item heads the list with its count and opens a notes card at the top that lists those problems in the log's words.
- **AC-3**: Each change saves at once. Only that value changes in `settings.toml`; every other line and comment stays byte for byte. (Exception: a gesture change rewrites the `[[gesture]]` blocks, see AC-12.)
- **AC-4**: A saved change is in effect within 1 s, with no restart. The one exception is `[restore]`, which only acts at start; its section says "applies from des-k's next start". Every other section says "applies now".
- **AC-5**: A hand edit to `settings.toml` while des-k runs applies within 2 s, by the same path as a page change. An open page shows the new values.
- **AC-6**: Lowering `workspaces` moves every window on a removed workspace onto the new last workspace of the same screen. A screen showing a removed workspace switches to the new last one, so the windows that move there are in view; on every other screen they stay hidden. Raising it adds empty workspaces; windows moved by an earlier lowering stay where they went. There is no undo: before you remove (× on the last tile, Delete, or Reset on the count), the page shows one line per screen saying what moves where, as "Screen 2: 3 windows from 7, 8 move to 6" (on screen windows of a removed workspace count). A screen where nothing moves gets no line; when no screen has one, it says "nothing moves". Screens not plugged in now are resized too and get a line under their device name.
  Order, per screen: a screen showing a removed workspace first switches to the new last one exactly like a normal switch (its windows hide bottom first). Then the removed workspaces' windows join the new last workspace after its own, by workspace, then saved order. If that workspace is the one on screen, they are shown top first ("in view"); otherwise they join its hidden list. `state.txt` is saved after each screen, so a crash midway strands at most one screen's moves, and the next start adopts them. The dots and the overview use the new count.
- **AC-7**: Recording a shortcut: you click the shortcut (or **+ Add**), press the combo, and it is captured. While recording, none of des-k's shortcuts fire, the Start menu does not open, and the keys reach no other program. Esc with no modifier held cancels; Esc with a modifier held is recorded like any key (the file names it `Escape`). Modifiers alone keep it waiting; the first other key down finishes it, with the modifiers held right then (`mods_held`). The allowed keys are the parser's own names (`vk_of`: named keys, A to Z, 0 to 9, F1 to F24). Any other key (a numpad digit, punctuation, media keys) is refused: recording stays on and says "This key can't be saved; try another". Keys Windows keeps for itself (Win+L, Ctrl+Alt+Del) never reach the hook and can't be recorded. With `copilot_as_right_ctrl` on, the Copilot key counts as Ctrl held (a modifier, not a key), since the hook rewrites it first. Recording also ends when the page loses focus or closes. A combo is valid by the file's own rule (one key plus at least one of Win, Ctrl, Alt, Shift).
- **AC-8**: A shortcut entry shows all its combos as chips, each with × to remove, plus **+ Add**. No chips means off. The three digit entries (`jump`, `send_to`, `send_to_stay`) record the modifiers held with any digit and show as "Win + 1…9"; they can be turned off. When a list holds one of a set's digits (say `finder = ["Win+3"]` and `jump = "Win"`), the set skips that digit and its row says so: "Win + 1…9, skips 3 (Finder)". This is a valid setting, not a problem: the parser stops logging it. Several skips read "skips 3 (Finder), 5 (Next)". The page's scan uses the hook's order: lists first, then `jump`, `send_to`, `send_to_stay`.
- **AC-9**: Recording a combo another entry already uses names that entry and offers **Move it here** (the other entry loses it) or **Cancel**. A combo also clashes with a digit entry when it is that entry's modifiers plus a digit 1 to 9: there, **Move it here** gives the combo to the list and the set skips that one digit (AC-8). Every Move it here saves both entries in one write (`Edit::Pair`). Recording a digit set whose modifiers another set uses offers **Move it here** (the other set turns off). A digit set whose modifiers match a list's digit combo is not a clash; the set skips that digit. The page never writes a clash into the file.
- **AC-10**: Numbers use a stepper (− and +, and arrow keys) within the spec 0001 ranges, in fixed steps: `workspaces` 2 to 9 by 1 (through the workspace pictures: **+ Add**, and × or Delete on the last tile), `hold_ms` 0 to 5000 by 50, `fade_ms` 0 to 2000 by 50, `restore.minutes` 1 to 30 by 1, `swipe_mm` 5 to 50 by 1, `twice_ms` 100 to 1000 by 50. On/off values are toggles. `animation` and `labels` are choice buttons. No setting takes typed text. A hand edited value off the step grid (say `hold_ms = 555`) is valid and shown as is; the next step lands on the grid (− gives 550, + gives 600).
- **AC-11**: A setting whose value differs from its default shows the default beside it ("default: 550") and a reset button that writes the default value.
- **AC-12**: Touchpad shows 10 fixed rows (3 and 4 fingers × left, right, up, down, tap). Each row has a **once** action picker and an optional **twice** picker. The picker lists every action by its label (`Action::label`) plus **Key combo…**, which records like AC-7 with no clash check. The twice picker starts with **Not set** (no twice rule: once runs at once), kept apart from **Nothing** (a twice rule that does nothing: once waits `twice_ms`, and a second go is swallowed). A row with no once rule in the file (a hand edit) shows **Nothing**, the same behavior, with "not in the file" beside it; the next gesture write records it as `do = "none"`. A change rewrites the `[[gesture]]` blocks as the full list of the 10 rows (once rules always, twice rules only when set). The comment above the first block is kept.
- **AC-13**: The Touchpad section has a **Touchpad setup** button that opens the welcome screen, or brings it to the front if it is already open. On the welcome screen, **Choose my own** opens this page scrolled to Touchpad instead of opening `settings.toml` (the rest of its spec 0011 behavior stays).
- **AC-14**: A bad value in the file shows the value des-k uses (the default) with a note naming the problem, the same words as the log. Changing it writes a good value and the note goes. A file that won't parse opens the page read only, with the parser's message (with its line number) and **Open settings.toml**. This holds at start and on a live reload, and also when the file can't be read at all: the last error is kept until a read succeeds, and the page reads it from there. The message is the `toml_edit` error's own text (it carries the line). The page leaves read only on the next good reload, within 1 s of the fix. On a live reload, a file that won't parse keeps the last good settings in effect (at start it still uses defaults, spec 0001 AC-11).
- **AC-15**: Every applied change logs one line per changed setting: its name, old value → new value, and where it came from (`page` or `file`).
- **AC-16**: Keyboard only, as in Windows Settings (changed 2026-11-02 at your request): Tab and Shift+Tab move between the side list and the settings; Up and Down move from row to row, one stop per row (a number's − and + are one stop, a choice is one stop, Reset is its own stop under its row); Left and Right change the focused row's value; Space or Enter flips a switch or presses a button; Home and End go to the first or last row; Esc cancels a recording or else closes the page. The page repaints on a light, dark or contrast theme switch, and stays sharp when dragged to a screen at another scale.
- **AC-17**: A save never leaves a half written file: des-k writes `settings.toml.tmp` next to it, then swaps it in with one rename.
- **AC-18**: Workspaces shows one picture per present screen, numbered left to right then top to bottom (the finder's order), headed "Screen N · name · width × height · scale%". The name is the monitor's own name, or "Built-in display" for a laptop panel, or the device name (`DISPLAY1`) when Windows gives none. Each workspace tile draws its windows where they sit, marks the one in view ("now") and counts its windows. The pictures refresh when the page opens or is activated, after any applied change, on a display change, and on the 1 s watch tick while the page is open and not minimized; a tick repaints only when the picture data changed. The page is a normal window, so it shows in its own picture like any other.
- **AC-19**: Indicator has a **Play** button that shows the real dots on the page's screen with the current hold and fade (the active workspace and count of that screen). Play is off while the dots are off. The screen is the one the page is on now (`MonitorFromWindow`), shown through `indicator::show` with `sent` = `None`.
- **AC-20**: A hand edit that lands while the page is open shows a banner: "settings.toml changed at HH:MM and is in use:" (the local time `reload(File)` ran, from `GetLocalTime`) with each changed setting as name, old → new, and for `workspaces` one line per screen of what moved. It stays until **Dismiss** or the next hand edit. Changes made on the page show no banner. Touchpad opens with a status line in one of six states (ok, waiting, can't read, off with Windows' gestures on, off with Windows' gestures set to Nothing, no touchpad), from `setup::state()`, checked in this order: no `pad` → no touchpad; not `enabled` → off, split by `off`; a row `CantRead` → can't read; a row `StillOn` → waiting, naming the first such row by its `setup::ROWS` label; else ok. Wording, title then line:
  - ok: "des-k reads your three and four finger gestures" / "Windows' own three and four finger gestures are all set to Nothing."
  - waiting: "des-k is waiting: Windows' own <row> are still on" / "des-k acts only when all of Windows' three and four finger gestures are Nothing, so one gesture never does two things."
  - can't read: "des-k can't read one of Windows' gesture settings, so it waits" / "Touchpad setup shows which row."
  - off, Windows' on: "Reading is off here, and Windows' own gestures are on" / "Your touchpad works the Windows way. Turn on Read touchpad gestures to use the gestures below."
  - off, Windows' Nothing: "No three or four finger gesture does anything now" / "Reading is off here, but Windows' own gestures are set to Nothing. Turn reading on, or give them back to Windows in Touchpad setup."
  - no touchpad: "No precision touchpad found" / "Your gestures are kept and work when one is plugged in."
- **AC-21**: The shortcut scenes (Switch, Send and go, Send and stay) and the overview's small picture are fixed drawings that show the current settings (combos, labels, animation), not your live windows.

## Decision

**Chosen option**: Option 1: live settings with in place file edits, on a custom drawn page in the welcome screen's style.

One `settings.toml` stays the single source of truth. The page edits it in place with `toml_edit`. des-k reloads it on every save and whenever its write time changes, and swaps the new settings in for every reader at once.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-serde` (`rewrite-rs/skills`, `.claude/skills/rust-serde/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

**Design source**: approved 2026-11-01: `pdesk-design/settings/micro-suite-r4.html` (sha256 `8f34660d8b59b387b0cf9c18036d883e65cec13930552a224173db9ceac39b5f`), its captures `captures/r4-*.png`, and the record in `pdesk-design/decisions.md`. Every section is a picture you act on: workspace tiles per screen, shortcut scenes with chips, the indicator timeline, the restore timeline, a small overview, and touchpad gesture cards with an editor (the 10 rows of AC-12 drawn as cards for 3 and 4 fingers). Window 760×620, tokens from `pdesk-design/design-system/` (provisional) plus the settings tile set. States marked "proposed" in the mockup follow this spec where they differ; the one copy change is the digit clash prompt, which now says the set skips one digit (AC-9) instead of turning all nine off.

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch** (no database; one file plus in memory state):
- `settings.toml`: unchanged format (spec 0001, 0011). The page writes values only, never a program name or a path (AGENTS.md rule).
- `Loaded { settings: Settings, problems: Vec<Problem> }`, where `Problem { key: String, message: String }`. `load` and the new `reload` build it; `problems` holds what spec 0001 logs today for each bad value.
- The current `Loaded` sits behind one `AtomicPtr`. A reload leaks the new one (`Box::leak`) and stores it. `settings::get()` keeps its `&'static Settings` signature, so no reader changes. Old ones are never freed (a few KB per save; fine for one person's tool).
- `settings::PARSE_ERROR`: the last parse error, set by `load` and `reload` on a file that won't parse, cleared by a good read.
- `ws::ScreenInfo { device, number, name, width, height, scale }`: built fresh each refresh, never stored.
- Page state (main thread only): the open section, scroll offset, focused control, the recording target (`None`, a shortcut entry, a digit entry, or a gesture row), the file's last seen write time, and the open banner (its time and lines) if any.

**State transitions** (the page's recording state):
`idle → recording(target)` on click of a shortcut or **+ Add** or **Key combo…** · `recording → idle` on a valid combo (saved, or the clash prompt), Esc, focus loss or close · `clash prompt → idle` on **Move it here** (saves both entries in one write) or **Cancel**.

**Interface surface** (internal functions and messages):
| Item | Kind | Inputs | Outputs | Thread | Key errors |
|---|---|---|---|---|---|
| `settings::get()` | fn | none | `&'static Settings` | any | none |
| `settings::problems()` | fn | none | `&'static [Problem]` | any | none |
| `settings::reload(source)` | fn | `Source::{Page, File}` | the changes, each as name, old, new | main | parse error: keep current, log, store in `PARSE_ERROR`, return it |
| `settings::parse_error()` | fn | none | `Option<String>` | main | none |
| `ws::screen_info()` | fn | none | `Vec<ScreenInfo>` in finder order | main | a failed name lookup falls back to the device name |
| `ws::resize(count)` | fn | new count | the per screen move lines for the banner and log | main | per window failures logged and skipped |
| `settings::write(edit)` | fn | `Edit::{Value(path, value), Combos(name, Vec<Combo>), Digits(name, Option<u8>), Gestures(Vec<Gesture>), Pair(Edit, Edit)}` | `Ok`, then calls `reload(Page)` | main | parse error (page goes read only), write or rename error (logged, shown on the page) |
| `settings::apply(old, new)` | fn | two `&Settings` | side effects below | main | per item failures logged and skipped |
| `settings_page::open(section)` | fn | `Option<Section>` | opens or focuses the page | main | window creation failure logged |
| `keys::record(on)` | fn | `bool` | sets an `AtomicBool` the hook reads | main | none |
| `WM_RECORDED` | thread message, `WM_APP + 10` | `wParam` = vk, `lParam` = modifier bits | the page's recorder | hook posts, main handles | none |
| file watch | 1 s timer on the main thread | the file's write time | calls `reload(File)` when it changed | main | missing file: skip |

What `apply` does when a value changed: `workspaces` → `ws` resizes every monitor per AC-6 and saves `state.txt`; `touchpad_enabled` → false makes the reader drop every report, true starts the reader if it never started; any change to `gestures` or `twice_ms` → drops a pending twice gesture; any change while the overview is open → closes the overview; then the welcome screen and the page refresh if open. Shortcuts, indicator, follow, Copilot, overview and swipe values need nothing, since their readers call `get()` each time. The temporary rule in `reload` that holds a new `workspaces` until the next start goes away in step 4, when `ws::resize` lands.

**Value sourcing**:
| Action | Value produced or displayed | Source |
|---|---|---|
| page row | current value | `settings::get()` |
| page row | default value and reset target | `Settings::default()`; gesture rows from `default_gestures()` |
| page row | problem note | `settings::problems()`, matched by key |
| page section | "applies now" or "applies from des-k's next start" | a fixed table in `settings_page.rs`: `[restore]` next start, all else now (AC-4) |
| page row | stepper range and step | AC-10 (ranges are spec 0001's) |
| gesture picker | action names and labels | `ACTIONS` and `Action::label` (`settings.rs`) |
| clash prompt | the entry that owns a combo | a scan of the current `Settings` shortcut lists and digit entries |
| read only page | parser message and line | the `toml_edit` parse error |
| page position | screen and work area | the monitor under the cursor, as the welcome screen does |
| notes card | problems with no row | `settings::problems()` whose key matches no page row (AC-2) |
| digit row | "skips N (entry)" | a scan of the shortcut lists for that set's modifiers plus a digit (AC-8) |
| read only page | parse error at start or live | `settings::parse_error()` (AC-14) |
| workspace pictures | screen number and order | `ws::screens()` sorted by left, then top (the finder's `screen_numbers` order) |
| workspace pictures | screen name | `QueryDisplayConfig`, then `DisplayConfigGetDeviceInfo`: `DISPLAYCONFIG_SOURCE_DEVICE_NAME` matched to the GDI device name, then `DISPLAYCONFIG_TARGET_DEVICE_NAME`'s friendly name; "Built-in display" when `outputTechnology` is INTERNAL, DISPLAYPORT_EMBEDDED or UDI_EMBEDDED; an empty name falls back to the device name. Needs the `Win32_Devices_Display` feature |
| workspace pictures | resolution and scale | the monitor rectangle (des-k is per monitor DPI aware, so real pixels) and `GetDpiForMonitor` / 96 |
| workspace pictures | windows per workspace, their spots | `ws::windows()` grouped by device and workspace, `ws::real_rect` relative to the screen rectangle |
| workspace pictures | the "now" marker | `ws::active(device)` |
| removal line | what moves where on each screen | `ws::windows()` and `ws::active()` for the workspaces being removed (AC-6) |
| change banner | time, name, old → new, moves | `GetLocalTime` when `reload(File)` ran (feature `Win32_System_SystemInformation`), its changes, and `ws::resize`'s lines (AC-20) |
| touchpad status | one of six states | `setup::state()` and `setup::ROWS`, in the order AC-20 gives |
| indicator Play | screen, active, count, hold, fade | `MonitorFromWindow(page)`, `ws::active`, `settings::get()`, through `indicator::show(…, None)` |
| scene and overview pictures | combos, labels, animation | `settings::get()`; the art itself is fixed (AC-21) |
| reload trigger | file write time | `GetFileAttributesExW` on `settings::path()` |
| log line | old and new values | `apply(old, new)` |
| recorder | vk and modifiers | the keyboard hook, via `WM_RECORDED` |

**Key invariants**:
- `settings.toml` is the only store. The page never keeps a second copy of a value; after every write it shows what `reload` read back.
- No clash is ever written (AC-9).
- A reload that fails to parse never changes the settings in effect.
- The hook thread only reads the recording flag and posts `WM_RECORDED`; parsing, clash checks and writing happen on the main thread.
- While recording, the hook swallows every key down and up (with `VK_MASK` after Win or Alt, as today) so nothing reaches Windows or another program.
- Writes go to `settings.toml.tmp`, then `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`.
- `ws`'s lock is not reentrant: `apply` and `ws::resize` run only from the page's window procedure or the main loop's timer, never inside a `ws` call or the foreground callback. The page copies what it draws out of `ws` and holds no lock while it paints.

**Security model**: des-k runs as admin and the page is one of its windows. The page writes only values that pass the same checks as the parser; nothing in the file names a program to run or code to load, so a value written by the page or by hand cannot make des-k run something. **Open settings.toml** goes through explorer as you (`crate::tray::open`). No message filters change: normal programs still can't send the page messages (UIPI). The recorder only acts while the page has focus.

**Configuration required**: none. One new crate: `toml_edit` (the editing half of the `toml` family; pick the version that builds with `toml` 1.x).

**Critical test scenarios**:
- Happy path: turn the dots off on the page → the file shows `enabled = false` with its comments intact, and the next switch shows no dots, without a restart. Verifies AC-3, AC-4.
- Hand edit: set `workspaces = 3` in an editor with windows on workspace 4 → within 2 s they are on workspace 3, and the log names the change with `file`. Verifies AC-5, AC-6, AC-15.
- Failure: save a broken file by hand → des-k keeps working with the old settings; the page opens read only with the parser line. Verifies AC-14.
- Clash: record Win+F on `next` → the prompt names Finder; Move it here leaves Finder with no combo, in one write. Verifies AC-9.
- Unit tests (`cargo test`): editing one value with `toml_edit` keeps every other line of `DEFAULT_FILE` byte for byte; the gesture rewrite parses back to the same 10 rows; clash detection across lists and digit entries; reload with bad TOML keeps the old settings.

## Build plan

Tracer Bullet: the first slice runs one setting through every layer (page → file → reload → live effect); later slices widen it. Steps 1 and 2 don't depend on the look and can be built before the design is approved.

1. **Live settings core**: `Loaded` behind an `AtomicPtr`, `reload`, `apply` (log lines only for now), `problems()`, the 1 s file watch, `toml_edit` writes through a temp file, unit tests for the in place edit and a bad reload. Satisfies AC-5, AC-14 (live part), AC-15, AC-17.
2. **Tracer page**: a plain `settings_page.rs` window with one toggle (`[indicator] enabled`) and Open settings.toml; tray item becomes **Settings**. Prove page → file → live dots end to end. Satisfies AC-1, AC-3, AC-4.
3. **Design pass** (`ui-design-explorer`, `pdesk-design/settings/`): mockups of every control and state on the design system; you approve one. Gates steps 4 to 6.
4. **All plain settings**: side list with the Notes item, sections, toggles, steppers, choice buttons, default and reset, problem notes, read only state from `parse_error()`, "applies now" lines, keyboard, theme and DPI. The workspace pictures (`ws::screen_info`, windows per workspace, refresh), the indicator Play, the restore and overview pictures, the change banner, the touchpad status line. Then the `apply` side effects: `ws::resize` (AC-6), touchpad on and off, overview close, twice drop. Satisfies AC-2, AC-4, AC-6, AC-10, AC-11, AC-14, AC-16, AC-18, AC-19, AC-20, AC-21.
5. **Shortcuts**: `keys::record` and `WM_RECORDED`, chips, + Add, digit entries with their skipped digits, clash prompt, the scenes. The parser stops logging a set's skipped digit (update the two unit tests that expect that warning). Unit tests for clash detection, including list versus set (skip) and set versus set (turn off). Satisfies AC-7, AC-8, AC-9.
6. **Gestures**: the 10 rows, once and twice pickers, Key combo…, the `[[gesture]]` rewrite with its test, the Touchpad setup button, and **Choose my own** opening the page at Touchpad. Satisfies AC-12, AC-13.
7. **Wrap up**: README (Settings and touchpad parts), install, and the on screen checks in [verify.md](verify.md). Satisfies all ACs on your screens.

## Consequences

**Positive**:
- Every setting is a click away, and changes are felt at once.
- The file stays the one source of truth; hand edits and page edits never fight.
- Readers of `settings::get()` don't change.

**Negative / tradeoffs**:
- Changes spec 0001: AC-2 (des-k never rewrites the file) and "read once at start" no longer hold. AGENTS.md says "read once" and needs a sync.
- Live apply adds the riskiest part of this feature: shrinking workspaces while windows are hidden. A bug there can strand windows, so `apply` must follow the "only uncloak what des-k hid" and "every exit shows all" rules.
- A gesture change drops your own comments inside the `[[gesture]]` blocks.
- Old settings are leaked on each reload, by design.
- One new crate, and a second custom drawn window about the size of the welcome screen.

**Neutral**:
- Spec 0011's "Choose my own opens settings.toml" changes to "opens the page at Touchpad" when step 6 lands.
- The keyboard hook gains a recording mode.

## Follow-up

- [x] When step 1 lands, add an "Updated" line to spec 0001: its AC-2 and "read once at start" are changed by this spec.
- [x] When step 6 lands, update spec 0011 (the Choose my own line, verify step 6 of its welcome list) and the README.
- [x] `/sync` after the build: add `WM_APP + 10` (`WM_RECORDED`) to AGENTS.md's thread message id list. ("Read once at start" and `+ 6` to `+ 9` were updated 2026-11-02.)
- [x] Record the approved settings design in `pdesk-design/decisions.md` (step 3).
- [x] Step 7: add on screen checks for AC-18 to AC-21 to [verify.md](verify.md) (screen names on both monitors, the banner after a hand edit, Play, the six touchpad states you can reach).
