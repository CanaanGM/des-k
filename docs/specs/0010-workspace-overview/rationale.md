# 0010. Workspace overview: rationale

## Context

des-k hides windows on per screen workspaces, so a window you want is often out of sight on one of N workspaces on one of three screens. The finder (spec 0007) solves "I know its name"; nothing solves "show me what is where". Windows' Task View cannot help: it shows Windows' own virtual desktops, which des-k does not use, and it ignores des-k's hidden windows.

Forces:
- One person, three screens of mixed scaling, a laptop, Prototype pace.
- You want GNOME's shape (a workspace strip plus the current workspace spread out), on every screen at once, with drag across screens, and you want to try animation, labels and live updates as settings rather than decide now.
- des-k is plain Win32 and GDI in Rust, with no UI framework. Raw COM lives only in `cloak.rs`.
- Previews of hidden windows: a probe on 2026-10-28 showed DWM thumbnails draw minimized windows (their last frame) and windows the shell cloaked (a suspended Store app). des-k's hiding is the same cloak type (2), so hidden windows very likely draw too, but no des-k hidden window existed during the probe; build step 1 confirms it.
- Win+Tab and the touchpad swipe up are where your hands already go for "show me everything".

## Options considered

### Option 1: Win32 popup per screen with DWM thumbnails (chosen)
One borderless popup per screen; `DwmRegisterThumbnail` draws each window live into it; des-k paints the rest with GDI. Animation moves thumbnail rectangles on a timer.

**Pros**: live, GPU drawn, a handful of documented calls; fits the indicator and finder code; works for minimized and cloaked windows.
**Cons**: thumbnails always draw over the host's own painting, so the hover X and the drag ghost need their own small popups; animation is hand driven, on a timer.

### Option 2: DirectComposition visual tree
Build the overview as composition visuals with real animations.

**Pros**: smooth, GPU timed animations; layering is natural.
**Cons**: a new COM heavy layer and much more code; still needs DWM thumbnails inside it for live window content.

### Option 3: Icon and title cards, no previews
Draw each window as a box with its icon and title at its place.

**Pros**: simplest; works for every window no matter what.
**Cons**: you cannot tell similar windows apart, which is the main point of an overview. Kept as the fallback for windows DWM cannot draw.

### Option 4: Snapshots taken as des-k hides a window
Capture each window's picture with `PrintWindow` on hide and show that.

**Pros**: works even if DWM refused cloaked windows.
**Cons**: stale pictures, memory per window, slow apps stall the capture, and many apps draw black through `PrintWindow`.

## Rationale

Live previews are the reason to build an overview at all, and DWM gives them for free in exactly the cases that matter here (hidden and minimized windows) with calls des-k already links (`Win32_Graphics_Dwm`). Option 2 buys smoother animation, but you chose `"off"` as the default animation, so paying for a composition layer up front is the wrong trade at Prototype pace. Option 3 survives as the per window fallback, and Option 4 is not needed while DWM draws hidden windows.

Choices you made, and the reasoning behind each:
- Every screen at once, with drag across screens: one look covers everything, and dropping uses the same cross screen move the finder's Shift+Enter already proved (`ws::move_to`).
- Switch and close on a tile click: one click does what the scope promised.
- Animation, labels and live updates are settings, with the defaults off, title on hover and live.

Calls I made (the runner up in brackets):
- The overview covers each whole screen, taskbar included, so nothing peeks through [work area only, which leaves a live taskbar showing the workspace under the overview].
- A drop keeps the overview open, so you can sort several windows in a row [close after each drop].
- Fully live mode polls `ws::windows()` every 250 ms instead of hooking window events: one code path for both `live` values, and des-k's foreground hook already shows that hooks reenter in awkward places (spec 0002) [`EVENT_OBJECT_SHOW` and `EVENT_OBJECT_DESTROY` hooks].
- The fade time is fixed at 250 ms (first 150 with a cubed ease out, raised on 2026-10-30 because you could not see it), not a setting, to keep the settings to what you asked to try [an `animation_ms` key].
- Zoom is gone (2026-10-30). Built and tried, it felt weird to you: the windows swept too far from their real places, the closing flight looked odd, and it felt choppy. Fade alone is what you kept [zoom with shorter flights; a smoother zoom on a composition timer].
- A window you close from the overview leaves the layout at once, and comes back if it is still open 2 s later (2026-10-30). Settings stretched into a blown up picture while it closed, since some Store apps shrink what they draw as they go [keep it until it goes, with its preview pinned to its old size].
- The thumbnails are not wrapped in a `cloak.rs` style layer: they are documented DWM calls, not raw vtables, so the AGENTS.md rule does not apply.

Changed on 2026-10-29, after you tried build step 4:
- Tab walks every window, then moves on to the next screen (AC-8). Tab first meant "next screen", but on your screens it only ever reached each screen's first window, so Tab felt broken; walking every window is what Tab does everywhere else (the finder, Alt+Tab). The arrows still move by place, and Tab still reaches every screen [Tab for screens only, arrows for windows].
- An empty screen keeps one Tab stop with nothing selected, so you can still Tab to it and press a digit to switch it from the keyboard [skip empty screens, which leaves the mouse as the only way to pick one].

Changed again on 2026-10-29, after you tried that Tab (replaces the two bullets above):
- Tab and Shift+Tab stay on one screen and wrap around there (AC-8). Walking on into the other screens mixed three screens' windows into one long cycle, when you only meant the screen you are looking at [Tab on into the next screen].
- The selected screen follows the mouse: moving onto another screen moves the selection there, at its first window. Since the keys no longer leave a screen, the mouse is how you pick one, with no reopening; an empty screen needs no Tab stop, since the mouse picks it for its digits [the screen stays where the overview opened].
