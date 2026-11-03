# Verify: Workspace overview · spec 0010 · updated 2026-10-30
_Steps derived from spec 0010 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual
Build step 2 (thin thread). All passed on your screens 2026-10-29.
- [x] With windows on two or three screens, press Win+Tab → every screen goes dark grey with numbered tiles on top and live previews below; Task View does not open → AC-1, AC-3, AC-13
- [x] Click a window in the spread → you land on it and the overview closes on every screen → AC-6
- [x] Open it, click another workspace's tile → that screen switches and the indicator shows → AC-6
- [x] Open it, press Esc; open again, click empty background → focus goes back to the window you came from → AC-6, AC-8 (Esc)
- [x] With it open, press Win+Ctrl+Right → the overview closes first, then the switch runs → AC-12
- [x] With it open, press Alt+Tab → the overview closes and Windows' Alt+Tab opens; no overview popup is listed → AC-12

Build step 3 (mini windows, placement, cards, labels). Passed on your screens 2026-10-29.
- [x] Windows on several workspaces, one maximized, one minimized; Win+Tab → each tile shows its windows live at their real places, stacked as on screen; the minimized one at its restored place; the maximized one does not spill onto the next tile; numbers sit under the tiles; an empty tile shows nothing inside → AC-2
- [x] In the spread, a window from the left of the screen is on the left and one from the bottom is in a lower row → AC-3
- [x] Hover a spread window → its title shows under it, cut with … when long, and goes when you move away → AC-5
- [x] Click a tiny window inside another workspace's tile, or on another screen → you land on it → AC-6
- [ ] (optional check, not confirmed yet) `[overview] labels = "always"` → icon and title under every spread window; `"none"` → no text; `"sometimes"` → hover, plus one log line → AC-5, AC-14
- [ ] A window Windows draws nothing for shows as a dark card with its icon and title (rare: step 1 found DWM draws des-k's hidden windows; check whenever one turns up) → AC-4

Build step 4 (keys, selection, closing). Close paths passed on your screens 2026-10-29; the Tab rework waits on more than one screen.
- [x] Hover a spread window → an X shows on its top right; click it → the window closes and drops out, the overview stays open → AC-9
- [x] Middle click a spread window, or select one and press Ctrl+W → it closes the same way → AC-9
- [ ] Arrows → the accent border moves to the nearest window that way; Enter → you land on it → AC-8
- [ ] Tab and Shift+Tab → the border walks this screen's windows in reading order and wraps around, never on to another screen; on an empty screen nothing happens → AC-8
- [ ] Move the mouse onto another screen → the border jumps to that screen's top left window → AC-8
- [ ] With the mouse on a screen, press a digit (even on an empty screen) → that screen switches to that workspace and the overview closes → AC-8
- [ ] (whenever it turns up) A window that refuses the close → the note "Can't close this window", and a log line → AC-9

Build step 5 (drag and drop). One screen checks passed on your screen 2026-10-30; the cross screen ones wait on a second monitor.
- [x] Drag a spread window a little way → a small live preview follows the mouse; a click without moving still just lands on the window → AC-7
- [x] Drop it on another tile of the same screen → it leaves the spread and shows in that tile; no screen switches; the overview stays open → AC-7
- [x] Drag a window out of a tile onto the spread → it shows in the spread, on top → AC-7
- [x] Drop on its own tile, on a side margin, press Esc mid drag, or right click mid drag → nothing moves → AC-7
- [ ] (needs two screens) Drop a window on another screen's hidden tile, then switch there → it sits at the same relative place and size; it never flashed on its old screen; a maximized one is still maximized → AC-7
- [ ] (needs two screens) Drop on another screen's spread → it shows there at once → AC-7
- [x] After a drop, rerun `install.ps1` alone (its hard kill keeps `state.txt`; `--quit` shows every window and clears the layout on purpose) → the window is still on the workspace you dropped it on → AC-7
- [ ] (whenever it turns up) A drop that fails → the note "Can't move this window" for about two seconds, the window stays put, and a log line says why → AC-7

Build step 6 (live refresh). Passed on your screen 2026-10-30.
- [x] With the overview open, start Character Map → it shows in the spread within about a quarter second, and the others move over → AC-10
- [x] With the overview open, close a spread window from outside (its taskbar entry, or a script) → it drops out and the spread rearranges → AC-10
- [x] Set `live = false` under `[overview]`, rerun `install.ps1`, open the overview, start Character Map → it does not show; close a window from outside → it drops out and the rest stay put → AC-10, AC-14
- [x] Leave the overview open and idle for a minute → it stays open (nothing closes it but you, a des-k shortcut or a screen change) → AC-10, AC-12
- [ ] Close Settings (a Store app) from the overview → it leaves the spread at once, with no stretched picture first (fix 2026-10-30) → AC-9
- [ ] Close an unsaved Notepad from the overview → it leaves, then comes back about 2 s later while it asks to save, and one `is still open` log line → AC-9

Build step 7 (animation). Built 2026-10-30, not tried yet. Set the style under `[overview]` in `settings.toml`, then rerun `install.ps1`.
- [ ] `animation = "fade"`: Win+Tab → the overview fades in over about 250 ms, and the previews fade with the grey, never popping in at full strength → AC-11
- [x] Fade: Esc → it fades out, and a click right away lands on the window beneath (passed on your screen 2026-10-30) → AC-11
- [x] Fade: press an arrow or Tab during the fade in → the selection moves (passed on your screen 2026-10-30) → AC-11
- [x] `animation = "fade"` is visible on your screen (2026-10-30, after the time went to 250 ms) → AC-11
- [ ] `animation = "zoom"` (from an older file) → one `overview.animation:` log line, and the overview opens at once → AC-14
- [x] Fade: Win+Tab twice fast (reopen during the fade out; passed on your screen 2026-10-30) → the old popups go at once and the new overview opens; nothing stays on screen after → AC-11
- [ ] `animation = "off"` → opens and closes at once, as before → AC-11

## Commands
- [x] `cargo test` → all pass (46 on 2026-10-30) → AC-2, AC-3, AC-7, AC-8, AC-14
- [x] `install.ps1` → log shows `shortcuts.overview=[Win+Tab]`, `overview.labels=hover` and `des-k running` → AC-1, AC-14
- [x] `cargo test` → all pass (50 on 2026-10-30, including `a_bad_overview_animation_only_resets_itself` and `an_animation_eases_out_and_ends_exactly_in_place`) → AC-11, AC-14
- [x] `install.ps1` → log shows `overview.animation=off overview.labels=hover overview.live=true` → AC-14
- [ ] `live = "no"` or `animation = "slide"`, rerun `install.ps1` → one `overview.live:` or `overview.animation:` log line, that key back to its default, the rest kept → AC-14

## Value sourcing
- [x] Open on three screens → each overview covers its own whole screen (`ws::screens()`) → AC-1
- [x] Tile click → the outlined tile matched the workspace the screen showed (`ws::active`) → AC-2 (outline only)
- [x] A window on the right half of the second screen sits in the right half of that screen's tile, not offset by the screen's position (`ws::real_rect` mapped into the tile) → AC-2
- [x] Hidden windows stack in their tile as they did on screen: the front one is on top (`ws::windows()` order, reversed) → AC-2
- [ ] Tab order on a screen with windows level with each other but a pixel apart → left one first, then right, then the next row (spread rects by center) → AC-8
- [ ] The selected screen is the one the mouse last moved over, not the one the overview opened on (`WM_MOUSEMOVE` on that popup) → AC-8
- [ ] A drop lands on the tile under the mouse, or in a spread on the workspace that screen shows (`drop_target`, then `ws::active`) → AC-7
- [ ] A drop across screens lands at the same relative place (`ws::place` through `ws::map_rect`) → AC-7
- [ ] Live refresh: a window that opens shows up without reopening the overview (`ws::windows()` again every 250 ms) → AC-10
- [ ] Animation: the style follows `settings.toml` after `install.ps1`, and the fade takes about a quarter second (fixed 250 ms) → AC-11

## Acceptance-criteria coverage
- AC-1, AC-2, AC-3, AC-5, AC-6, AC-12, AC-13 covered by the step 2 and 3 checks · AC-4 card coded, not yet seen · AC-14 shortcut and `labels` only (`live`, `animation`: steps 6, 7)
- AC-8, AC-9 by the step 4 checks (close paths passed) · AC-7 by the step 5 checks (not tried yet)
- AC-10 by the step 6 checks, AC-11 by the step 7 checks (both built, not tried yet) · AC-14 `live` and `animation` by the Commands checks
