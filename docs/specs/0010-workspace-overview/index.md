# 0010. Workspace overview: a GNOME style overview on every screen, with live previews

**Date**: 2026-10-28
**Status**: Accepted
**Amended by**: [0017](../0017-look-b-screens/0017-overview.md): AC-13 (look), and the fade time becomes `[overview] fade_ms`.

## Summary

Win+Tab (or your touchpad swipe up, once you set it to Win+Tab) opens an overview on every screen at once. Each screen shows a strip of its workspaces along the top, each a tiny live copy, and the windows of the workspace it shows spread out below so none overlap. Click a window to go to it, click a workspace to switch there, or drag a window onto any workspace on any screen to send it. Windows itself draws the live previews (DWM thumbnails, the same pictures Alt+Tab uses), so des-k only draws the dark background, the frames and the labels.

## Requirements

**User stories**:
- As the des-k user, I want to see what is on every workspace of every screen at a glance, so that I can find a window without stepping through workspaces.
- As the des-k user, I want one click to land me on a window or a workspace.
- As the des-k user, I want to drag a window to another workspace, on this screen or another, so that sorting windows is direct.
- As the des-k user, I want to try the animation, label and live list choices myself, so the settings offer each.

**Acceptance criteria** ("listed windows" means what `ws::app_windows` accepts, as in spec 0007; "N" is the `workspaces` setting; screens are ordered left to right, then top down, as in spec 0007):
- **AC-1**: Win+Tab (the `overview` setting) opens an overview on every screen at once, each covering its whole screen, taskbar included. Win+Tab is swallowed: Task View does not open. The keyboard selection starts on the screen under the mouse. A touchpad gesture opens it once that gesture is set to the custom shortcut Win+Tab in Settings, Bluetooth and devices, Touchpad, Advanced gestures (the default "Task view" action does not go through the keyboard). If Windows refuses des-k focus, the overview closes again and the log says so.
- **AC-2**: Along the top of each screen, a strip shows that screen's N workspaces as tiles in a row, numbered 1 to N. Each tile is a tiny copy of the screen with that workspace's listed windows as live previews at their real places (a minimized window at its restored place), back to front as they stack. The workspace the screen shows has an accent outline. An empty workspace's tile shows only its number.
- **AC-3**: Below the strip, the listed windows of the workspace the screen shows, minimized ones included, are spread out with no overlap, each scaled to fit its slot (never larger than its real size), placed near where it really sits (a window on the left lands on the left). Each is a live preview; a minimized one shows its last picture. When there are none, the area says "No windows".
- **AC-4**: A window des-k hid shows its live preview like any other. If Windows draws nothing for a window (checked in build step 1), it shows as a card instead: its icon and title on a dark box of the window's shape.
- **AC-5**: Labels follow the `labels` setting: `"hover"` (default) shows the title under the window the mouse is over; `"always"` shows the app icon and title under every window in the spread; `"none"` shows no text. Titles are cut with … when too long. Strip tiles never carry titles.
- **AC-6**: A click on a window in the spread focuses it (restored if minimized) and the overview closes on every screen. A click on a window inside a strip tile lands on it like the finder's Enter (spec 0007 AC-7 and AC-8). A click elsewhere on a tile switches that screen to that workspace exactly like a switch (the indicator shows there) and the overview closes; a click on the shown workspace's tile just closes it. A click on empty background closes the overview and gives focus back to the window you came from (the desktop if it is gone).
- **AC-7**: Dragging a window (from the spread or from inside a tile, past the system drag distance) carries a small live preview of it under the mouse, across screens. Dropping it on any screen's tile moves it to that workspace of that screen: hidden there if that workspace is not shown, visible and on top if it is. Dropping it on any screen's spread moves it to the workspace that screen shows. From another screen it lands at the same relative place and size, scaled to the new work area (spec 0007 AC-9 rules: maximized stays maximized, minimized is restored, a hidden one never flashes on its old screen). No screen switches, the overview stays open and redraws, and `state.txt` records the move. Dropping where it already is, pressing Esc, or a right click during the drag cancels it. If the move fails, the window stays exactly where it was, the screen shows the note "Can't move this window" for about two seconds, and the log says why.
- **AC-8**: Keys: the arrows move the selection to the nearest window in that direction on the selected screen; Tab steps it through every spread window of the selected screen in reading order (rows top down, left to right within a row) and Shift+Tab steps back the same way, both wrapping around within that screen, never on to another screen (on a screen whose spread is empty they do nothing); the selected screen is the one under the mouse: the selection starts there (AC-1), and when the mouse moves onto another screen the selection moves with it, to that screen's first window; 1 to N switch the selected screen to that workspace and close the overview; Enter acts like a click on the selected window; Esc or Win+Tab closes the overview with focus back. The selected window has an accent border. The first selection is the window you came from when it is on that screen, else the first window there.
- **AC-9**: Hovering a window in the spread shows an X on its top right corner. A click on the X, a middle click on the window, or Ctrl+W on the selected window asks it to close, as its own X button would. The overview stays open; the window drops out once it is gone. If the close cannot be sent, the note "Can't close this window" shows and the log says why.
- **AC-10**: The `live` setting: `true` (default) adds windows that open, drops windows that close or hide, and rearranges the spread and tiles to match, within about a quarter second. `false` takes the list when the overview opens: windows that close or hide drop out and the rest stay put; new windows show the next time it opens.
- **AC-11**: The `animation` setting: `"off"` (default) opens and closes at once. `"fade"` fades the whole overview in and out over about 250 ms. Any choice still answers clicks and keys during the animation.
- **AC-12**: Any des-k switch, jump, send or finder shortcut, or a screen change, while the overview is open closes it first (focus back), then runs as usual. Win+Tab while the finder is open closes the finder and opens the overview. The overview's windows never show in Alt+Tab or the taskbar, des-k never hides them, and follow activation ignores them.
- **AC-13**: The overview looks like the indicator and the finder: solid dark grey background, light text, the accent blue for the shown workspace and the selection, sharp at every screen's scaling.
- **AC-14**: `settings.toml` `[shortcuts]` takes `overview`, a list like `finder`, default `["Win+Tab"]`, with the spec 0001 rules; a combo also in a switch, send or finder list turns `overview` off with a log line. A new `[overview]` table takes `animation` (`"off"`, `"fade"`; default `"off"`), `labels` (`"hover"`, `"always"`, `"none"`; default `"hover"`) and `live` (default `true`); a bad value resets just that key with a log line.

## Decision

**Chosen option**: Option 1: one Win32 popup per screen with DWM live thumbnails.

A new `src/overview.rs` owns one borderless tool popup per screen; Windows draws every preview into it through `DwmRegisterThumbnail`, des-k paints the background, frames and labels with GDI like the indicator, and every change to where a window lives goes through `ws`.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`) · `rust-serde` (`rewrite-rs/skills`, `.claude/skills/rust-serde/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch**: nothing new in `state.txt`. Everything below lives in memory on the main thread, one session per open.

| Piece | Fields | Owner | Life |
|---|---|---|---|
| `Session` | `screens: Vec<Screen>` (left to right, then top down), `selected: (usize, Option<usize>)` (screen, slot), `hover: Option<(usize, Hit)>`, `drag: Option<Drag>`, `came_from: HWND`, `anim: Option<Anim>`, `live: bool` (from settings at open) | `overview.rs`, `thread_local` | open to close |
| `Screen` | `device: String`, `popup: HWND`, `rect: RECT` (whole monitor), `dpi: u32`, `shown: usize` (0 based), `tiles: Vec<Tile>` (N), `slots: Vec<Slot>`, `note: Option<(&'static str, Instant)>` | `overview.rs` | per open |
| `Tile` | `rect: RECT` (in popup coordinates), `minis: Vec<Mini>` (back to front) | `overview.rs` | rebuilt on each layout |
| `Mini` / `Slot` | `hwnd`, `thumb: isize` (`HTHUMBNAIL`, unregistered on drop), `rect: RECT` (destination), `real: RECT` (the window's real or restored rect, screen coordinates), `card: bool` (AC-4 fallback); a `Slot` also has `title: String`, `icon: HICON` (borrowed, never destroyed) | `overview.rs` | rebuilt on each layout |
| `Drag` | `hwnd`, `from: (usize, Option<usize>)` (screen, workspace), `ghost: HWND` (small topmost popup with its own thumbnail), `start: POINT` | `overview.rs` | one drag |
| `Hit` | `Slot(i)`, `Close(i)` (the hover X), `Mini(tile, i)`, `Tile(i)`, `Background` | `overview.rs` | per mouse move |
| Settings | `overview: Vec<Combo>` default `[Win+Tab]`; `overview_animation: Animation` (`Off`, `Fade`), `overview_labels: Labels` (`Hover`, `Always`, `None`), `overview_live: bool` | `settings.rs` | read once at start |
| `ws` additions | `pub fn active(device: &str) -> usize`; `pub fn show_workspace(device: &str, ws: usize)` (a switch on that screen, through `switch_to`); `pub fn place(hwnd, device: &str, ws: usize) -> Result<(), Miss>` (AC-7: `bring` when `ws` is the shown one, else cloak if needed, `move_to` while cloaked when the screen differs, and move it between `hidden` lists) | `ws.rs` | |

`DEFAULT_FILE` gains `overview = ["Win+Tab"]` under `[shortcuts]` after `finder`, and:

```toml
[overview]
animation = "off"   # "off" or "fade"
labels = "hover"    # "hover", "always" or "none"
live = true         # false: the list is taken when the overview opens
```

**Layout** (all sizes at 96 dpi, scaled per screen):
- Strip: tiles keep the screen's aspect, height 12% of the screen, 16 px gaps, centered, 24 px from the top; shrunk so all N fit the width with 48 px side margins.
- Spread: the area below the strip with 48 px margins. Pick the row count that makes the windows largest; fill rows by each window's real center top to bottom, then left to right within a row; scale each to fit its cell keeping its aspect, never above 1.0; center the rows. Hover X: a 24 px round button on the slot's top right corner, in its own tiny tool popup above the overview (a DWM thumbnail always draws over its host's own painting). Labels: 22 px band under each slot. Selection: 3 px accent border just outside the slot.
- Drag ghost: 240 px wide, the window's aspect, centered on the mouse.

**State transitions** (one session): closed → open (Win+Tab) → closing (click, Esc, Enter, digit, Win+Tab, another des-k shortcut, a screen change) → closed. Within open: idle ↔ dragging (past drag distance; drop or cancel returns to idle). With `animation` other than `"off"`, open and closing run their animation first; input is taken throughout.

**API surface** (messages and functions; no network):

| Entry | Kind | Key inputs | Key outputs | Who calls | Key errors |
|---|---|---|---|---|---|
| `WM_OVERVIEW = WM_APP + 7` | thread message | none | `overview::toggle()` | `keys.rs` hook on an `overview` combo, swallowed with `VK_MASK` | Windows refuses focus: close, log |
| `overview::toggle()` / `close(give_back)` | fn | `give_back: bool` | opens or closes every screen's popup | main loop; `finder::toggle` and the switch, jump and send handlers call `close(true)` first | none |
| `overview::key(&MSG) -> bool` | fn | the message | true when used | main loop before dispatch, like `finder::key` | none |
| `ws::go_to(hwnd)` | fn (spec 0007) | window | switch plus focus | click, Enter | `Miss::Gone`: drop it from the layout |
| `ws::show_workspace(device, ws)` | fn (new) | screen, workspace | a switch there | tile click, digits | screen gone: log |
| `ws::place(hwnd, device, ws)` | fn (new) | window, screen, workspace | moved, `state.txt` saved | drop | `Miss::Gone`, `Miss::CantMove` (note, log) |
| close request | `PostMessageW(WM_CLOSE)` | window | asked to close | X, middle click, Ctrl+W | post fails: note, log |
| refresh | `SetTimer`, 250 ms | none | layout rebuilt (live) or gone windows dropped (not live) | popup timer | none |

**Value sourcing**:

| Action | Value produced / displayed | Source |
|---|---|---|
| Open | which screens, their rects and order | `ws::screens()`, ordered by spec 0007's `screen_numbers` rule |
| Open | screen the selection starts on | `ws::under_mouse()` |
| Open, close | the window you came from | `GetForegroundWindow` at open, root owner (`GA_ROOTOWNER`) |
| Strip | number of tiles | `settings::get().workspaces` |
| Strip, spread | which workspace a screen shows | `ws::active(device)` (new) |
| Strip, spread | each window's screen and workspace, and whether it is on screen | `ws::windows()` (`Place`) |
| Strip, spread | each window's real rect | `GetWindowRect`; minimized: `GetWindowPlacement` `rcNormalPosition` shifted by the primary work area (as `ws::move_to` does) |
| Strip stacking | back to front order | on screen: the reverse of `ws::windows()` order (front to back); hidden: the reverse of the saved `hidden` list order (front = top) |
| Previews | the picture | `DwmRegisterThumbnail(popup, hwnd)`; source size from `DwmQueryThumbnailSourceSize` |
| Keys (AC-8) | Tab order | the selected screen's spread slot rects by center: top down, then left to right |
| Keys (AC-8) | selected screen after open | the screen whose overview popup last got a mouse move (`WM_MOUSEMOVE`); on a change of screen the selection is that screen's first window |
| Card fallback (AC-4) | icon, title | `finder::icon`, `ws::title` (made `pub(crate)`) |
| Labels | title, icon, style | `ws::title`, `finder::icon`, `settings` `overview_labels` |
| Colors | background, text, accent | `finder::BG`, `finder::TEXT`, `indicator::ACCENT` |
| Drop | target screen and workspace | cursor position hit tested against every screen's tiles, then spreads; the spread means `ws::active(device)` |
| Drop | where the window lands | `ws::place` (relative place and size from `ws::map_rect`) |
| Live refresh | the current list | `ws::windows()` again every 250 ms |
| Animation | style, duration | `settings` `overview_animation`; fixed 250 ms fade, ease out (squared) |
| Notes | text, how long | fixed strings in AC-7 and AC-9; 2 s |

**Key invariants**:
- The overview never cloaks or uncloaks anything itself; every move goes through `ws` (AGENTS.md rule).
- Every thumbnail registered is unregistered when its slot, tile or session goes away; closing the overview leaves no popup, ghost or thumbnail behind, including on the panic and `--quit` paths.
- A failed `ws::place` changes nothing about the window or `state.txt`.
- At most one overview session; Win+Tab while open closes it.

**Security model**: no new data leaves des-k. The popups get no message filters, so normal programs cannot post to them (spec 0008 keeps the filter list to `WM_CLOSE`, `WM_TRAY` and `TaskbarCreated`). `settings.toml` values here are only enums and a bool, never a path or program.

**Configuration required**: none beyond the `settings.toml` keys in AC-14.

**Critical test scenarios**:
- Happy path: three screens, windows on several workspaces; Win+Tab, every screen shows its strip and spread with live pictures; click a hidden window in a tile on another screen and land on it, verifies **AC-1**, **AC-2**, **AC-3**, **AC-6**
- Failure case: drag a window to another screen's hidden workspace, then drag one that cannot move (the debug copy run without admin, onto an admin window); the first lands hidden there with no flash, the second stays put with the note, verifies **AC-7**
- Hidden previews: a window on a hidden workspace shows its real picture, or the card if step 1 finds Windows draws nothing, verifies **AC-4**
- Permission: the overview popups never show in Alt+Tab, never get hidden by a switch, and Task View never opens on Win+Tab, verifies **AC-1**, **AC-12**

## Build plan

1. [x] Check that a DWM thumbnail draws a window des-k hid (the scratchpad probe, run while a screen shows an empty workspace). If it does, AC-4's card is only for windows that draw nothing; if it does not, cards are what hidden windows show, satisfies **AC-4**
2. [x] Thin thread: the `overview` shortcut and `WM_OVERVIEW` in `keys.rs`, `src/overview.rs` with a dark popup per screen, numbered tiles, the spread in a plain grid with live thumbnails, click a window to go there, click a tile to switch, Esc and background click to close, and the close first rule for other shortcuts. Add `ws::active` and `ws::show_workspace`, satisfies **AC-1**, **AC-3**, **AC-6**, **AC-12**, **AC-13**, **AC-14**
3. [x] Strip mini windows at real places, the near their real place spread, card fallback, and the `labels` setting, satisfies **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-14**
4. [x] Keys (arrows, Tab, digits, Enter) with the selection border, and closing windows (hover X popup, middle click, Ctrl+W), satisfies **AC-8**, **AC-9**
5. [x] Drag and drop across screens with the ghost popup, and `ws::place`, satisfies **AC-7**
6. [x] The 250 ms refresh and the `live` setting, satisfies **AC-10**, **AC-14**
7. [x] The `animation` setting: fade (zoom was built too, then dropped on 2026-10-30), satisfies **AC-11**, **AC-14**
8. [x] Settings tests (defaults parse, bad values reset one key, conflicts), then try it on your screens and reinstall with `install.ps1`, satisfies all ACs

## Consequences

**Positive**:
- One look shows every workspace on every screen, and moving a window anywhere is a single drag.
- Windows does the expensive drawing on the GPU; des-k keeps to the GDI it already uses.
- Reuses the finder's window list, icons and `go_to`, and `bring`'s cross screen move.

**Negative / tradeoffs**:
- Win+Tab no longer opens Task View; you lose it unless you change the `overview` setting.
- The biggest feature so far (one new file, about the finder's size or more), with three settings that each double some test paths.
- In live mode the spread can move as you aim, by your choice; `live = false` is there for when it bothers you.

**Neutral**:
- `finder::icon`, `finder::BG`, `finder::TEXT` and `ws::title` become shared (`pub(crate)`).
- The touchpad swipe up needs a one time change in Windows Settings (AC-1).

## Follow-up

- [ ] Build step 1 decides whether hidden windows show live pictures or cards; if cards, say so in this spec's AC-4.
- [ ] A dimmed wallpaper background (you chose solid dark for now) · needs a decision
- [ ] `verify.md` for this spec, written when the build starts
