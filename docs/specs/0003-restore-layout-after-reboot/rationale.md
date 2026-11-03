# Rationale: 0003. Restore layout after reboot: match reopened windows to their saved workspaces

**Date**: 2026-10-26

## Context

> ⚠️ Premise note: you picked "saved boot time" to tell a fresh boot from a des-k restart. With Fast Startup on (the Windows default), a shutdown is a partial hibernate: the machine's uptime keeps counting, so the boot time does not change, yet you are signed out and every window is new. Boot time would miss the most common daily case. The logon session id (a number Windows gives each sign in) changes on a reboot, a Fast Startup boot, and a sign out, and stays the same across des-k restarts and explorer restarts. It has every property you wanted from boot time, so this spec uses it instead (`L` line, not `B`).

des-k remembers a layout in `state.txt` as window handles plus process ids, and only for windows it hid. That survives a des-k restart, an update, or a hard kill, because the windows themselves live on. A reboot, a Windows crash or power loss, a Fast Startup shutdown, and a sign out all end every window. Apps then reopen with new handles and new process ids, through Windows' "restart apps" setting, startup apps, and browser session restore, spread over the first minutes after login. Today `ws::load()` finds no live handle, drops every entry, and every window reopens visible on whatever screen it lands on.

The forces: there is no stable window identity on Windows across process lifetimes, so any match is a guess built from the program, the window class, and the title. Titles drift (browser tabs, modified markers in editors). Apps reopen at their own pace, some before des-k itself starts at login. A wrong guess is costly: a window hidden on the wrong workspace looks lost. And des-k must never fight you: a window you open on purpose must not vanish.

The existing rules constrain the design: all window changes happen on the main thread, callbacks only post messages, des-k only ever uncloaks windows it hid, and every exit path shows hidden windows again. Follow activation (spec 0002) jumps a screen to any hidden window that takes focus, which a reopening app often does.

## Options considered

### Option 1: Fingerprint every window, match during a short period after a fresh sign in

Save program, class, and title for every hidden and visible window. When des-k starts in a new logon session, check for candidate windows every half second for a few minutes, match each to a saved entry with a strict rule, and cloak the hidden ones back into place.

**Pros**:
- Fits the existing model exactly: restore only cloaks and fills `hidden[ws]`, so every exit path already handles it.
- One timer handles the startup sweep, late windows, title changes, retries after a failed cloak, and the deadline.
- Strict matching plus "leave it visible when unsure" keeps wrong guesses rare.

**Cons**:
- A window can be visible for up to half a second before it is hidden.
- Fingerprints are only as fresh as the last save.

### Option 2: Same matching, driven by win event hooks (`EVENT_OBJECT_SHOW`, `EVENT_OBJECT_NAMECHANGE`)

React to windows appearing and titles changing instead of polling.

**Pros**:
- Hides a reopened window within milliseconds, so there is almost no flash.

**Cons**:
- Still needs a timer for the deadline, the startup sweep, and retries after a failed cloak, so it is a timer plus two hooks.
- `NAMECHANGE` fires for every title change system wide; it needs careful filtering and a new thread message id.
- More moving parts in exactly the place (callbacks next to `ws` state) the threading rules guard most.

### Option 3: Make des-k a session manager that relaunches apps

Save each window's command line and start the programs itself after login, then place them.

**Pros**:
- Restores apps that don't reopen on their own.

**Cons**:
- Command lines don't restore documents, tabs, or app state; most apps reopen better on their own.
- Starting programs at login doubles windows for apps that also restore themselves.
- A much larger feature, and against your choice to only place what reopens.

### Option 4: Match by title only, any time a matching window opens

No logon detection and no period: whenever a window appears whose title matches a saved entry, hide it there.

**Pros**:
- Simplest code; also catches apps you reopen by hand later.

**Cons**:
- Hides windows you open on purpose, the one thing des-k must never do.
- Titles alone collide (every "New Tab", every "Untitled").

## Rationale

Option 1 is chosen because it reuses everything des-k already trusts. A restored window ends up in `hidden[ws]` like any window a switch hid, so showing, quitting, the panic hook, and saving need no new path. The logon session id cleanly separates "same windows, adopt by handle" from "new windows, match by fingerprint", so the proven restart path is left untouched (AC-7).

Polling over hooks (Option 2) is a deliberate trade of a short flash for simplicity. A timer is needed anyway for the deadline, the startup sweep, and retries after a brand new window fails `GetViewForHwnd`; adding two system wide hooks on top would put more code beside the non reentrant `ws` mutex for a gain of under half a second, once per login. If the flash proves annoying on your screens, Option 2 can replace the timer's sweep without changing the matching rule or the file format.

The matching rule is strict on purpose, following your choice that a wrong guess is worse than a miss: one window per entry, the same program and class always required, the title used only to break ties, and a match that is unclear stays visible. Saving visible windows as `V` entries lets them soak up matches, so an app with windows both hidden and visible doesn't hide the wrong one. Requiring the window to reopen on its saved screen follows the same rule: hiding it into another screen's workspace would make it reappear on a screen you didn't switch.

Smaller calls made while writing:
- **Logon session id over boot time**: see the Premise note. Runner up: boot time, which misses Fast Startup and sign out.
- **Logon time stored next to the id** (added after the cross check): restore only runs when you signed in at most `minutes` + 2 minutes before des-k started, so starting des-k by hand hours later never hides windows you opened yourself. It also covers the rare case of Windows reusing a logon id after a reboot. Runner up: also trigger when no saved window can be adopted by handle, which misfires if you closed everything while des-k was off.
- **Matching hardened after the cross check**: a `resolved` set so a window is never placed twice, rule 2 judged on the whole saved group so using up a `V` entry cannot turn a careful match into a guess, and two passes per tick so an exact title match always beats a guess. A window on the wrong screen stays pending instead of using up its entry, because apps often open on the main screen and move a moment later.
- **`app` as exe path, app id for Store apps**: every Store app runs as `applicationframehost.exe`, so the exe alone would lump them together. Runner up: exe path only, with the title separating Store apps.
- **Half second tick**: short enough that a reopened window barely shows, cheap enough over 3 minutes with the fingerprint cache. Runner up: 1 second.
- **`[restore]` table** with `enabled` and `minutes`, instead of the top level `restore_after_reboot` / `restore_minutes` names shown during the interview: the `[indicator]` table already groups a feature's keys with `enabled` this way (spec 0001), and conformance wins.
- **Pending entries written back during a restore**: Windows Update often restarts twice; without this the second boot would find nothing. Runner up: accept the loss.
- **`restored` map with 2 seconds of follow suppression**: `follow()` would otherwise see a hidden, cloaked window that has focus and jump the screen to it. Runner up: a flag that disables follow for the whole period, which would also block real activations you trigger during login.
- **New `src/restore.rs`**: keeps fingerprinting and the pure matcher (with its tests) out of `ws.rs`, which keeps owning the state and the cloaking. Runner up: everything in `ws.rs`.
