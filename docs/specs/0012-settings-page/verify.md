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
