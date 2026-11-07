# 0014. Pin a window to every workspace of its screen

**Date**: 2026-11-03
**Status**: Accepted
**Amended by**: [0017](0017-look-b-screens/0017-indicator.md): the indicator's look for pin and unpin.

## Summary

A call, a music player or a notes window should stay in view whatever workspace a screen shows. Win+Ctrl+U pins the focused window: switches on its screen leave it where it is. Press it again to unpin; the window then belongs to the workspace on screen. Pins survive a restart (`P` lines in `state.txt`) and a reboot (matched like any saved window, spec 0003). Raised by the five reviewer report of 2026-11-03; the owner chose the shortcut and the per screen rule. The shortcut was Win+Ctrl+P until 2026-11-03; Windows opens its Project screen on that combo, so the owner moved it to Win+Ctrl+U.

## Requirements

**Acceptance criteria**:
- **AC-1**: Win+Ctrl+U on a focused window pins it: the indicator shows on its screen with every other workspace lit in the accent color. Switching that screen, by any shortcut, gesture, the finder or the overview, leaves the window in view.
- **AC-2**: Win+Ctrl+U on a pinned window unpins it: the indicator shows plain dots, and the next switch hides it with the workspace on screen.
- **AC-3**: A pinned window dragged (or moved with Win+Shift+Left/Right) to another screen stays pinned there.
- **AC-4**: Sending a pinned window to a workspace (any send shortcut, a gesture, an overview drop on a hidden workspace) unpins it, and the log says so.
- **AC-5**: A restart in the same sign in (install, update, hard kill) keeps every pin whose window is still open. A reboot or new sign in pins a reopened window only when restore matches it to a saved pinned entry; a guess between a pinned and a plain window of one app never counts (rule 2 treats pinned as its own place).
- **AC-6**: The shortcut is `pin` in `[shortcuts]` (default `["Win+Ctrl+U"]`), on the settings page, and `pin` is a gesture action name.
- **AC-7**: A focused window des-k would never hide (the desktop, the taskbar, a tool window) is not pinned: the log says nothing to pin.

## Design

- `ws::State.pinned`: a set of hwnds. `switch_to` skips them when it hides a screen's windows. Closed windows drop out at each save.
- `state.txt`: `P <device> <hwnd> <pid>` TAB app TAB class TAB title, written instead of the window's `V` line. `load` pins a `P` window again when its hwnd still has that pid. A new sign in turns `P` lines into restore entries with `pin` set; a pinned entry that wins pins the window and leaves it visible. A pending one is saved as `P <device> 0 0`.
- `keys.rs`: `WM_PIN` (`WM_APP + 12`), once per press like the finder. `gestures.rs`: `Action::Pin`.
- A Quit forgets pins, as it forgets the layout; `--quit` rescue drops `P` lines like `V` lines.

## Limits

- The overview draws a pinned window on its screen's shown workspace only.
- A pinned window's owned dialogs follow their owner, like any dialog.

## Verify

- [ ] Pin a window, switch its screen both ways: it stays. Unpin, switch: it hides. → AC-1, AC-2
- [ ] Pin, drag to another screen, switch that screen: it stays. → AC-3
- [ ] Pin, then Win+Ctrl+Alt+Right: it leaves, unpinned. → AC-4
- [ ] Pin, run install.ps1: still pinned after the restart. → AC-5
