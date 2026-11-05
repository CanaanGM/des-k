# Verify: Look B screens · spec 0017 · updated 2026-11-05 (slice 2)
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

## Acceptance-criteria coverage
- Slice 2: AC-FND-1 to AC-FND-9 the finder steps · AC-1 its render step · AC-2, AC-4, AC-5 its theme, scaling and Narrator steps
- AC-1 render step · AC-2 theme steps · AC-4 corners, shadow, scaling · AC-5 and AC-IND-9 Narrator · AC-6 and AC-IND-8 `fastest` · AC-IND-1 to AC-IND-7 the card steps · AC-IND-10 is built in the settings page and first run slices (the drawing call exists: `indicator::draw_preview`)
