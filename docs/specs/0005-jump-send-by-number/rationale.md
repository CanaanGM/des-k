# Rationale: 0005. Jump and send by number: go straight to workspace N, or send the focused window there

Build spec: [index.md](index.md).

## Context

des-k moves between workspaces one step at a time: Win+Ctrl+Left/Right and the touchpad swipe switch a screen by one, and the spec 0004 sends move a window by one. With four workspaces, reaching the far one takes up to three presses, and you have to watch the indicator to know when to stop. You want to name the workspace instead: go to 4, or put this window on 4.

The pieces this needs already exist. A screen's current workspace is simply what is visible on it, `switch_to` can show any workspace, and spec 0004's `ws::send` already handles the hard parts of moving a window (dialogs, focus, the follow guard, the quick fix repeat, saving). What is missing is a way to say "workspace N" from the keyboard, and a send that takes an absolute target instead of a step.

The keys matter. Windows already gives Win plus a digit to the taskbar (open pinned app N), and Win+Ctrl, Win+Shift, Win+Alt and Win+Ctrl+Shift plus a digit to related taskbar actions. Any number shortcut takes one of those, or an app's Alt plus digit. The numpad adds a quirk: with Num Lock on and Shift held, Windows turns numpad digits into navigation keys before a low level hook sees them.

## Options considered

### Option 1: Numbered targets folded into the existing switch and send paths

The hook maps a digit plus a modifier set to an index and posts it. `ws::jump(index)` calls `switch_to`; `ws::send` takes `To::Step` or `To::Index`, so the arrows and the numbers share one path.

**Pros**:
- Every 0004 rule (dialogs, follow guard, quick fix, saving, accent dot) applies to numbers with no copy.
- Small: one new public function, one enum, two message ids.

**Cons**:
- Changes the signature of `ws::send`, which already shipped, so the arrow sends need a quick check again.

### Option 2: A separate `ws::send_to` next to `ws::send`

Leave `ws::send` untouched and write a second function for absolute targets.

**Pros**:
- The shipped arrow path is not touched at all.

**Cons**:
- Steps 1 to 3 and 5 to 8 of spec 0004 are duplicated, and the shared quick fix rule (an arrow press after a numbered one, and the reverse) would have to be kept in sync across two functions.

### Option 3: Twenty seven explicit lists in `settings.toml`

`jump_1 = ["Win+Ctrl+1"]` up to `send_to_stay_9`, matched like the arrow lists.

**Pros**:
- Any key per workspace, including non digits.

**Cons**:
- A very long settings file for a choice you said you'd make once (the modifiers), and per entry clash rules across up to 33 lists.

## Rationale

Option 1 wins because the hard behavior already exists and is tested on your screens: a number is just a different way to pick the target. Folding it into `ws::send` means a stay arrow press and a stay number press share one quick fix window, which is what you'd expect when you mix them. Option 2 buys safety for the shipped arrow path at the price of two copies of the same eight steps, and the copies would drift. Option 3 solves a flexibility problem you don't have.

### Choices you made in the interview

- A new feature next to feature 5; feature 5 (send to another screen) stays planned.
- Jump keys Win+Ctrl+1 to 9 at first, chosen over Alt+N (clashes with apps) and Win+N (pinned taskbar apps). Changed after you tried it; see the update below.
- Both numbered sends, like the arrows: Win+Ctrl+Shift+N follows, Win+Ctrl+Alt+N stays.
- A jump acts on the screen under the mouse, like the arrows and the swipe.
- Jumping to the workspace already shown only shows the indicator (no back and forth toggle).
- A number past the workspace count is swallowed and shows the indicator, rather than reaching Windows.
- Settings take the three modifier sets, not fixed keys and not a list per workspace.
- A quick second stay press moves the window just sent (the 0004 rule).
- Number row and numpad both count; send and follow skips the numpad because of the Shift quirk, rather than decoding navigation keys.
- The pointer never moves.

### Calls made in the spec (recommended, not asked)

- **Two new messages** (`WM_JUMP`, `WM_SEND_TO`) rather than packing an index into `WM_SEND`'s `wParam` next to the step. Runner up: one message with a flag; it saves an id but makes every reader decode which kind it got.
- **Explicit lists win per digit**, not by turning a whole set off. Runner up: turn the set off, as 0004 does for lists. A single shared digit should not cost you the other eight.
- **A modifier set needs Win, Ctrl or Alt.** Shift alone, or nothing, would swallow typed digits and symbols everywhere. Runner up: allow anything and trust the file; one typo would break typing numbers.
- **Repeats of a held jump are not swallowed specially**: they land on the same workspace and only show the indicator again, so no `SEND_HELD` for jumps. Runner up: treat jumps like sends; extra state for no visible difference.
- **A first press to the workspace the window is on stops early.** Without that guard, a stay press would cloak the window into the active workspace's own list, which breaks the main invariant.

## Update 2026-10-27: default keys

After using the build, you changed the defaults from Win+Ctrl / Win+Ctrl+Shift / Win+Ctrl+Alt to **Win / Win+Shift / Win+Ctrl**.

- **Why:** Win+Ctrl+Alt+N was very awkward to press, and the Win+Ctrl set took a Windows shortcut anyway. Every Win plus digit combo belongs to the taskbar, so if one has to be taken, the shortest keys win.
- **Tried and dropped:** Alt / Alt+Shift / Ctrl+Alt (short and free in Windows, but they clash with your game key bindings); CapsLock as the key (needs a new kind of modifier and new hook code for no gain over Win).
- **Cost:** Windows' Win+N (open pinned app N), Win+Shift+N (new copy of app N) and Win+Ctrl+N (last window of app N) stop working. `""` in `settings.toml` gives a set back.
- **No new rules:** the parsing, clash and swallowing rules are unchanged; only the three defaults move.
