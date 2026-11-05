# 0017. Indicator: the card, and the dots as a second style

## Summary

The indicator becomes a card: a number block in the workspace's color, the screen's name when you have two or more screens, and a bar per workspace, with an optional line under it ("Sent to 3", "Pinned"). Today's dots stay as a second style, restyled, chosen with `[indicator] style`. This is the first slice, so it also builds the shared layer every later screen uses (see [index.md](index.md), Shared design).

**Design source**: `pdesk-design/restart/indicator-r5.html` (r2.3), sha256 `706d62e0ab52060596d6340c87c53a0597f28b4fb151efce093f87a55ab9c410`, captures `restart/captures/ir5-*` (sums `SHA256SUMS-ir5`). Approved 2026-11-05.

**Replaces**: the dots drawing in `src/indicator.rs` and the look wording in specs 0004, 0014 and 0016 (each gets a pointer line here). Behavior (when it shows, hold and fade timing) is unchanged unless an AC below says so.

## Acceptance criteria

- **AC-IND-1**: With `style = "card"` (the default), every switch, jump, send, pin, follow, finder and overview landing shows the card as drawn in the design:
  - the number block in that workspace's color with its number;
  - one bar per workspace, the current one filled;
  - with two or more screens, the screen's name (`ws::screen_label`, see index).

  The card is at most 380 dip wide; a long name ends in … and a long line wraps.
- **AC-IND-2**: The card's lines, in the design's words: sent, sent back, already there, pinned, unpinned, past the count, a pinned window sent. A card with a line stays at least 1500 ms, even with `hold_ms = 0`.
- **AC-IND-3**: Where it shows:
  - switch and jump: the screen under the mouse;
  - send, pin, follow, finder and overview: the screen of the window or tile acted on;
  - screen notes (spec 0016): the main display, or the screen that came back;
  - the settings page's Show button: the settings page's screen.
- **AC-IND-4**: Screen notes (spec 0016) are passed as data (screen name, what happened, counts, whether it left), not as a finished string. The card says today's "N windows moved here" words. When a note's windows went to another screen, "here" becomes that screen's name (main display). Spec 0016 AC-11's notes fit one card.
- **AC-IND-5**: Light or dark follows `SystemUsesLightTheme`. The card's see through level is one token (`ui` tokens, the design's card alpha). In high contrast the card uses system colors and is 100% opaque. It is also 100% opaque when Windows' transparency effects are off (`Personalize\EnableTransparency` = 0, read by `ui::theme()`).
- **AC-IND-6**: The card stays a layered window, as today: `WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT`, faded with `SetLayeredWindowAttributes` (`indicator.rs:116`). It is made through `ui::popup`'s layered flag, with round corners and the shadow from DWM. Slice 1 checks by hand that DWM draws the shadow on this layered window. If it doesn't, the spec gets a fallback before slice 2 (index popup helper). Text is drawn on the card color first, so GDI text never loses its alpha.
- **AC-IND-7**: `style = "dots"` draws today's dots restyled as in the design (same tokens, same placement rules, same lines). A bad value resets to `"card"` with a log line.
- **AC-IND-8**: The fade goes through `ui::motion_ms([indicator] fade_ms)`: instant when `fastest` is on (index AC-6).
- **AC-IND-9**: Each card raises a UI Automation notification with its text ("Workspace 3 of 4, DELL U2720Q, Sent to 3"), so Narrator reads switches without the card taking focus.
- **AC-IND-10**: The settings page's Indicator section and the first run's More panel get the Style row (Card, Dots; greyed while the indicator is off) and a preview of the chosen style. Those rows are built in their own slices ([0017-settings-page.md](0017-settings-page.md), [0017-first-run.md](0017-first-run.md)); this slice only exposes the drawing as a call they can use: `indicator::draw_preview(canvas, style, screen)`.

## Build notes

- Colors are the shared tokens; `indicator::ACCENT` moves to `ui.rs` for the finder and overview until their slices.
- Smooth shapes come from GDI+ on the Canvas.
- `settings.rs`: `[indicator] style` via `choice()` (like `overview.animation`); `INDICATOR_KEYS`; `DEFAULT_FILE`; `settings::write` values; the problem title; a test. Spec 0012's "[indicator] 3" becomes 4.
- Top level `fastest` lands in this slice too (index data model).
- Render tests: the card for one screen and for two screens, each line, a long name, dots; light, dark and HC; 100% and 150%. Output goes to `target/indicator/`.

## Check on the real machine

- DWM shadow and round corners on the layered card, in light, dark and with transparency off.
- DWM shadow on a plain non layered test popup made by `ui::popup` (for the slices after this one).
- Narrator reads a switch.
- Mixed scaling between the laptop and an external screen.

## Rationale (short)

The card is what you asked for (number block, screen name, bars). The dots stay because you like keeping a familiar look as an option. It goes first because it is the smallest window, shows on every switch, and proves the shadow, smooth shapes and theme path that every later screen reuses.
