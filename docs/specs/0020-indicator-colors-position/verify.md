# Verify: Your colors and indicator position · spec 0020 · updated 2026-11-07
_Steps derived from spec 0020 acceptance criteria and its Value sourcing table. `/check verify` runs these; `/test` locks the durable ones. Try them on your own screens with the installed or a dev copy of des-k._

## UI / manual

Colors (Settings › Colors):
- [ ] With no `[colors]` in the file, change your Windows accent in Windows Settings with the settings page open → every open des-k window repaints in the new accent within about 1 s → AC-1
- [ ] Accent color › My own → nothing changes color; settings.toml gets `accent = "#…"` holding the accent in use → AC-12
- [ ] In Your accent, type `2E8B57`, press Enter → every des-k window (indicator, finder, overview, tray menu, settings) turns green, with readable black or white text on it → AC-2, AC-5, AC-12
- [ ] Accent color › Windows, then My own again (page still open) → it comes back as `#2E8B57`; the Windows caption says My own goes back to it → AC-12
- [ ] Your accent with a rose `#E0457B`, light and dark → headers turn red, badges purple; every Reset link reads clearly on its card → AC-3, AC-15
- [ ] Workspace 2: type `ABC`, then Tab away → warn edge and "Six digits are needed" line, the file is unchanged; Esc in the box puts back the saved color and keeps focus → AC-12
- [ ] Workspace 2: Ctrl V with `seagreen` on the clipboard → refused with a line, nothing changes; Ctrl V with ` #2e8b57 ` → the box shows `2E8B57` and the swatch follows → AC-12
- [ ] Workspace 2: type `DDB43C` (its default), Enter → the `ws2` line is gone from settings.toml → AC-12
- [ ] Workspace 5: type `FFF4B0`, Enter, open the overview and the Workspaces tiles → the window marks on that pale tile are dark and readable; the other tiles keep white marks → AC-16
- [ ] Workspace 3: Reset → the `ws3` key is removed and des-k's green is back everywhere → AC-12
- [ ] Put `ws3 = "blue"` in the file by hand → Colors is counted in the side list, the row shows the log line; typing a color or Reset fixes the file → AC-11
- [ ] Turn on Windows high contrast → des-k ignores your accent and workspace colors; Colors says why, the boxes and swatches are greyed and blank, Reset still works → AC-6
- [ ] The tray icon and the welcome screen's mark keep their own purple and red with any accent → AC-14

Where it shows (Settings › Indicator):
- [ ] All screens: click the top right third of the picture → `spot = "top-right"` in `[indicator]`; the next switch shows the indicator top right, 24 px (times the scale) from the edges, on every screen → AC-7, AC-13
- [ ] Spots or Custom › Custom, then drag on the picture → on release `x` and `y` are written in whole percent; Across and Down rows appear → AC-8, AC-13
- [ ] Custom at 100 % and 100 % → the card sits in the bottom right corner, fully on screen, and the caption says des-k keeps it inside → AC-8, AC-10
- [ ] Spots from Custom → the spot nearest the card is chosen, and `x` and `y` stay in the file → AC-13
- [ ] With two screens: Screen › your second screen › click a spot → the file gets `[indicator.screens."<id>"]` with that spot; Show it there shows the real indicator there, on that screen → AC-9, AC-13
- [ ] On that screen, set Distance to All screens' value while the table also has a spot → the `gap` key goes; set the spot back to All screens' value when it is the table's last key → it stays → AC-13
- [ ] Same as all screens → the whole table goes, focus stays on the chooser → AC-13
- [ ] Turn the indicator off → Place, Distance, Across and Down grey out and don't respond; the Screen chooser still opens and switches screens; Show it there is off → AC-13

## Value sourcing (each value from its named source)
- [ ] Unplug a screen that has its own place → the chooser lists it as Not plugged in, its table stays in the file and does nothing → AC-9
- [ ] Add a table for an id des-k never saw (`[indicator.screens."ACR-9"]`) → the chooser lists `ACR-9` as Unknown screen → AC-13
- [ ] Choose a portrait or ultrawide screen with the taskbar on the left or top → the picture takes that screen's shape and draws the taskbar on that edge; All screens draws 16:9 with the taskbar at the bottom → AC-13
- [ ] The label under the picture shows the chosen screen's name, size in pixels and scale (for example 3840 × 2160 · 150%) → AC-13
- [ ] On a 150% screen, gap 24 → the card sits 36 px from the work area's edges (above the taskbar) → AC-7
- [ ] A dark accent (`#1E2A78`) → white text on buttons and badges; a pale one (`#FFE066`) → near black → AC-5

## Commands
- [ ] `cargo test` → all pass, including `links_reach_4_5_to_1_on_the_card`, `tile_marks_stay_readable`, `screen_tables_come_and_go_whole`, `a_default_color_takes_its_key_out`, `a_screen_key_equal_to_all_screens_goes_but_never_the_last`, `paste_takes_exactly_six_hex_digits` → AC-12, AC-13, AC-15, AC-16
- [ ] `cargo test render -- --ignored` → `target/settings/colors-*.png`, `colors-own-*.png`, `place-*.png`, `place-screen-*.png` match the approved board (`pdesk-design/colors/handoff-2/settings-r12.html`) → AC-12, AC-13

## Acceptance-criteria coverage
- AC-1 Windows accent step · AC-2 My own typing · AC-3 rose neighbors · AC-5 ink steps · AC-6 high contrast · AC-7 spot click, 150% gap · AC-8 Custom drag and 100/100 · AC-9 second screen, unplugged · AC-10 card kept inside · AC-11 bad `ws3` · AC-12 My own, hex box, paste, default removal, Reset · AC-13 picture, chooser, Same as all screens, greying, shapes · AC-14 the mark · AC-15 rose links and test · AC-16 pale tile marks and test
