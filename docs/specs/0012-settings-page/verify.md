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
