# Verify: touchpad gestures · spec 0011 · updated 2026-11-03
_Steps derived from spec 0011 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual (welcome screen, build step 5)
- [ ] Stop the installed des-k (`& "$env:ProgramFiles\des-k\des-k.exe" --quit`), `cargo build`, start `target\debug\des-k.exe`; tray › Settings › Touchpad › Touchpad setup → the screen opens centered on the screen under the mouse, as tall as its content but never taller than the screen; the tray menu has only Settings, Open log and Quit, and Quit's label says what happens to the gestures → AC-8, AC-9
- [ ] On a screen too short for it (a laptop at 150%) → the band stays at the top, the buttons and footer stay pinned at the bottom, the middle scrolls with the wheel and Page Up / Page Down, and a thumb shows where you are (0011-first-run-setup, Change of 2026-11-03)
- [ ] With every three and four finger gesture set to Nothing in Settings → four "✓ Windows: Nothing" chips, "4 of 4 off", "des-k runs your gestures" (each row comes from its own registry values, the Rows table) → AC-9
- [ ] Set Four-finger Taps back to an action in Settings, return to the screen → that row reads "Windows: still on", "3 of 4 off", the "Set up in Windows Settings" button shows (refresh when the window gets focus back) → AC-9
- [ ] The screen says in plain lines what des-k is, and each of the four rows shows what des-k does for it (4 finger swipes: ← next workspace, → previous workspace, ↑ overview, ↓ show desktop), with "Under each gesture: what des-k runs for it. Choose my own changes them, and changes apply at once." ABOVE the rows (unticked 2026-11-03: text and place changed) → AC-8
- [ ] Turn Read touchpad gestures off on the settings page, then give gestures back → in both, the four rows and the "n of 4 off" count still show, with a caption saying des-k runs none of them → AC-9
- [ ] Keyboard only: Tab goes Set up in Windows Settings › Choose my own › Close; Enter presses the focused button; Esc closes → AC-8
- [ ] Mouse: "Choose my own" opens des-k's settings page at Touchpad (and Windows Settings at Touchpad while gestures are still on); "Set up in Windows Settings" opens Settings at Touchpad (unticked 2026-11-03: button renamed) → AC-8
- [ ] Switch light and dark (Settings › Personalization › Colors), then a contrast theme → the screen repaints each time → AC-11
- [ ] Drag the screen to a monitor at another scale → it resizes and stays sharp

## UI / manual (installed des-k, build step 6)
- [ ] Focus Task Manager, four finger swipe left then right → one workspace switch per swipe, the log has one line per gesture naming its action → AC-1, AC-13
- [ ] Try each gesture in the default map (4 and 3 finger left, right, up, down, 3 and 4 finger tap) → each runs its action; two finger scroll, zoom and right click still behave as before → AC-2 to AC-7, AC-11
- [ ] Plug in or pair a USB or Bluetooth touchpad (or remove one) with des-k running → the log names it, its swipes work, and an open Touchpad setup screen refreshes, all without a restart → AC-12
- [ ] Tray › Quit, start des-k again without signing out → gestures still work through des-k; with values saved, Quit, sign out and in → Windows' own three finger swipe works again → AC-10
- [ ] With values saved: Touchpad setup › Give gestures back to Windows → each row says it was written back, the next swipe logs as skipped (des-k stopped reading), Settings opens at Touchpad → AC-9
- [ ] With the screen open, click Touchpad setup on the settings page → the same screen comes to the front, no second one; minimize it and click again → it is restored → AC-9

## Commands
- [ ] `cargo test` → all pass, including the rows and give back result tests in `setup.rs` → AC-9
- [ ] `cargo test render -- --ignored` → PNGs of every state in `target/welcome/`, matching `pdesk-design/welcome/micro-suite-r4.html` → AC-9

## Acceptance-criteria coverage
- AC-8 … steps 1, 4, 5 · AC-9 … steps 1, 2, 3, the give back step, the bring to front step and both commands · AC-11 … step 6 and the gesture map step
- Step 6: AC-1, AC-13 … Task Manager step · AC-2 to AC-7 … gesture map step · AC-10 … Quit step · AC-12 … touchpad plug step
