# 0011. Touchpad gestures: rationale

## Context

The four finger swipe reaches pdesk as `Win+Ctrl+Left/Right`, typed by a part of Windows after you set a custom shortcut in Settings, Touchpad, Advanced gestures (spec 0001, README). On 2026-10-31 you found that the swipe does nothing while Task Manager has focus, though the same keys pressed by hand work (pdesk runs as admin since spec 0008). The likely cause: whatever types the swipe's keys is not admin, and Windows (UIPI, its rule that a normal program cannot type into an admin window) drops them before any keyboard hook sees them. That is unverified; the check step confirms it.

pdesk cannot fix this on the key route, because the keys never arrive. Windows' own gesture choices are also fixed: per finger count, one preset for all four swipes, one tap action, nothing done twice. You want every three and four finger gesture to be yours. Two forces shape the choice: pdesk's AGENTS.md rule that nothing in `%LOCALAPPDATA%\pdesk` names a program to run (anything running as you can write there), and that Windows' gesture settings live in undocumented registry values.

## Options considered

### Option 1: Keep the Windows shortcut route, document the gap
Windows types the keys; the README says gestures stop over admin windows.
- Pros: no code; nothing undocumented.
- Cons: the bug stays; Windows' fixed choices stay.

### Option 2: Read the touchpad through Raw Input, own the setup (chosen)
pdesk reads touchpad reports, recognizes gestures, runs mapped actions, and turns Windows' gestures off through a welcome screen.
- Pros: works over admin windows; any gesture, any action, twice gestures; one source, no double actions.
- Cons: gesture recognition is now pdesk's code; setup writes undocumented values; a hard kill leaves gestures off until the next start.

### Option 3: Raw Input only over admin windows
pdesk reads the touchpad only while an admin window has focus and trusts the typed keys elsewhere.
- Pros: smallest behavior change.
- Cons: two code paths for one gesture; an elevation check on every swipe; no custom map.

## Rationale

Only reading the touchpad itself gets around UIPI, since pdesk runs as admin and Raw Input does not depend on which window has focus. Once pdesk reads gestures, keeping Windows' gestures on would act twice, so pdesk must own the setting; doing it in a welcome screen makes the takeover visible and reversible. Option 3 fixes the bug with less code but keeps two paths and gives you no map, which you asked for.

You chose to let key combos in `settings.toml` reach admin windows. I recommended keeping combos in an admin only file, or skipping them over admin windows, because anything running as you can write `settings.toml` and an admin pdesk would type its combos into admin windows (for example a paste then Enter in an admin terminal). You accepted that risk; it is recorded in index.md Consequences, and AGENTS.md needs a matching note.

Other calls made with you: the list of rules format (room for `times = 2`); waiting for a second go only when a twice rule exists (plain switching stays instant); one action per swipe; swipe left goes to the next workspace by default; `swipe_mm` and `twice_ms` as settings; all three and four finger gestures turned off at setup; Windows' values back on Quit and uninstall (uninstall starts with Quit); the welcome screen designed in `pdesk-design` first.

Calls I made: a separate gesture thread (like the keyboard hook); firing a swipe at the distance, not at lift; tap at 250 ms and 3 mm; `show_desktop` closes an open overview or finder first, as Windows' swipe down closes Task View; `app_next` and `app_previous` rotate the stack for a fixed visiting order; combos typed without `MAGIC` so a combo that is a pdesk shortcut runs it; `touchpad.txt` saved only at Apply, never at start.

## Update 2026-10-31: setup when Windows' gestures can't be switched live

The check step found that no registry write takes effect without a sign in, so the first plan (pdesk turns Windows' gestures off and back on) could not work. You settled three points:

- **The gate reads the registry.** Settings writes the 13 values the moment you change a gesture, so pdesk reads them once per recognized gesture and acts only while all are off. Runner up: a "Done" button that sets `applied=1`; dropped because pdesk cannot see your clicks, and a missed one makes every gesture act twice.
- **Quit still gives gestures back, from the next sign in.** Your call over the recommended "no exit writes anything". It costs a `restored` line holding the sign in's logon time, so a start in the same sign in can set the registry off again, and sign out and shutdown write nothing (or Windows would get its gestures back at every sign in). It rests on one more check (build step 4); if a sign in ignores the written values, the recommended option becomes the fallback.
- **`enabled = false` with Windows' gestures off only warns**, in the start log and the setup screen; nothing opens by itself.

With the gate needing Windows' gestures off, `[touchpad] enabled` can default to `true` as AC-6 says, which settles the conflict found in build step 3.
