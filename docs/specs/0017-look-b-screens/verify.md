# Verify: Look B screens · spec 0017 · updated 2026-11-05
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

## Acceptance-criteria coverage
- AC-1 render step · AC-2 theme steps · AC-4 corners, shadow, scaling · AC-5 and AC-IND-9 Narrator · AC-6 and AC-IND-8 `fastest` · AC-IND-1 to AC-IND-7 the card steps · AC-IND-10 is built in the settings page and first run slices (the drawing call exists: `indicator::draw_preview`)
