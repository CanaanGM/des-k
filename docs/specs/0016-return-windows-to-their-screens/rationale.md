# 0016. Rationale: return windows to their screens and workspaces

## Context

You run des-k on a laptop with two external screens, each with its own set of workspaces. When you undock, only the laptop panel is left, and when you dock again, or wake from sleep, or boot after a crash or a power cut, you expect each window to be back on the screen and workspace where you left it.

Today des-k does not do that. When a screen is gone for 10 one second checks in a row, `ws::check_screens` shows every window it hid on that screen and drops the screen's workspaces from memory (`s.monitors.remove`). Windows moves those windows onto a remaining screen, all on the workspace that screen shows. When the screen comes back later, nothing moves back. After a new sign in, restore (spec 0003, AC-8) leaves a window visible when its saved screen is missing, and at the end of its 3 minute period deletes every unmatched entry, so a crash while undocked loses the layout for good.

Two more forces shape this. First, des-k keys screens by Windows' device name (`DISPLAY2`), which can change between reboots or when the same monitor is plugged into another port or dock; the scope's Deferred list already names this ("Stable screen names"). Second, Windows moves a removed screen's windows before des-k's display change message is handled, and no check runs while the PC sleeps, so des-k cannot read a window's position at the moment of the unplug; it must already know it.

The owner rejects designs that hide state they could see, so whatever des-k remembers about an unplugged screen must be visible somewhere. Workflow tier is Prototype: the owner tries it on their own screens.

## Options considered

### Option 1: Fold by workspace number, remember each window's home, return by itself (chosen)

On unplug, hidden windows go into the same numbered workspace of the main display, shown ones stay in view, and each carries an away record (home screen, workspace, spot, pinned). The screen is remembered by its monitor ID. When it is back for 2 s, its away windows go home unless you moved them.

**Pros**:
- Groups survive on the laptop, and the full layout comes back with no action from you.
- Works the same for unplug, sleep, crash and power cut, because the memory is in `state.txt`.
- Reuses what exists: hidden lists, `map_rect`, `move_to`'s placement, restore's fingerprints.

**Cons**:
- The most code of the options: screen ids, spots, away records, a new line type, a claim rule.
- The laptop's workspaces get crowded while undocked.

### Option 2: Keep the gone screen's workspaces parked, show nothing, return on replug

Do not fold: an unplugged screen's hidden windows stay hidden in its own (now invisible) workspaces until it returns. Only the windows Windows moved stay in view.

**Pros**:
- Less code: no fold, and return is mostly "uncloak in place".
- The laptop stays uncluttered.

**Cons**:
- Windows you cannot reach for hours or days: the finder and overview would have to show parked windows of a screen that does not exist, and a frozen app or a crash leaves them cloaked with no screen to show them on. This is exactly what today's 10 s rule exists to prevent.
- Goes against "show all information": the windows are there but out of your reach.

### Option 3: Lean on Windows' "Remember window locations based on monitor connection"

Turn that Windows setting on and let Windows move windows back; des-k only keeps its workspace lists keyed by a stable screen id.

**Pros**:
- Least des-k code for positions.

**Cons**:
- Windows only handles windows in view and only within one sign in; it knows nothing about des-k's hidden windows or workspaces, and nothing about a reboot or a power cut.
- Behavior depends on a setting des-k does not control.

### Option 4: Fold, but return only when you ask (tray item or shortcut)

As option 1, but windows go back only when you click "Put windows back".

**Pros**:
- Nothing moves without you; no surprise moves on a short dock.

**Cons**:
- A step after every dock, which is the chore this feature exists to remove.
- The claim rule is still needed, so it saves little code.

## Rationale

Option 1 is the only one that meets all four cases you named (unplug, replug, sleep, crash or power cut) with no action from you, and it keeps every window reachable at all times, which option 2 does not. Its extra code sits on parts des-k already has: hidden lists per workspace, `map_rect` and `move_to` for placing a window on another screen, and restore's fingerprints for matching after a reboot. Option 3 cannot see hidden windows at all, and option 4 brings back the manual step.

Your answers fixed the behavior: fold by number onto the main display, shown windows stay in view, your own moves win, return by itself, the monitor's own ID, remembered with no limit and shown on the settings page, crash handled like an unplug, pinned windows stay pinned, placement by spot and state, retry a stuck window for 10 s, and tell you on the indicator and in a balloon.

Calls made in this spec (not asked, open to change):
- **Spots are refreshed each 1 s check and frozen when a screen goes missing** (runner up: read spots at the display change). Windows moves windows before des-k hears of the change and no check runs during sleep, so only a recent snapshot is trustworthy.
- **A normal window's spot is its window rect, not its restore rect** (runner up: `GetWindowPlacement` for all). A snapped half's restore rect is its size before the snap, which would come back wrong.
- **The EDID serial is read from the monitor's registry key** (runner up: the `monitorDevicePath` alone). The path contains the port, which would make the same monitor on another port a new screen, against your choice.
- **A remembered screen with no away windows left is forgotten by itself** (runner up: keep it listed until Forget). With nothing to return, keeping it only clutters the settings page with old hotel TVs and projectors.
- **A claim is detected by screen change at the 1 s check plus des-k's own move calls** (runner up: a `EVENT_SYSTEM_MOVESIZEEND` hook). Win+Shift+Left/Right is Windows' own (spec 0006) and fires no des-k code, so a check is needed anyway, and it covers drags too.
- **Return waits for 2 checks present** (runner up: return at once). A screen often flickers on wake and on a DisplayPort relink; 2 s costs nothing noticeable.
- **The indicator note shows for 3 s**, longer than a switch, so a line of text can be read.

## Cross check (2026-11-04)

A read only pass on another model found two bugs and eleven open choices; you chose to apply its fixes:
- **Windows' own moves looked like yours.** Windows moves windows off a removed screen, and back on with its "remember window locations" setting, before des-k sees it. A claim now needs a move between two present screens that does not land on the window's home (AC-6).
- **A frozen app could stall des-k.** Move calls skip `hung` windows (invariant 8), and notes and balloons are shown after the `ws` lock is released (invariant 7).
- Also settled: how spots are read for minimized and cloaked windows, away records for windows in view start at the first missing check, twins with no serial ordered left to right (the port id is not stable across reboots), which connections count as the laptop panel, note and balloon wording, pin while away is your move, closed windows noticed each check, adoption checks app and class too, the rect set again after a move between scales, a 30 s grace after wake (AC-18), and the one time loss on an upgrade while undocked.

Two suggestions were not taken:
- **Split the reboot memory into its own spec.** You named crash and power cut as part of the ask, and the build plan already makes it a separate step (5 and 6) you can try on its own.
- **A home field on every window instead of `A` lines.** Fewer types, but you confirmed the `A` line model, and a separate away record keeps the away state explicit and easy to list on the settings page.
