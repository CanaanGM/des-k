# Verify: settings page · spec 0012 · 2026-11-01
_Steps derived from spec 0012 acceptance criteria. You run the on screen steps; `cargo test` covers the file edits._

## UI / manual
- [ ] Tray › Settings → the page opens on the screen under the mouse; Settings again brings it to the front; the tray has no "Open settings" → AC-1
- [ ] Every setting from settings.toml is on the page; each side list name jumps to its section → AC-2
- [ ] Turn the dots off → the file changed on that one line, comments intact; the next switch shows no dots, no restart → AC-3, AC-4
- [ ] Restore says "applies from pdesk's next start", the other sections "applies now" → AC-4
- [ ] Put a window on workspace 4, set `workspaces = 3` by hand in the file → within 2 s it's on workspace 3; the page shows 3; the log says `file` → AC-5, AC-6, AC-15
- [ ] Record a shortcut for next: Win+Ctrl+N → nothing fires and Start doesn't open while recording; the chip shows it; Esc cancels a recording → AC-7, AC-8
- [ ] Record Win+F on next → the prompt names Finder; Move it here → Finder has no chip; Cancel → nothing changes → AC-9
- [ ] Step hold_ms up and down with − + and arrow keys; it stops at 0 and 5000; "default: 550" and reset show when changed → AC-10, AC-11
- [ ] Change the 4 finger tap to Key combo… Ctrl+Shift+T, add a twice action to 3 finger tap → both work on the touchpad; the file has the full gesture list → AC-12
- [ ] Welcome › Choose my own → the page opens at Touchpad; Touchpad setup on the page opens the welcome screen → AC-13
- [ ] Put `hold_ms = "x"` in the file → the page shows 550 with the problem note; save a broken file → pdesk keeps working, the page is read only with the parser line → AC-14
- [ ] Keyboard only through the page: Tab between the side list and the settings, Up and Down row to row, Left and Right change values, Space flips a switch; Esc closes; switch light, dark and contrast themes; drag to another scale → AC-16

## Commands
- [ ] `cargo test` → all pass, including the in place edit, gesture rewrite, clash and bad reload tests → AC-3, AC-9, AC-12, AC-14, AC-17

## Steps 5 and 6, added 2026-11-02 (shortcut recorder, gesture editor)
- [ ] While recording, press Win+; and a numpad digit → "This key can't be saved; try another", still recording; press F alone → asks for a modifier, still recording → AC-7
- [ ] While recording, Ctrl+Esc → recorded as Ctrl+Escape; Esc alone → cancels; a click on the page or on another window → cancels → AC-7
- [ ] With `copilot_as_right_ctrl` on, record Copilot+F → the chip reads Ctrl+F, not Win+Shift → AC-7
- [ ] Hold Win before clicking + Add, then press F → Win+F is recorded, and letting go of Win opens no Start menu → AC-7
- [ ] × on Finder's only chip → the file says `finder = []`, Win+F does nothing, and the log names no problem; × on jump → `jump = ""` → AC-8
- [ ] Record Win+3 for Finder → the prompt says Go to workspace skips 3; Move it here → the jump chip reads "Win + 1…9, skips 3 (Finder)", Win+3 opens the finder, Win+4 still jumps → AC-8, AC-9
- [ ] Record Win+Shift on Go to workspace → the prompt names Send window to 1 to 9, and go; Move it here → that set is off, in one write → AC-9
- [ ] 4 fingers, swipe left: set Twice to Overview → the file gets a `times = 2` block and keeps the comment above the first block; one swipe now waits, two open the overview; set it back to Not set → the block goes → AC-12
- [ ] Hand delete the 3 finger tap block → its row shows Nothing and "not in the file"; pick Nothing → the file gets `do = "none"` for it → AC-12

## Commands
- [ ] `cargo test render_settings -- --ignored` → target/settings/*.png show the recording, refused, digit and clash states in light and dark → AC-7, AC-9

## Step 4b, added 2026-11-02 (pictures, live resize, Play, banner, touchpad status, scenes)
- [ ] Open Settings → Workspaces shows one picture per screen, numbered left to right, headed "Screen N · name · width × height · scale%"; the laptop panel reads "Built-in display"; each tile shows its windows where they sit, "now" on the one in view, and the right window counts → AC-18
- [ ] With the page open, switch a workspace or move a window on any screen → its picture follows within about 1 s → AC-18
- [ ] Hover × on the last tile → a note lists, per screen, what moves where ("Screen 2: 3 windows from 4 move to 3"), or "nothing moves" → AC-6
- [ ] Put windows on workspace 4 of a screen that shows 2, click × → they are hidden on workspace 3 there; on a screen that showed 4, it now shows 3 with those windows in view → AC-6, AC-10
- [ ] + Add → every screen gets an empty last workspace; earlier moved windows stay where they went; at 9 it reads "9 max" → AC-6, AC-10
- [ ] Keyboard: focus the pictures, Right adds one, Left removes one, Delete removes one; hold Delete → only one goes → AC-10, AC-16
- [ ] With 6 workspaces, hover or focus Reset → the note says Reset removes workspaces 5 to 6 and what moves → AC-6, AC-11
- [ ] Set `workspaces = 3` in settings.toml by hand with the page open → a banner "settings.toml changed at HH:MM and is in use:" lists workspaces 4 → 3 and the move lines; Dismiss hides it; a change made on the page shows no banner → AC-5, AC-20
- [ ] Indicator › Play → the real dots show on the page's screen with your hold and fade; with the dots off, Play is greyed and does nothing → AC-19
- [ ] Touchpad opens with a status: all of Windows' gestures Nothing and reading on → "pdesk reads your three and four finger gestures"; turn on a 3 finger swipe in Windows' Settings → within about 1 s "pdesk is waiting: Windows' own 3-finger swipes are still on" → AC-20
- [ ] Turn Read touchpad gestures off → swipes do nothing and the status says reading is off; turn it on → swipes work again, no restart (also when pdesk started with it off) → AC-4, AC-20
- [ ] The other touchpad states: reading off and Windows' gestures Nothing → "No three or four finger gesture does anything now"; reading off and a Windows gesture back on → "Reading is off here, and Windows' own gestures are on". (No touchpad and "can't read a setting" are hard to reach on the laptop; skip them unless you have a way.) → AC-20
- [ ] Shortcuts shows three scenes (Switch, Send and go, Send and stay) with your combos under them; change one → its scene shows the new combo → AC-21
- [ ] Overview's picture: pick Always, On hover, None → the names in the picture follow; the line under it says at once or fades in → AC-21
- [ ] Open the overview (Win+Tab), then save a hand edit → the overview closes → AC-5

## Commands
- [ ] `cargo test` → the three resize tests pass (lowering keeps every window, the shown removed workspace switches, raising adds empty ones) → AC-6
- [ ] `cargo test render_settings -- --ignored` → target/settings/pictures-light.png and pictures-dark.png show two screens, the removal note, the banner and the waiting status → AC-6, AC-18, AC-20
