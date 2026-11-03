# Verify: Send window to workspace · spec 0004 · updated 2026-10-27
_Steps derived from spec 0004 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

Before you start: stop the installed copy (`& "$env:LOCALAPPDATA\des-k\des-k.exe" --quit`), then `cargo run` so the log prints in the console. Have Notepad and one or two other windows open on one screen.

## UI / manual
- [ ] Notepad focused on workspace 1, Win+Ctrl+Shift+Right → the screen shows 2, Notepad is on top, focused, same size and place, the other windows of 1 are gone, the indicator shows 2; log has `send: "..." DISPLAYn workspace 1/4 -> 2/4, follow` → AC-1
- [ ] Same, Win+Ctrl+Shift+Left → back on 1 with Notepad → AC-1
- [ ] Notepad focused on 1, Win+Ctrl+Alt+Right → Notepad and its taskbar button go, the next window on that screen has focus (the desktop if none), the indicator shows 1 with the 2 dot in blue; switch to 2 → Notepad is there on top → AC-2
- [ ] On the last workspace, Win+Ctrl+Shift+Right and Win+Ctrl+Alt+Right both send to 1; on 1, both "Left" shortcuts send to the last → AC-3
- [ ] Focus a window on screen A, mouse on screen B, send with each pair → only A changes, the pointer does not move → AC-4
- [ ] Win+Ctrl+Alt+Right twice quickly → the same window ends two workspaces over, the window that got focus in between stays put → AC-5
- [ ] Win+Ctrl+Alt+Right four times quickly (4 workspaces) → the window comes back visible, on top and focused; the indicator has no blue dot that time → AC-5
- [ ] Win+Ctrl+Alt+Right then Win+Ctrl+Alt+Left quickly → the window is straight back → AC-5
- [ ] Stay send, wait more than a second, stay send again → the second press sends the window that has focus now → AC-5
- [ ] Hold Win+Ctrl+Shift+Right for two seconds → exactly one step; hold it again and let go of Shift first → still no extra switch → AC-6
- [ ] Notepad's Save As dialog open, Win+Ctrl+Alt+Right → Notepad goes, the dialog stays visible (if Windows hides it with its owner, note it in the deferred owned dialogs item) → AC-7
- [ ] Click the desktop, press each send shortcut → nothing moves, `send: nothing to send ("...", Progman)` (or `WorkerW`), no Start menu; same on the taskbar (`Shell_TrayWnd`) → AC-8
- [ ] AC-9 has no reliable manual trigger: check in review that the failure path in `ws::send` changes no list and undoes a half done cloak → AC-9
- [ ] Stay send a window, `taskkill /f /im des-k.exe`, start again, switch to the target workspace → the window is there; `state.txt` had a `W` line for it → AC-10
- [ ] A few stay sends in a row, watch the log → any `follow: just sent "...", ignored` line means the guard caught a focus bounce, and the screen never jumped after the window → AC-11
- [ ] Set `send_next = ["Win+Alt+N"]` in `target\debug\settings.toml`, restart → Win+Alt+N sends with follow, Win+Ctrl+Shift+Right does nothing → AC-12
- [ ] Set `send_next_stay = ["Win+Ctrl+Right"]`, restart → log has `shortcuts: Win+Ctrl+Right is in both next and send_next_stay, send_next_stay turned off`, Win+Ctrl+Right and the swipe still switch, the other send shortcuts still work → AC-12
- [ ] Optional: the Copilot key (as Right Ctrl) with Win+Shift+Right or Win+Alt+Right → note whether it sends (spec follow up) → AC-1, AC-2

## Value sourcing
- [ ] Window: focus a dialog, send → its owner moves, not the dialog → window = root owner of the foreground
- [ ] Screen: window on screen A, mouse on B → A acts → screen = `MonitorFromWindow`, never the cursor
- [ ] Target: with `workspaces = 3`, stay send from 3 with Right → lands on 1 → `(from + count + dir) % count`
- [ ] Repeat window: two quick stay presses with another window getting focus in between → the first window moves twice → `State.sent` under 1 second old
- [ ] Focus after stay: the only window on the screen sent away → the desktop gets focus → `app_windows_on(mon).first()`, else `GetShellWindow()`

## Commands
- [ ] `cargo test` → all pass, including `a_send_list_clashing_with_a_switch_list_is_turned_off`, `a_switch_list_beats_a_default_send_list`, `a_stay_list_clashing_with_a_follow_list_is_turned_off`, `a_bad_send_entry_only_resets_its_list`, `default_file_is_the_defaults` → AC-12
- [ ] `powershell -File install.ps1`, then `tasklist /fi "imagename eq des-k.exe"` → des-k.exe is running (the install race from feature F) → build plan task 4

## Acceptance-criteria coverage
- AC-1 steps 1, 2 · AC-2 step 3 · AC-3 step 4 · AC-4 step 5 · AC-5 steps 6 to 9 · AC-6 step 10 · AC-7 step 11 · AC-8 step 12 · AC-9 step 13 (review) · AC-10 step 14 · AC-11 step 15 · AC-12 steps 16, 17 and `cargo test`
