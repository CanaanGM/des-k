# 0017. First run in look B

## Summary

The welcome screen becomes the approved first run: one column of steps with a thin purple progress line, where every setting des-k has can be changed on the spot and applies at once. It includes the keys des-k takes from Windows and the touchpad step. Everyone sees it once, including people upgrading, marked by `first_run_seen` in `settings.toml`. It is the last slice, reusing the settings page's pickers and key fields.

**Design source**: `pdesk-design/restart/first-run-r11.html` (r9.2 plus the brighter yellow), sha256 `5f760f27f80f14ea4f3b78d19de3531442dfda09d1e99b3b9c174bd7fe5f5da5`, captures `restart/captures/fr11-*` (sums `SHA256SUMS-fr11`). Approved 2026-11-04. Also:
- the indicator approval's More panel amendment (Style row, heading "Indicator style and timing, restore time, overview");
- the rows for `fastest` and `[overview] fade_ms` from the short design pass, once you approve it.

**Replaces**: `src/welcome.rs`'s screen and spec 0011's first run setup (0011-first-run-setup) where they differ, listed in AC-FR-5. Today's first start rule (no `touchpad.txt`, `setup::start()`) no longer opens it; `first_run_seen` does (index AC-9).

## Acceptance criteria

- **AC-FR-1**: It opens at start when `first_run_seen` is missing or false, on the screen under the mouse. des-k writes `first_run_seen = true` as it opens. Settings › Welcome reopens it at any time.
- **AC-FR-2**: Saved as you go: every choice applies at once, except Restore, which starts at des-k's next start (its row says so).
- **AC-FR-3**: Leaving:
  - Skip goes to Done.
  - Esc first cancels a recording, a clash or an open picker; after that it closes, like the title bar's X.
  - Closing keeps what is set.
- **AC-FR-4**: Keys:
  - Giving a key back takes that one Windows key from des-k, and the action keeps no key until you set one. Switched back, it gets its last key.
  - When an action has more than one combo, giving one back keeps the others.
  - Win Ctrl ← → records as a pair: hold the keys, press ← or →, and both arrows get them.
  - Lowering workspaces warns first with what moves, as the settings page does.
  - Done lists the main keys and the taken over line from live settings.
- **AC-FR-5** (updates spec 0011 first run setup):
  - Pickers replace "Choose my own".
  - Enter presses Next on the touchpad step too.
  - The new status words and "Try again".
  - A failed give back's result (some or every write failed) stays until a later give back replaces it or des-k exits. Unlike a successful one, it is not cleared when `windows_off()` returns `Ok` again. Meanwhile des-k still runs the gestures, since Windows' values still read off. This amends 0011 first run setup's rule (its line "The result stays in memory until…").
  - des-k runs gestures only while all four read Nothing.
  - Give back shows whenever values are saved and hides during Try again; after a give back, the rows read what was written.
  - With no precision touchpad, the step is left out.
  - The twice wait time comes from live settings.
- **AC-FR-6**: Keyboard:
  - Tab through every control, arrows inside a choice or a picker's menu.
  - Enter presses Next (the purple button) when focus is on no other button, and Next has first focus on each step.
  - While recording, every key is recorded except Esc alone.
  - Focus is a 2 dip text colored ring, 2 dip out.
- **AC-FR-7**: The window grows from its top edge between its 600 and 680 dip heights over 150 ms, through `ui::motion_ms` (instant with `fastest`, which also turns off Windows' own open animation on this window, index AC-6). The scrolling area has a 1 dip top inset.
- **AC-FR-8**: The More panel holds:
  - the indicator Style row;
  - indicator timing;
  - restore time;
  - the overview's values, with the rows for `fastest` and `[overview] fade_ms` from the design pass.
- **AC-FR-9**: Narrator reads every step's heading, each control with its value and state, and the progress (which step of how many, as the progress line shows).

## Build notes

- `first_run_seen` goes into `Settings`, `TOP_KEYS`, `DEFAULT_FILE` (`first_run_seen = false`), write values and a test. It also gets the insert when missing test from the index.
- `main.rs` opens the first run from `settings::get().first_run_seen` instead of `setup::start()`'s return. `setup::start()` keeps its logging.
- `welcome::show(first)`: `first` now means "opened at start because `first_run_seen` was false". It still picks the Skip label over Close. Skip no longer writes an empty `touchpad.txt` (`welcome.rs:770`): that only stopped the screen from opening again, which `first_run_seen` now does.
- If writing `first_run_seen` fails (a `settings.toml` that is not valid TOML), log it once and open anyway (index AC-9).
- Render tests: each step and state from the design's frames, light, dark and HC, 100% and 150%. Output goes to `target/welcome/` (today's folder).

## Check on the real machine

- After a give back, whether Windows Settings shows the gestures as on while they stay off until sign in (UX review M11).

## Rationale (short)

The first run is the biggest window and reuses the settings page's parts, so it comes last. The marker in `settings.toml` is what you chose so upgraders see it once. Writing it on open means a crash can never make it loop.
