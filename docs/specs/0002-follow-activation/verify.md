# Verify: Follow activation · spec 0002 · updated 2026-10-26
_Steps derived from spec 0002 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

Before you start: stop the installed copy (`& "$env:LOCALAPPDATA\des-k\des-k.exe" --quit`), then `cargo run` so the log prints in the console. Set Notepad to open files in its existing window (Notepad Settings, Opening files).

## UI / manual
- [ ] Notepad hidden on workspace 1, switch that screen to 2, open a `.txt` from Explorer → the screen jumps to 1, Notepad shows the new tab, is on top and has focus; log has `follow: DISPLAYn -> workspace 1/4 for "..."` → AC-1
- [ ] Same run → the indicator pops up on that screen showing workspace 1 → AC-2
- [ ] Same run, with other screens on other workspaces → only Notepad's screen changed, the pointer did not move → AC-3
- [ ] A window hidden on another screen's workspace, Alt+Tab to it (if listed) → that screen jumps, not the one under the mouse → AC-4
- [ ] Type in a Notepad tab, switch away, run `taskkill /im notepad.exe` (no `/f`) → the screen jumps to Notepad's workspace with the "save changes?" dialog in front and focused → AC-5
- [ ] AC-6 has no reliable manual trigger: check in review that only `EVENT_SYSTEM_FOREGROUND` is hooked → AC-6
- [ ] Open two `.txt` files from Explorer at once while Notepad is hidden → one jump, one `cooling down` log line; a third open after a second jumps normally → AC-7
- [ ] With a window focused on the screen, swipe back and forth quickly ten times on a screen with several windows → every swipe lands where asked, nothing bounces; any `just left workspace` line means the guard caught one → AC-8
- [ ] Set `follow_activation = false` in `target\debug\settings.toml`, restart → the Notepad case no longer jumps, and no hook is installed → AC-9
- [ ] Set `follow_activation = "yes"`, restart → it jumps, and the log has a `settings: follow_activation: expected true or false` line → AC-9
- [ ] After a follow jump, `taskkill /f /im des-k.exe`, start again → each screen comes back on the workspace you saw, same windows hidden → AC-10
- [ ] A window on another Windows virtual desktop gets activated (switch Windows desktops with Win+Ctrl+D, then back via Alt+Tab or taskbar) → des-k logs no `follow:` line → AC-11

## Value sourcing (one step per source)
- [ ] Which window: click inside a visible app's child control → no `follow:` line (only `OBJID_WINDOW` / `CHILDID_SELF` events post) → AC-11
- [ ] Still current: swipe away from a focused window → no follow back (des-k's own focus change is skipped or stale) → AC-8
- [ ] Which tracked window: the `taskkill` dialog case above finds the owner through `GA_ROOTOWNER` → AC-5
- [ ] Target screen and workspace: hide Notepad on screen A workspace 3, put the mouse on screen B, open a `.txt` → screen A goes to 3, screen B untouched → AC-3
- [ ] `HMONITOR` for the screen: undock with a window hidden on the external screen → windows are shown by the display change handler, no crash; a `not found` line is fine → AC-11
- [ ] Still hidden: Alt+Tab to an already visible window → no `follow:` line → AC-11
- [ ] Bounce guard: covered by the swipe step → AC-8
- [ ] Cooldown: covered by the two files step; jumps on a different screen within the second are not blocked → AC-7
- [ ] Enabled: covered by the two settings steps → AC-9
- [ ] Window to focus: in the dialog case the dialog, not Notepad, has the caret → AC-5
- [ ] Window title in the log: a window with no title logs `(no title)` → AC-1

## Commands
- [ ] `cargo test` → 5 passed, including `follow_activation_can_be_turned_off` and `a_bad_value_only_resets_itself` → AC-9
- [ ] `cargo clippy` → no warnings → all

## Acceptance-criteria coverage
- AC-1 happy path · AC-2 indicator · AC-3 one screen only · AC-4 Alt+Tab · AC-5 dialog · AC-6 review only · AC-7 two files · AC-8 swipes · AC-9 settings + tests · AC-10 hard kill · AC-11 other desktop, still hidden
