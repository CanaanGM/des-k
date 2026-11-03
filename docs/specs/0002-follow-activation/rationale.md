# Rationale: 0002. Follow activation: jump to a hidden window's workspace when it comes to the front

_Decision record for [spec 0002](index.md): why this design was chosen._

## Context

des-k hides a workspace's windows by cloaking them (making them invisible through the shell, while they keep running). An app does not know its window is cloaked. When it decides to reuse that window, it brings it to the front as usual. On 2026-10-26 the `spikes/cloakwatch` log recorded exactly this on your machine: Notepad was hidden on workspace 1, you opened a `.txt` from Explorer while on workspace 2, and Windows fired `EVENT_SYSTEM_FOREGROUND` for the Notepad window (`61b92`) while it stayed cloaked. Nothing appeared, no taskbar flash happened (des-k removed its taskbar button), and the window only showed up when you switched back by hand nine seconds later. Keystrokes in that gap go into a window you cannot see.

The same test showed Photos on your machine opens a new window for every image, which lands on the visible workspace. So Photos never hits this case, and the scope's "Done when" example (Photos) cannot prove the feature. Notepad, Alt+Tab, and dialogs owned by a hidden window can.

Forces:
- des-k only learns about the activation after the app has acted. It never sees which file was opened, so it cannot start a new copy of the app on the current workspace. It can only move you or the window.
- AGENTS.md threading rule: all window changes, the indicator, and saving happen on the main thread. `ws.rs` guards its state with a `std::sync::Mutex`, which is not reentrant, so a callback must never be able to call into `ws` while a switch holds the lock.
- des-k's own switching changes the foreground too (it focuses a shown window, or the desktop, after hiding the focused one). Windows may also briefly hand focus to a window in the middle of being cloaked. Reacting to those would bounce you straight back.
- AGENTS.md safety rule: only ever uncloak windows des-k hid itself. A window on another Windows virtual desktop, or a suspended UWP window, also reports `DWMWA_CLOAKED == 2`.
- One person, Prototype workflow: verified by hand on your three screens, `cargo test` covers settings parsing only.

## Options considered

### Option 1: Foreground win event hook, posted into the main loop

`SetWinEventHook(EVENT_SYSTEM_FOREGROUND, …, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)` on the main thread. Out of context hooks (the callback runs in des-k's own process, not injected into other apps) deliver their callback on the hooking thread while it waits in `GetMessageW`. The callback only posts a message to the main loop, which then looks the window up in `ws.rs` and jumps.

**Pros**:
- Already proven on your machine: cloakwatch saw this exact event for the cloaked Notepad window.
- Event driven, so there's no cost while nothing happens (it's a laptop, battery matters).
- Fits the threading rule: the work runs on the main thread, outside any `ws` lock.

**Cons**:
- It also reports des-k's own and Windows' transient focus changes, so it needs a "still the foreground window?" check to drop stale events.
- One more global hook to keep alive. It dies with the process, so it needs no cleanup, but it is also invisible when it breaks.

### Option 2: Shell hook window (`RegisterShellHookWindow`)

Register the indicator window for shell hook messages and react to `HSHELL_WINDOWACTIVATED` / `HSHELL_RUDEAPPACTIVATED`.

**Pros**:
- Plain window messages, no callback. It also gets `HSHELL_FLASH`, if flashes ever matter.

**Cons**:
- The shell sends these for taskbar windows. des-k deletes a hidden window's taskbar button, so activation of exactly the windows we care about may never be reported. Unverified, and the likely answer is no.
- Ties the feature to the indicator window, which already carries two other jobs.

### Option 3: Poll the foreground window on a timer

Check `GetForegroundWindow()` every 100 to 250 ms and jump when it is a window des-k hid.

**Pros**:
- Simplest code, no hook, no stale event problem in the same form.

**Cons**:
- Wakes the CPU several times a second, forever, on a laptop.
- Adds up to a quarter second of typing into an invisible window, and can miss a brief activation entirely.

## Rationale

The deciding fact is the evidence: the foreground event fires for a cloaked window and the window stays cloaked. That makes Option 1 the one signal known to work here. Option 2's signal is tied to taskbar buttons, which des-k deliberately removes, and Option 3 pays a constant battery cost to be slower. The stale event problem that comes with Option 1 is real (des-k's own switch moves focus, and Windows can briefly activate a window mid cloak). It is solved cheaply by checking, when the posted message is handled, that the window is still the foreground one. By then des-k's own switch has finished and set the final focus.

You chose to jump to the window rather than pull it to the current workspace. des-k cannot spawn a fresh app window for you, because it never learns which file was opened. The "new window here" half belongs in each app's own setting (Notepad: open files in a new window; Photos already does it). Jumping keeps every window where you put it. Pulling would slowly scatter windows and is really feature 4 (send window to workspace).

Recommended calls made in this spec (runner up in brackets):
- The win event callback only posts `WM_FOLLOW` (`WM_APP + 2`, hwnd in `wParam`) to des-k's own thread with `PostThreadMessageW`, as `keys.rs` does; the main loop calls `ws::follow` [doing the work inside the callback]. Strictly, nothing in the switch path pumps messages (the main thread is MTA, so the cross process COM calls in `Shell::hide` do not pump), so a callback could not land mid switch today. Posting is still the right call: it keeps that true if the switch path ever changes, and it gives the stale check its "after everything else" timing.
- Drop the event if `GetForegroundWindow()` is no longer that window when the message is handled [a short "ignore follows right after a manual switch" timer, which also eats real activations].
- Bounce guard: each screen remembers its last switch (any kind: shortcut, swipe or follow) as the time and the workspace it left. For 1 second, a follow that would take that screen back to the workspace it just left is dropped and logged. This covers the case where des-k's own "focus a shown window" call is refused and Windows leaves focus in a window that was just hidden [arming the full cooldown on manual switches, which would also block an unrelated real activation right after a swipe].
- Do not call `SetForegroundWindow` in the follow case unless needed: the activated window already has focus. After showing the workspace, if `GetForegroundWindow()` is not the activated window, call `SetForegroundWindow` once and log if it still is not. des-k is not the foreground process at that moment, so Windows may refuse the call [always calling it, which hides a refusal; or the injected key trick to gain focus rights, kept unused until a log shows refusals].
- Look the window up both as itself and as `GetAncestor(hwnd, GA_ROOTOWNER)` (its top owner), so owned dialogs follow their owner [only the window itself, which misses AC-5].
- Find the target screen from the `ws.rs` hidden lists, not from `MonitorFromWindow` [the window's position, which an app can change while it is hidden].
- Cooldown is a fixed constant `FOLLOW_COOLDOWN_MS = 1000` in `ws.rs`, kept in memory per screen and never saved [a settings key, which nobody needs until it proves wrong]. It is a rate limit against apps that keep grabbing focus, not a correctness guard; the stale check and the bounce guard carry correctness.
- Only follow jumps start the cooldown; a shortcut or swipe switch does not [cooling down after manual switches too, which would block a real activation right after you swipe].
- When `follow_activation` is off, the hook is never installed [installing it and checking the flag per event].

