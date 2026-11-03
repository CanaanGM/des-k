# 0011 child: the gesture map and its actions

## Summary

`settings.toml` gets a `[touchpad]` section with three keys and a list of `[[gesture]]` rules. Each rule says which gesture runs which action. Actions are des-k's own moves, media and volume keys, Windows things des-k redoes itself, or any key combo you write.

## Settings

```toml
[touchpad]
# Read three and four finger gestures from the touchpad (after the welcome screen's setup).
enabled = true
# How far the fingers travel before a swipe counts, in mm (5 to 50).
swipe_mm = 15
# How long a gesture with a "times = 2" rule waits for its second go, in ms (100 to 1000).
twice_ms = 300

# One block per gesture. fingers = 3 or 4; swipe = "left", "right", "up" or "down", or tap = true;
# times = 1 or 2 (optional); do = an action name below or a key combo like "Ctrl+Shift+T".
# Writing any [[gesture]] block replaces this whole default list.
[[gesture]]
fingers = 4
swipe = "left"
do = "next"
# ... the rest of the default map (index.md AC-2), each written out
```

- `settings.rs`: `TOP_KEYS` gains `touchpad` and `gesture`; new `TOUCHPAD_KEYS = ["enabled", "swipe_mm", "twice_ms"]` and `GESTURE_KEYS = ["fingers", "swipe", "tap", "times", "do"]`. The default settings text writes the full default map out, so you see and edit it.
- `Settings` gains `touchpad_enabled: bool`, `swipe_mm: u32`, `twice_ms: u32`, `gestures: Vec<Gesture>`.
- `Gesture { fingers: u8, motion: Motion, times: u8, action: Action }`, `Motion = Left | Right | Up | Down | Tap`.
- `Action` = one of the names below, or `Combo(Combo)`, one combo per rule, parsed by the existing `parse_combo`.
- Validation per index.md AC-4 and AC-6. `gesture` that is not an array of tables: the whole list resets to the default with a log line. A field not in `GESTURE_KEYS` makes that rule bad (skipped, logged). `tap` must be `true`; `tap = false` is a bad rule. If `do` matches an action name (case insensitive), it is that action, never a combo.
- `settings::rules_for(fingers, motion) -> (Option<usize>, Option<usize>)` returns the indexes of the once and twice rules for a gesture; the reader calls it (0011-touchpad-reader).
- Tests (settings is the only tested module): the default map parses to AC-2; each bad rule kind is skipped with its warning and the others kept; a duplicate is skipped; a combo parses; `times = 2` kept; `[touchpad]` range resets.

## Actions

| `do` | What runs (main thread, `src/gestures.rs`) |
|---|---|
| `previous`, `next` | `ws::switch(-1 / +1)`, as `WM_APP` |
| `send_previous`, `send_next` | `ws::send(Step(∓1), true)` |
| `send_previous_stay`, `send_next_stay` | `ws::send(Step(∓1), false)` |
| `jump_1` … `jump_9` | `ws::jump(n - 1)` |
| `send_to_1` … `send_to_9` | `ws::send(Index(n - 1), true)` |
| `send_to_stay_1` … `send_to_stay_9` | `ws::send(Index(n - 1), false)` |
| `finder` | as `WM_FINDER` (toggle) |
| `overview` | opens the overview; does nothing if it is open |
| `show_desktop` | if the overview or finder is open, closes it (focus back) and stops. Else, on the screen under the mouse: if that screen has a "desktop shown" list, restores the windows in it that are still minimized, the old top one last so it ends on top with focus, and clears the list; otherwise minimizes every listed window (`ws::app_windows`) visible and not already minimized on that screen's shown workspace (so windows you minimized yourself are never restored by it) and keeps them, top first, as that screen's list. A switch, jump or send on that screen clears its list. |
| `app_next` | on the screen under the mouse, among listed, visible, not minimized windows of the shown workspace: the bottom one comes to the top and gets focus (repeated, this walks through all of them) |
| `app_previous` | the same set: the top one goes to the bottom (`SetWindowPos HWND_BOTTOM`, no activate) and the new top gets focus |
| `search` | types `Win+S` (with `MAGIC`, straight to Windows) |
| `notifications` | types `Win+N` (with `MAGIC`) |
| `middle_click` | a middle button down and up at the cursor (`SendInput`, with `MAGIC`) |
| `play_pause`, `next_track`, `previous_track` | the media keys (with `MAGIC`) |
| `volume_up`, `volume_down`, `mute` | the volume keys (with `MAGIC`) |
| `none` | nothing (useful to switch off one default gesture) |
| a combo, e.g. `"Ctrl+Shift+T"` | types its modifiers down, the key down and up, the modifiers up, **without** `MAGIC`, so des-k's keyboard hook sees it like a real press: a combo that is a des-k shortcut runs that shortcut. Win or Alt combos end with `VK_MASK` as the hook does, so no Start menu or menu bar opens. |

- Any des-k action while the overview or finder is open behaves exactly as its key would (spec 0010 AC-12).
- Focus (`app_*`, the `show_desktop` restore) goes through the same helper `ws` already uses to focus a window after a switch, so it gets the same foreground handling. The check step (index.md Build plan step 1) also tries `app_next` focus from a gesture. Follow activation needs no change: these windows are all on the shown workspace, so it has nothing to follow.
- A combo while you hold a key yourself (say Shift) combines with it, as a real press would. Windows never lets a program type `Ctrl+Alt+Del`, and `Win+L` is blocked the same way; such a combo does nothing (logged at settings load).
- A gesture made while a menu (the tray menu) or a window drag is open is dropped, as keys are today, because thread messages are lost in those loops.
- Each run logs one line: `gesture 4 left → next`.
- An action that cannot run (no focused window to send, no listed window for `app_next`) does nothing and logs why.
- Key typing reuses `keys.rs`'s `send` (made `pub(crate)`) for keys with `MAGIC`; the combo path needs one without `MAGIC` (a flag on the same function).

## Value sourcing

| Value | Source |
|---|---|
| the rule to run | `wparam` of `WM_GESTURE` = index in `settings::get().gestures` |
| target screen | the screen under the mouse (`GetCursorPos`, `MonitorFromPoint`), as the keys |
| windows for `show_desktop`, `app_*` | `ws::app_windows` on that screen's shown workspace, z order from `GetWindow`/`EnumWindows` |
| "desktop shown" list | per screen, in memory on the main thread; cleared on a switch, jump or send on that screen; lost at exit |
| combo keys | `Combo` from `parse_combo` |

## Security

- `do` names an action or a key combo, never a program or a path; nothing here runs a program.
- A combo is typed by an admin process, so it reaches admin windows. Accepted by you on 2026-10-31 (index.md Consequences). Every other action does its job without typing into the focused window, except `middle_click`, which clicks where you point, like your own click.

## Short rationale

A list of rules leaves room for `times = 2` now and new gesture kinds later without new key names, and matches TOML's array of tables. Action names reuse the `[shortcuts]` names so one vocabulary covers keys and gestures. `show_desktop` minimizes per screen because des-k's workspaces are per screen; minimizing (not hiding) keeps those windows on the taskbar, so you can bring one back by hand. `app_next` and `app_previous` rotate the window stack, so repeated swipes visit every window in a fixed order, where most recently used order would bounce between two. Runner up for the format: nested `[gestures.four]` keys, rejected because "twice" would need new key names for every gesture.
