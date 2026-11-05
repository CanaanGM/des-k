# Verify: return windows to their screens · spec 0016 · updated 2026-11-04
_Steps derived from spec 0016 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones. Every UI step is yours to run on your own screens; the log is Tray → Open log._

## UI / manual
- [ ] Start des-k docked: the log lists every screen as `screen <id>: DISPLAYn "name"`, the laptop as `internal`, each external as maker, model and serial → AC-1, AC-17
- [ ] Plug one external into another port or dock, restart des-k: the log shows the same id for it → AC-1
- [ ] Put windows on workspaces 1 to 3 of each external, show workspace 2 on one. Unplug both externals and wait 10 s: hidden windows land hidden in the same numbered workspace of the main display; workspace 2's windows show there if the main display shows 2; the windows you were looking at stay in view. The indicator says "<name> unplugged: N windows moved here" and a balloon says the same → AC-2, AC-3, AC-11, AC-13
- [ ] Plug both back in: about 2 s later every window is back on its screen, workspace, place, size, maximized or minimized; each screen shows the workspace it showed before; focus and the other screens do not change. The note says "<name> is back: N windows returned" → AC-5, AC-13, AC-14
- [ ] Unplug for under 10 s, then plug back in: nothing folds; windows Windows moved during the drop go back to their spots → AC-4
- [ ] While undocked, send one folded window to laptop workspace 3 and drag another within the laptop. Dock: the first stays on workspace 3 (the log says "stays where it is ... you sent it"), the second goes home → AC-6
- [ ] Unplug and just wait: the log never says "you moved it" → AC-6
- [ ] Pin a window on an external, unplug: it stays pinned on the laptop; dock: it goes back pinned. Pin or unpin another while away: it stays where it is → AC-7
- [ ] Undock, Quit from the tray, start des-k again, dock: the windows go home → AC-8
- [ ] Undock, close one away window and move all others yourself: that screen is forgotten in the log and leaves the settings page list → AC-8
- [ ] Undock, open Settings → Workspaces: "Screens not plugged in" lists each external with its count; Forget one: its windows stay where they are and the next dock moves nothing for it → AC-9
- [ ] Undock, pull the power (or hard reset), boot on the laptop only, let apps reopen: matched windows land on the main display's workspace N (or stay in view); dock an hour later: they go home → AC-8, AC-10
- [ ] Freeze an app (pause it in a debugger), undock and dock: des-k keeps switching; after 10 s the log and the note say "1 could not be moved"; unfreeze, undock and dock again: it goes home → AC-12
- [ ] With 4 workspaces, undock, lower the count to 2 in Settings: away windows of workspaces 3 and 4 go home to workspace 2 when you dock → AC-16
- [ ] Sleep docked, wake with externals that take over 10 s to light up: nothing folds (wait under 30 s) → AC-18
- [ ] Close the laptop lid while docked, wait, open it: the laptop's windows fold to the main display and come back → AC-1, AC-2, AC-5

## Value sourcing
- [ ] Two monitors of the same model with no serial: ids end `#1` and `#2`, left to right; swap them in Display settings and the ids swap → AC-1 (screen id)
- [ ] Change the external's resolution while it is unplugged elsewhere (or move its taskbar), dock: windows land at the same relative spot → AC-5 (return spot when the area changed)
- [ ] Make another screen the main display before undocking: the windows fold onto that screen → AC-2 (fold target)
- [ ] Unplug the main display itself: its windows and its away windows go to the new main display; an away window keeps its first home → AC-11
- [ ] A snapped half window goes back as the same half, a maximized one maximized, a minimized one minimized → AC-5 (spot state)

## Commands
- [ ] `cargo test` → all pass, including the spec 0016 tests (EDID ids, twins, upgrade, A lines, rescue, claims, spot freeze, retries, wake grace) → AC-1, AC-6, AC-8, AC-12, AC-15, AC-18
- [ ] `cargo test render -- --ignored` → `target/settings/pictures-*.png` show "Screens not plugged in" with two screens and Forget → AC-9
- [ ] Copy an older `state.txt` (DISPLAYn names) in while docked, start des-k: no error, the layout loads, the file is rewritten with ids → AC-15

## Acceptance-criteria coverage
- AC-1 ids, ports, twins, lid · AC-2 fold · AC-3 in view · AC-4 short drop · AC-5 return · AC-6 your moves · AC-7 pinned · AC-8 memory, quit, forget by closing · AC-9 settings list · AC-10 after a power cut · AC-11 several screens, main display · AC-12 frozen app · AC-13 note and balloon · AC-14 focus · AC-15 upgrade · AC-16 fewer workspaces · AC-17 log · AC-18 wake grace
