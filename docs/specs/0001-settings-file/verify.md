# Verify: Settings file · spec 0001 · updated 2026-10-26
_Steps derived from spec 0001 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

Before you start: edit the installed copy's file, `%LOCALAPPDATA%\des-k\settings.toml`. Settings are read once at start, so after every edit restart des-k with a hard stop so your layout survives. des-k runs as admin since spec 0008, so do it from an admin terminal: `Stop-Process -Name des-k -Force; Start-ScheduledTask des-k`. Do not use `--quit` between tests: it shows every hidden window, so everything lands on workspace 1. Problems only show in `%LOCALAPPDATA%\des-k\des-k.log`.

## UI / manual
- [x] Delete or rename `settings.toml`, start des-k → the commented default file appears, log has `settings: wrote defaults to ...`, everything behaves as before (done 2026-10-26 at install) → AC-1
- [ ] Edit the file, restart a few times → your edits are never rewritten or reformatted → AC-2
- [ ] `workspaces = 3`, restart → every screen shows 3 dots, and switching stops at workspace 3 → AC-3
- [ ] Hide a window on workspace 4 (count 4), set `workspaces = 2`, restart → that window is visible; log says `N shown again` → AC-4
- [ ] Add `"Alt+Ctrl+Left"` to `previous`, restart → both previous combos switch the screen under the mouse, and the touchpad swipe still works → AC-5
- [ ] Press `Win+Ctrl+Shift+Left` → des-k does not switch, and Windows gets the key → AC-5
- [ ] Remove `Win+Ctrl+Right` from `next` (put another combo there), restart, press `Win+Ctrl+Right` → Windows handles it (its own desktop switch), des-k does not swallow it. Put it back afterwards → AC-5
- [ ] `previous = ["Alt+Ctrl+Left", "Ctrl+Banana"]` → log drops `Ctrl+Banana`, and `Alt+Ctrl+Left` still works. `previous = ["Ctrl+Banana"]` → log says no valid entry left, and the default is used → AC-6
- [ ] Put `"Win+Ctrl+Right"` in both lists → log names the clash, both actions use their defaults → AC-7
- [ ] `[indicator] enabled = false` → no popup on switch → AC-8
- [ ] `hold_ms = 2000`, `fade_ms = 0` → popup stays about 2 s and disappears at once; `fade_ms = 2000` → a slow fade → AC-8
- [ ] `copilot_as_right_ctrl = false` → the Copilot key does what Windows normally does with it → AC-9
- [ ] `workspaces = 12` and `hold_ms = "long"` → both use defaults, the log names each key, problem and default, other settings still apply → AC-10
- [ ] `workspaces = = 4` → des-k starts on all defaults, log shows `not valid TOML` with the line number → AC-11
- [ ] `typo = 1` → log says `unknown key typo, ignored` → AC-12
- [x] Start des-k → log shows the settings path and every effective value (seen 2026-10-26 at install) → AC-13

## Value sourcing
- [ ] Settings path is the data folder: the log line of `cargo run` and of the installed copy both name `%LOCALAPPDATA%\des-k\settings.toml` → load: file path
- [ ] Held modifiers: hold Copilot (remapped to Right Ctrl) plus Win plus Left → counts as Win+Ctrl+Left and switches → hook match: held modifiers
- [ ] A combo in `previous` goes back one workspace, a combo in `next` goes forward → hook match: direction
- [ ] `hold_ms = 0` → popup still flashes briefly (Windows' 10 ms minimum timer), no hang → indicator: hold
- [ ] With a saved active workspace of 4, set `workspaces = 2` → that screen starts on workspace 1 → load (state): saved active ≥ count

## Commands
- [ ] `cargo test` → 5 passed (default file equals defaults, per key fallback, clash, combo syntax, follow switch) → AC-1, AC-6, AC-7, AC-10, AC-11
- [ ] `cargo build` and `cargo clippy` → no warnings → all

## Acceptance-criteria coverage
- AC-1 install step + `cargo test` · AC-2 restart step · AC-3 workspaces = 3 · AC-4 shrink step · AC-5 three shortcut steps · AC-6 Banana step · AC-7 clash step · AC-8 two indicator steps · AC-9 Copilot step · AC-10 12 / "long" step · AC-11 `= =` step · AC-12 typo step · AC-13 start log
