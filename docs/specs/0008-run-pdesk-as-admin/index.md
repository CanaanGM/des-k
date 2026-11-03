# 0008. Run pdesk as admin so admin windows cannot deafen its keys

**Date**: 2026-10-28
**Status**: In Progress

## Summary

While an admin window (Task Manager, an admin terminal) has focus, Windows stops delivering keys to programs that run without admin rights. pdesk's keyboard hook goes deaf, so Win+Ctrl+Right falls through to Windows and switches Windows' own virtual desktops instead. From now on pdesk starts as admin at login through a scheduled task (a Windows job that can start a program with admin rights and no prompt). The exe moves to `C:\Program Files\pdesk`, where only admins can write, so nothing running as you can swap it for something else and get admin. Your settings, log and layout stay in `%LOCALAPPDATA%\pdesk`.

## Requirements

**User stories**:
- As the pdesk user, I want my shortcuts to keep working while an admin window has focus, so that a switch never lands on Windows' own desktops and scrambles my screens.
- As the pdesk user, I want pdesk to start as admin at login without a UAC prompt (the "allow this app to make changes" dialog), so that the fix costs me nothing day to day.
- As the pdesk user, I want an admin pdesk not to become a free path to admin for other programs, so that running it as admin stays safe on my laptop.

**Acceptance criteria**:
- **AC-1**: With the installed pdesk running and Task Manager focused, Win+Ctrl+Right switches pdesk's workspace on the screen under the mouse, and Windows' own virtual desktop does not change. Win+F opens pdesk's finder, not Feedback Hub.
- **AC-2**: After you sign out and back in (or reboot), pdesk is running as admin with no UAC prompt, and the first lines of `pdesk.log` say it runs as admin.
- **AC-3**: The scheduled task starts pdesk on battery, does not stop it when you unplug, has no time limit, and runs it at normal priority (Task Scheduler's own default is below normal, and it ends tasks after 72 hours).
- **AC-4**: Running `install.ps1` from a normal terminal builds as you (nothing in `target\` is owned by Administrators), shows exactly one UAC prompt, replaces a running admin pdesk while keeping the layout, and ends with pdesk running as admin. If pdesk is not running at the end, the script says so and exits with an error.
- **AC-5**: After install, `pdesk.exe` lives in `C:\Program Files\pdesk`. The old `%LOCALAPPDATA%\pdesk\pdesk.exe` and the `pdesk` value under `HKCU\...\CurrentVersion\Run` are gone. `settings.toml`, `pdesk.log` and `state.txt` stay in `%LOCALAPPDATA%\pdesk`, and your current settings still apply.
- **AC-6**: `& "C:\Program Files\pdesk\pdesk.exe" --quit` from a normal terminal stops the admin pdesk and shows every hidden window.
- **AC-7**: The tray icon still answers left and right clicks, its Quit still works, and after explorer restarts the icon comes back.
- **AC-8**: The tray's "open settings" and "open log" open the file in an editor that does not run as admin (Task Manager, Details tab, "Elevated" column says No). If that route fails, pdesk logs a line and opens the file the old way.
- **AC-9**: Started without admin (for example `cargo run` in a normal terminal), pdesk works as it does today and logs one line saying its keys stop while an admin window has focus.
- **AC-10**: The first admin start after the switch keeps the layout: every hidden window is either back on its workspace or shown. None is left hidden where you cannot reach it.

## Decision

**Chosen option**: Option 1: Run pdesk as admin from a scheduled task, with the exe in Program Files.

pdesk runs with admin rights from a logon task, which lets its keyboard hook see keys whatever window has focus. The exe moves to a folder only admins can write to.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Pieces that change**:

| Piece | Today | After |
|---|---|---|
| Exe location | `%LOCALAPPDATA%\pdesk\pdesk.exe` | `C:\Program Files\pdesk\pdesk.exe` (`$env:ProgramFiles\pdesk`) |
| Start at login | HKCU Run key, no admin | Scheduled task `pdesk`, logon trigger for you, "run with highest privileges" |
| `settings.toml` | next to the exe (`src/settings.rs:118`) | `ws::data_dir()`, which is `%LOCALAPPDATA%\pdesk` |
| `pdesk.log`, `state.txt` | `ws::data_dir()` | unchanged |
| `--quit`, tray clicks | plain messages | the receiving windows let these messages in from normal programs |
| Tray "open settings" / "open log" | `ShellExecuteW` from pdesk | opened by explorer, so the editor runs as you |

**Scheduled task definition** (built by `install.ps1`):
- Name `pdesk`, at the Task Scheduler root.
- Trigger: at logon of the installing user only.
- Principal: that user, interactive logon, run level Highest.
- Settings: allow start on battery, don't stop on battery, no execution time limit (`PT0S`), priority 4 (normal), if already running do not start a new copy.
- Action: `C:\Program Files\pdesk\pdesk.exe`, no arguments.

**install.ps1 flow**:
1. Build with `cargo build --release` as you, in the normal terminal.
2. If the script is not running as admin, it captures your account name, then starts itself again elevated (`Start-Process -Verb RunAs -Wait`) with a switch that skips the build and an argument carrying the account name. It passes on the elevated copy's exit code.
3. Elevated part: stop pdesk (end the task, then force stop any `pdesk` process, the hard kill that keeps `state.txt` on purpose), copy the exe into `C:\Program Files\pdesk`, register or update the task, remove the old Run value and the old `%LOCALAPPDATA%\pdesk\pdesk.exe`, then start the task.
4. Check that a `pdesk` process is running. If not, print it plainly and exit with an error (Rule 12, and the install race of scope feature F).

**Message filters** (so normal programs can still talk to an admin pdesk; Windows blocks most messages from a lower rights program to a higher one):
- The indicator window allows `WM_CLOSE` (used by `--quit`).
- The tray window allows `WM_TRAY` and the registered `TaskbarCreated` message.
- Use `ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, None)` right after each window is created. A failure is logged and skipped.

**Opening files as you**: ask the desktop's shell view for `IShellDispatch2` and call its `ShellExecute` with the file path. These are documented COM calls, so they belong in `src/tray.rs`, not `src/cloak.rs`. The documented path: `IShellWindows::FindWindowSW(SWC_DESKTOP)`, then `IServiceProvider::QueryService(SID_STopLevelBrowser)` to get `IShellBrowser`, `QueryActiveShellView`, `GetItemObject(SVGIO_BACKGROUND)` as `IShellFolderViewDual`, and its `Application` as `IShellDispatch2`. Both menu items share one helper. On any failure, log it and fall back to today's `ShellExecuteW`.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| Register task | Account the task runs as | Captured by `install.ps1` before it elevates (`[Security.Principal.WindowsIdentity]::GetCurrent().Name`) and passed to the elevated copy as an argument. Never read after elevating, because an over the shoulder admin prompt would give a different account. |
| Copy exe | Target folder | `$env:ProgramFiles\pdesk` |
| Clean up | Old exe and Run value | `$env:LOCALAPPDATA\pdesk\pdesk.exe`, `HKCU:\Software\Microsoft\Windows\CurrentVersion\Run` value `pdesk` |
| Read settings | `settings.toml` path | `ws::data_dir().join("settings.toml")`. The elevated token uses the same user profile, so `%LOCALAPPDATA%` is the same folder. |
| Start log line | Running as admin or not | `GetTokenInformation(TokenElevation)` on pdesk's own process token |
| Open as you | File to open | The same paths the tray uses today (`settings::path()`, `data_dir()\pdesk.log`) |

**Key invariants**:
- Exactly one way starts pdesk at login: the task. No Run value may remain, or a normal copy could start first and win the single instance check.
- Nothing pdesk reads from `%LOCALAPPDATA%\pdesk` (`settings.toml`, `state.txt`) may ever name a program to run or a file to load as code. Those files can be written by anything running as you, and an admin pdesk acting on them would bring back the hole that Program Files closes.
- Only `WM_CLOSE`, `WM_TRAY` and `TaskbarCreated` are let through the message filters. Nothing else.
- Every exit path still shows all hidden windows (AGENTS.md rule), whether pdesk runs as admin or not.

**Security model**: pdesk runs with your admin rights and hooks every key, as it already did for normal windows. Only admins can replace the exe. Any program running as you can close pdesk (`WM_CLOSE`), which is the same as today and shows every window. It can also edit `settings.toml`, which can only change key bindings and switches. No compliance scope.

**Critical test scenarios**:
- Happy path: focus Task Manager, press Win+Ctrl+Right, and pdesk switches. Windows' desktop count stays 1 and Win+F opens the finder. Verifies **AC-1**.
- Login: sign out and in on battery, then check `pdesk.log` and the task's settings. Verifies **AC-2**, **AC-3**.
- Install: run `install.ps1` from a normal terminal with pdesk running and windows hidden on several workspaces. Expect one prompt, the same layout afterwards, and the files where AC-5 says. Verifies **AC-4**, **AC-5**, **AC-10**.
- Talk from normal rights: `--quit` from a normal terminal, tray clicks, and restarting explorer. Verifies **AC-6**, **AC-7**.
- Editor rights: open settings from the tray and check the "Elevated" column. Verifies **AC-8**.
- Dev copy: `cargo run` in a normal terminal, then check the log line. Verifies **AC-9**.

## Build plan

Tracer Bullet: get one admin pdesk working end to end by hand first, then make install produce it, then smooth the edges.

1. Add the start log line (admin or not) and the three message filters (indicator `WM_CLOSE`, tray `WM_TRAY` and `TaskbarCreated`). Satisfies **AC-2**, **AC-6**, **AC-7**, **AC-9**.
2. Move `settings.toml` to `ws::data_dir()` in `src/settings.rs`, and update the settings path test if one covers it. Satisfies **AC-5**.
3. Thin thread, by hand: stop the installed pdesk, run `cargo run` from an admin terminal, and check AC-1, AC-6 and AC-7 plus the basics (switch, cloak, taskbar buttons, finder, follow). **Gate**: if cloaking, the taskbar or the tray fails from an admin process, stop here and come back to `/architect`, with the fallback being "stay normal, warn me" (rationale Option 2). Satisfies **AC-1**, **AC-6**, **AC-7**.
4. Rewrite `install.ps1` per the flow above (build as you, elevate once, Program Files, task, cleanup, start, running check). Satisfies **AC-3**, **AC-4**, **AC-5**.
5. Tray "open settings" and "open log" through explorer (`IShellDispatch2::ShellExecute`), falling back to `ShellExecuteW` with a log line. Satisfies **AC-8**.
6. Try it on your screens: install from a normal terminal, sign out and in on battery, then run every critical test scenario. Satisfies **AC-1** to **AC-10**.

## Migration plan

**Strategy**: big bang, one time. The next `install.ps1` run switches from the Run key and `%LOCALAPPDATA%` exe to the task and the Program Files exe. Your `settings.toml` already sits in `%LOCALAPPDATA%\pdesk`, because that is where the installed exe lives today, so no file moves.
**Rollback**: in an admin terminal run `Unregister-ScheduledTask pdesk -Confirm:$false`, stop pdesk, and delete `C:\Program Files\pdesk`. Then build the old `install.ps1` and `src/settings.rs` and run them. The repo is not under git, so keep a copy of both before step 2 and step 4.
**Risks**: the restore code ties `state.txt` to a logon session, and an admin process has a different logon session id from a normal one. The first admin start may treat the saved layout as coming from a new sign in and match windows by fingerprint (spec 0003), which shows a window whenever in doubt (AC-10). Later admin starts share one session id, so this happens once.

## Consequences

**Positive**:
- Admin windows no longer turn pdesk's keys off, which removes the "pdesk crashed" scenario from the 2026-10-28 debug run.
- The exe can only be replaced by an admin, which closes the hole that running as admin would otherwise open.
- `settings.toml`, `pdesk.log` and `state.txt` now live in one folder for both the installed and the dev copy.

**Negative / tradeoffs**:
- Every install asks for UAC once.
- An admin process hooks every key you type. A bug in pdesk now runs with admin rights.
- `cargo run` now reads the installed `settings.toml`. To try a setting in the dev copy, you change your real file.
- A dev copy run from a normal terminal still has the deaf hook problem. To test admin window behavior, run it from an admin terminal.
- Any program running as you can still quit pdesk through `WM_CLOSE`, as it can today.

**Neutral**:
- Alt+Tab keeps listing hidden admin windows such as Task Manager. Picking one jumps to its workspace through follow activation, and now your keys keep working afterwards. Spec 0009 (Alt+Tab per workspace) records this as accepted, with no code.
- `AGENTS.md` (Commands, settings location, the install line) and spec 0001 (settings live next to the exe) become stale once this ships. `/sync` owns those edits.

## Follow-up

- [ ] Write spec 0009, Alt+Tab per workspace: no code, admin windows are the known exception (decided earlier in this session, now numbered 0009).
- [x] After build step 2, update spec 0001's settings location line (an `/architect` update) and let `/sync` fix `AGENTS.md`.
- [x] A setup exe installer (`pdesk.iss`) now does the same install as `install.ps1`, plus an uninstaller: spec [0013](../0013-setup-exe-installer.md). The two task definitions must stay identical.
- [ ] Scope feature F (install race): the new running check in `install.ps1` makes the race loud. Whether to also retry the copy stays with feature F.
