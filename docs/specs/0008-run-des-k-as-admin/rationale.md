# 0008. Run des-k as admin: rationale

Build spec: [index.md](index.md).

## Context

des-k catches its shortcuts with a low level keyboard hook (a Windows callback that sees every key before apps do). Windows has a security rule called UIPI (User Interface Privilege Isolation): a program running without admin rights cannot receive input meant for, or send most messages to, a program running as admin. In practice, while an admin window has focus, des-k's hook never sees the keys, and they go straight to Windows.

The 2026-10-28 `/debug` run proved this. With Task Manager focused, Win+F opened Feedback Hub, and with a normal window focused it opened des-k's finder. The damage comes from Win+Ctrl+Left/Right: Windows treats it as "switch my own virtual desktop", which hides every window on every screen. Follow activation (spec 0002) and Task View both focus admin windows on their own, so you can land in this state without touching an admin window yourself. You said you have an admin window focused often, so this is a daily risk, not a rare one.

The same rule shows up elsewhere. The `spikes/switchers` spike could not change Task Manager's window style ("Access is denied"). Alt+Tab still lists hidden admin windows, while explorer already leaves out every other window des-k hid.

Constraints: one person's tool on their own laptop (it runs on battery), Rust and Win32 only, installed by `install.ps1`, Prototype workflow, and no code signing setup.

## Options considered

### Option 1: Run as admin from a scheduled task (chosen)

A logon task with "run with highest privileges" starts des-k as admin with no prompt. The exe moves to Program Files so only admins can replace it.

**Pros**:
- Fixes the cause: an admin hook sees keys whatever window has focus.
- Little code. Most of the change is in `install.ps1`, plus message filters, one path change, and a shell open helper.
- Boring and well known. Plenty of admin tools start this way.

**Cons**:
- An admin process hooks every key, so a des-k bug carries admin rights.
- Each install asks for UAC.
- Needs message filters so normal programs can still reach it, and a detour so the editor it opens doesn't run as admin.
- Without the Program Files move it would be an easy way for anything running as you to get admin.

### Option 2: Stay normal, warn when keys are paused

des-k keeps running without admin. When an admin window gets focus, the indicator says the keys are paused.

**Pros**:
- No change to how des-k runs or installs, and no admin process.
- Small code change in the foreground hook.

**Cons**:
- The keys still don't work. It only makes the failure visible.
- A switch pressed after you missed the warning still moves Windows' own desktops.

### Option 3: uiAccess manifest

Windows lets accessibility tools (screen readers, on screen keyboards) see admin windows without running as admin, through `uiAccess=true` in the manifest.

**Pros**:
- Fixes the cause without giving des-k admin rights.

**Cons**:
- The exe must be signed with a certificate the machine trusts and must sit in Program Files or System32. For a personal tool that means making a certificate, trusting it machine wide, and signing every build.
- Fails silently if any condition is missed, and is poorly documented.

### Option 4: Accept it and document

No code. Keep exactly one Windows desktop, so a missed Win+Ctrl+Left/Right does nothing, and click a normal window to get the keys back.

**Pros**:
- Free.

**Cons**:
- Keys still stop working, which happens often for you.
- One accidental Win+Ctrl+D (or a desktop added in Task View) brings back the scrambled screens.

## Rationale

You often work with an admin window focused, so the fix has to make the keys work, not just explain why they don't. That rules out Options 2 and 4 as the main answer. Options 1 and 3 both fix the cause. Option 3 avoids admin rights but needs a code signing chain that a one person Prototype project would keep breaking on every build, so the cost is in operating it, not in writing it.

Option 1 is the cheapest real fix, but running as admin has a known failure pattern. A program that starts with admin rights from a folder you can write to lets anything running as you swap the exe and get admin at the next login. That is why the exe moves to Program Files, while the files des-k only reads (settings, state, log) stay in the user folder, under the invariant that they never name code to run. The scheduled task beats an admin manifest with the Run key because it starts silently, and because Windows does not reliably start elevated Run entries. The task's defaults (stop on battery, 72 hour limit, below normal priority) are wrong for a laptop tool that must answer keys quickly, so the spec sets each one.

Keeping spec 0009 (Alt+Tab) as no code fits this choice. Once the keys keep working after follow activation focuses an admin window, the one leftover Alt+Tab entry is harmless. Changing other apps' window styles would add restore duties to every exit path.
