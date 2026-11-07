# Verify: Look B screens · spec 0017 · updated 2026-11-07 (slice 6)
_Steps derived from spec 0017 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones. Each slice adds its own section._

## Slice 1: indicator and the shared layer

Run the build from an admin terminal (`target\debug\des-k.exe`) after stopping the installed copy, or install it with `install.ps1`.

### UI / manual
- [ ] Win Ctrl → with two screens plugged in → the card on the screen under the mouse: number block in the workspace's color, the screen's name, one bar per workspace → AC-IND-1, AC-IND-3
- [ ] Unplug down to one screen, switch → the card has no name → AC-IND-1
- [ ] The card has round corners and a soft shadow from Windows, in light and in dark (Settings › Personalization › Colors › Windows mode) → AC-4, AC-IND-6
- [ ] The card's 1 px edge is drawn once, not doubled → AC-IND-6
- [ ] Turn transparency effects off (Settings › Personalization › Colors) → the next card is fully opaque → AC-IND-5
- [ ] With a card fading, switch Windows mode from dark to light → within about 1 s the next card (and an open settings page or welcome screen) uses the new mode → AC-2
- [ ] High contrast Aquatic, then Desert → block and lit bars in Highlight, other bars and edge in WindowText, no see through → AC-IND-5
- [ ] Send window to 3 and stay → line "Window sent to [3]" / "You stay on 2", bar 3 ringed; the card stays at least 1.5 s even with `hold_ms = 0` → AC-IND-2
- [ ] Press the same stay send again quickly → "Window back on 2" → AC-IND-2
- [ ] Win Shift 2 for a window already on 2 → "Window already on 2" → AC-IND-2
- [ ] Win Ctrl U → "Pinned", every bar lit; again → "Unpinned" / "Only on 2 now" → AC-IND-2
- [ ] Pin a window, then send it and stay → "Unpinned, you stay on 2" → AC-IND-2
- [ ] Win 7 with 4 workspaces → "No workspace 7" / "You have 4" → AC-IND-2
- [ ] Unplug a screen with windows on it → on the main display "NAME unplugged" / "N windows moved here", held 3 s; plug it back → on that screen "Back" / "N windows returned" → AC-IND-3, AC-IND-4
- [ ] Two screens of the same model → names end " #1" and " #2", left to right → index screen names
- [ ] `[indicator] style = "dots"` in settings.toml → the next switch shows the dots pill; note: Windows' corners are 8 dip, so the pill's ends are less round than the design's 16 → AC-IND-7
- [ ] `[indicator] style = "pill"` → the log says it resets to "card", only that key → AC-IND-7
- [ ] `fastest = true` → the card disappears with no fade after its hold; the overview with `animation = "fade"` opens with no fade; turn it off → both fades come back → AC-6, AC-IND-8
- [ ] Narrator on, switch → Narrator reads "Workspace 2 of 4, DELL U2720Q" without the card taking focus → AC-5, AC-IND-9
- [ ] Laptop at 150% and an external screen at 100% → the card is sharp and the same size in dip on each → AC-4
- [ ] Settings page › Show → the card on the page's screen → AC-IND-3
- [ ] Test popup made by `ui::popup` without the layered flag gets the shadow too (needed from the finder slice on) → index popup helper

### Commands
- [ ] `cargo test` → all pass, including `style_and_fastest_reset_only_themselves`, `lines_say_what_happened`, `narrator_reads_the_painted_words` → AC-IND-7, AC-IND-2, AC-IND-9
- [ ] `cargo test render -- --ignored` → `target/indicator/` holds card and dots for every state in light, dark, Aquatic, Desert and light 150%; compare with `pdesk-design/restart/captures/ir5-*` → AC-1

### Value sourcing
- [ ] Colors: switch light, dark, HC → each card's colors come from the `ui` tokens of `ui::system_theme()` (the taskbar's mode, not the apps' mode: set them apart in Settings › Colors › Custom) → AC-2, AC-IND-5
- [ ] Sizes: the same card on a 100% and a 150% screen measures 1.5× in pixels → AC-4
- [ ] Fade length: `fade_ms = 2000` fades slowly; `fastest = true` makes it 0 → AC-6
- [ ] Screen names: rename nothing, but plug twins → " #n" comes from `ws` screen order → index screen names
- [ ] Workspace color: workspace 2's block is #DDB43C in light and dark → index tokens
- [ ] Narrator names: the words read equal the words painted → AC-IND-9

## Slice 2: finder

Same build and start as slice 1. The finder is des-k's first popup that is not layered, so its shadow check also closes slice 1's last step.

### UI / manual
- [ ] Win F with two screens → the finder on the screen under the mouse, 720 dip wide, its top a fifth of the way down, round corners, a soft shadow from Windows and a 1 px edge, in light and in dark → AC-FND-1, AC-FND-8, AC-4
- [ ] The finder is not in Alt Tab or the taskbar, and is not listed in its own rows → AC-4
- [ ] Empty box → "Find a window" in grey just after the caret; type a letter, erase it → the grey words come back → AC-FND-1
- [ ] Caret and selected text are readable in dark and in high contrast → AC-FND-1 (spec "check on the real machine")
- [ ] The chosen row has a light purple fill and the purple bar on its left; the row under the mouse is grey; high contrast: the chosen row in Highlight, the pointed row outlined → AC-FND-2
- [ ] Two screens → each row names its screen at the right; one screen → no names; two screens of the same model, cut short → the name keeps its " #1" or " #2" → AC-FND-3
- [ ] Chips: a window in view or minimized → a full chip in its workspace color; one des-k hid → a soft chip; a pinned window → a pin on the current workspace's color → AC-FND-3
- [ ] Unplug a screen with hidden windows and open the finder within 5 s → those rows say "NAME · unplugged" with a grey outline chip; after it folds they move to the main display → AC-FND-3
- [ ] App names: Chrome reads "Google Chrome"; an exe with no description reads its exe name; a running Store app (Calculator) reads its package name; a suspended Store app reads its window title. Note: Explorer's own description is "Windows Explorer", so its rows say that, not the design's "File Explorer" → AC-FND-4
- [ ] More than nine windows → a thin scroll thumb; the mouse wheel scrolls the list; Down to the last row keeps it in view → AC-FND-4
- [ ] The count reads "12 windows", and "2 of 12" while typing → AC-FND-4
- [ ] Ctrl W on a window → its row fades and reads "Closing…"; when it closes the row goes and the count drops → AC-FND-4
- [ ] The key line reads Enter Go to, Shift Enter Bring here, Ctrl W Close window, Esc Cancel; type "zzzq" → the first three grey out and the list says "No match" with its grey line → AC-FND-5
- [ ] Type "chrome" → the matched letters are bold in the name column; a long title whose match falls in the cut part → the cut still ends in … and nothing overlaps → AC-FND-6 (spec "check on the real machine")
- [ ] Shift Enter on an admin window → "Can't move this window" / "It stays where it was." with the warning icon; Ctrl W on one → "Can't close an admin window" / "It stays open." → AC-FND-7
- [ ] Enter on a window that closed after the finder opened → "That window is gone" with the info icon; its row goes → AC-FND-7
- [ ] Enter, then Shift Enter, on an unplugged row → "Its screen was just unplugged" / "In a moment it moves to <main display>; try again then." → AC-FND-7
- [ ] Narrator on, Win F → it reads the text box ("Find a window, edit"), then the count and the chosen row; Down reads the next row once; a note is read when it appears; scan mode shows a list whose chosen item is selected → AC-FND-9, AC-5
- [ ] With the finder open, switch Windows mode from dark to light → within about 1 s the finder (text box included) repaints in light → AC-2
- [ ] Open it on a 150% and on a 100% screen → sharp, and the same size in dip on each → AC-4

### Commands
- [ ] `cargo test` → all pass, including `matched_letters_split_into_the_program_and_title_columns`, `narrator_reads_the_chosen_row_once_per_change`, `refilter_matches_every_word_fuzzily_and_selects_the_best` → AC-FND-6, AC-FND-9, AC-FND-4
- [ ] `cargo test render -- --ignored` → `target/finder/` holds 16 states in light, dark, Aquatic, Desert and light 150%; compare with `pdesk-design/restart/captures/fdr3-*` → AC-1

### Value sourcing
- [ ] Colors: set apps dark and Windows light (Settings › Colors › Custom) → the finder is dark, since it follows the apps' mode → AC-2
- [ ] Screen names: plug twins → " #n" comes from the `ws` screen order, the same as the indicator's card → index screen names
- [ ] Workspace chip: a window on workspace 2 → its chip is #DDB43C, full or soft → index tokens
- [ ] Unplugged note: make another screen the main display, then unplug a third → the note names the new main display → AC-FND-7
- [ ] App name: a second window of the same exe → the same name, read once (no new log lines) → AC-FND-4
- [ ] Narrator names: the words read equal the row's painted words → AC-FND-9

## Slice 3: overview

Same build and start as slice 1. Turn on `[overview] animation = "fade"` for the fade steps.

### UI / manual
- [ ] Win Tab with two screens → each screen: a card colored band with a 1 px rule, tiles with a chip under each (full chip on the shown workspace, soft on the others), the shown tile ringed in the text color, the page grey under the band → AC-OVR-1, AC-OVR-2
- [ ] Purple shows only on the chosen window's ring and on drop targets; workspace colors only on the chips → AC-OVR-1
- [ ] Spread windows have a 1 px edge and a soft floating shadow; the spread starts under the band and stops above the key bar; labels are 14 px regular → AC-OVR-2, AC-OVR-3
- [ ] With `animation = "fade"`, watch the shadows while the overview fades in and out: no black boxes, no flashing (spec "check on the real machine") → AC-OVR-3
- [ ] `fastest = true` → no fade and no painted shadows; off again → both come back → AC-6, AC-OVR-3
- [ ] Each tile shows that screen's wallpaper behind its windows, fitted as the desktop shows it; a solid color desktop shows that color; a slideshow shows the background color → AC-OVR-3
- [ ] Spotlight on the desktop → tiles show the background color, not the picture. Note: des-k reads `DesktopSpotlight\Settings\EnabledState` to know; if Spotlight's picture shows instead, say so → AC-OVR-3
- [ ] Change the wallpaper, open again → the new picture; open twice more → no `wallpaper:` log lines (nothing decoded again) → AC-OVR-3
- [ ] Point at a window → a grey ring 3 px off it, its title, and the X (28 dip square, round corners); point at the X → it turns red; move off → back → AC-OVR-2, AC-OVR-6
- [ ] Labels "hover": the chosen window is labeled too, and a long title runs up to its cell's width before it is cut; a pinned window shows a pin before its title → AC-OVR-6
- [ ] Point at a tile → a thin grey edge; high contrast: dotted → AC-OVR-4
- [ ] Drag a window: its picture fades to 40% (high contrast: a dashed outline), the chosen ring hides, the bar reads "Drop on a workspace · Esc Cancel"; over a tile of another workspace → that tile gets the purple ring; over its own tile or its own spread → no ring → AC-OVR-4, AC-OVR-7
- [ ] Drag over the band → the moving picture shrinks to its tile size; back over the spread → 240 dip wide again; move along the band's rule: no flicker (spec "check on the real machine") → AC-OVR-5
- [ ] The moving picture has a 1 px edge in the text color, round corners and Windows' shadow → AC-OVR-5
- [ ] Drag onto the other screen's spread → that spread gets a purple ring 16 dip outside it; the bar moves to that screen → AC-OVR-4, AC-OVR-7
- [ ] The key bar shows only on the screen under the mouse and moves with it; it reads Enter Go to, Drag Move (dashed cap), 1 to N Show workspace, Ctrl W Close window, Esc Cancel → AC-OVR-7
- [ ] Open on an empty workspace → Enter and Ctrl W are greyed; a screen with no window at all also greys Drag → AC-OVR-7
- [ ] Drop an admin window (dev copy, not admin) → "Can't move this window" / "It stays where it was." in that screen's bar for 2 s → AC-OVR-8
- [ ] Ctrl W on an admin window from a non admin dev copy → "Can't close an admin window" / "It stays open." → AC-OVR-8
- [ ] Ctrl W on Notepad with unsaved text → its place empties; after 2 s it comes back chosen, with "A window didn't close" / "It may be asking you something. Enter goes to it." on the screen you asked on; Enter goes to it → AC-OVR-8
- [ ] Narrator on, Win Tab → it reads the overview, the shown workspace, the window count, and the chosen window; arrows read each window once; a drag and a note are read when they appear; scan mode shows a Workspaces list and a Windows list → AC-OVR-11, AC-5
- [ ] With the overview open, switch Windows mode dark to light → within about 1 s it repaints in light → AC-2
- [ ] A 150% and a 100% screen → sharp, and the same size in dip on each → AC-4

### Speed (spec "check on the real machine")
- [ ] Open the overview ten times on the screens you have, the first right after start (cold wallpaper cache) → every `overview: shown on N screens in M ms` line in `%LOCALAPPDATA%\des-k\des-k.log` is at most 100 ms; a slower one ends `(over the 100 ms limit)` → AC-8, AC-OVR-10
- [ ] Move the mouse over windows and tiles quickly → no stutter (only the changed spots repaint) → AC-OVR-10

### Commands
- [ ] `cargo test` → all pass, including `a_partial_repaint_matches_the_whole_paint`, `the_spread_leaves_room_for_the_band_and_the_bar`, `only_access_denied_says_admin`, `the_speed_line_flags_an_open_over_the_limit`, `the_shadow_fades_out_and_is_full_under_the_box`, `the_bar_shows_on_the_selected_screen_and_a_note_on_its_own`, `the_bar_greys_what_cannot_act`, `a_bad_overview_fade_time_only_resets_itself` → AC-OVR-10, AC-OVR-2, AC-OVR-8, AC-8, AC-OVR-3, AC-OVR-7, AC-7
- [ ] `cargo test render -- --ignored` → `target/overview/` holds 17 states in light, dark, Aquatic, Desert and light 150%; compare with `pdesk-design/restart/captures/ovr3-*` → AC-1

### Value sourcing
- [ ] Colors: set apps dark and Windows light → the overview is dark (it follows the apps' mode) → AC-2
- [ ] Fade length: `[overview] fade_ms = 1000` fades slowly; `0` is instant; `1500` resets to 250 with a note in the settings page's Overview section → AC-7
- [ ] Speed clock: open the overview while des-k is busy (right after a screen change) → the logged time includes the wait → AC-8
- [ ] Workspace chips: workspace 2's chip is #DDB43C, full on the shown tile, soft on the others → index tokens
- [ ] Wallpaper: set Fit, then Fill, then Span on two screens → each tile matches its screen's desktop → AC-OVR-3
- [ ] Narrator names: the words read equal the painted title and the app name the finder shows → AC-OVR-11
## Slice 4: tray menu and Shortcut hints

Same build and start as slice 1. For the keyboard steps, Win B puts focus on the taskbar's corner; arrows reach des-k's icon.

### UI / manual: tray menu
- [ ] Left click des-k's icon, then right click it → each opens des-k's own menu by the icon, inside the work area, never over the icon: the mark, des-k and its version, your screens with workspace chips and names, then Shortcut hints, Settings | Open log, Check for updates | Quit des-k with its grey line → AC-TRY-1, AC-TRY-2, AC-TRY-3, AC-TRY-5
- [ ] The tooltip "des-k" still shows when you point at the icon (version 4 needs NIF_SHOWTIP) → AC-TRY-8
- [ ] Point at an item → menu grey; nothing purple except New → AC-TRY-1
- [ ] Click outside, press Esc, or click the icon again → it closes; the click on the icon does not open it again → AC-TRY-9
- [ ] With the menu open, press Win Ctrl →, Win F, Win Tab, or swipe → the menu closes first, then that runs → AC-TRY-9
- [ ] Win B, arrows to the icon, Enter (then again with Shift F10, and the menu key) → it opens with a ring on Shortcut hints; arrows wrap, Home and End jump, Q moves to Quit des-k without quitting, Enter on Quit quits (spec "check on the real machine") → AC-TRY-8
- [ ] Three or more screens (or a test with a TV) → a map in Windows' arrangement, each screen in its workspace's color with its number; the bar sits under the screen the menu opened on; point at a screen for about half a second → its name, plus "main display" → AC-TRY-3, AC-TRY-4
- [ ] Settings, Open log, Check for updates, Quit des-k do what they did before; with no log file yet (a debug build in a console) Open log is greyed → AC-TRY-5
- [ ] Check for updates → while it runs the item reads "Checking for updates…", greyed; the answer comes as Windows' notification as before; click a "newer" notification → its page opens (balloon clicks after the move to version 4, spec "check on the real machine") → AC-TRY-7, AC-TRY-8
- [ ] Hiding broken (hard to cause; a render shows it): the warning box sits under the screens, and every update answer stays on the item until the next check → AC-TRY-6, AC-TRY-7
- [ ] Narrator on, open from the keyboard → it reads the menu, each item's name, its grey line, and "unavailable" for a greyed one; screens are read with their workspace (spec "check on the real machine") → AC-TRY-10, AC-5
- [ ] Only the main taskbar holds tray icons on build 26200 (spec "check on the real machine")
- [ ] Switch workspaces with the menu open on another screen → the menu is never hidden → AC-TRY-11

### UI / manual: Shortcut hints
- [ ] Tray › Shortcut hints → the window opens centred on the screen under the mouse, in front with focus, with every row of your settings; not in Alt Tab, no taskbar button → AC-HNT-1, AC-HNT-7
- [ ] Set key → "Press the new key"; des-k's shortcuts pause; Esc alone cancels; press Win Ctrl H → saved, the footer reads "Opens with Win Ctrl H" with Change and Remove → AC-HNT-4
- [ ] Set key, press Win F → "Win F is already in use", Cancel has focus; Cancel changes nothing; again with Move it here → Find a window loses Win F (settings page and file agree) → AC-HNT-4
- [ ] Press Win H while recording → it saves, and Win H then opens hints instead of voice typing (spec "check on the real machine") → AC-HNT-4
- [ ] Start recording on the settings page, then Set key here → the settings page's recording stops; a key goes only here → AC-HNT-4
- [ ] With a key set: press it on the window's screen → it closes; open it, move the mouse to the other screen, press it → it moves there → AC-HNT-2
- [ ] Esc or the title bar's X closes it → AC-HNT-2
- [ ] Change a shortcut on the settings page with the window open → it shows at once; edit `settings.toml` by hand → it shows within 2 s → AC-HNT-3
- [ ] Settings › Shortcuts shows a "Shortcut hints" row in today's look; setting it there shows in the window → AC-HNT-4
- [ ] Tab reaches Set key (or Change and Remove), the touchpad note's button, and Open settings; Enter presses the focused one; your other shortcuts keep working while it is open → AC-HNT-6
- [ ] Turn one of Windows' gestures back on → reopen or click back to the window: "1 of 4 is still on in Windows", cards greyed, Touchpad setup opens Settings › Touchpad → AC-HNT-5
- [ ] `[touchpad] enabled = false` → "Touchpad reading is off" with Turn reading on, which turns it back on → AC-HNT-5
- [ ] Give gestures back on the welcome screen, then open hints → "Given back to Windows at your next sign-in" (UX review M11, spec "check on the real machine") → AC-HNT-5
- [ ] A 1366 × 768 screen at 125% (or a short work area) → the window fits it, the middle scrolls with the wheel and Page Up or Down, the footer stays in view → AC-HNT-1
- [ ] Narrator: each row is read as its action then its keys, each note, and the footer's buttons (spec "check on the real machine") → AC-HNT-8, AC-5
- [ ] A 150% and a 100% screen → sharp at each; dark and high contrast → AC-1, AC-4

### Commands
- [ ] `cargo test` → all pass, including `the_hints_key_is_off_by_default_and_loses_a_clash`, `the_hints_key_fires_only_once_set`, `only_the_window_recording_can_stop_it`, `the_map_fits_and_no_screen_is_too_small`, `the_items_say_what_they_do_now`, `the_rows_follow_the_settings`, `the_gestures_say_what_each_one_does`, `every_touchpad_state_says_why`, `a_recorded_key_saves_or_asks_first` → AC-HNT-1, AC-HNT-4, AC-TRY-4, AC-TRY-5, AC-TRY-7, AC-HNT-3, AC-HNT-5
- [ ] `cargo test render -- --ignored` → `target/tray/` holds 13 states and `target/hints/` 12, each in light, dark, Aquatic, Desert and light 150%; compare with `pdesk-design/restart/captures/trr4-*` and `hr3-*` → AC-1

### Value sourcing
- [ ] Menu colors follow Windows' mode (taskbar), the hints window the apps' mode: set them apart and look → AC-2
- [ ] Version: the header shows the version the update check reads → AC-TRY-2
- [ ] Map: rearrange screens in Display settings → the next open shows the new arrangement → AC-TRY-3
- [ ] Quit's line: Windows' gestures off with values saved → "…return at your next sign in"; none saved → "…no gestures until you turn Windows' on" → AC-TRY-5
- [ ] Hints rows: every list and set in `[shortcuts]` and every `[[gesture]]` rule shows as the file has it → AC-HNT-3

## Slice 5: settings page

Built to `pdesk-design/restart/settings-r3.html` (r2.1) with the motion rows of `settings-r7.html` (handoff `des-k-motion-rows` revision 2, approved 2026-11-07). Run the build as in slice 1.

### UI / manual
- [ ] Tray › Settings with the mouse on each screen → opens there, 1040 wide; drag it wider → the column stops at 1000; it never goes narrower than 1040 → AC-SET-1
- [ ] Side list: Workspaces, Shortcuts, Indicator, Restore, Overview, Motion, Touchpad; Welcome opens the welcome screen; Open settings.toml opens the file → AC-SET-2
- [ ] Tiles: + adds a workspace on every screen; point at a last tile → × appears with a note listing what moves per screen, an unplugged one included; × removes one → AC-SET-3
- [ ] Tab to the tiles: Right adds, Left and Delete remove; hold Delete → only one goes → AC-SET-3, spec 0012 AC-16
- [ ] Indicator stays to 800 → "Default: 550 ms. Reset" in its caption; Reset puts 550 back. At 6 workspaces, point at Reset → the note says it removes 5 and 6 → AC-SET-3
- [ ] Shortcuts: Open overview shows Win Tab with "Taken from Windows"; choose Windows → Win Tab goes and the caption says it is Windows' again; choose des-k → back → AC-SET-4
- [ ] Add a key to Find a window, press Win Tab → the clash note, Cancel focused; Move it here → Open overview says Win Tab goes to "Find a window", its des-k / Windows is greyed → AC-SET-4
- [ ] Open shortcut hints: Set key records one, and that key opens the hints window → AC-SET-4
- [ ] In settings.toml set `hold_ms = "x"` → a note in the Indicator stays row with des-k's log line and a plain title, Indicator shows 1 in the side list; add `workspace = 3` → Notes appears with "“workspace” isn't a setting, so des-k skips it" → AC-SET-5
- [ ] Touchpad: with Windows' gestures on, then off, then reading off → each note and row state; Open Windows Settings opens Settings › Touchpad; Give back → its result note, the same one the welcome screen shows → AC-SET-6
- [ ] Indicator off → Style and Show greyed; on → the preview draws Card, then Dots, for the page's screen; Show puts the real one up → AC-SET-7
- [ ] Overview fades for greyed with "Not in use: Overview animation is None"; choose Fade → it works, − and + step 50 ms; turn Skip animations on → greyed again with both reasons; the Skip animations link scrolls to Motion and focuses the switch → AC-SET-8, AC-11
- [ ] Skip animations on → Indicator fades out greyed, its time line shows no fade, every Motion lane says At once; set the indicator fade to 1500 → the chart's axis runs to 2000 ms → AC-11
- [ ] Set `fastest = "yes"` by hand → a note in the Skip animations row, Motion shows 1; Reset writes false → AC-11, AC-SET-5
- [ ] Break settings.toml (a line with no `=`) → the read only banner, nothing changes on a click, links still scroll → spec 0012 AC-14
- [ ] Light, dark, Aquatic: compare with `captures/sr3-*` and `sr7-B-*`; switch Windows' mode with the page open → it repaints within 1 s → AC-1, AC-2, AC-SET-9
- [ ] Narrator: Tab and Up / Down through the page → a switch reads as a toggle with on or off, a number with its value, a greyed one as unavailable with its reason, the Motion chart as one sentence, Reset and the chart's links as links → AC-SET-10, AC-5
- [ ] Keyboard: one stop per row; Left / Right pick des-k or Windows on a shortcut row; Enter records a new key; Delete removes its last key; Esc closes a menu, then Cancel on a clash, then a banner, then the page → spec 0012 AC-16
- [ ] A 150% and a 100% screen → sharp at each, and resizing at 150% keeps it sharp (spec "check on the real machine") → AC-SET-1, AC-4

### Commands
- [ ] `cargo test` → all pass, including `greyed_rows_name_every_reason`, `problems_get_plain_titles`, `problems_have_a_home`, `windows_key_first_maps_back_to_the_file`, `a_taken_windows_key_names_its_holder`, `hand_edits_read_in_the_page_words`, `the_menu_offers_every_action_once`, `every_row_round_trips_through_the_file` → AC-SET-4, AC-SET-5, AC-SET-8
- [ ] `cargo test render_settings -- --ignored` → `target/settings/` holds 9 states in light, dark, Aquatic and light 150%; compare with the captures above → AC-1

### Value sourcing
- [ ] Screen names: two screens of one model → "#1" and "#2", left to right, in the rows and the removal note → AC-SET-3
- [ ] Workspace numbers: switch on one screen → within 1 s its row's ringed tile and its picture's number move → AC-SET-3
- [ ] The preview's screen: drag the page to the other screen → the preview names that screen and its workspace → AC-SET-7

## Slice 6: first run

Built to `pdesk-design/restart/first-run-r11.html` (r9.2, approved 2026-11-04) with the More rows of `first-run-r15.html` (handoff `des-k-motion-rows` revision 2, approved 2026-11-07). Run the build as in slice 1.

### UI / manual
- [ ] A fresh install (`first_run_seen = false`) → the first run opens at start on the screen under the mouse, and the file now says `first_run_seen = true`; start again → it stays shut → AC-9, AC-FR-1
- [ ] Upgrade: a 0.3.0 `settings.toml` (no `first_run_seen`) and an existing `touchpad.txt` → it opens once, step 2 says "Your shortcuts from before", the key lands above `[shortcuts]` with the comments kept → AC-9
- [ ] Break settings.toml (a line with no `=`), start → it opens, the log says it couldn't write `first_run_seen`; it opens at every start until the file is fixed → AC-9
- [ ] Settings › Welcome → it opens at any time, or comes to the front → AC-FR-1
- [ ] Step 1: press Win Ctrl → → the row ticks, says which screen switched and to what, the picture's number changes; Win Tab, then Win F and Enter on a window → all three tick and "That's des-k. Next: your keys." shows, Skip goes → AC-FR-2
- [ ] Next from Try it to Shortcuts → the window grows from its top edge (bottom moves, top stays) over about 150 ms; with Skip animations on → at once → AC-FR-7, AC-6
- [ ] Step 2: Find a window → Windows → "No key. Windows keeps Win F: opens Feedback Hub." and Set key; → des-k → Win F is back → AC-FR-4
- [ ] Give Win 1–9 back to Windows on Go to workspace, then record Win Ctrl and a digit there → the clash note, Cancel focused; Move it here → Send window to 1–9 and stay reads "No key. Win Ctrl 1–9 now goes to workspace 1–9." with des-k greyed → AC-FR-4
- [ ] Record Switch workspace with Win Alt held, press ← → both arrows get Win Alt, the row shows Win Alt ← →; a letter instead → "Press ← or →", still recording → AC-FR-4
- [ ] While recording, Shift and a digit on Go to workspace → "Shift alone can't be used", still recording; Esc alone cancels; a click anywhere cancels → AC-FR-3, AC-FR-6
- [ ] An action with two keys (add Ctrl Alt O to Open overview in the settings page), give Win Tab back → Ctrl Alt O stays → AC-FR-4
- [ ] Step 3: Tab to the workspaces − → the warning names what moves per screen; press it → every screen drops to 3 → AC-FR-4
- [ ] Step 3 More: Overview fades for greyed with "Not in use: Overview animation is None"; − and + do nothing but stay Tab stops; Fade → it works; Skip animations on → Indicator fades out and Overview fades for greyed with every reason → AC-FR-8, AC-11
- [ ] Restore after restart off → its caption says it applies from des-k's next start → AC-FR-2
- [ ] Step 4: with Windows' gestures on → "Turn off four gestures in Windows" and Set up in Windows Settings; turn two off → "2 of 4 done", the rows say which; all four → "des-k runs these" → AC-FR-5
- [ ] Step 4: click a picker → the menu with every action, the chosen one checked, kept inside the window; Done twice opens its own menu; Key combo… records into the picker → AC-FR-5, AC-FR-6
- [ ] Step 4: Give back → its result note; make a write fail (lock the key) → "Give back failed" with Try again, Give back hidden, and it stays when Windows' gestures read off again → AC-FR-5
- [ ] No precision touchpad → four steps, no Touchpad step, Done says none was found → AC-FR-5
- [ ] Step 5: Show opens Shortcut hints; Set key records the hints key and that key opens them; the main shortcuts and the "Taken over from Windows" line follow what you changed in step 2 → AC-FR-4
- [ ] Keyboard: Tab visits every control (a stepper's − and + apart), footer last; first focus on each step is Next; Enter presses Next unless a button has focus; Esc cancels a recording, a clash or a menu first, then closes; closing keeps what is set → AC-FR-3, AC-FR-6
- [ ] Light, dark, Aquatic: compare with `captures/fr11-*` and `fr15-*`; switch Windows' mode with it open → it repaints within 1 s → AC-1, AC-2
- [ ] Narrator: each step reads "Step n of N", its heading, then every control with its value and state; a greyed stepper reads unavailable with its reason → AC-FR-9, AC-5
- [ ] A 150% and a 100% screen → sharp at each; drag it between them → it follows; on a 1080p screen at 150% both heights fit → AC-4, AC-FR-7
- [ ] After a give back, whether Windows Settings shows the gestures as on while they stay off until sign in (UX review M11, spec "check on the real machine")

### Commands
- [ ] `cargo test` → all pass, including `first_run_seen_tells_fresh_from_upgrade`, `first_run_seen_lands_above_the_first_table`, `the_taken_line_follows_what_des_k_holds`, `a_shortcut_row_says_where_its_windows_key_is`, `a_pair_shows_as_one_key_only_when_it_is_one`, `the_touchpad_states_word_each_case` → AC-9, AC-FR-4, AC-FR-5
- [ ] `cargo test render_first_run -- --ignored` → `target/welcome/` holds 16 states in light, dark, Aquatic and light 150%; compare with the captures above → AC-1

### Value sourcing
- [ ] Step 1's result: switch on the other screen → it names that screen and its new workspace; with one screen → "Switched to workspace 2" → AC-FR-2
- [ ] Done's touchpad line: change Windows' gestures with the first run open → within 1 s the row follows → AC-FR-5

## Acceptance-criteria coverage
- Slice 6: AC-FR-1 to AC-FR-9 the first run steps · AC-9 the start steps · AC-11 its More rows · AC-1 its render step · AC-2, AC-4, AC-5, AC-6 its theme, scaling, Narrator and fastest steps
- Slice 5: AC-SET-1 to AC-SET-10 the settings page steps · AC-11 its motion rows · AC-1 its render step · AC-2, AC-4, AC-5 its theme, scaling and Narrator steps
- Slice 4: AC-TRY-1 to AC-TRY-11 the tray menu steps · AC-HNT-1 to AC-HNT-8 the hints steps · AC-1 their render step · AC-2, AC-4, AC-5 their theme, scaling and Narrator steps
- Slice 3: AC-OVR-1 to AC-OVR-11 the overview steps · AC-1 its render step · AC-2, AC-4, AC-5, AC-6, AC-7, AC-8 its theme, scaling, Narrator, fastest, fade time and speed steps
- Slice 2: AC-FND-1 to AC-FND-9 the finder steps · AC-1 its render step · AC-2, AC-4, AC-5 its theme, scaling and Narrator steps
- AC-1 render step · AC-2 theme steps · AC-4 corners, shadow, scaling · AC-5 and AC-IND-9 Narrator · AC-6 and AC-IND-8 `fastest` · AC-IND-1 to AC-IND-7 the card steps · AC-IND-10 is built in the settings page and first run slices (the drawing call exists: `indicator::draw_preview`)
