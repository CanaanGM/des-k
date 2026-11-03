# Rationale: 0004. Send window to workspace: move the focused window one workspace over, with or without you

_Decision record for [spec 0004](index.md): why this design was chosen._

## Context

Today des-k has no way to move one window to another workspace. A switch hides everything visible on the screen at once, so a window ends up on whatever workspace was active when it was opened or dragged onto that screen. To regroup windows you'd have to minimize the others, or drag a window to another screen and back. The scope lists this as feature 4, leaving the "follow or stay" behavior to this spec.

Forces:
- des-k tracks windows only by what it hid. A workspace is "whatever is visible" on the active one and a list of hidden handles on the others (`ws.rs`). Moving a window means moving its handle between those lists, and cloaking or uncloaking it.
- AGENTS.md threading rule: the keyboard hook thread does no slow work and only posts `WM_APP` messages. Cloaking, focus, the indicator, and saving happen on the main thread.
- AGENTS.md safety rule: only ever uncloak windows des-k hid itself. A window des-k would not hide on a switch (tool windows, owned windows, untitled windows) must not become hidden by a send either, or nothing would ever show it again.
- Follow activation (spec 0002) jumps a screen to a hidden window when that window comes to the front. Windows sometimes hands focus to a window mid cloak, so a window you just pushed away could pull the screen after it.
- Shortcuts are configurable (spec 0001). A combo bound to two actions makes a key do two things.
- One person, Prototype workflow: verified by hand on your three screens, `cargo test` covers settings parsing only.

## Options considered

### Option 1: A `ws::send` built on `switch_to`, posted from the keyboard hook

A new `WM_SEND` message carries direction and follow. Follow calls `switch_to` with one window kept visible. Stay cloaks the window and puts it at the front of the target's hidden list.

**Pros**:
- Reuses the switch machinery (hide order, focus, bounce guard, indicator, save) for follow, with a single new parameter.
- The window you send never flickers: in follow mode it is never cloaked at all.
- Stays inside des-k's own model (its lists, its cloak), so restart, undock, and quit already handle sent windows.

**Cons**:
- `switch_to` gains a parameter that only sends use.
- Stay mode needs its own small focus and repeat logic beside `switch_to`.

### Option 2: Compose follow out of stay plus a switch

Follow = cloak the window into the target list (stay), then `switch_to(target)`, which uncloaks it with the rest.

**Pros**:
- One send path. Follow is just "stay, then switch".
- No change to `switch_to`.

**Cons**:
- The window blinks: hidden, then shown a moment later.
- Focus leaves the window and comes back, which fires the foreground hook twice and leans on the follow guards to not bounce.
- The window lands at the top of the hidden list, but whether it ends up above the target's other windows depends on uncloak order, not on it having been in front.

### Option 3: Windows' own "move window to desktop"

Use the public `IVirtualDesktopManager::MoveWindowToDesktop` and Windows' virtual desktops.

**Pros**:
- Documented API, no cloak call.

**Cons**:
- Windows desktops switch every screen at once. That is the exact problem des-k exists to avoid (approach A ignores them).
- des-k's state would no longer describe where windows are.

## Rationale

Option 1 keeps the one thing a send must not do, flash or bounce the window you are working in, out of reach by construction. In follow mode the window is never cloaked, so neither the foreground hook nor the taskbar ever sees it change. Option 2 is less code on paper, but it makes the most common action (follow) depend on spec 0002's guards behaving perfectly, and those guards were built for a different case. Option 3 contradicts the project's founding decision.

Both modes work only on windows already in des-k's model. A send accepts a window only when `app_windows()` (the switch filter) does. That way a sent window is always one that switching, quitting, undocking, and restarting already know how to show again, and the AGENTS.md safety rule holds without any new case.

### Choices you made in the interview

- **Both pairs, follow on Shift and stay on Alt**: Win+Ctrl+Shift+Left/Right follows (extends the switch combo the way Windows' Win+Shift moves windows between screens), Win+Ctrl+Alt+Left/Right stays. Runner up: a single pair with a `send_follows` setting.
- **Previous and next only**: you were fine with either; direct "send to workspace N" was left out to keep the key set small. It can be added later as more lists, with no change to `ws::send` beyond the target.
- **Wrap around** at the ends, for both pairs, even though switching clamps.
- **Stay repeat keeps moving the same window** within 1 second, so a double press never hides a second window.
- **Accent colored target dot** for stay feedback, over a bright dot or a ring.
- **Top owner only** for dialogs: consistent with switching; the deferred item stays open for both.
- **One action per press**: with wrap, key repeat would spin the window around every workspace.
- **Names** `send_previous`, `send_next`, `send_previous_stay`, `send_next_stay`.

### Calls made in the spec (recommended, not asked)

- **Message**: `WM_APP + 3` with `wParam` direction and `lParam` follow, following the existing id list. Runner up: two message ids. One id keeps the main loop to one arm.
- **One `State.sent` field** serves both the stay repeat and the follow guard, both on `FOLLOW_COOLDOWN_MS` (1 second). Runner up: reuse the `restored` map from spec 0003. That map has a 2 second window and a different meaning, and mixing them would make the restore guard harder to reason about.
- **Sent window lands at the top** of the target's hidden list, so it is in front when you get there. Runner up: the bottom, which hides it behind windows you have not touched.
- **Accent color is a fixed blue constant** in `indicator.rs`. Runner up: Windows' accent color (`DwmGetColorizationColor`), which follows your theme but adds a call and can be low contrast against the dark pill. The redesign can revisit it.
- **Switch keys win a clash, the send list is turned off** (changed after the cross check, which you approved): the first draft reset all six lists to defaults on any clash. That would quietly throw away your own `previous`/`next` keys whenever a new send default happened to match them, even though you never wrote a send key. Now the switch lists keep today's rule, and a send list that clashes with them (or with an earlier send list) is turned off with a log line. Runner up: reset only the clashing send list to its default, which can clash again.
- **Cross check fixes, also approved**: a stay repeat uses the new press's direction and any switch clears it; held key repeats are swallowed whatever modifiers are still down; the window joins its hidden list right after the cloak so the panic hook can show it; no retry when hiding fails, as on a switch; the accent is `RGB(96, 165, 250)`.
- **Hold once is tracked in `keys.rs`** by remembering the send key that is down. The low level hook has no "repeat" flag, so the first down is told apart by state.
