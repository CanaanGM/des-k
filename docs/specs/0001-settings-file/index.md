# 0001. Settings file for workspaces, shortcuts and indicator

**Date**: 2026-10-22
**Updated**: 2026-10-28, settings moved to `%LOCALAPPDATA%\pdesk` by [spec 0008](../0008-run-pdesk-as-admin/index.md)
**Updated**: 2026-11-01, [spec 0012](../0012-settings-page/index.md) makes settings live: pdesk reads the file again whenever it changes, and the settings page edits values in place. AC-2 and "read once at start" no longer hold.
**Status**: Accepted

## Summary

pdesk gets one `settings.toml` file in `%LOCALAPPDATA%\pdesk`, the same folder as its log and layout. It holds the workspace count, the switch shortcuts, the indicator timing, and an on/off switch for the Copilot key remap. pdesk reads it once at start, so you restart pdesk after an edit (since spec 0012, pdesk reads it again whenever it changes, and a change applies within two seconds). A broken value only resets that one setting (the log says which), a broken file resets everything, and pdesk always starts.

## Requirements

**User stories**:
- As the pdesk user, I want to change the workspace count, shortcuts and indicator timing in a file so that I don't have to rebuild.
- As the pdesk user, I want a mistake in the file to cost me one setting at most, with a log line telling me what went wrong.

**Acceptance criteria**:
- **AC-1**: If `settings.toml` does not exist in `%LOCALAPPDATA%\pdesk`, pdesk writes the commented default file there on start, and behaves exactly as it does today.
- **AC-2**: pdesk never overwrites or rewrites an existing `settings.toml`. (Changed by spec 0012: the settings page writes one value at a time, keeping every other line and comment.)
- **AC-3**: With `workspaces = N` (2 to 9), after a restart every screen has N workspaces, the indicator shows N dots, and switching stops at workspace N.
- **AC-4**: After you lower the count, windows that `state.txt` lists on a workspace that no longer exists are shown on the next start; none stay cloaked.
- **AC-5**: Every shortcut in `[shortcuts] previous` and `next` switches the screen under the mouse. A combo matches only when exactly its modifiers are held. A combo that is not listed is no longer swallowed and reaches Windows normally.
- **AC-6**: An invalid entry in a shortcut list is dropped and logged; the valid entries still work. If no valid entry is left, that action uses its default.
- **AC-7**: If the same combo appears under both `previous` and `next`, both actions use their defaults and the clash is logged.
- **AC-8**: `[indicator] enabled = false` shows no popup. `hold_ms` and `fade_ms` set how long the popup holds and how long it fades.
- **AC-9**: `copilot_as_right_ctrl = false` lets the Copilot key through untouched.
- **AC-10**: A value with the wrong type or out of range makes only that setting fall back to its default; the log names the key, the problem, and the default used. Every other setting still applies.
- **AC-11**: A file that is not valid TOML makes pdesk use all defaults and log the parser's message (with its line number). pdesk still starts.
- **AC-12**: An unknown key is ignored with a warning in the log.
- **AC-13**: At start the log shows the settings path and the effective value of every setting.

## Decision

**Chosen option**: Option 1: TOML, parsed into a table and read key by key.

pdesk reads `settings.toml` from `%LOCALAPPDATA%\pdesk` once at start into an immutable `Settings` (since spec 0012, again on every change, swapped whole for every reader). Bad values fall back one setting at a time; bad TOML falls back to all defaults; problems go to the log.

**Implementation skills**: `rust-serde` (`rewrite-rs/skills`, `.claude/skills/rust-serde/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Feature design

**Data model sketch**:

`settings.toml` (the default file, written verbatim when missing; the comments are part of it):
```toml
# pdesk settings. Restart pdesk after editing.

workspaces = 4              # 2 to 9, every screen
copilot_as_right_ctrl = true

[shortcuts]
# Keep these two if you switch with the touchpad swipe.
previous = ["Win+Ctrl+Left"]
next     = ["Win+Ctrl+Right"]

[indicator]
enabled = true
hold_ms = 550               # 0 to 5000
fade_ms = 200               # 0 to 2000
```

`Settings` (in `src/settings.rs`, all fields required, filled from defaults when missing or bad):

| Field | Type | Default | Rule |
|---|---|---|---|
| `workspaces` | `usize` | 4 | 2 to 9 |
| `copilot_as_right_ctrl` | `bool` | true | |
| `previous` | `Vec<Combo>` | `[Win+Ctrl+Left]` | 1 or more valid entries |
| `next` | `Vec<Combo>` | `[Win+Ctrl+Right]` | 1 or more valid entries, none shared with `previous` |
| `indicator_enabled` | `bool` | true | |
| `hold_ms` | `u32` | 550 | 0 to 5000 |
| `fade_ms` | `u32` | 200 | 0 to 2000 |

`Combo { mods: u8, vk: u16 }`. `mods` is a bit set of Win, Ctrl, Alt, Shift. Shortcut text: parts joined by `+`, any order, any case. Modifiers `Win`, `Ctrl`, `Alt`, `Shift` mean either side of the keyboard. Exactly one key, one of: `Left Right Up Down`, `A` to `Z`, `0` to `9`, `F1` to `F24`, `Home End PageUp PageDown Insert Delete Space Enter Tab Escape`. At least one modifier, no part repeated.

`ws.rs` `Monitor.hidden` changes from `[Vec<isize>; WORKSPACES]` to `Vec<Vec<isize>>` of length `settings::get().workspaces`; the `WORKSPACES` constant goes away. The `state.txt` format is unchanged.

**State transitions**: none. Settings are read once and never change while pdesk runs. (Since spec 0012, a reload replaces them whole; a file that won't parse keeps the ones in effect.)

**API surface** (module functions; no network or CLI surface):

| Surface | Kind | Inputs | Outputs | Errors |
|---|---|---|---|---|
| `settings::load()` | called once in `main` after `indicator::init()`, before `ws::load()` and `keys::spawn` | `settings.toml` in `ws::data_dir()` | fills the `OnceLock`; log lines | missing file: write defaults (write failure: log, use defaults) · bad TOML: all defaults · bad key: that key's default |
| `settings::get()` | `&'static Settings` | none | the settings | before `load()`: defaults (never panics) |
| `keys::handle` | hook | vk, up | swallow or pass | none |
| `ws::switch` / `ws::load` | main thread | `workspaces` | clamp, Vec size, shrink handling | none |
| `indicator::show` | main thread | `indicator_enabled`, `hold_ms`, `fade_ms` | popup or nothing | none |

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| load | file path | `ws::data_dir()` (`%LOCALAPPDATA%\pdesk`, created if missing) + `settings.toml`. The installed copy and `cargo run` share this one file. It used to be the exe's folder; spec 0008 moved it because the installed exe now lives in Program Files, where you cannot write. |
| load | default file text | `const DEFAULT_FILE: &str` in `settings.rs` |
| load | each setting | the TOML key, else `Settings::default()` |
| load | log line text | `settings: <key>: <problem>, using default <value>` · `settings: not valid TOML (<toml error>), using all defaults` · `settings: unknown key <key>, ignored` · `settings: wrote defaults to <path>` · `settings: <path>: <every effective value>` |
| hook match | held modifiers | `GetAsyncKeyState` of `VK_LWIN`/`VK_RWIN`, `VK_CONTROL`, `VK_MENU`, `VK_SHIFT` at the event (today's `held()`); the Copilot remap's injected Right Ctrl counts as Ctrl |
| hook match | direction | which list the matched combo is in: `previous` = -1, `next` = +1, posted as `WM_APP` as today |
| switch | workspace count | `settings::get().workspaces` |
| load (state) | where a saved window goes | its workspace < count: as today; workspace ≥ count: shown, like a window from a gone screen. A saved active ≥ count is ignored (active 0) |
| indicator | fade step per 16 ms tick | `ceil(OPAQUE / max(1, fade_ms / 16))`; `fade_ms = 0` hides on the first tick after the hold |
| indicator | hold | `hold_ms` passed to `SetTimer` (0 gives Windows' 10 ms minimum) |

**Key invariants**:
- `Settings::default()` equals what parsing `DEFAULT_FILE` produces, with no warnings.
- Every `Settings` value pdesk runs with is in range; nothing out of range reaches `ws`, `keys` or `indicator`.
- `previous` and `next` are never empty and share no combo.
- Every window in `state.txt` is either adopted into a workspace that exists or shown. None is left cloaked without being tracked.
- The hook thread never touches the file and never blocks; it only reads `settings::get()`.

**Security model**: a local file in a folder only your user account can write, read by pdesk, which runs as admin since spec 0008. It holds no secrets and must never name a program to run or a file to load as code, since anything running as you can edit it (spec 0008 invariant). A bad edit can only change pdesk's own keys and timing.

**Configuration required**: no new environment variables or credentials. One new crate, `toml` (add with `cargo add toml` to get the current version; it brings `serde` along).

**Critical test scenarios**:
- Happy path: set `workspaces = 3`, add `"Alt+Ctrl+Left"` to `previous`, restart: three dots, both previous combos switch, the swipe still works. Verifies AC-3, AC-5.
- Failure case: with windows hidden on workspace 4, set `workspaces = 2` and restart: those windows are visible. Verifies AC-4.
- Failure case: break the file (`workspaces = = 4`), restart: pdesk runs on defaults and the log shows the TOML error with its line. Verifies AC-11.
- Failure case: `workspaces = 12` and `hold_ms = "long"`: both use defaults with log lines; the other settings apply. Verifies AC-10.
- Unit test: `DEFAULT_FILE` parses to `Settings::default()` with zero warnings. Verifies AC-1.
- Permission: not applicable (single user local file).

## Build plan

Tracer Bullet: first one setting all the way from file to behavior, then the others.

1. `cargo add toml`. Create `src/settings.rs` (file header comment per AGENTS.md): path next to the exe (since moved to the data folder by spec 0008), write `DEFAULT_FILE` when missing, parse to a table, read `workspaces` with its range check, warn on unknown keys, all defaults on bad TOML, `OnceLock` plus `get()`, the startup summary line. Call `settings::load()` from `main`. Satisfies **AC-1**, **AC-2**, **AC-10**, **AC-11**, **AC-12**, **AC-13**.
2. `ws.rs`: drop `WORKSPACES`, make `hidden` a `Vec` sized from settings, use the count in `switch` and the `println!` in `main`, show saved windows whose workspace is gone in `load`. Satisfies **AC-3**, **AC-4**.
3. Add a `#[test]` that `DEFAULT_FILE` parses to `Settings::default()` with no warnings. Satisfies **AC-1**.
4. Indicator keys: read `enabled`, `hold_ms`, `fade_ms`; replace `HOLD_MS` and the fixed fade step. Satisfies **AC-8**, **AC-10**.
5. Copilot switch: skip the Copilot handling in `keys::handle` when `copilot_as_right_ctrl` is false. Satisfies **AC-9**.
6. Shortcuts: parse combos (drop bad entries, defaults when empty or when `previous` and `next` clash), and replace the hard coded arrow match in `keys::handle` with an exact match against both lists, sending `VK_MASK` for Win or Alt combos. Satisfies **AC-5**, **AC-6**, **AC-7**.
7. Try it by hand on your screens with the scenarios above, then run `install.ps1`. Satisfies **AC-1** to **AC-13**.

## Consequences

**Positive**:
- You change the count, keys and timing without a rebuild; `install.ps1` only copies the exe, so your file survives updates.
- One function per key keeps every problem message specific.
- The project gets its first unit test, guarding the one place two copies of the defaults could drift apart.

**Negative / tradeoffs**:
- First dependency besides `windows`: `toml` and `serde` add a few crates and some clean build time.
- Changes need a restart; there is no live reload.
- `cargo run` reads `target\debug\settings.toml`, a different file from the installed copy's.
- Mistakes only show in `pdesk.log`; nothing pops up.
- Exact modifier matching changes today's behavior: `Win+Ctrl+Shift+Left` no longer switches.
- Dropping `Win+Ctrl+Left/Right` from the lists silently breaks the touchpad swipe until the Windows gesture is changed too.

**Neutral**:
- `state.txt` keeps its format; a smaller count only changes how `load` treats old lines.

## Follow-up

- [ ] For `/sync`: AGENTS.md needs `src/settings.rs`, the settings path, `cargo test` (tests now exist), and a `rust-serde` bullet under `## Agent skills` noting that this spec deliberately departs from its fail whole read advice.
- [ ] Feature 8 (tray icon): its "Open settings" item opens this file (since spec 0012, the tray's Settings opens the settings page, which has an Open settings.toml button).
- [ ] Features 4, 5 and 6 add their own lists under `[shortcuts]` with the same combo syntax and the same clash rule across all actions.
