# 0020. Your own colors and where the indicator appears: rationale

The decision record for [index.md](index.md), the build spec.

## Context

des-k's look B (spec 0017) draws every window from one set of tokens in `src/ui.rs`: the accent `acc` is the des-k mark's purple `#8E3FD9`, and the nine workspace colors are one fixed list `WS`, the same hex in light and dark. Six windows read them: the indicator, finder, overview, tray menu, welcome and settings page. Nothing about them can be changed without rebuilding. In look B the accent appears twice: solid on buttons, toggles and the selected row, and as lighter tints on section headers, count badges, picker marks and the first run's progress line.

The indicator always shows centered on the work area of the screen you switched (`indicator.rs`, `show`). On a large or ultrawide screen the middle is where you are working, and with three 43 inch screens planned, the right spot differs by person and by screen. Screens range from 1366 pixels wide to over 5000, and Windows scale changes the pixel count again, so one pixel position cannot suit them all.

Constraints that shape this: settings live in `settings.toml`, a bad value resets only its key (spec 0001), and the settings page writes one value back in place (spec 0012). Nothing in the data folder may name a program to run; colors and positions are plain values, so they are safe. High contrast mode already replaces every color with Windows' own (`ui::high_contrast`). Windows exposes the user's accent, with lighter and darker shades of it for dark and light mode, in the registry under the same `HKCU` area `ui::read_look` already reads for light and dark mode.

The indicator can already be turned off: `[indicator] enabled = false`, or the settings page's Workspace indicator toggle.

## Options considered

### Option 1: Fix in place, colors resolved once in `ui::theme_of`

Keep look B's token set. When `theme_of` builds a light or dark `Theme`, it replaces `acc`, the tints, `link`, `held`, `ws` and the inks with values resolved from the Windows accent (read with the rest of Windows' look in `read_look`) and from `settings::get()`. Every window keeps reading the same fields, so only `ui.rs` and the few places that compute a tint change. Position becomes one pure function used by `indicator::show`.

**Pros**: one place decides every color, so all six windows agree; high contrast keeps winning for free, since it returns before any of this; reuses the existing 1 s look refresh and repaint broadcast; the position math is testable without a screen.
**Cons**: tints that are computed inline in a window (`mix(acc, …)`) must be found and pointed at the new neighbor fields; `theme_of` now reads settings, so a settings reload that changes colors must also trigger the repaint broadcast.

### Option 2: A user theme file replacing the tokens

A `[theme]` table, or a separate file, that can override any look B token (`bg`, `card`, `line`, `acc`, every workspace color), with presets.

**Pros**: maximum freedom; themes could be shared between people.
**Cons**: far more than you asked for; many combinations break the approved look (your earlier rejection of a window tinted by workspace color is one example); every token becomes a support question; much larger settings page.

### Option 3: Indicator only colors and position

Colors and position as `[indicator]` settings that only the popup reads; other windows keep look B's fixed colors.

**Pros**: smallest change, touches only `indicator.rs` and settings.
**Cons**: workspace 3 could be blue in the indicator and green in the finder, breaking the "same color everywhere" cohesion look B was approved for; the accent following Windows would not reach the settings page, buttons or toggles, which is what you asked for.

## Rationale

The forces are cohesion across six windows that share one token set, a settings file that must survive any bad value, and an existing look refresh that already repaints every window when Windows changes. Option 1 meets all three: colors are decided once in `theme_of`, so no window can disagree; each new key parses and resets on its own; and the Windows accent rides the same `read_look` path as light and dark mode, so the 1 s timer catches a change even when the admin des-k misses Windows' broadcast (UIPI). Option 3 would split the colors between windows, and Option 2 solves a problem you don't have while making the approved look easy to break.

Percent for custom coordinates was your call after weighing resolutions: it lands in the same place on a laptop and on an ultrawide, and survives a change of Windows scale. The edge spots use a scaled pixel gap instead, because a gap that grows with screen size looks wrong on a wide screen.

Decisions made for you (each with its runner up):
- **Where the accent comes from**: the registry value `AccentPalette` (Windows' 8 shades of the accent), read with the rest of the look. Runner up: WinRT `UISettings`, which needs a new `windows` crate feature and COM activation for the same numbers.
- **Which shade per mode**: light mode uses the palette's Dark1 shade, dark mode its Light2, the shades Windows' own apps fill accent buttons with. Runner up: the base shade in both, which you turned down.
- **Your typed accent**: used exactly as typed in both modes, with the ink rule keeping text readable. Runner up: lighten it in dark mode, which would show a color you didn't type.
- **Neighbor order**: hue minus 30 is the first neighbor, hue plus 30 the second. Arbitrary but fixed, so the same accent always looks the same.
- **Settings shape**: a new `[colors]` table with `accent` and `ws1` to `ws9`, one key each, so the settings page writes one value at a time with `settings::write`. Runner up: an array of nine, which toml_edit can't change one entry of without rewriting the line and losing your formatting.
- **Spot words**: `top-left` and so on, the same style as `style = "card"`.
- **Ink rule**: WCAG relative luminance contrast, the higher of black (`#14141A`, today's `on_ws`) or white wins. Runner up: Windows' simpler brightness cut at 128, which misjudges saturated blues.
- **Screen id reaches the indicator in the card**: `Card.screen` is the screen's display name (and `None` with one screen), so `Card` gains a new `screen_id: String`, set in `ws::card`, where the id is already in hand. The indicator never takes the `ws` lock (it is called while that lock is held).
- **A card that grows**: each new line is a new `indicator::show` call, as today, and every `show` runs `place` again, so the anchor holds.
- **Missing palette**: warn once when it goes missing and once when it comes back, never every second. A failed read keeps the last good palette, so a half written value never flashes purple.

Settled after the design pass (handoff `des-k-colors-place` revision 2, approved by you on 2026-11-07, in `pdesk-design/colors/`), each with its runner up:
- **The action's name**: Same as all screens, the words the rows already use (From All screens). Runner up: the spec's Use the default, which reads as factory defaults, not the All screens place.
- **My own's first color**: the accent in use at that moment, and the last own color while the page is open. Runner up: an empty box, which repaints every window to purple the instant you choose it.
- **When a write removes a key**: a workspace color equal to `WS[i]`, and a screen key equal to All screens' value, are removed, so the file only holds what differs; a screen table's last key is never removed that way, so walking the spots can't silently drop a kept place. Only Same as all screens, or Reset on the last key, removes a table. Runner up: always write the value, which leaves screens pinned to a copy of today's default.
- **Tile marks**: white unless under 1.6:1 on the tile, then `mix(ws, #14141A, 0.55)`. des-k's nine keep white marks. Runner up: always the ink, which changes all nine approved tiles.
- **Link contrast**: `link` as today, then mixed toward black (light) or white (dark) 5% at a time until 4.5:1 on `card`. It changes des-k purple's approved dark link `#AE75E4` (4.36:1 on `#2B2B2B`) to `#B27CE5` in every window; rose's light link goes from 3.96:1 to 4.74:1. Runner up: keep the link as built, under the WCAG line for small text.
- **`held`**: a light tint, `mix(card, acc, 0.12)` in light and `0.20` in dark, as built (you chose this during the build). Runner up: `acc` solid, which this spec first said and which hides the key cap's label.
- **Place row stops**: the picture is one stop and Spots or Custom is its own stop after it, as Reset is. Runner up: Custom as a tenth arrow stop, which jumps the card on a stray key.
- **Reset on a bad `[indicator]` value**: writes the default back (`center`, 24, 50), as every `[indicator]` row's Reset does today, since `DEFAULT_FILE` lists those keys. `[colors]` and screen table Reset remove the key.
- **Screens for the page**: `ws::View` gains `present: Vec<Present>` (`id`, `device`, `name`, `full: RECT`, `work: RECT`), filled in `ws::view` where ids are in hand. Runner up: widen `ws::screens()`, which other callers use for device names only.
- **Kept inside caption**: `indicator::place` becomes `pub(crate)`; the page calls it with the plain card's size (as `draw_preview` measures it) at that screen's scale and says the card is kept inside when the result differs from the unclamped center.
- **Clipboard for the hex box** (settled 2026-11-07, during the build): copy and paste go through Win32's own clipboard calls in the `windows` crate already used, which needs the `Win32_System_DataExchange` and `Win32_System_Memory` features. Runner up: no Ctrl C, X or V, which breaks the paste rule the approved design asks for.
