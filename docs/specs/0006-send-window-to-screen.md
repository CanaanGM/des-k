# 0006. Send window to screen: use Windows' own Win+Shift+Left/Right, no des-k code

**Date**: 2026-10-28
**Status**: Accepted

## Summary

Moving the focused window to another screen is already handled by Windows itself: Win+Shift+Left or Win+Shift+Right moves it to the next screen, keeps a sensible size across different scaling, and keeps a maximized window maximized. des-k already treats a window that arrives on a screen as part of whatever workspace that screen is showing, so the two work together with no new code. You tried it on your screens and it works, so feature 5 is settled by this decision and nothing gets built. The one rule this adds: des-k must never take Win+Shift+Left/Right for itself.

## Context

Scope feature 5 asked for a way to move the focused window to another screen, landing on that screen's current workspace, keeping a sensible size between the 150% laptop screen and the external screens, and keeping maximized windows maximized.

des-k's workspace model (`src/ws.rs`) only remembers the windows it hid. The active workspace of a screen is simply whatever is visible on that screen when you switch (it finds them with `MonitorFromWindow` at switch time). So a visible window that changes screens needs no tracking: from that moment it belongs to the new screen's active workspace, it is hidden with that workspace when you switch away, and comes back when you switch back. Visible windows are not saved in `state.txt`, so nothing there goes stale either.

Moving a window across screens with different scaling is the hard part (the window has to be resized for the new DPI, the screen's pixel density, and a maximized window has to be maximized again on the new screen). Windows already does this for its own shortcut.

## Requirements

**User stories**:
- As the des-k user, I want to move the window I'm working in to another screen with a key press, so that I can rearrange my screens without dragging.

**Acceptance criteria**:
- **AC-1**: With a window focused, Win+Shift+Right (Left) moves it to the next screen to the right (left), wrapping around. It lands visible on that screen's current workspace, and the other screens keep their workspace.
- **AC-2**: A window moved between the 150% laptop screen and an external screen keeps a sensible size and position, and a maximized window stays maximized.
- **AC-3**: After the move, switching the target screen to another workspace hides the moved window, and switching back shows it again. Switching the screen it came from no longer touches it.
- **AC-4**: des-k never swallows Win+Shift+Left or Win+Shift+Right: no default in `settings.toml` uses those combos.

## Options considered

### Option 1: Use Windows' Win+Shift+Left/Right as is

Windows' own shortcut moves the window, and des-k's existing model picks it up with no change.

**Pros**:
- Zero code, nothing new to break or maintain.
- Windows handles the scaling change and the maximized state, which is the hardest part to get right.
- A shortcut you may already know, and it works without des-k running.

**Cons**:
- With 3 screens, reaching the far screen takes two presses (it moves one screen at a time).
- It can only land on the target screen's visible workspace, not a hidden one.

### Option 2: A des-k shortcut that sends to screen N

A new shortcut set that sends the focused window straight to a numbered screen.

**Pros**:
- One press to any screen.

**Cons**:
- des-k would have to redo the scaling and maximize handling Windows already does, plus pick a screen numbering you would need to learn.
- Another set of shortcuts to fit around the many des-k already has.

### Option 3: Send to a screen and a chosen workspace on it

Move the window to another screen and onto a workspace there, possibly a hidden one, with or without following it.

**Pros**:
- Something Windows cannot do at all.

**Cons**:
- The most code of the three (cloaking into another screen's list, scaling, focus), for a need that has not come up.

## Decision

**Chosen option**: Option 1: use Windows' Win+Shift+Left/Right as is.

des-k adds no code for sending a window to another screen; Windows' shortcut does it and des-k's per screen model already accepts the moved window.

## Rationale

You tried Win+Shift+Left/Right with des-k running and it did what feature 5 asked for. The hard parts of this feature (resizing across 150% and 100% scaling, keeping maximized windows maximized) are exactly what Windows already gets right, and des-k's model was designed so windows moving between screens need no tracking. Building Option 2 or 3 would mean redoing that work in des-k for a gain (one press instead of two, or landing on a hidden workspace) nobody has asked for yet.

If one of those gains turns out to matter in daily use, Option 2 or 3 can be designed then as a new feature on top of this one; nothing here blocks it.

## Build plan

No build tasks: the behavior exists today. AC-1 and AC-2 are Windows' own behavior, AC-3 is des-k's existing switch logic in `src/ws.rs`, and AC-4 holds for today's defaults in `src/settings.rs` (no default uses Win+Shift with an arrow).

## Consequences

**Positive**:
- Feature 5 is done with no code and no new settings.

**Negative / tradeoffs**:
- Reaching the far screen of three takes two presses.
- A window can't be sent straight onto a hidden workspace of another screen; you move it, then use the send shortcuts from spec 0004 or 0005 on that screen.

**Neutral**:
- New des-k shortcuts must keep Win+Shift+Left/Right free. A user can still bind them in `settings.toml`, and then des-k wins over Windows.

## Follow-up

- [ ] Optionally try AC-3 once on your screens (move a window, switch the target screen away and back). It follows from the existing design but has not been tried by hand.
