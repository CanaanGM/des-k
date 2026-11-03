# Verify: Window finder · spec 0007 · updated 2026-10-28
_Steps derived from spec 0007 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual

Open, list, go
- [ ] Mouse on screen 1, press Win+F → the finder opens on screen 1, centered, top about a fifth down, caret in the box; no Feedback Hub, no Start menu; log `finder: opened on DISPLAYn, N windows` → AC-1
- [ ] Hide Notepad on screen 3 workspace 2, minimize another window, then Win+F → both listed; windows on another Windows virtual desktop, suspended Store apps and des-k's own windows are not → AC-2
- [ ] Type `note`, Enter → screen 3 switches to workspace 2 with Notepad focused and the indicator there; screens 1 and 2 unchanged; mouse did not move; `state.txt` M line for screen 3 shows the new workspace → AC-7
- [ ] Win+F on a minimized window that is on screen, Enter → it is restored and focused, no screen switches → AC-8
- [ ] Esc → finder closes, focus back where you were; Win+F twice → same; click on the desktop → closes, focus stays on the desktop; Win+Ctrl+Right while open → closes, then switches → AC-11

Rows and finding
- [ ] Each row shows an icon, program name (exe without `.exe`), title cut with … when long, and `S · W`; ● only on windows on screen now; a Store app shows its own exe, a suspended one `ApplicationFrameHost` → AC-3
- [ ] In Chrome, Win+F, pick Code, Enter; then Win+F, Enter → back in Chrome, from any screen. Set `follow_activation = false`, restart des-k, repeat → same → AC-4
- [ ] With nothing typed, the second row is selected; with one window only, the first → AC-4
- [ ] `vsc` puts Visual Studio Code first; `code des-k` finds the Code window with des-k in its title; `explorer` lists every File Explorer window; `zzzq` shows "No match" → AC-5
- [ ] Down, Tab and Ctrl+N move the same way and stop at the last row; Up, Shift+Tab, Ctrl+P stop at the first; with more than 9 windows the list scrolls → AC-6
- [ ] Ctrl+Backspace removes the last word; Left/Right, Home/End, Shift selection, Ctrl+V paste and IME input work in the box; a left click on a row acts like Enter → AC-6
- [ ] Close a listed window with the mouse while the finder is open, then Enter (and Shift+Enter, Ctrl+W, click) on it → "That window is gone", row drops, finder stays open → AC-12
- [ ] On the 150% laptop and a 100% screen the finder is sharp and the same size; Alt+Tab while open does not show it; it never disappears on a switch → AC-13

Shift+Enter and Ctrl+W
- [ ] Chrome filling the left half of the 150% laptop, mouse on an external screen, Win+F, Shift+Enter → it fills the left half there, focused; a maximized window stays maximized; a minimized one comes back restored → AC-9
- [ ] A window hidden on another screen, Shift+Enter → appears on your screen without flashing on its old screen; a window hidden on your own screen → appears at its own place; no screen switches → AC-9
- [ ] An admin terminal: Shift+Enter either moves it or shows "Can't move this window" with the window untouched (still hidden if it was); Ctrl+W shows "Can't close an admin window", row stays → AC-9, AC-10
- [ ] Ctrl+W on a Notepad with no changes → its row disappears, finder stays; with changes → the save prompt takes focus, finder closes; an app that closes to the tray → its row disappears → AC-10

Value sources (one per row of the spec's Value sourcing table)
- [ ] Set `finder = ["Win+G"]` and restart → Win+G opens it, Win+F opens Feedback Hub again → hook combo source
- [ ] Mouse on screen 2 while focus is on screen 1 → opens on screen 2 (mouse, not focus) → open screen
- [ ] On a screen with the taskbar at the top or left, the finder still sits a fifth down the work area → popup place
- [ ] Focus a dialog (Save As), Win+F, Esc → focus returns to the dialog; its owner counts as most recent → window you came from
- [ ] Rename a tab in Chrome while hidden, Win+F → the new title shows (read at open) → title
- [ ] An elevated window shows its program name; Calculator (Store) shows `CalculatorApp` → program
- [ ] A frozen app (Task Manager says Not responding) does not make the finder slow to open; it shows a generic or class icon → icon
- [ ] Rearrange screens in Display settings so the left screen changes → numbers follow left to right (then top) → screen number
- [ ] A window on screen shows the workspace its screen shows now; a hidden one the workspace it was hidden on → workspace number
- [ ] Minimized windows on screen still show ● → ●
- [ ] Use A, then B, then C (with follow off too), Win+F → C first, B second (selected), A third → recency
- [ ] Windows never used since start: on screen ones front to back, then hidden ones by screen, workspace → tie order
- [ ] `fire` with Firefox and a "fire.txt" Notepad: the closer letter match ranks first → score
- [ ] Shift+Enter with the finder opened on screen 2 → window lands on screen 2's shown workspace → Shift+Enter target
- [ ] Shift+Enter between screens of different scaling → same relative rectangle (map_rect) → new rect
- [ ] Ctrl+W on an admin window → note plus log line `can't close ... admin` → admin refusal
- [ ] Each note text appears exactly as written in the spec → notes
- [ ] `%LOCALAPPDATA%\des-k\des-k.log` has the `finder: opened`, `finder: ->`, `finder: brought`, `finder: closed` lines → log

## Commands
- [ ] `cargo test` → 34 passed, including the settings finder tests (AC-14), `map_rect` (AC-9), `recency_rank`, `rank`, `first_pick`, `refilter`, `screen_numbers`, `delete_word` (AC-3 to AC-6) → AC-3, AC-4, AC-5, AC-6, AC-9, AC-14
- [ ] Add `finder = ["Win+Ctrl+Right"]` to `settings.toml`, restart → log says finder turned off, Win+Ctrl+Right still switches → AC-14
- [ ] `install.ps1`, then `Get-Process des-k` → running → all ACs

## Acceptance-criteria coverage
- AC-1 open · AC-2 list · AC-3 rows, program, icon · AC-4 Alt+Tab habit, second row, recency · AC-5 fuzzy · AC-6 keys, box, click · AC-7 Enter hidden · AC-8 Enter on screen · AC-9 Shift+Enter steps · AC-10 Ctrl+W steps · AC-11 closing · AC-12 gone window · AC-13 look · AC-14 settings commands
