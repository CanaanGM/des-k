# 0004. Send window to workspace: move the focused window one workspace over, with or without you

**Date**: 2026-10-27
**Status**: Accepted

## Summary

Two new shortcut pairs move the focused window to the previous or next workspace on its own screen. Win+Ctrl+Shift+Left/Right sends the window and the screen follows it, so it stays in front of you. Win+Ctrl+Alt+Left/Right sends it away while you stay, and the indicator marks where it went with an accent colored dot. Sending wraps around at the ends, a held key acts once, and a quick second "stay" press keeps moving the same window rather than sending a second one (it comes back into view if it lands on the workspace you are on). The move is saved in `state.txt` like any hidden window, and the four shortcuts live in `settings.toml`.

## Requirements

**User stories**:
- As the des-k user, I want to take the window I'm working in to another workspace and go with it, so that I can rearrange my workspaces without dragging anything.
- As the des-k user, I want to push a window off to another workspace and stay where I am, so that I can clear a screen quickly.
- As the des-k user, I want a quick double press to move one window two steps, so that I never hide a second window by accident.

**Acceptance criteria**:
- **AC-1**: With a window focused on screen S, which shows workspace n, Win+Ctrl+Shift+Right (Left) switches S to workspace n+1 (n-1). The window is visible there, on top, and keeps keyboard focus, its position and its size. The other windows of n are hidden, and the indicator shows the new workspace.
- **AC-2**: With a window focused on S showing n, Win+Ctrl+Alt+Right (Left) hides the window and its taskbar button. S stays on n, and focus goes to the top remaining window on S (the desktop if there is none). The indicator pops up on S showing n, with the dot for n+1 (n-1) drawn in an accent color. Switching S to that workspace later shows the window on top.
- **AC-3**: Both pairs wrap: "next" from the last workspace sends to the first, and "previous" from the first sends to the last.
- **AC-4**: The window's own screen is the one that acts, never the screen under the mouse. Other screens keep their workspace, and the pointer does not move.
- **AC-5**: A stay press within 1 second of the previous stay press moves the window that press sent one more step in the new press's direction (so Right then Left undoes the move), wrapping, instead of the focused window. Each repeat restarts the 1 second. If that step lands on the workspace S is showing, the window reappears on top with focus. A stay press more than 1 second later, any follow press, and any stay press after a switch of any screen act on the focused window again.
- **AC-6**: Holding a send shortcut down acts once. Key repeats are swallowed and do nothing until the key is released, even if a modifier is let go meanwhile (so a held Win+Ctrl+Shift+Left never turns into Win+Ctrl+Left switches).
- **AC-7**: When the focused window is a dialog (an owned window), its top owner is sent. Owned windows stay visible where they are, exactly as on a switch today.
- **AC-8**: When there is nothing sendable in focus (the desktop, the taskbar, no window, or a window des-k would not hide on a switch: tool windows, untitled windows, already hidden ones), nothing changes and the log says why. The shortcut is still swallowed.
- **AC-9**: If hiding the window fails, nothing changes: the window stays visible and focused, and the log has a line.
- **AC-10**: After any send, `state.txt` records the window on its new workspace. A hard kill (`taskkill /f`) and a restart keep it there.
- **AC-11**: For 1 second after a stay send, the sent window getting focus causes no follow jump (spec 0002), and the log says so. After that second, follow activation treats it like any other hidden window.
- **AC-12**: `settings.toml` `[shortcuts]` takes `send_previous`, `send_next` (follow) and `send_previous_stay`, `send_next_stay` (stay), with the defaults above. Missing keys mean the defaults, and a bad entry resets only its own list (spec 0001). Switch keys always win: a send list that shares a combo with `previous`, `next`, or an earlier send list is turned off (no shortcut) with a log line naming the combo, and the clash between `previous` and `next` keeps today's rule. Win+Ctrl+Left/Right and the swipe still only switch.

## Decision

**Chosen option**: Option 1: a `ws::send` built on `switch_to`, posted from the keyboard hook.

The keyboard hook posts a new `WM_SEND` message to the main thread. `ws::send` then either switches the window's screen while keeping that window visible (follow), or cloaks the window straight into the target workspace's list (stay). In both cases it saves the layout and shows the indicator.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch**:

No change to `state.txt`. A sent window is either visible (a `V` line) or in a workspace's hidden list (a `W` line), exactly like any other window.

`ws.rs` `State` gets one in memory field:

| Field | Type | Saved | Meaning |
|---|---|---|---|
| `sent` | `Option<(isize, Instant)>` | no | the window the last stay send moved, and when (stamped on the main thread when the send runs; the drift from the key press is accepted). Read by the stay repeat (AC-5) and the follow guard (AC-11). Set only by a successful stay send; cleared by every `switch_to` (any switch, including a follow send). `None` at start |

`settings.rs` `Settings` gets four fields, parsed by the existing `combos()` exactly like `previous` and `next`:

| Field | Type | Default |
|---|---|---|
| `send_previous` | `Vec<Combo>` | `["Win+Ctrl+Shift+Left"]` |
| `send_next` | `Vec<Combo>` | `["Win+Ctrl+Shift+Right"]` |
| `send_previous_stay` | `Vec<Combo>` | `["Win+Ctrl+Alt+Left"]` |
| `send_next_stay` | `Vec<Combo>` | `["Win+Ctrl+Alt+Right"]` |

`SHORTCUT_KEYS` gains the four names, `Display` (the line `load()` logs) includes them, and `DEFAULT_FILE`'s `[shortcuts]` becomes:
```toml
[shortcuts]
# Keep these two if you switch with the touchpad swipe.
previous = ["Win+Ctrl+Left"]
next     = ["Win+Ctrl+Right"]
# Send the focused window to the previous or next workspace (wraps around).
send_previous      = ["Win+Ctrl+Shift+Left"]   # and follow it there
send_next          = ["Win+Ctrl+Shift+Right"]
send_previous_stay = ["Win+Ctrl+Alt+Left"]     # and stay where you are
send_next_stay     = ["Win+Ctrl+Alt+Right"]
```
Clash check: today's `previous` / `next` check runs first, unchanged. Then each send list, in the order `send_previous`, `send_next`, `send_previous_stay`, `send_next_stay` (empty lists already refilled with defaults), is compared with `previous`, `next`, and the send lists before it that are still on. On a shared combo, that send list becomes empty (off: the hook never matches it) and the log says `shortcuts: <combo> is in both <a> and <b>, <b> turned off`. Combos match on exact modifiers (`Combo` equality, as today), so Win+Ctrl+Shift+Left never matches Win+Ctrl+Left. A combo repeated inside one list is not a clash.

`keys.rs` gets one thread local: `send_held: Cell<Option<u16>>`, the key of a send combo that is down. It is checked in `shortcut()` before any combo matching: a key down for that key is a repeat, swallowed with nothing posted and no `VK_MASK`, whatever modifiers are held now. A key up for that key, or a key down of any other key, clears it (so a missed key up, say after Win+L, costs at most one press).

**State transitions**: none new. A follow send is a switch of one screen by one step, with one window kept visible. A stay send moves one window between the screen's lists (visible to hidden, or hidden to hidden on a repeat, or hidden to visible when a repeat lands on the active workspace).

**API surface** (module functions and messages; no network or CLI surface):

| Surface | Kind | Key inputs | Key outputs | Thread | Key errors |
|---|---|---|---|---|---|
| `keys::shortcut(vk, up)` | existing, extended | key event | for a send combo: on first down sends `VK_MASK` (Win/Alt combos, as today) and posts `WM_SEND`; repeats and up swallowed with nothing sent | hook | none |
| `WM_SEND` | `WM_APP + 3`, thread message | `wParam` = -1 / +1 as `isize`, `lParam` = 1 follow, 0 stay | main loop calls `ws::send(dir, follow)` | main | none |
| `ws::send(dir, follow)` | new, public | direction, follow flag | a follow switch, a stay move, or a log line | main | nothing sendable, cloak failed, shell unavailable: log, nothing changes |
| `ws::switch_to(s, mon, name, target, focus, keep)` | existing, one new parameter | `keep: Option<HWND>`: a visible window to leave visible | as today, except `keep` is skipped when hiding the active workspace's windows, and a successful switch sets `State.sent = None` | main | unchanged |
| `ws::follow(hwnd)` | existing, one new check | | drops `hwnd` or its root owner when it equals `State.sent` within `FOLLOW_COOLDOWN_MS`, with a log line, next to the existing `restored` check | main | |
| `indicator::show(mon, active, count, sent)` | existing, one new parameter | `sent: Option<usize>`, 0 based like `active`, never equal to `active` | draws that dot (same size as the other dots) in the new constant `ACCENT = RGB(96, 165, 250)`; every existing caller passes `None` | main | |

`ws::send(dir, follow)` order:
1. **Stay repeat**: if `!follow` and `State.sent` is `Some((h, t))` with `t` under 1 second old, `IsWindow(h)`, and `h` in some screen's hidden list `i`: the window is `h`, its screen is that list's screen, and `from = i`. Go to step 4.
2. **Window**: `GetAncestor(GetForegroundWindow(), GA_ROOTOWNER)`. It must be in `app_windows()` (the same filter a switch hides by). Otherwise log `send: nothing to send ("<title>", <class>)` and stop. A follow press never looks at `State.sent`.
3. **Screen**: `MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST)`, its `device_name`, the `Monitor` entry (created if new). `from = active`.
4. **Target**: `(from + count + dir) % count`, where `count = settings::get().workspaces`.
5. **Follow**: `switch_to(s, mon, name, target, Some(h), Some(h))`. It hides every other visible window on the screen, shows the target's, keeps `h` (already in front, so it stays on top), saves, shows the indicator.
6. **Stay, first press**: `Shell::new()`, `shell.hide(h, true)`; on error log and stop, with no retry (a switch does not retry either). Insert `h` at the front of `hidden[target]` (top of z order) right after the cloak, before anything else, so the panic hook can always show it again. Then move focus with `SetForegroundWindow` to `app_windows_on(mon).first()` (top of z order first; `h` is cloaked now, so the filter already skips it), else `GetShellWindow()`.
7. **Stay, repeat**: remove `h` from `hidden[from]`. If `target == active`, `shell.hide(h, false)` and `SetForegroundWindow(h)`; on an uncloak error put it back in `hidden[from]`, log and stop. Otherwise insert it at the front of `hidden[target]` (it stays cloaked, no shell call).
8. **Stay, both**: set `State.sent = Some((h, now))`, `indicator::show(mon, active, count, Some(target))` (or `None` when it landed on `active`), `save`, log.

Focus uses plain `SetForegroundWindow`, as `switch_to` does today (des-k just received the key press, so Windows allows it). AC-9 covers the stay path; in a follow send, a window that fails to hide is logged and skipped by `switch_to` as on any switch.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| hook | which action | the combo held (`mods_held()` plus vk) found in one of the four send lists of `settings::get()` |
| hook | first press or repeat | `send_held` thread local in `keys.rs` |
| send | direction, follow | `WM_SEND` `wParam` and `lParam`, set by the hook from which list matched |
| send | window | stay repeat: `State.sent`; otherwise the root owner of `GetForegroundWindow()` |
| send | is it sendable | membership in `app_windows()` (repeat: in a hidden list and `IsWindow`) |
| send | its screen | `MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST)` (repeat: the screen whose list holds it), never the cursor |
| send | from workspace | the screen's `Monitor.active` (repeat: the index of the list holding it) |
| send | workspace count | `settings::get().workspaces` |
| send | target | `(from + count + dir) % count` |
| send | where it lands in z order | the front of `hidden[target]` (top first), so it shows on top |
| send, stay | window that gets focus | `app_windows_on(mon).first()` after the cloak, else `GetShellWindow()` |
| send, stay | repeat window | `State.sent`, valid while under `FOLLOW_COOLDOWN_MS` (1000) old; the same constant drives the follow guard, so both rules share one second |
| send, stay | repeat direction | the new press's `dir` |
| indicator | accent dot | `sent` argument = the target workspace (0 based); color `ACCENT = RGB(96, 165, 250)` in `indicator.rs` |
| follow | sent guard | `State.sent` compared with the activated window and its root owner, within `FOLLOW_COOLDOWN_MS` |
| log | send line | `send: "<title>" <device> workspace <from>/<count> -> <target>/<count>, follow` (or `stay`); 1 based |
| log | nothing to send | `send: nothing to send ("<title>", <class>)`; `(no title)` when there is none; the class (`class_name`) tells the desktop, the taskbar and tool windows apart |
| log | cloak failed | `send: could not hide "<title>": <error>` |
| log | follow guard | `follow: just sent "<title>", ignored` |
| log | clash | `shortcuts: <combo> is in both <a> and <b>, <b> turned off` |

**Key invariants**:
- A send only ever cloaks a window a switch would also hide (`app_windows()`), and only ever uncloaks a window that is in des-k's own lists.
- A window is in at most one hidden list at any time, and never in one while visible.
- A send changes exactly one screen: the window's own.
- `state.txt` is saved after every successful send.
- `State.sent` is set only by a successful stay send and cleared by every successful switch.
- A cloaked window is in a hidden list from the moment it is cloaked.
- All of it runs on the main thread; the hook thread only posts.

**Security model**: no new data and no new privileges. Same medium integrity process and same windows as a switch. The keyboard hook already sees every key; it only matches four more combos.

**Configuration required**: no environment variables or credentials. Four new `settings.toml` keys under `[shortcuts]`. An existing settings file is never rewritten, so they appear in your file only if you add them; missing means the defaults.

**Critical test scenarios** (each maps to an acceptance criterion in ## Requirements):
- Happy path, follow: Notepad focused on workspace 1 of a screen, Win+Ctrl+Shift+Right: the screen shows 2 with Notepad on top and focused, same size and place, indicator shows 2; Left takes it back to 1. Verifies AC-1.
- Happy path, stay: Notepad focused on 1, Win+Ctrl+Alt+Right: Notepad and its taskbar button go, the next window has focus, the dots show 1 with the 2 dot in blue; swipe to 2: Notepad is there on top. Verifies AC-2.
- Wrap: on the last workspace, both "next" shortcuts send to workspace 1; on the first, both "previous" send to the last. Verifies AC-3.
- Other screen: focus a window on screen A, put the mouse on screen B, send: only A changes. Verifies AC-4.
- Double press: Win+Ctrl+Alt+Right twice quickly: the same window ends two workspaces over, the window that got focus in between stays put; with 4 workspaces, four quick presses bring it back visible and focused; Right then Left quickly brings it straight back. Verifies AC-5.
- Hold: hold Win+Ctrl+Shift+Right for two seconds: exactly one step; hold it again and let go of Shift first: still no switch. Verifies AC-6.
- Dialog: open Notepad's Save As dialog, send with the stay pair: Notepad goes, and the dialog stays visible (if Windows hides it along with its owner instead, note it in the log and the deferred item). Verifies AC-7.
- Failure case: click the desktop, press a send shortcut: nothing moves, a `nothing to send` line, no Start menu. Verifies AC-8.
- Failure case: AC-9 has no reliable manual trigger (a brand new window fails to cloak only for a moment); the review checks the error path changes no list.
- Restart: send a window with stay, `taskkill /f /im des-k.exe`, start again, switch to the target workspace: the window is there. Verifies AC-10.
- Follow guard: send with stay and watch the log for a few sends: any `just sent` line means the guard caught a focus bounce, and the screen never jumped after the window. Verifies AC-11.
- Config: change `send_next` to `["Win+Alt+N"]`, restart: it sends, Win+Ctrl+Shift+Right does nothing; set `send_next_stay = ["Win+Ctrl+Right"]`: a `turned off` log line, Win+Ctrl+Right still switches, the other send shortcuts still work. `cargo test` covers the default file and the clash rules. Verifies AC-12.

## Build plan

Tracer Bullet: task 1 is a working follow send from key press to saved layout. Then stay, then the finer rules.

1. [x] Follow send end to end: the `send_previous` / `send_next` settings fields with their defaults, `DEFAULT_FILE`, `SHORTCUT_KEYS`, `Display`, and the six list clash check; the hook matching them and posting `WM_SEND` (`lParam` 1); the main loop arm; `ws::send` steps 2 to 5 with wrap; the `keep` parameter on `switch_to`. Extend the unit tests: the default file still parses to the defaults, a send list clashing with `previous` or `next` is turned off while the switch lists are kept, a bad send entry resets only its list. Satisfies **AC-1**, **AC-3**, **AC-4**, **AC-7**, **AC-8**, **AC-10**, **AC-12**.
2. [x] Stay send: the two `_stay` settings fields (with a test that a stay list clashing with a follow list is turned off), the hook posting `lParam` 0, `ws::send` step 6 with the focus move, the cloak failure path, and `State.sent` with the follow guard in `ws::follow`. Satisfies **AC-2**, **AC-9**, **AC-11**, **AC-12**.
3. [x] Finer rules: the stay repeat (steps 1, 7, 8), `send_held` in `keys.rs`, and the accent dot (`sent` parameter on `indicator::show`, every existing caller passing `None`). Satisfies **AC-2**, **AC-5**, **AC-6**.
4. Try every critical test scenario on your screens, then run `install.ps1` and confirm `des-k.exe` is running afterward. Satisfies **AC-1** to **AC-12**.

## Consequences

**Positive**:
- You can sort windows into workspaces from the keyboard, with no drag between screens and no switching back and forth.
- The follow pair reuses `switch_to`, so it keeps the bounce guard, indicator, focus, and save behavior that switching already has.
- The accent dot gives the indicator a second state, which the deferred redesign can build on.

**Negative / tradeoffs**:
- Four more global shortcuts. Win+Ctrl+Alt+Left/Right is now taken system wide, and an app that uses it loses it while des-k runs (change it in `settings.toml`).
- Sending wraps but switching clamps. You chose this on purpose, so the two shortcut families behave differently at the ends.
- Owned dialogs and tool palettes stay behind on the old workspace, as on a switch. Sending a window while its Save As dialog is open leaves the dialog floating.
- Like switching, the shortcuts most likely do nothing while an elevated (admin) window has focus: a normal process's keyboard hook doesn't see keys meant for an admin window. Not designed around. The verify step confirms it.
- The stay repeat adds a 1 second piece of hidden state. A third press just after the window came back visible acts on it again (it is now focused), which is the expected result but one more rule to keep in mind.

**Neutral**:
- `switch_to`, `follow`, and `indicator::show` each gain one parameter or check. `ws::switch` and the `WM_APP` path are unchanged.
- The thread message list in AGENTS.md gains `WM_APP + 3` send.
- While a restore runs (spec 0003, the first minutes after sign in), a window you send is no longer visible, so restore never considers it again.

## Follow-up

- [ ] Add `WM_APP + 3` send to the thread message ids line in AGENTS.md `## Rules` (/sync owns that).
- [ ] The deferred "owned dialogs on hidden workspaces" item stays open and now covers sends too (you chose top owner only).
- [ ] If the Copilot key (as Right Ctrl) doesn't combine with Win+Shift or Win+Alt for these shortcuts, note it. keys.rs swallows LWin and LShift downs while the Copilot key is held.
