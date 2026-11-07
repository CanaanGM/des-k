# 0017. Tray menu: des-k's own popup

## Summary

The tray menu stops being Windows' `TrackPopupMenu` and becomes a des-k popup in look B. It has:
- a header with des-k's version;
- your screens (chips for one or two screens, a map for three or more);
- the hiding warning when it applies;
- the items Shortcut hints, Settings, Open log, Check for updates and Quit des-k.

Narrator must read it, and the keyboard must open and drive it.

**Design source**: `pdesk-design/restart/tray-r4.html` (r3, direction B), sha256 `4541256257b9e9af09c8ff320b444cfe36f068743291e620784c5540f7d1e457`, captures `restart/captures/trr4-*` (sums `SHA256SUMS-trr4`). Approved 2026-11-05.

**Replaces**: `tray::menu()` (`src/tray.rs:248`) and spec 0015 AC-9 (amended below). Unchanged:
- the tray icon and its tooltip (kept visible under version 4 by `NIF_SHOWTIP`);
- Windows' notifications and their words;
- the update check itself;
- the message filters (spec 0008).

## Acceptance criteria

- **AC-TRY-1**: Clicking the tray icon (left or right) opens the popup at the icon. The anchor point is the one version 4 passes in `wParam` (or `Shell_NotifyIconGetRect` for a keyboard open). The popup's position comes from `CalculatePopupWindowPosition` with `TPM_WORKAREA`, keeping clear of the icon's rectangle, as Windows places a tray menu. It is look B:
  - 288 dip wide, card color, radius 8, 1 px edge;
  - DWM shadow (`ui::popup`), `WS_EX_TOOLWINDOW`, topmost;
  - items 32 dip high (40 with a second line), 16 dip icons;
  - 14/20 text and 12/16 grey second lines;
  - hover grey; keyboard focus adds a 2 px text colored ring;
  - greyed items in the third grey (`GrayText` in HC);
  - "New" in the one purple.
- **AC-TRY-2**: The header shows the mark, "des-k" and the version: `update::ours()`, else the Cargo version, as `answered()` does.
- **AC-TRY-3**: Screens:
  - **One or two screens**: workspace chips with the indicator's screen names (`ws::screen_label`).
  - **Three or more**: a map of the screens as Windows arranges them, from `ws::screen_info()` rectangles and `ws::active`.
- **AC-TRY-4**: How the map is drawn:
  - It is fitted into 264 × 88 dip. Each screen is in its workspace's color with the number: 12 px normally, and 10 px when the screen is drawn under 18 px on its shorter side. When it is drawn under 12 px on its shorter side, no number shows. No screen is drawn smaller than 8 dip on a side; if the fit would make one smaller, the whole map scales so that it isn't, and stays centred.
  - A 3 px bar (DPI scaled) marks the screen the popup actually landed on (`MonitorFromPoint` of its placed rectangle), and never covers the number. Normally that is the main display, whose taskbar holds the icon.
  - Each screen has a tooltip with its name, plus "main display" where it applies.
  - The map is not clickable and does not look like a button.
  - Rectangles are physical pixels, so with mixed scaling it can differ slightly from Windows' picture.
- **AC-TRY-5**: Items, in spec 0015 AC-1's order:
  - Shortcut hints, Settings;
  - Open log (greyed when the log is missing), Check for updates;
  - Quit des-k, with its consequence line from `setup::state()`.

  Groups are split as in the design.
- **AC-TRY-6**: While hiding is broken (today's `BROKEN`), a note box sits under the header, titled as the balloon: "des-k cannot hide windows".
- **AC-TRY-7** (amends spec 0015 AC-9): while hiding is broken, every update answer shows on the Check for updates item until the next check:
  - newer: "Update to 0.4.0" with New, second line "Opens its page on github.com";
  - up to date;
  - failed.
- **AC-TRY-8**: Keyboard:
  - arrows, Home, End, Enter, Esc;
  - a letter only moves focus to its item, so Q never quits at once.

  The icon moves to `NOTIFYICON_VERSION_4`, so a keyboard open is told apart from a click; a keyboard open puts focus on the first item. Every tray message is read the new way, balloon clicks included:
  - `tray::add` sends `NIM_SETVERSION` after its `NIM_ADD` (or `NIM_MODIFY`) at `tray.rs:132`. That function also runs on `TaskbarCreated` (`tray.rs:73`), so the version is set again after every re-add.
  - `add` sets `NIF_SHOWTIP` beside `NIF_TIP`, or version 4 hides the tooltip.
  - The event is `LOWORD(lParam)`, and the icon point is in `wParam`.
  - Only `NIN_SELECT` (left click), `NIN_KEYSELECT` (keyboard) and `WM_CONTEXTMENU` (right click, or Shift+F10 and the menu key) open or close the popup. `WM_LBUTTONUP` and `WM_RBUTTONUP` are ignored, or every click would toggle twice.
  - An open counts as keyboard when it is `NIN_KEYSELECT`, or a `WM_CONTEXTMENU` with no `WM_RBUTTONUP` just before it.
  - A repeat of the same event within `GetDoubleClickTime()` is ignored (Enter often sends `NIN_KEYSELECT` twice).
  - Balloon events (`NIN_BALLOONUSERCLICK` and the rest) keep today's handling, read from `LOWORD(lParam)`.
- **AC-TRY-9**: Not modal:
  - Any des-k action (shortcut, gesture, follow, finder, overview) closes it first.
  - It closes when it loses focus.
  - A second click on the icon closes it. Guard: when the popup closed because it lost focus, an open event within `GetDoubleClickTime()` of that close is ignored. So the click on the icon that took its focus doesn't reopen it.
  - The tray's open and close join main's close first list (`main.rs:218`): a switch, jump, send, pin or screen change closes the popup, as it does the finder and overview.
  - Closing it never ends des-k's main loop (only `WM_CLOSE` to the indicator or the tray window does).
- **AC-TRY-10**: Narrator reads it as a menu with menu items: name, second line, enabled state, and focus events. Screens are read with their workspace, plus "main display" where it applies.
- **AC-TRY-11**: The popup is one of des-k's own windows: never hidden, never listed.

## Build notes

- Render tests: one, two, three, twelve (4 × 3 wall) and twelve mixed screens; checking, newer, failed; broken; each Quit text; keyboard focus; light, dark and HC. Output goes to `target/tray/`.
- The popup is a new window class in `src/tray.rs` (or `src/tray_menu.rs` if it passes about 300 lines), drawn through the Canvas.

## Check on the real machine

- Narrator on the popup.
- A keyboard open (Win+B, then arrows to the icon, then Enter or Shift+F10).
- A balloon click after the move to version 4.
- That only the main taskbar holds tray icons on build 26200.

## Rationale (short)

Windows' own menu can't show the screen map, the note box or the second lines in look B. Direction A (Windows' menu with look B words) passed review, but you chose B after asking how it scales to many screens. The keyboard and Narrator rules are the price of leaving the native menu.
