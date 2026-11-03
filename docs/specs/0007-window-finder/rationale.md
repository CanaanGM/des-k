# Rationale: 0007. Window finder: a search popup that lands you on any window, on any screen and workspace

Build spec: [index.md](index.md).

## Context

With three screens and four workspaces each, des-k can hide a window in any of twelve places. Switching, follow activation, and jump and send by number all assume you remember where a window is. When you don't, the only way back is to step through workspaces on each screen until it shows up. Windows' own Alt+Tab and Task View don't help: des-k hides windows by cloaking them (a shell flag that makes a window invisible without closing or minimizing it), and both of those skip cloaked windows.

What you want is the way you find things on a Mac or in GNOME: press a key, type a few letters of the window or the app, press Enter, and you are there. "There" means des-k's own idea of place: the right screen switched to the right workspace, with the window focused. Only des-k knows which workspace holds a hidden window, so whatever finds windows has to read des-k's state, and landing on one has to go through des-k's own switch.

This is also the first des-k window that takes keyboard focus. The dots indicator never does. Windows limits which process may bring a window to the front (the foreground lock), so the popup only works if des-k is allowed to take focus right after the shortcut. des-k already counts as the last input source after a Win shortcut, because the hook injects the `VK_MASK` key, and `switch_to` already sets focus successfully today.

Two facts about Windows shape the edges. A program that is not running as admin cannot send most messages to an admin window (UIPI, User Interface Privilege Isolation), so closing or perhaps moving an admin window from des-k can fail. And a window moved between screens with different scaling may resize itself when Windows tells it the scaling changed (`WM_DPICHANGED`), which can undo a size des-k just set.

## Options considered

### Option 1: des-k's own finder popup, fed by des-k's state

A new module `src/finder.rs` owns a dark popup with a text box and a list it paints itself. It lists windows from `ws` state, ranks them with a fuzzy matcher crate, and lands on a window through new `ws` functions that reuse `switch_to`.

**Pros**:
- Sees hidden windows and knows their screen and workspace, because it reads the same state that hid them.
- Landing reuses `switch_to`, so a finder jump behaves exactly like a switch (bottom first hiding, the indicator, saving to `state.txt`).
- One process, one message loop, one look, matching the indicator.

**Cons**:
- The most new UI code des-k has had: a focusable popup, painting, keyboard routing, icons.
- The main loop has to learn `TranslateMessage` and a key filter for the popup.
- Relies on Windows letting des-k take focus.

### Option 2: An existing launcher (PowerToys Command Palette or Run with its window walker)

Use a launcher you may already have, which lists open windows and switches to the one you pick. Follow activation (spec 0002) would then switch des-k's workspace when the picked window comes to the front.

**Pros**:
- No des-k UI code at all, and a polished, maintained search box.

**Cons**:
- These walkers skip cloaked windows, which is every window des-k hides, so the windows you most need to find never appear.
- No screen or workspace label, and no bring here action, because the launcher knows nothing about des-k.
- Depends on another tool being installed and kept compatible.

### Option 3: A popup built with a GUI crate (egui or Slint)

Same behavior as Option 1, but the popup is drawn by a GUI library instead of plain Win32.

**Pros**:
- Ready made text input, lists, scrolling and styling.

**Cons**:
- A large dependency and its own window and event loop next to des-k's plain Win32 loop, against the project's minimal stack in `AGENTS.md`.
- Focus, DPI and topmost behavior go through another layer, harder to reason about when something breaks.

## Rationale

Option 1 is the only one that can see hidden windows, which is the whole point of the feature: a finder that can't list cloaked windows finds only what you can already see. It also keeps des-k's rule that all window changes happen on the main thread through `ws`, so a finder jump can't disagree with a switch about where a window lives. Option 3 would buy nicer widgets at the cost of a second UI stack in a tool that has deliberately stayed on plain Win32 with two dependencies.

Inside Option 1, three smaller calls were made with you:

- **Text box: Windows' own EDIT control, borderless and styled dark.** You first leaned toward drawing the box, then asked for a caret, Left/Right, paste, Ctrl+Backspace and IME (the input method that composes Chinese, Japanese or Korean text). The EDIT control gives all of those for free, and inside des-k's own painted frame it looks drawn. Drawing it ourselves would have been about 250 more lines, IME included, for the same result. Runner up: drawing it but dropping the caret and IME.
- **Matching: the `nucleo-matcher` crate (0.3.1).** You chose fuzzy matching (letters in order, not adjacent), and good fuzzy ranking is fiddly to tune: word starts, runs of letters, case. The crate is the matcher from the Helix editor, pure Rust, small. Its last release is from February 2024, which is acceptable for a finished algorithm with no outside dependencies. Runner up: our own scorer of about 60 lines, if the crate ever becomes a problem.
- **Order: recently used first.** A fuzzy query like "chrome" often matches several windows equally; the one you used last is almost always the one you want, and starting on the second row makes Win+F then Enter flip between your last two windows from anywhere, like Alt+Tab. This needs the foreground hook installed always, not only when `follow_activation` is on, which costs nothing measurable. Recency stays in memory: losing it on restart is fine, saving it would tie it into restore for little gain.

Decisions made at write time (your call to override):

- **Haystack is "program title" in one string.** Then a query like "code des-k" matches across both, since every word must match somewhere. Runner up: score program and title separately and take the best, which can't match one word in each.
- **Pattern without fzf syntax.** The crate's `Pattern::parse` treats `^`, `$`, `!` and `'` as operators; titles contain those characters, so the finder uses plain fuzzy atoms. Runner up: the full syntax, which would surprise more than it helps here.
- **Screen numbers left to right by each screen's left edge, then top edge.** Matches how you see your desk. Runner up: Windows' `DISPLAYn` numbers, which you rejected.
- **Upper third placement: the top of the popup at 20% of the work area height.** The list grows downward and the text box never moves.
- **At most 9 rows visible, then scrolling with the selection.** Fits a 150% laptop screen comfortably.
- **No wrap at the ends of the list.** Holding Down stops on the last row instead of jumping to the top unexpectedly.
- **Focus moves to the target before the popup hides.** If the popup hid first, Windows would pick some window to activate, possibly a hidden one, and follow activation would then switch to it.
- **Any des-k switch, jump, send or screen change closes the finder first.** The list is a snapshot and would be stale after a switch.
- **Act on current state, not the snapshot.** Restore (spec 0003) may hide windows while the finder is open, so every action looks up where the window is now.
- **Icons: `WM_GETICON` with a 50 ms timeout that gives up at once on a frozen app, then the class icons, then the generic app icon.** A frozen app can never hang the popup.
- **Store apps: the program name of the hosted app.** Their frame window belongs to `ApplicationFrameHost.exe`; the real app owns a child window of class `Windows.UI.Core.CoreWindow`. When that child is missing (a suspended app), the name stays `ApplicationFrameHost`.
- **Moving across screens with different scaling: set the placement, then check the result and set the size once more.** This undoes an app's own resize on `WM_DPICHANGED`. Windows' own Win+Shift+Arrow, which you confirmed works well in spec 0006, is the bar to compare with.
- **A row disappears after Ctrl+W when the window is gone or no longer visible.** Apps that close to the tray hide their window instead of destroying it.
- **`finder = []` falls back to the default**, like every other shortcut list (spec 0001). There is no off switch; rebinding it to another combo gives Win+F back to Windows.
- **The Agent Skill found for the crate (`smithery.ai/nucleo-matcher`) was skipped.** It was unverified and almost unused; the spec names the few calls needed.
