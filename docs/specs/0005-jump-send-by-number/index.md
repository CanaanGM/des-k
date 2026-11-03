# 0005. Jump and send by number: go straight to workspace N, or send the focused window there

**Date**: 2026-10-27
**Status**: Accepted

## Summary

Three sets of number shortcuts let you reach any workspace directly instead of stepping through the ones in between. Win+N switches the screen under the mouse straight to workspace N. Win+Shift+N sends the focused window to workspace N and the screen goes with it. Win+Ctrl+N sends it there while you stay. They reuse everything spec 0004 built for the arrow sends (the quick fix second press, the accent dot, the follow guard, saving to `state.txt`), and the three modifier sets live in `settings.toml`.

## Requirements

**User stories**:
- As the des-k user, I want to jump straight to workspace 4 so that I don't have to scroll through 2 and 3 to get there.
- As the des-k user, I want to send the window I'm in to workspace 4 in one press, with or without going there myself.
- As the des-k user, I want a mistyped number fixed by simply pressing the right one, the same way the stay arrows work.

**Acceptance criteria** (N is a digit 1 to 9; "count" is `workspaces` in `settings.toml`):
- **AC-1**: With the mouse on screen S, Win+N (N up to count) switches S straight to workspace N exactly as a switch does today: the windows of the old workspace are hidden, those of N are shown, focus follows today's switch rule, and the indicator shows N. Other screens keep their workspace.
- **AC-2**: Win+N when S already shows N changes nothing; the indicator pops up on S showing N.
- **AC-3**: Any of the three shortcuts with N greater than count changes nothing, is still swallowed (Windows never sees it), and writes a log line. A jump shows the indicator on the screen under the mouse; a send shows it on the focused window's screen (and when nothing sendable has focus, only the 0004 AC-8 log line).
- **AC-4**: With a window focused on screen S showing workspace n, Win+Shift+N (N not n) switches S to N, and the window is visible there, on top, keeps focus, its position and its size, as 0004 AC-1 describes. With N equal to n nothing changes and the indicator pops up showing n.
- **AC-5**: With a window focused on S showing n, Win+Ctrl+N (N not n) hides the window and its taskbar button, S stays on n, focus goes to the top remaining window on S (the desktop if there is none), and the indicator shows n with the dot for N in the accent color, as 0004 AC-2 describes. With N equal to n nothing changes, the indicator pops up showing n, and the log says the window is already there.
- **AC-6**: A stay press (numbered or arrow) within 1 second of the previous successful stay send acts on the window that send moved, not on the focused window: a numbered press moves it to N, an arrow press moves it one step. If it lands on the workspace S shows, it reappears on top with focus. Every such press restarts the 1 second. A stay press more than 1 second later, any follow press, and any stay press after a switch of any screen (a jump included) act on the focused window again. This is 0004 AC-5 with numbered presses added.
- **AC-7**: Sends act on the focused window's own screen, jumps on the screen under the mouse. The pointer never moves. The rest of spec 0004 applies to numbered sends unchanged: a dialog sends its top owner (AC-7), nothing sendable in focus changes nothing and logs why (AC-8), a failed hide changes nothing (AC-9), `state.txt` records every send and survives a hard kill (AC-10), and the follow guard ignores the sent window for 1 second (AC-11).
- **AC-8**: Holding a numbered send acts once; key repeats are swallowed until the key is released, even if a modifier is let go meanwhile. Holding a jump never switches past N (repeats only show the indicator again).
- **AC-9**: Both the number row digits and the numpad digits (Num Lock on) count. With Shift held, Windows turns numpad digits into navigation keys (End, Down, and so on), so send and follow works from the number row only; des-k does not match those navigation keys and they reach Windows untouched.
- **AC-10**: `settings.toml` `[shortcuts]` takes `jump`, `send_to` and `send_to_stay` as modifier strings, defaults `"Win"`, `"Win+Shift"`, `"Win+Ctrl"`. A missing key means its default. `""` turns that set off, and its digits reach Windows. A value with an unknown name, or with neither Win, Ctrl nor Alt (Shift alone would eat typed digits and symbols), is bad: only that key resets to its default, with a log line (spec 0001). If two keys name the same modifiers, the later one (in the order `jump`, `send_to`, `send_to_stay`) is turned off with a log line. A combo in `previous`, `next` or a `send_*` list that equals a set plus a digit keeps winning for that one digit, with a log line naming it; the set keeps its other digits. Win+Ctrl+Left/Right and the swipe are unchanged.

## Decision

**Chosen option**: Option 1: numbered targets folded into the existing switch and send paths.

The keyboard hook maps a digit plus one of the three modifier sets to a workspace index and posts it to the main thread (`WM_JUMP` for a jump, `WM_SEND_TO` for a send). `ws::jump` reuses `switch_to`; `ws::send` takes a target that is either one step (the arrows) or an index (the numbers), so both kinds share one code path, one quick fix rule, and one set of guards.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch**: no new state and no change to `state.txt`. `State.sent` (spec 0004) is reused as is. `Settings` gains three fields:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `jump` | `Option<u8>` (modifier bits, `None` = off) | `Some(WIN)` | Win+N jumps |
| `send_to` | `Option<u8>` | `Some(WIN \| SHIFT)` | Win+Shift+N sends and follows |
| `send_to_stay` | `Option<u8>` | `Some(WIN \| CTRL)` | Win+Ctrl+N sends and stays |

`DEFAULT_FILE` gains, under `[shortcuts]` after the send lists:

```toml
# Go straight to workspace N, or send the focused window there (N = 1 to 9, number row or numpad).
jump         = "Win"         # "" turns a set off
send_to      = "Win+Shift"   # and follow it there
send_to_stay = "Win+Ctrl"    # and stay where you are
```

The modifier names are parsed the same way `parse_combo` reads them (`Win`, `Ctrl`, `Alt`, `Shift`, any case, joined by `+`), with no key part. `Display` prints them the same way, and `off` for `None`.

**State transitions**: none new. A jump is a switch by index. A numbered send is a 0004 send with an absolute target.

**API surface** (module functions and messages; no network or CLI surface):

| Surface | Kind | Key inputs | Key outputs | Thread | Key errors |
|---|---|---|---|---|---|
| `keys::shortcut(vk, up)` | existing, extended | key event | after the existing list match fails: a digit vk (`'1'..='9'` or `VK_NUMPAD1..=VK_NUMPAD9`) with `mods_held()` equal to an enabled set posts `WM_JUMP` or `WM_SEND_TO` on the first down; sends set `SEND_HELD` as today; downs, repeats and ups of a matched combo are swallowed, with `VK_MASK` sent for Win/Alt combos as today | hook | none |
| `WM_JUMP` | `WM_APP + 4`, thread message | `wParam` = index (0 based, 0 to 8) | main loop calls `ws::jump(index)` | main | none |
| `WM_SEND_TO` | `WM_APP + 5`, thread message | `wParam` = index, `lParam` = 1 follow, 0 stay | main loop calls `ws::send(To::Index(index), follow)` | main | none |
| `WM_SEND` | existing | unchanged | main loop now calls `ws::send(To::Step(dir), follow)` | main | none |
| `ws::jump(index)` | new, public | index | a switch, or the indicator only, plus a log line | main | shell unavailable: logged by `switch_to` |
| `ws::send(to, follow)` | existing, signature changed | `to: To` (`pub enum To { Step(isize), Index(usize) }`), follow flag | as spec 0004, with the steps below | main | as spec 0004, plus "no workspace N" |

`ws::jump(index)`:
1. Screen: the one under the mouse (`GetCursorPos`, `MonitorFromPoint`), exactly as `ws::switch` finds it.
2. `index >= count`: log `jump: no workspace <N> (<count> workspaces)`, `indicator::show(mon, active, count, None)`, stop.
3. `index == active`: `indicator::show(mon, active, count, None)`, stop.
4. Otherwise `switch_to(s, mon, name, index, None, None)`.

`ws::send(to, follow)`, changes to the 0004 order:
- Steps 1 to 3 (stay repeat, window, screen) are unchanged.
- Step 4, target: `To::Step(dir)` gives `(from + count + dir) % count` as today. `To::Index(i)` with `i >= count`: log `send: no workspace <N> (<count> workspaces)`, `indicator::show(mon, active, count, None)`, stop with nothing changed (`State.sent` untouched). Otherwise the target is `i`.
- New guard before steps 5 and 6, first press only (not a repeat): `target == from` (the window is already visible on that workspace): log `send: "<title>" already on <device> workspace <N>/<count>`, `indicator::show(mon, active, count, None)`, stop. This guard is what keeps a first stay press from cloaking a window into the active workspace's own list.
- A repeat whose target equals `from` runs step 7 as written: the window is moved to the front of the same list (no visible change), the 1 second restarts, and the accent dot shows again.
- Steps 5 to 8 are otherwise unchanged.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| hook | which set | `mods_held()` compared with `settings::get().jump`, `send_to`, `send_to_stay` (`None` never matches) |
| hook | index | the vk: `vk - '1'` for the number row, `vk - VK_NUMPAD1` for the numpad |
| hook | explicit lists win | the existing list match runs first; a digit is only tried when it found nothing |
| hook | first press or repeat (sends) | `SEND_HELD` in `keys.rs` (spec 0004) |
| jump | screen | the screen under the mouse, as `ws::switch` |
| jump, send | workspace count | `settings::get().workspaces` |
| jump | current workspace | the screen's `Monitor.active` |
| send | window, screen, from | as spec 0004 (`State.sent` on a repeat, else the root owner of `GetForegroundWindow()`; its own screen; `active`, or the list holding it on a repeat) |
| send | target | `To::Index(i)` from `WM_SEND_TO` `wParam`, or `To::Step(dir)` from `WM_SEND` |
| send, stay | repeat window and its 1 second | `State.sent` and `FOLLOW_COOLDOWN_MS`, shared with the arrows |
| log | too high | `jump: no workspace <N> (<count> workspaces)` / `send: no workspace <N> (<count> workspaces)`, N 1 based |
| log | already there | `send: "<title>" already on <device> workspace <N>/<count>` |
| log | settings | `shortcuts.<key>: <reason>, using <default>`; `shortcuts: <mods> is in both <a> and <b>, <b> turned off`; `shortcuts: <combo> is in <list>, <key> skips digit <N>` |

**Key invariants**:
- A hidden list never holds a window of the workspace its screen shows (the first press `target == from` guard keeps this for stay).
- Everything spec 0004 lists as invariant still holds for numbered sends.
- A jump changes at most one screen: the one under the mouse.
- Each digit plus modifier combo has at most one meaning: explicit lists first, then `jump`, `send_to`, `send_to_stay` in that order.
- When a set is on, all nine of its digits are swallowed, whatever the workspace count.
- The hook only posts; all window changes run on the main thread.

**Security model**: no new data and no new privileges. Same medium integrity process, same windows as a switch. The hook already sees every key; it matches up to 27 more combos.

**Configuration required**: no environment variables or credentials. Three new `settings.toml` keys under `[shortcuts]`. An existing settings file is never rewritten, so they appear in your file only if you add them; missing means the defaults.

**Critical test scenarios** (each maps to an acceptance criterion in ## Requirements):
- Happy path, jump: mouse on a screen showing 1, Win+4: it shows 4 directly, the indicator shows 4, other screens unchanged; Win+4 again: only the indicator. Verifies **AC-1**, **AC-2**.
- Too high: with 4 workspaces, Win+7, Win+Shift+7, Win+Ctrl+7: nothing changes, no taskbar app opens, a `no workspace 7` line each. Verifies **AC-3**.
- Send and follow: Notepad on 1, Win+Shift+3: the screen shows 3 with Notepad on top and focused, same size and place; Win+Shift+3 again: only the indicator. Verifies **AC-4**.
- Send and stay: Notepad on 1, Win+Ctrl+4: Notepad goes, the next window has focus, the dots show 1 with 4 in blue; Win+Ctrl+1 on a window already on 1: an `already on` line. Verifies **AC-5**.
- Quick fix: Win+Ctrl+3 then within a second Win+Ctrl+4: the same window ends on 4, the window that got focus stays; Win+Ctrl+3 then Win+Ctrl+1 (the shown workspace) quickly: it is straight back, focused; Win+Ctrl+Alt+Right then Win+Ctrl+4 quickly: the same window ends on 4. Verifies **AC-6**.
- Other screen: focus a window on screen A, mouse on screen B: Win+Ctrl+3 changes only A; Win+3 changes only B; the pointer never moves. Verifies **AC-7**.
- Hold: hold Win+Shift+3 for two seconds, letting go of Shift first: one send, no jump. Verifies **AC-8**.
- Numpad: on an external keyboard with Num Lock on, Win+Numpad 2 jumps and Win+Ctrl+Numpad 2 sends; Win+Shift+Numpad 2 does nothing in des-k. Verifies **AC-9**.
- Config: `jump = ""`, restart: Win+3 reaches Windows again; `send_to = "Shift"`: a log line, Win+Shift+3 still sends (default); `next = ["Win+2"]`: Win+2 switches next, a `skips digit 2` line, Win+3 still jumps. `cargo test` covers the default file, `""`, bad values and both clash rules. Verifies **AC-10**.

## Build plan

Tracer Bullet: the jump first, end to end from `settings.toml` to the screen, then the sends on the same path.

1. [x] Jump end to end: the `jump` settings field with its default, parsing of a modifier string (`""` = off, bad value resets with a log line), `DEFAULT_FILE`, `SHORTCUT_KEYS`, `Display`; the explicit list clash log; digit matching in the hook (number row and numpad) posting `WM_JUMP`; the main loop arm; `ws::jump`. Unit tests: the default file still parses to the defaults, `""` turns it off, a value with only Shift or an unknown name resets only `jump`, an explicit list combo on a digit is logged. Satisfies **AC-1**, **AC-2**, **AC-3**, **AC-7**, **AC-8**, **AC-9**, **AC-10**.
2. [x] Numbered sends: the `send_to` and `send_to_stay` fields, the same modifiers clash rule (later key turned off) with a test; the hook posting `WM_SEND_TO` with `SEND_HELD`; `ws::To` and the `ws::send` target change, the too high stop, and the first press `target == from` guard; the `WM_SEND` arm passing `To::Step`. Satisfies **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-7**, **AC-8**, **AC-9**, **AC-10**.
3. [x] Try every critical test scenario on your screens, then run `install.ps1` and confirm `des-k.exe` is running afterward. Satisfies **AC-1** to **AC-10**.
4. [x] Default keys changed to Win, Win+Shift, Win+Ctrl (2026-10-27, see rationale): `Settings::default()` and `DEFAULT_FILE`; update the two tests that relied on the old sets (the same modifiers clash test now needs a value that equals an earlier set, and the list on a digit test now expects the set that owns `Win+Ctrl`); `cargo test`, then `install.ps1` and confirm `des-k.exe` is running. Your own `settings.toml` already uses these keys, so nothing changes on your screens. Satisfies **AC-10**.

## Consequences

**Positive**:
- Any workspace is one press away, for you and for a window.
- No new state, no change to `state.txt`, and the arrow sends keep working exactly as before.
- One quick fix rule and one set of guards for arrows and numbers.

**Negative / tradeoffs**:
- With the defaults, Windows' own taskbar shortcuts Win+N (open or switch to pinned app N), Win+Shift+N (open a new copy of app N) and Win+Ctrl+N (go to the last window of app N) stop working for all of 1 to 9, even digits past your workspace count. Setting a key to `""` gives them back.
- Send and follow does not work from the numpad (Windows rewrites Shift plus numpad digits before des-k sees them).
- `ws::send` changes signature, so the existing `WM_SEND` arm changes too.

**Neutral**:
- Two new thread message ids, `WM_APP + 4` and `WM_APP + 5`, which `AGENTS.md` lists.

## Follow-up

- [ ] After the build, `/sync` adds `WM_APP + 4` jump and `WM_APP + 5` send to (spec 0005) to the thread message ids in `AGENTS.md` `## Rules`.
- [ ] Feature 5 (send window to another screen) stays planned; Windows' own Win+Shift+Left/Right may already cover it, worth trying before designing it.
