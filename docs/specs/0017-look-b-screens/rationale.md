# 0017. Look B screens: rationale

## Context

des-k has seven windows drawn by hand: the welcome screen (`src/welcome.rs`), the settings page (`src/settings_page.rs`), the indicator, the finder, the overview, and the tray menu (Windows' own `TrackPopupMenu`); a Shortcut hints window does not exist yet. They grew one feature at a time, each with its own colors: the finder and overview share a dark grey and the indicator's blue accent, the welcome and settings page draw through `ui.rs`'s Canvas (GDI+ shapes, GDI text), and the indicator is a dots popup faded with whole window alpha.

From 2026-11-04 to 2026-11-05 you approved a full redesign in look B (one column, one purple accent #8E3FD9, true greys, nine workspace colors), one window at a time, each through UI, UX and parity reviews and bound to its design file's sha256. The designs leave build questions open: what draws them, in what order, what happens to the old code, how they ship, and how the carry notes each approval listed (new settings keys, UI Automation, the tray's notification mode, shadows) become buildable criteria.

Forces: des-k is one person's tool built Tracer Bullet style and tried by hand; it idles in the tray all day and must stay light; you plan three 43 inch 4K screens and care about setups with up to twelve; you are more sensitive to motion and delay than most; the tray design makes screen reader support a requirement. Five of the seven windows already have specs whose look criteria the designs replace.

## Options considered

### Spec shape

**Option 1: one umbrella with a child per screen.** Shared layer and shared criteria once in `index.md`; each child builds one screen and names the older criteria it replaces.
- Pros: one place to build from; the shared layer is specified once; each child maps to one approved design.
- Cons: older specs now point elsewhere for their look, so a reader follows one more link.

**Option 2: edit each older spec in place, new specs only for hints and the tray popup.**
- Pros: each feature's spec stays complete on its own.
- Cons: the shared layer has no home or gets repeated in five specs; the cross screen rules (fastest, themes, Narrator) scatter.

**Option 3: seven flat specs.**
- Pros: simple numbering.
- Cons: same content as Option 1, with the shared layer buried in the first one.

### Drawing

**Option 1: keep GDI+ and GDI.**
- Pros: no new library; instant first show; least memory while idle; every look in the designs reachable; the Canvas stays the one seam.
- Cons: GDI text loses alpha on a see through window (the indicator must draw on its card color); the overview's shadows are a stretched pre blurred image; a full 4K overview paint is CPU work, estimated several to about 20 ms per screen (not measured).

**Option 2: Direct2D and DirectWrite everywhere.**
- Pros: sharpest text, true variable weights, real blur, GPU drawn big paints.
- Cons: loads Direct3D and the graphics driver's libraries into des-k (usually tens of MB more for its whole life); a device to create before the first show; device loss handling; rewrites the Canvas under every window first.

**Option 3: Direct2D for the overview only, from the start.**
- Pros: the one heavy window gets the GPU.
- Cons: the same memory cost all day for one window, and two drawing systems before there is a measured need.

### Build order

**Option 1: small to big** (indicator, finder, overview, tray + hints, settings page, first run).
- Pros: each popup reuses the last one's parts; the first run comes last and reuses the settings page's pickers and key fields.
- Cons: the hints window, the one new feature, waits until the fourth slice.

**Option 2: new features early** (tray + hints second).
- Pros: hints sooner.
- Cons: the tray popup carries the most new work (own popup, UIA, notification mode) without the finder's lessons.

**Option 3: first run right after the indicator.**
- Pros: new users meet the new look first.
- Cons: the two biggest windows come before the shared parts settle.

### Old look

**Option 1: replace each old drawing as its new one works** (keeping only the restyled dots you approved).
- Pros: one drawing path per screen.
- Cons: no way back to an old look short of reverting.

**Option 2: a classic switch.**
- Pros: a fallback for anyone who dislikes the new look.
- Cons: twice the drawing code, for every screen, forever.

### Release

**Option 1: all together as 0.4.0.**
- Pros: no user ever sees half old, half new windows.
- Cons: the first new screen waits for the last.

**Option 2: each screen as a patch release.**
- Pros: new looks arrive sooner.
- Cons: mixed looks in every release between.

**Option 3: two waves (popups, then settings and first run).**
- Pros: a middle ground.
- Cons: the settings page would show the old indicator preview while the indicator is new, and the reverse.

## Rationale

The umbrella fits because the seven designs were approved as one look: their tokens, fonts, popup shadow and Narrator rules are one thing to build once, and spreading them across five older specs would let them drift. Each child still stands alone to build from, so a slice never needs the others' details.

GDI+ and GDI stay because des-k is a tray app that idles all day: the biggest cost Direct2D brings is memory held for the process's whole life, paid for a gain that only shows on the overview across many big screens. Every look in the designs is reachable on the Canvas, and the Canvas is the one seam: if the overview misses its 100 ms limit (AC-8) after it repaints only what changed and reuses its shadow image, the overview alone moves to Direct2D behind the same calls. Speed is measured, not assumed, through a log line that keeps working after release. You asked for the limit and for control over motion because you are more sensitive than most; per animation times match the file's existing `fade_ms` style, and `fastest` is des-k's own switch (you chose not to follow Windows' Animation effects).

Small to big puts the riskiest new painting (shadows, smooth shapes, themes) into the smallest window you see most, so it is right before the finder and overview reuse it; the tray popup then gets the finder's lessons, and the first run, which reuses the settings page's pickers, comes last. Replacing old code keeps one drawing path per screen; the dots stay only because you approved them as a second style. One 0.4.0 release follows from the mixed look problem: the screens show each other (the settings page previews the indicator, the tray opens hints), so a half release would show both looks side by side.

Two answers from today were reconciled with earlier approvals: the hints key stays empty by default, as the approved hints design and your 2026-11-04 words say, and the upgrade marker you chose on 2026-11-04 becomes `first_run_seen`, written the moment the first run opens so a crash cannot make it loop.
