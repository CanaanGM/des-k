# 0017. Shortcut hints window

## Summary

A new des-k window lists every shortcut and gesture you have now, read live from `settings.toml`. It opens from the tray menu, or from its own key, which has no default and can be set from the window's footer or in Settings. It is built in the same slice as the tray menu.

**Design source**: `pdesk-design/restart/hints-r3.html` (r2.1), sha256 `f30523c360ac08d4bc5b8ef8a2d8353eda964bc01c217ce09b5fad5a2b8a7edf`, captures `restart/captures/hr3-*` (sums `SHA256SUMS-hr3`). Approved 2026-11-04.

**Replaces**: nothing (new).

## Acceptance criteria

- **AC-HNT-1**: Tray › Shortcut hints, or the `[shortcuts] hints` key, opens it centred in the work area of the screen under the mouse, in front, with focus. On a short screen (1366 × 768 at 125%) it is 560 dip tall and the middle scrolls, keeping the footer in view.
- **AC-HNT-2**: Its key closes it while it is open on the screen under the mouse. While it is open on another screen, the key moves it to the screen under the mouse. Esc or the title bar's X closes it.
- **AC-HNT-3**: Every row comes from `settings::get()`: each shortcut, the digit sets, the Copilot key and each gesture. A row with more than one combo shows each real combo. A split pair (Left and Right) shows both.
  - Covered states: your own key, a key combo, "twice", Nothing, a workspace.
  - A settings change shows at once. A hand edit shows within 2 s, also while the window is open.
- **AC-HNT-4**: The footer holds the key, each control shown when it applies: Set key, Change and Remove.
  - Recording reuses the settings page's recorder (`keys::record` and its rules, spec 0012 AC-7 and AC-9): des-k's shortcuts pause, Esc alone cancels, and a clash offers Move it here or Cancel (Cancel has focus).
  - `keys::record` takes an owner window. Main sends `WM_RECORDED` to the owner's handler: today it always goes to `settings_page::recorded` (`main.rs:243`), and it now goes to `hints::recorded` when the hints window started it.
  - Only one recording runs at a time; starting one cancels any other.
  - Nothing is saved until you choose.
  - A key Windows uses (say Win+H, voice typing) may be taken, and the recording line says so.

  The key also shows in Settings › Shortcuts. This slice adds a plain row for it to today's settings page, in today's look. The look B row comes with the settings page slice (AC-SET-4).
- **AC-HNT-5**: The touchpad notes follow spec 0011 and the design's states:
  - 2 of 4 gestures still on in Windows;
  - a Windows setting can't be read;
  - given back, before the next sign in;
  - reading off, Windows' gestures off;
  - reading off, Windows runs its own;
  - no precision touchpad (the touchpad part is left out).

  Touchpad setup opens Settings › Touchpad, and Open settings opens Shortcuts.
- **AC-HNT-6**: Keyboard:
  - Tab reaches Set key (or Change and Remove), a note's button, and Open settings; Enter presses the focused button.
  - Rows are read only.
  - Your other shortcuts keep working while it is open.
- **AC-HNT-7**: It is one of des-k's own windows: never hidden by a switch, never listed by the finder or overview, not in Alt+Tab.
- **AC-HNT-8**: Narrator reads each row (action, then its keys or gesture), each note, and the footer's controls.

## Data and messages

- `[shortcuts] hints`, a list of combos, default `[]` (see index data model).
- It is matched by the keyboard hook like `finder`.
- The hook posts `WM_APP + 14` (`WM_HINTS`) to the main thread, which opens, moves or closes the window.
- `WM_HINTS` joins main's close first list (`main.rs:218`), so it closes the finder, overview and tray popup before the hints window opens.
- A new `hints::key(&msg)` joins main's key chain beside `welcome::key` (`main.rs:272`), using `IsDialogMessageW` as the welcome screen does, so Tab and Enter work.
- `settings::reload` calls `hints::refresh()` beside `welcome::refresh()` (`settings.rs:546`), so an open window shows a change within 2 s (AC-HNT-3).
- The file's top comment for `hints` says it opens the Shortcut hints window.

## Check on the real machine

- That a Win+H key really takes voice typing's key when you choose it.
- UX review M11 (the gesture state after a give back).

## Rationale (short)

You asked for a cheat sheet read live, with a key that is "nothing by default, can be sat from the menu itself". The approved design keeps that, and this spec follows it.
