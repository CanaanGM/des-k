# 0020. Your own colors and where the indicator appears

**Date**: 2026-11-07
**Status**: In Progress

## Summary

des-k's accent (today a fixed purple) follows your Windows accent color, or a color you type, and des-k adds two neighbor hues (30 degrees either side) for its tinted parts, so a rose accent brings a red and a purple. You can type your own color for each of the 9 workspaces; any you leave alone keeps today's color. The indicator can sit at one of 9 spots with a gap from the edge, or at exact coordinates given as a percent of the screen, set once for every screen with an optional override per screen. Turning the indicator off already works and stays as it is.

## Requirements

**User stories**:
- As the owner, I want des-k's colors to match my Windows accent so des-k looks like part of my desktop.
- As the owner, I want to pick my own color for each workspace so I recognize workspaces at a glance.
- As the owner, I want the indicator where it does not cover my work, on every screen, with a different spot on a screen that needs one.

**Acceptance criteria**:
- **AC-1**: With no `[colors]` settings, des-k's accent is the Windows accent: in a light des-k window the shade Windows uses for light mode, in a dark one the shade for dark mode. Changing the Windows accent repaints every open des-k window within about 1 s, without a restart.
- **AC-2**: `[colors] accent = "#RRGGBB"` uses that color as the accent in both modes instead of the Windows accent; `accent = "windows"` or no key goes back to following Windows.
- **AC-3**: From the accent, des-k makes two neighbor hues, 30 degrees either side on the color wheel, with the same saturation and lightness. Buttons, toggles and the selected row use the accent itself; section header tints and the progress line use the first neighbor, count badges and picker marks the second. A rose accent gives a red and a purple.
- **AC-4**: `[colors] ws1` to `ws9 = "#RRGGBB"` sets that workspace's color in every des-k window (indicator card and dots, finder, overview, tray menu, welcome, settings page). A missing key keeps today's color for that workspace. One value serves light and dark mode.
- **AC-5**: Text and numbers drawn on a workspace color or on the accent use black or white ink, whichever has the higher contrast ratio against it.
- **AC-6**: When Windows' high contrast mode is on, des-k ignores the Windows accent, your accent and your workspace colors, exactly as today.
- **AC-7**: `[indicator] spot` takes one of `top-left`, `top`, `top-right`, `left`, `center`, `right`, `bottom-left`, `bottom`, `bottom-right`, `custom` (default `center`, today's place). For the eight edge spots, the card's nearest edges sit `gap` pixels from the work area's edges (default 24, times the screen's Windows scale); `center` ignores the gap.
- **AC-8**: With `spot = "custom"`, `x` and `y` (0 to 100, decimals allowed, default 50) place the card's center at that percent of the work area's width and height. The card is moved inward if it would poke past any edge of the work area.
- **AC-9**: A table `[indicator.screens."<screen id>"]` may set any of `spot`, `gap`, `x`, `y` for one screen (its id from spec 0016); a key it leaves out comes from the `[indicator]` default. A table for a screen that isn't connected is kept and does nothing.
- **AC-10**: A card that grows (extra lines such as a note) keeps its anchored edges, or its center for `custom`, in place.
- **AC-11**: A bad value (a color that isn't `#RRGGBB`, an unknown spot, `x` or `y` outside 0 to 100, `gap` outside 0 to 400) resets only that key, with a `warn!` line, per spec 0001. In a screen table it falls back to the default's value.
- **AC-12**: The settings page has a Colors section: an accent row (Windows or My own, and with My own a swatch, a `#RRGGBB` box and Reset), and one row per workspace in use with a swatch, a `#RRGGBB` box and Reset. An invalid entry is not saved and the box shows it is wrong. Choosing My own writes the accent des-k uses at that moment, so nothing changes until you type; while the page is open, My own goes back to the last own color you had. Saving a workspace color equal to des-k's default for it removes the key. Reset removes the key.
- **AC-13**: The settings page's Indicator section adds a second card, Where it shows: a screen chooser (All screens, then each known screen by name, shown only with two or more), a spot picker drawn as a small screen in the chosen screen's shape with 9 spots plus Custom, a gap row, and x and y rows shown only for Custom. With a single screen chosen, Same as all screens removes that screen's table, and Show it there shows the real indicator on that screen. Rows grey out when the indicator is off, like the Style row does today; the screen chooser stays live then, and in read only, since it only picks which place you see.
- **AC-15**: A link (Reset and the links in captions) has at least 4.5:1 contrast with the card it sits on, in light and dark, for any accent.
- **AC-16**: The window marks on a workspace tile stay readable on a pale workspace color you choose: white, unless white is under 1.6:1 on the tile.
- **AC-14**: The des-k mark (the tray icon and the mark in the welcome screen) keeps its own colors; it is des-k's logo, not an accent.

## Decision

**Chosen option**: Option 1: fix in place, colors resolved once in `ui::theme_of`.

The accent follows Windows (or your color) with two 30 degree neighbors for tints, workspace colors take your values with today's as defaults, all resolved where the theme is built, and the indicator's place comes from one pure position function with a default and per screen overrides.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

The decision record (context, options considered, why, and the choices the design pass settled) is in [rationale.md](rationale.md).

## Feature design

**Data model sketch** (`settings.toml`):
```toml
[colors]
accent = "windows"        # or "#RRGGBB"; missing means "windows"
ws1 = "#79A4F3"           # ws1 to ws9; each missing key keeps today's color
# ...

[indicator]
enabled = true            # unchanged
style = "card"            # unchanged
spot = "center"           # top-left, top, top-right, left, center, right, bottom-left, bottom, bottom-right, custom
gap = 24                  # 0 to 400, pixels at 100% scale
x = 50.0                  # 0 to 100, used when spot = "custom"
y = 50.0

[indicator.screens."DEL-A1B2-12345"]   # a screen id from spec 0016; any subset of spot, gap, x, y
spot = "top-right"
```
`settings.rs` gains `accent: Accent` (`Windows` or `Rgb(u32)`), `ws_colors: [Option<u32>; 9]`, `indicator_pos: Pos { spot, gap, x, y }` and `indicator_screens: BTreeMap<String, PosOverride>` (each field `Option`). `INDICATOR_KEYS` grows by `spot`, `gap`, `x`, `y`, `screens`; a new `COLORS_KEYS` lists `accent`, `ws1` to `ws9`.

**Theme resolution** (in `ui.rs`):
- `Look` gains `accent: Option<[u32; 8]>`, read in `read_look` with `RegGetValueW` and `RRF_RT_REG_BINARY` into a 32 byte buffer from `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Accent`, value `AccentPalette`. Layout: 8 shades of 4 bytes each, bytes in the order R, G, B, A (A ignored), shades in the order Light3, Light2, Light1, Base, Dark1, Dark2, Dark3, spare. So Light2 is bytes 4 to 6 and Dark1 is bytes 16 to 18. Not exactly 32 bytes, or unreadable: keep the last good value (start: `None`, which means des-k purple `#8E3FD9`), with the warning rule in Rationale.
- `LIGHT` and `DARK` stay `const` look B defaults; `theme_of(light, look)` copies one and, after the high contrast return, overwrites: `acc` = typed accent, else the palette's Dark1 (light) or Light2 (dark), else purple. `acc_a`, `acc_b` = `acc` rotated by minus and plus 30 degrees in HSL, saturation and lightness kept, each channel rounded to the nearest whole number; a grey (saturation 0) stays the same grey. `link` = `acc` in light, `mix(acc, 0xFFFFFF, 0.28)` in dark (the rule `settings_page` already uses for its bar), then mixed from that base toward `#000000` (light) or `#FFFFFF` (dark) by 0.05, 0.10, and so on until its contrast with `card` is at least 4.5 (AC-15). `held` = `mix(card, acc, 0.12)` in light, `0.20` in dark. `ws[i]` = `ws_colors[i]` or `WS[i]`.
- Ink: an `ink(bg) -> u32` helper returns `#14141A` or `#FFFFFF`, whichever has the higher WCAG contrast ratio with `bg`. `on_acc` = `ink(acc)`. `on_ws` becomes `[u32; 9]`, `on_ws[i]` = `ink(ws[i])`, and every reader indexes it by workspace (`finder`, `overview`, `indicator`, `settings_page`, `welcome`, `tray_menu`). `high_contrast` fills all nine with `hl_fg`. `link` and `held` are colors, not inks, so the ink rule doesn't apply to them. Tile marks: `mk_ws[i]` = `#FFFFFF` if its contrast with `ws[i]` is at least 1.6, else `mix(ws[i], 0x14141A, 0.55)` (AC-16); `high_contrast` fills all nine with `hl_fg`.
- Which accent each inline use takes (functions, since line numbers drift):

| Where | Today | After |
|---|---|---|
| section header tint: `settings_page::header`, `welcome::header`, `hints::header` | `mix(card, acc, 0.09)` | `acc_a` in the same mix |
| tinted bands: `welcome::tint`, `settings_page::workspaces`, `settings_page::key_row` | `mix(card, acc, 0.06)` | `acc_a` |
| the first run's progress line (`welcome`, above the step) | `acc` | `acc_a` |
| count badge (the `n` drawn by each `header`) and `settings_page::chip` fill, edge and icon | `acc` mixes | `acc_b` |
| marks inside filled pickers (`Choice` and workspace tile marks) | `acc` | `acc_b` |
| buttons, toggles, the selected row, `settings_page::side` selection, `finder` chosen row, overview drop targets, `fade_line` gradient, the motion bar, hover states | `acc` | `acc`, unchanged |

**Position** (in `indicator.rs`): `fn place(work: RECT, w: i32, h: i32, pos: Pos, scale: f32) -> (i32, i32)`, pure. Edge spots: align the named edges to `work` inset by `(gap * scale).round()`, centered on the other axis. Center: centered. Custom: center at `work.left + x% * width`, `work.top + y% * height`, rounded. Then every spot is clamped so the whole card stays inside `work`; a card wider or taller than `work` puts its left or top edge on `work`'s. `scale` is whatever `show` already uses (96 DPI when `GetDpiForMonitor` fails). `show` resolves `Pos` for `card.screen_id` (default, then the screen's override per key) and calls `place` instead of today's centering line.

**Settings parsing and writing** (in `settings.rs`):
- Colors: `#RRGGBB` only, hex digits in either case; `#abc`, a missing `#` or anything else is a bad value. The settings page writes upper case.
- `screens` must be a table of tables; anything else resets `screens` with a `warn!`. Inside a screen table a bad value warns with its full path (`indicator.screens."<id>".x`) and that key falls back to the default; an unknown key warns and is ignored. Ids are any string, always written as quoted keys.
- `INDICATOR_KEYS` accepts `screens`, and the unknown key check skips the ids under it. `DEFAULT_FILE` gains `spot`, `gap`, `x`, `y` and a commented `[colors]` example, so the test that every key appears in `DEFAULT_FILE` still holds (`screens` exempted, it has no default).
- `settings::write` gains two edits: `Edit::Remove(path)` (for Reset and Same as all screens) and an owned path form (`Vec<String>`) so `["indicator", "screens", id, "spot"]` writes `[indicator.screens."<id>"]` with `screens` as an implicit table. Removing a screen table's last key removes the table.
- `apply` lists every new key in `fields()`, including the per screen ones; a change to any `[colors]` key calls `ui::repaint_all()`. An open indicator picks up the new place on its next `show`.

**Settings page rows** (steps 5 and 6): the approved board is the visual and behavioral truth: `pdesk-design/colors/handoff-2/settings-r12.html`, frames f0 to f33 and its notes "How the new rows work (r12)", with captures beside it (manifest `pdesk-design/colors/handoff-2.json`, sha256 `9a9e3b2d4b7752d550c88004351d6f4b1eec77dbb132db68ef16cd45f91ddabe`). In short:
- Colors sits between Overview and Motion in the side list. It opens with a sample of des-k's parts in the colors in use, named by what each color paints.
- The hex box: a fixed `#` and up to six hex digits, Cascadia Mono. Tab in selects the digits; other keys are ignored; paste takes exactly six hex digits (spaces and a leading `#` trimmed) or is refused with a line. Enter, or leaving with six digits, saves; Esc puts the saved value back; fewer than six shows a warning edge and a line and saves nothing. The swatch follows a valid value as you type.
- Where it shows: the header names the screen you are changing. The chooser lists present screens, remembered ones (marked not plugged in) and any screen table id des-k never saw (by its id), with identical screens by position from the left. The page opens on All screens.
- The picture: each third across and down picks its spot; Left and Right walk the nine and stop at the ends. Custom: a click or drag puts the card's center there in whole percent, and Left and Right move it 1%. Spots, from Custom, picks the nearest spot and keeps x and y in the file.
- Show it there shows the indicator on the chosen screen at its place (All screens: the screen the settings window is on); it is off while the indicator is off and for a screen not plugged in.
- High contrast: a note at the top of Colors, the sample hidden, the boxes and swatches greyed and blank; Reset stays live.

**State transitions**: none.

**Interface surface**:
| Surface | Inputs | Effect | Errors |
|---|---|---|---|
| `settings.toml` `[colors]` | `accent`, `ws1`..`ws9` | colors in every window, live on the 1 s reload | bad value resets that key, `warn!` |
| `settings.toml` `[indicator]` | `spot`, `gap`, `x`, `y` | default place on every screen | bad value resets that key, `warn!` |
| `settings.toml` `[indicator.screens."<id>"]` | any of the four | that screen's place | bad value falls back to the default's |
| Settings page, Colors section | Windows or My own, hex boxes, Reset | `settings::write` of one key; Reset removes the key | invalid hex not saved, box marked |
| Settings page, Indicator section | screen chooser, spot picker, gap, x, y, Same as all screens, Show it there | `settings::write` of one key in `[indicator]` or the screen's table; a screen key equal to All screens' value is removed (never the table's last key) | none beyond the steps' ranges |
| Windows accent change | Windows Settings | repaint within about 1 s | palette unreadable: last good palette (purple if none), one `warn!` per change of state |

**Value sourcing**:
| Action | Value | Source |
|---|---|---|
| any des-k paint | accent | `[colors] accent` if a color; else `AccentPalette` Dark1 (light) or Light2 (dark) by the window's mode (`apps_light` or `system_light`, as today); else `#8E3FD9` |
| any des-k paint | neighbor hues | derived: accent's HSL hue minus 30 (`acc_a`) and plus 30 (`acc_b`) |
| any des-k paint | workspace color i | `[colors] ws{i+1}`, else `WS[i]` in `ui.rs` |
| any des-k paint | ink on a color | derived: WCAG contrast of `#14141A` vs `#FFFFFF` against it |
| high contrast on | every color | `ui::high_contrast(sys)`, unchanged |
| indicator show | which screen | the screen id `ws` puts in the `Card` (spec 0016 id) |
| indicator show | spot, gap, x, y | `[indicator.screens."<id>"]` key if set, else `[indicator]` key, else defaults `center`, 24, 50, 50 |
| indicator show | work area, scale | `GetMonitorInfoW` `rcWork` and `GetDpiForMonitor`, as today |
| indicator show | card size | `draw` measuring the card, as today |
| settings page | screens in the chooser | `View.present` (id, name, full and work rect), `View.waiting` (id, name), and screen table ids in `settings::get()` found in neither (shown by id) |
| settings page | picture shape and taskbar edge | the chosen present screen's `full` and `work`; All screens and any screen not present: 16:9 with the taskbar at the bottom |
| settings page | kept inside caption | `indicator::place` with the plain card's size at that screen's scale |
| settings page | My own's first color | the resolved `acc` of the page's theme; later, the last own color the page holds while open |
| settings page | swatch color | the value in the hex box if valid, else the resolved color |
| settings page | link, tile marks | `Theme.link`, `mk_ws` (AC-15, AC-16) |

**Key invariants**:
- High contrast returns before any custom color is applied.
- Every color is resolved in `theme_of`; no window reads `[colors]` itself.
- The indicator never takes the `ws` lock; the screen id arrives in the card.
- The indicator card is always entirely inside the work area.
- A color value is only ever `#RRGGBB` text; nothing in these tables names a file or a program.

**Security model**: no change to who can do what. These are plain values in a file anything running as you can already write; the worst a hostile edit does is ugly colors or an indicator in a corner. Nothing here is a path or a program, so the data folder rule holds.

**Configuration required**: none beyond the `settings.toml` keys above. Two new `windows` crate features, `Win32_System_DataExchange` and `Win32_System_Memory`, for the hex box's copy and paste (the clipboard); the registry API is already enabled. No new crate.

**Critical test scenarios**:
- `place`: each of the 9 spots on a 1920 by 1040 work area at scale 1.5 with gap 24 lands where AC-7 says; verifies **AC-7**.
- `place`: custom 100, 100 with a 300 by 120 card ends with its bottom right corner on the work area's corner, and a growing card keeps its anchor; verifies **AC-8**, **AC-10**.
- Override merge: a screen table with only `spot` takes `gap`, `x`, `y` from the default; a table for an unknown id changes nothing; verifies **AC-9**.
- Parsing: `ws3 = "blue"`, `x = 120` and `spot = "middle"` each reset only that key; verifies **AC-11**.
- Hue rotation: rose `#E0457B` gives a red and a purple neighbor; a grey accent gives two greys; verifies **AC-3**.
- Ink: `#FFE066` gets black ink, `#1E2A78` gets white; verifies **AC-5**.
- Link: des-k purple's dark link is `#B27CE5` (the existing theme test's `#AE75E4` changes); rose `#E0457B` in light gives a link of at least 4.5:1 on the light card; verifies **AC-15**.
- Tile marks: des-k's nine `WS` keep white marks, `#FFF4B0` gets the ink mix; verifies **AC-16**.
- Writes: saving `ws2 = "#DDB43C"` (its default) removes the key; in a screen table `{spot, gap}`, setting gap to All screens' value removes `gap`; in a table `{spot}`, setting spot to All screens' value keeps it; verifies **AC-12**, **AC-13**.
- Palette pick: a fixed 32 byte `AccentPalette` whose shade at bytes 16 to 18 is `C4 2B 1C` gives `#C42B1C` in light, and bytes 4 to 6 give the dark shade; a 31 byte value keeps the previous palette, or purple at start; verifies **AC-1**.
- Theme: a custom workspace 3 of `#1E2A78` gives `on_ws[2]` white while the others stay `#14141A`; high contrast gives `hl_fg` for all nine; verifies **AC-5**, **AC-6**.
- By hand: change the Windows accent with des-k's settings page open and see it repaint; turn on high contrast and see your colors step aside; put the indicator top right on one screen and custom on another; verifies **AC-1**, **AC-6**, **AC-9**, **AC-13**.

## Build plan

Tracer Bullet: each slice works end to end before the next.

1. Position slice: `[indicator] spot, gap, x, y` parsing, the pure `place` function with its tests, the screen id in `Card`, and `show` using it, satisfies **AC-7**, **AC-8**, **AC-10**, **AC-11**.
2. Per screen overrides: the `[indicator.screens."<id>"]` tables, the merge, and their tests, satisfies **AC-9**, **AC-11**.
3. Workspace colors: `[colors] ws1..ws9` parsing, `theme_of` using them, the `ink` helper and per color `on_ws`, repaint after a settings reload that changes colors, satisfies **AC-4**, **AC-5**, **AC-6**, **AC-11**.
4. Accent: `AccentPalette` in `read_look`, `[colors] accent`, the neighbor hues, the tints switched to `acc_a` and `acc_b`, the mark left alone, satisfies **AC-1**, **AC-2**, **AC-3**, **AC-5**, **AC-6**, **AC-14**.
5. Settings page Colors section, from the approved board: sample, swatch, hex box (a new control kind), Reset, Windows or My own, the link and tile mark rules in `theme_of`, `Edit::Remove` and the owned path edit, satisfies **AC-12**, **AC-15**, **AC-16**.
6. Settings page Where it shows: `View.present`, screen chooser, the picture with Spots or Custom, gap, x and y, Same as all screens, Show it there, greying when off, satisfies **AC-13**.
7. Render tests: `cargo test render -- --ignored` gains a rose accent, a custom workspace set, and the indicator at a corner, so you can look at them as PNGs, satisfies **AC-3**, **AC-4**, **AC-7**.

## Consequences

**Positive**:
- des-k looks like part of your desktop and follows your Windows accent without any setup.
- Workspace colors are yours and stay the same in every des-k window.
- The indicator can stay out of the way on big screens, differently per screen.

**Negative / tradeoffs**:
- The approved look B was judged with des-k purple; a Windows accent you pick for other reasons may look less balanced in des-k, and the two neighbors can clash with some accents (a yellow gives a green and an orange).
- The settings page gets its first typed text box, a new control with its own keyboard and paste handling to build and keep working.
- A dark workspace color now changes the number's ink, so the card can look different from the approved design.
- `on_ws` and `on_acc` change from one value to one per color, touching every place that draws on them.
- The link rule makes des-k purple's approved dark link slightly lighter in every window.
- The Place row has two keyboard stops (the picture, then Spots or Custom), a second exception to one stop per row (spec 0012 AC-16) after Reset.
- Known and accepted: the dark count badge's ink is 4.44:1 by the ink rule, just under 4.5 (large bold digits, which meet the 3:1 line for large text).

**Neutral**:
- The des-k mark keeps its purple and red; the brand stays recognizable whatever the accent.
- Existing `settings.toml` files need nothing: every new key has today's behavior as its default.

## Follow-up

- [x] Done 2026-11-07 (handoff `des-k-colors-place` revision 2, approved; its eight proposals settled in Rationale): a design pass (`/ui-design-explorer`) for the Colors section, the 9 spot picture and the screen chooser. It settles the hex box's behavior (focus, caret, paste, Enter saves, Esc cancels, tab order, how an invalid value shows, the swatch updating live as you type a valid value) and the chooser's rules (hidden with one screen, remembered but unplugged screens listed as such, All screens chosen when the page opens). Workspace color rows follow the `workspaces` count live.
- [x] Enrolled on the scope as feature 23 (Slice 14).
- [x] Done 2026-11-07: `/sync` after the build: AGENTS.md's settings line should mention `[colors]` and that colors resolve only in `ui::theme_of`.
