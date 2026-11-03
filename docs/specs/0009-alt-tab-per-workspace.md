# 0009. Alt+Tab per workspace: explorer already does it, no des-k code

**Date**: 2026-10-28
**Status**: Accepted

## Summary

Alt+Tab and Task View already leave out the windows des-k hides, because explorer skips cloaked windows when it builds those lists. So feature 7 needs no code. The one exception is admin windows (like Task Manager): a hidden admin window can still show up in Alt+Tab. Picking it is harmless, because follow activation (spec 0002) switches its screen to that window's workspace. This spec records that, and records the two "hide it ourselves" routes the spike ruled out.

## Context

Scope feature 7 asked that Alt+Tab only lists windows on the workspaces you can see, and that hidden windows come back to Alt+Tab when their workspace is shown again or des-k quits.

des-k hides windows by cloaking them (`src/cloak.rs`): the window still exists, but explorer and DWM (the part of Windows that draws windows) treat it as not shown. The `spikes/switchers` run on 2026-10-28 listed every window with its cloak state and checked Alt+Tab and Task View by hand:

- Normal windows des-k cloaked did not appear in Alt+Tab or Task View, and came back once shown again.
- Admin windows des-k cloaked (Task Manager) still appeared in Alt+Tab.

The spike also tried to hide windows from the switchers directly:

- `IApplicationView::SetShowInSwitchers` (vtable slot 28) returns `E_NOTIMPL` (0x80004001) when called from another process.
- Adding `WS_EX_TOOLWINDOW` (a window style that keeps a window out of Alt+Tab) works on normal windows but fails with Access denied (0x80070005) on admin windows, for the same UIPI reason as spec 0008.

## Requirements

**User stories**:
- As the des-k user, I want Alt+Tab to show only the windows I can see, so that switching apps doesn't pull me into a workspace I hid.

**Acceptance criteria**:
- **AC-1**: A normal window on a hidden workspace does not appear in Alt+Tab or Task View.
- **AC-2**: It appears again once its workspace is shown, or after des-k quits (every exit path shows all hidden windows).
- **AC-3**: A hidden admin window may appear in Alt+Tab. Picking it switches its screen to that window's workspace (follow activation), so you never land on an active but invisible window.

## Options considered

### Option 1: Document it, no code (chosen)

Rely on explorer skipping cloaked windows, and accept the admin window exception.

**Pros**:
- Zero code. The behavior already exists and needs nothing restored on exit.
- The exception is safe, since follow activation handles it.

**Cons**:
- Hidden admin windows still clutter Alt+Tab.

### Option 2: Run des-k as admin and toggle `WS_EX_TOOLWINDOW` on admin windows

Once spec 0008 makes des-k admin, the style change would no longer be denied.

**Pros**:
- Hidden admin windows leave Alt+Tab too.

**Cons**:
- des-k would be changing other apps' window styles, so every exit path (including a crash) must put them back, or a window stays out of Alt+Tab for good.
- It also changes the taskbar behavior of that window, which des-k already manages separately.

### Option 3: des-k draws its own Alt+Tab

Take over Alt+Tab with a des-k switcher that lists only visible windows.

**Pros**:
- Full control over what is listed.

**Cons**:
- A large feature that redoes something Windows already gets almost entirely right.
- Taking Alt+Tab from Windows is risky (games, admin windows under UIPI before spec 0008).

### Option 4: `SetShowInSwitchers`

The undocumented call meant for exactly this.

**Cons**:
- Returns `E_NOTIMPL` from another process on this build. Ruled out.

## Decision

**Chosen option**: Option 1: document it, no code.

des-k adds nothing for Alt+Tab. Explorer's own handling of cloaked windows covers feature 7, and follow activation covers the admin window exception.

## Rationale

The spike showed the feature already works for every window except admin ones, and the leftover case is safe because picking a hidden admin window switches you to its workspace. Option 2 would trade a small annoyance for a new duty to restore other apps' styles on every exit path, which is exactly the kind of state that goes wrong after a hard kill (`install.ps1` kills des-k on purpose). Option 3 is far more code than the gap is worth, and Option 4 does not work.

If hidden admin windows in Alt+Tab turn out to annoy you in daily use, Option 2 can be designed then on top of spec 0008.

## Build plan

No build tasks. AC-1 and AC-2 are explorer's behavior with des-k's existing cloaking in `src/cloak.rs` and `src/ws.rs`. AC-3 is follow activation (spec 0002) as built.

## Consequences

**Positive**:
- Feature 7 is done with no code and no new settings.

**Negative / tradeoffs**:
- Hidden admin windows still appear in Alt+Tab.

**Neutral**:
- `spikes/switchers` stays as research code (modes: list, `on|off <hex>`, `tool on|off <hex>`), in case Option 2 is ever needed.

## Follow-up

- [ ] Optionally, after spec 0008 is built, pick a hidden Task Manager from Alt+Tab once and confirm its screen switches to it (AC-3).
