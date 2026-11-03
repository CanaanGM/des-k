# 0002. Follow activation: jump to a hidden window's workspace when it comes to the front

**Date**: 2026-10-26
**Status**: Accepted

## Summary

When an app brings back a window that des-k hid (Notepad opening a file as a tab in its hidden window, Alt+Tab, a dialog popping up), des-k now switches that window's screen to the workspace it lives on. Without this, the window has keyboard focus while invisible, so you type into nothing. des-k listens for the Windows "foreground changed" event (the signal sent whenever a different window becomes the active one). It only reacts to windows it hid itself, and it ignores a second jump on the same screen within one second. Apps that can open a new window instead of reusing an old one (Notepad, VS Code) are best set to do so in their own settings. des-k cannot spawn a new copy for them.

## Requirements

**User stories**:
- As the des-k user, I want a hidden window that comes to the front to bring its workspace with it, so that I never type into a window I cannot see.
- As the des-k user, I want to switch the behavior off in `settings.toml`, so that a misbehaving app cannot keep moving my screens.

**Acceptance criteria**:
- **AC-1**: With Notepad (set to open files in its existing window) hidden on workspace 1 of a screen that shows workspace 2, opening a `.txt` file from Explorer switches that screen to workspace 1, and Notepad is visible, on top, and has keyboard focus.
- **AC-2**: The indicator pops up on the screen that jumped, showing the new workspace.
- **AC-3**: Only the hidden window's screen switches. The other screens keep their workspace, and the mouse pointer does not move.
- **AC-4**: Any way of bringing a hidden window to the front triggers the same jump, including Alt+Tab when it lists the hidden window.
- **AC-5**: When a dialog owned by a hidden window (for example a "save changes?" prompt) becomes the active window, its screen jumps to the owner's workspace, so the owner shows behind its dialog. (des-k never hides owned dialogs, so the dialog itself was visible all along.)
- **AC-6**: A hidden window that only flashes (asks for attention without becoming the active window) causes no switch.
- **AC-7**: After a follow jump on a screen, any further follow trigger on that same screen within 1 second is ignored with a log line. A trigger after the second has passed jumps normally. Other screens are not affected by this cooldown.
- **AC-8**: Switching by shortcut or swipe still lands on the workspace you asked for. des-k's own focus changes never cause a follow jump back, and for 1 second after any switch on a screen, a follow that would take that screen back to the workspace it just left is ignored with a log line.
- **AC-9**: `follow_activation = false` in `settings.toml` turns the feature off after a restart. A missing key means on. A bad value (say `"yes"`) falls back to on with a log line, following spec 0001.
- **AC-10**: After a follow jump, `state.txt` records the new active workspace and the windows the jump hid, so a restart keeps the layout.
- **AC-11**: Windows des-k did not hide (on another Windows virtual desktop, suspended UWP windows, never hidden windows) never trigger a jump.

## Decision

**Chosen option**: Option 1: Foreground win event hook, posted into the main loop.

When a window des-k hid (or a dialog it owns) becomes the foreground window, des-k switches that window's screen straight to the window's workspace, gives it focus, shows the indicator, and saves, with a one second per screen cooldown and an on/off setting.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Feature design

**Data model sketch**:

No change to `state.txt` (same `M` and `W` lines).

`ws.rs` `Monitor` gets one in memory field:

| Field | Type | Saved | Meaning |
|---|---|---|---|
| `last_follow` | `Option<std::time::Instant>` | no | when this screen last jumped because of a follow (set only after a successful jump); `None` at start, and a recreated `Monitor` starts at `None` |
| `last_switch` | `Option<(std::time::Instant, usize)>` | no | when this screen last switched by any means, and the workspace it left; `None` at start |

`settings.rs` `Settings` gets one field:

| Field | Type | Default | Rule |
|---|---|---|---|
| `follow_activation` | `bool` | true | a non boolean value resets to true with a log line |

`DEFAULT_FILE` gets the key under `copilot_as_right_ctrl`:
```toml
follow_activation = true    # switch to a hidden window's workspace when it comes to the front
```
`TOP_KEYS` gains `"follow_activation"`; the `Display` line that `load()` logs includes it.

**State transitions**: none new. A follow jump is a switch of one screen from its active workspace straight to another, same as a shortcut switch but by index instead of by one step.

**API surface** (module functions and messages; no network or CLI surface):

| Surface | Kind | Key inputs | Key outputs | Thread | Key errors |
|---|---|---|---|---|---|
| `ws::switch(dir)` | existing, now a thin wrapper | -1 / +1 | resolves the screen under the mouse and the clamped target, calls `switch_to` | main | unchanged |
| `ws::switch_to(mon, name, target, focus)` | new, private, the body of today's `switch` | `HMONITOR`, device name, target index (already valid, never clamped here), `Option<HWND>` to focus | hides the visible windows, shows the target's, sets focus, records `last_switch`, indicator, save | main | shell unavailable: log, nothing changes; `target == active`: return without changes |
| `ws::follow(hwnd)` | new, public | the activated window | a jump, or nothing | main | not ours, stale, cooldown, bounce, screen gone: see Value sourcing for which ones log |
| `on_foreground` | win event callback in `main.rs` | event hwnd, object id, child id | posts `WM_FOLLOW` with `PostThreadMessageW` to the main thread id, for a non null hwnd with `OBJID_WINDOW` / `CHILDID_SELF` only | main (out of context) | none |
| `WM_FOLLOW` | `WM_APP + 2` (`WM_APP` is the switch, `WM_APP + 1` the display change), thread message | `wParam` = hwnd | main loop calls `ws::follow` | main | none |
| `SetWinEventHook` install | in `main` after `ws::load()`, before the loop, only if `follow_activation`: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT \| WINEVENT_SKIPOWNPROCESS)` | | the handle is a local in `main`, kept until the process exits and never unhooked (the hook dies with the process) | main | null handle: log `follow: hook not installed`, des-k runs without the feature |

`WINEVENT_SKIPOWNPROCESS` is what keeps des-k's own `SetForegroundWindow` calls (and the indicator) from ever reaching `follow`. That is the main reason AC-8 holds; the stale check and the bounce guard cover what Windows does on its own while des-k hides windows.

Focus inside `switch_to`: when `focus` is `Some(h)`, after showing the target workspace, check `GetForegroundWindow()`. If it is already `h` (the normal case), do nothing. Otherwise call `SetForegroundWindow(h)` once, and if `h` still is not the foreground window, log `follow: could not focus "<title>"`. For an owned dialog, `h` is the dialog; the owner is only uncloaked, never focused. When `focus` is `None`, today's "first shown window" rule applies unchanged.

`ws::follow(hwnd)` order: (1) `GetForegroundWindow() == hwnd`, else drop; (2) lock state and look `hwnd` up, then its root owner, else drop; (3) the found window must still be cloaked (`cloaked(h) == 2`), else drop; (4) find the screen's `HMONITOR`, else drop with a log line; (5) bounce guard; (6) cooldown; (7) `switch_to`, then set `last_follow`. Dropped triggers never touch `last_follow` or `last_switch`.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| callback | which window | the `hwnd` argument of the win event, filtered to `idObject == OBJID_WINDOW` and `idChild == CHILDID_SELF` |
| follow | is it still current | `GetForegroundWindow() == hwnd` when `WM_FOLLOW` is handled; otherwise drop silently |
| follow | which tracked window | `hwnd` if it is in any `hidden` list, else `GetAncestor(hwnd, GA_ROOTOWNER)` if that is; else drop silently (AC-11) |
| follow | target screen and workspace | the device name key and the list index of the `ws.rs` `hidden` list that holds it |
| follow | `HMONITOR` for the screen | enumerate monitors and match `device_name` (new helper next to `present_monitors`); not found: drop (the display change handler is about to show those windows) |
| follow | is it still hidden | `cloaked(h) == 2` for the found window under the lock; a visible window is dropped silently |
| follow | is it a bounce | `Monitor.last_switch`: within `FOLLOW_COOLDOWN_MS` and the found workspace equals the workspace the screen left |
| follow | is the screen cooling down | `Monitor.last_follow` vs `Instant::now()`, compared with `FOLLOW_COOLDOWN_MS` (1000) |
| follow | enabled | `settings::get().follow_activation`, read once in `main` when deciding to install the hook; nothing else reads it |
| switch_to | workspace count for the indicator | `settings::get().workspaces` (the shortcut wrapper still clamps with it; `switch_to` does not) |
| switch_to | `last_switch` | `(Instant::now(), active before the switch)`, set by every successful switch |
| switch_to | window to focus | `focus` argument: the activated `hwnd` (not the root owner, so a dialog keeps focus) |
| log | window title | `GetWindowTextW(hwnd)` into a 256 character buffer; empty shows as `(no title)` |
| log | jump line | `follow: <device> -> workspace <n>/<count> for "<window title>"` |
| log | cooldown line | `follow: <device> cooling down, ignored "<window title>"` |
| log | bounce line | `follow: <device> just left workspace <n>, ignored "<window title>"` |
| log | screen gone line | `follow: <device> not found, ignored "<window title>"` |
| log | silent drops | not des-k's window, no longer foreground, no longer cloaked: no log line (they happen on every focus change) |

**Key invariants**:
- A follow jump only ever uncloaks windows listed in `ws.rs` state; it never acts on a window it did not hide.
- No `ws` state is touched from inside the win event callback; it only posts.
- A follow changes exactly one screen: the one whose list holds the window.
- `state.txt` is saved after every follow jump, same as after a shortcut switch.
- `Monitor.last_follow` is set only by a successful follow jump; `Monitor.last_switch` by every successful switch. A dropped trigger changes neither.

**Security model**: no new data and no new privileges. The hook is out of context, so nothing is injected into other processes, and it only receives window handles. It sees the same windows medium integrity des-k already enumerates, and it acts only on windows des-k hid.

**Configuration required**: no environment variables or credentials. One new `settings.toml` key, `follow_activation`. An existing settings file is never rewritten, so the key only appears in the file if you add it yourself; missing means on.

**Critical test scenarios** (each maps to an acceptance criterion in ## Requirements):
- Happy path: Notepad hidden on workspace 1, switch to 2, open a `.txt` from Explorer: the screen jumps to 1, Notepad has the new tab and focus, the indicator shows 1, the other screens and the pointer stay put. Verifies AC-1, AC-2, AC-3.
- Happy path: with a window hidden on another screen's workspace, Alt+Tab to it (if listed): that screen jumps. Verifies AC-4.
- Failure case: type in a Notepad tab, switch away, then close Notepad from the taskbar preview or with `taskkill /im notepad.exe` (no `/f`) so its "save changes?" dialog appears: the screen jumps to Notepad's workspace with the dialog in front. Verifies AC-5.
- Failure case: with a window focused on the screen you swipe, swipe back and forth quickly ten times on a screen with several windows: every swipe lands where you asked and nothing bounces; any `just left workspace` log line shows the guard caught a bounce. Verifies AC-8.
- Failure case: trigger two follow jumps on the same screen within a second (open two `.txt` files from Explorer at once while Notepad is hidden): one jump, one `cooling down` log line. Verifies AC-7.
- Failure case: a window on another Windows virtual desktop gets activated by Windows: des-k does nothing. Verifies AC-11.
- Config: `follow_activation = false`, restart: the Notepad case no longer jumps; `follow_activation = "yes"`: it jumps and the log has a settings line. Verifies AC-9.
- Restart: after a follow jump, hard kill des-k (`taskkill /f /im des-k.exe`, not `--quit`, which shows everything) and start it again: each screen comes back on the workspace you saw, with the same windows hidden. Verifies AC-10.
- AC-6 has no reliable manual trigger. It holds by construction (only the foreground event is hooked, never flash), and the review checks that.

## Build plan

Tracer Bullet: the first two tasks already give a working end to end jump, from the event to the saved layout. Cooldown and the setting then thicken it.

1. [x] Split today's `ws::switch` into a thin wrapper (screen under the mouse, clamped step) plus `switch_to(mon, name, target, focus)`, with no behavior change for shortcuts and swipes. Add the device name to `HMONITOR` helper. Satisfies **AC-8**, **AC-10**.
2. [x] Add `ws::follow(hwnd)` (stale check, lookup by window then root owner, jump with `focus = Some(hwnd)`, log line), the `WM_FOLLOW` message, the `on_foreground` callback, and the hook install in `main` after `ws::load()`. Satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-10**, **AC-11**.
3. [x] Add `Monitor.last_follow` and `Monitor.last_switch`, the 1 second per screen cooldown, and the bounce guard, with their log lines. Satisfies **AC-7**, **AC-8**.
4. [x] Add `follow_activation` to `Settings`, the default, `DEFAULT_FILE`, `TOP_KEYS`, the `Display` line, and skip the hook install when it is off. Extend the existing unit tests: the default file still parses to the defaults, and a bad value resets only this key. Satisfies **AC-9**.
5. Try every critical test scenario on your screens, then run `install.ps1`. Satisfies **AC-1** to **AC-11**.

## Consequences

**Positive**:
- Keyboard focus can no longer sit in an invisible window. That was the worst failure of the cloak approach, since you lose typing without any sign.
- Alt+Tab becomes a cross workspace switcher for free, wherever it lists hidden windows. This takes some pressure off feature 6 (window finder).
- `switch_to` by index is the building block that features 4, 6 and 9 need anyway.

**Negative / tradeoffs**:
- An app that brings itself forward on its own (some chat or update dialogs do) will now move your screen. The cooldown limits ping pong, and `follow_activation = false` is the escape. There is no per app exclusion list yet.
- A reused window still means a jump away from where you are. Avoiding that depends on each app's "open in new window" setting, outside des-k's control.
- Two more pieces of hidden machinery (a global hook and a posted message) to keep in mind when debugging focus issues. The log lines are the only trace.
- The scope's Photos example cannot exercise this feature on your machine, because Photos opens new windows. Notepad replaces it.

**Neutral**:
- If feature 7 (Alt+Tab per workspace) later removes hidden windows from Alt+Tab, AC-4's Alt+Tab path goes away. The rest of the feature is unaffected.
- `ws::switch` keeps its signature, so `keys.rs` and the `WM_APP` path are untouched.

## Follow-up

- [ ] Set Windows Notepad to open files in a new window (Notepad Settings, Opening files) once AC-1 is verified, if you'd rather get a new window where you are than jump. Do the same for any other app that offers it.
- [x] Update scope feature 3's "Done when" to use Notepad instead of Photos.
- [ ] If an app turns out to steal focus repeatedly, decide on a per app exclusion list in `settings.toml` (new `/architect` decision; on the scope's Deferred list).
- [ ] Dialogs owned by a hidden window are never cloaked by des-k today (`app_windows_on` skips owned windows), so they float over the wrong workspace. Worth checking when building feature 4 (on the scope's Deferred list).
