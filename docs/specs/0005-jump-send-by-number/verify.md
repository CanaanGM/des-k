# Verify: Jump and send by number · spec 0005 · updated 2026-10-28
_Steps derived from spec 0005 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual
- [ ] Mouse on a screen showing 1, press Win+4 → it shows 4 directly, the indicator shows 4, other screens unchanged → AC-1
- [ ] Press Win+4 again → only the indicator pops up → AC-2
- [ ] With 4 workspaces, press Win+7, Win+Shift+7, Win+Ctrl+7 → nothing changes, no taskbar app opens, one `no workspace 7 (4 workspaces)` log line each (N is 1 based) → AC-3
- [ ] Notepad focused on 1, press Win+Shift+3 → the screen shows 3, Notepad on top and focused, same size and place; press it again → only the indicator → AC-4
- [ ] Notepad focused on 1, press Win+Ctrl+4 → Notepad hides, the next window gets focus, the dots show 1 with 4 in the accent color → AC-5
- [ ] Win+Ctrl+1 on a window already on 1 → a `send: "<title>" already on <device> workspace 1/4` log line → AC-5
- [ ] Win+Ctrl+3 then Win+Ctrl+4 within a second → the same window ends on 4, the window that got focus keeps it → AC-6
- [ ] Win+Ctrl+3 then Win+Ctrl+1 (the shown workspace) quickly → the window is straight back, focused → AC-6
- [ ] Win+Ctrl+Alt+Right then Win+Ctrl+4 quickly → the same window ends on 4 (arrows and numbers share the quick fix window) → AC-6
- [ ] Win+Ctrl+3, wait more than 1 second, Win+Ctrl+4 → acts on the focused window, not the one sent before → AC-6
- [ ] Focus a window on screen A, mouse on screen B: Win+Ctrl+3 changes only A; Win+3 changes only B; the pointer never moves → AC-7
- [ ] Send a dialog with Win+Ctrl+3 → its top owner moves → AC-7
- [ ] Focus the desktop, press Win+Ctrl+3 → nothing changes, a log line says why → AC-7
- [ ] After a numbered send, hard kill des-k and start it → `state.txt` kept the window on its new workspace → AC-7
- [ ] Hold Win+Shift+3 for two seconds, letting go of Shift first → one send, no jump → AC-8
- [ ] Hold Win+3 → the screen stays on 3, repeats only show the indicator again → AC-8
- [ ] External keyboard, Num Lock on: Win+Numpad 2 jumps, Win+Ctrl+Numpad 2 sends, Win+Shift+Numpad 2 does nothing in des-k → AC-9
- [ ] `jump = ""`, restart → Win+3 reaches the taskbar again (opens pinned app 3) → AC-10
- [ ] `send_to = "Shift"`, restart → a log line, Win+Shift+3 still sends and follows (default) → AC-10
- [ ] `next = ["Win+2"]`, restart → Win+2 switches next, a `skips digit 2` log line, Win+3 still jumps → AC-10
- [ ] Delete `settings.toml`, restart → the new file it writes has `jump = "Win"`, `send_to = "Win+Shift"`, `send_to_stay = "Win+Ctrl"`, and the log line shows them → AC-10

## Value sourcing
- [ ] Which set: press Win+Shift+2 and Win+Ctrl+2 on the same window → one follows, the other stays (each set maps to its own action) → AC-4, AC-5
- [ ] Index: Win+1 and Win+Numpad 1 both land on workspace 1, Win+4 on 4 (not off by one) → AC-1, AC-9
- [ ] Explicit lists win: `next = ["Win+2"]` → Win+2 steps next, not jump to 2 → AC-10
- [ ] First press or repeat: covered by the hold step above → AC-8
- [ ] Jump screen: mouse on a screen without the focused window, Win+3 → the screen under the mouse switches → AC-7
- [ ] Workspace count: set `workspaces = 3`, restart, Win+4 → `no workspace 4 (3 workspaces)` → AC-3
- [ ] Current workspace: Win+N on the shown N → indicator only → AC-2
- [ ] Send window, screen, from: covered by the AC-4, AC-5 and AC-7 steps above → AC-7
- [ ] Target: Win+Ctrl+3 sends to 3, Win+Ctrl+Alt+Right steps one → AC-5, AC-6
- [ ] Repeat window and its 1 second: covered by the AC-6 steps above → AC-6
- [ ] Too high and already there log lines: covered by the AC-3 and AC-5 steps above → AC-3, AC-5

## Commands
- [ ] `cargo test` → 24 passed (default file is the defaults, `""` off, bad values, both clash rules, list on a digit) → AC-10
- [ ] `powershell -File install.ps1`, then `Get-Process des-k` → des-k.exe is running → AC-1 to AC-10

## Acceptance-criteria coverage
- AC-1 jump, index · AC-2 same workspace · AC-3 too high, workspace count · AC-4 send and follow · AC-5 send and stay, already there · AC-6 quick fix · AC-7 screens, dialogs, nothing focused, `state.txt` · AC-8 hold · AC-9 numpad · AC-10 settings, defaults, `cargo test`
