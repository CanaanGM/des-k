# Verify: keys work over admin windows · spec 0008 · updated 2026-10-28
_Steps derived from spec 0008 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual
- [ ] From a normal terminal run `.\install.ps1` → one UAC prompt, ends with `des-k started as admin from C:\Program Files\des-k\des-k.exe`, layout comes back → AC-4, AC-10
- [x] Tray, Open settings and Open log → each opens; Task Manager, Details, "Elevated" column says No for the editor; no `explorer did not open` line in `des-k.log` → AC-8
- [ ] Focus Task Manager, press Win+Ctrl+Right → des-k switches the screen under the mouse, Windows still has one desktop → AC-1
- [ ] Focus Task Manager, press Win+F → des-k's finder opens, not Feedback Hub → AC-1
- [ ] Unplug, hide windows on a few workspaces, sign out and back in → no UAC prompt, des-k running, layout back → AC-2, AC-3, AC-10
- [ ] Restart explorer → tray icon comes back and answers left and right clicks; its Quit shows every window → AC-7
- [x] Stop the installed copy, run `.\target\debug\des-k.exe` from a normal terminal → first line says it runs without admin and its keys stop while an admin window has focus → AC-9

## Commands
- [ ] `& 'C:\Program Files\des-k\des-k.exe' --quit` from a normal terminal → admin des-k stops, every hidden window shows → AC-6
- [ ] `Get-Content $env:LOCALAPPDATA\des-k\des-k.log -TotalCount 2` → `des-k runs as admin`, then `settings: C:\Users\<you>\AppData\Local\des-k\settings.toml: ...` → AC-2, AC-5
- [ ] `Get-ScheduledTask des-k` → RunLevel Highest, Interactive, logon trigger for your account only; settings DisallowStartIfOnBatteries False, StopIfGoingOnBatteries False, ExecutionTimeLimit PT0S, Priority 4, MultipleInstances IgnoreNew → AC-3
- [ ] `Test-Path $env:LOCALAPPDATA\des-k\des-k.exe` → False; Run key has no `des-k` value; `C:\Program Files\des-k\des-k.exe` writable only by SYSTEM and Administrators → AC-5
- [ ] `(Get-Acl target\release\des-k.exe).Owner` → your account, not Administrators → AC-4

## Value sourcing
- [ ] Task account: the task's user is the account that ran `install.ps1`, even if another admin answers the prompt → Register task row
- [ ] Settings path: change a key in `%LOCALAPPDATA%\des-k\settings.toml`, reinstall, and the log shows the new value → Read settings row
- [ ] Admin or not: the log line flips between the installed copy (admin) and the debug copy from a normal terminal (not admin) → Start log line row

## Acceptance-criteria coverage
- AC-1 Task Manager steps · AC-2 sign out, log line · AC-3 sign out on battery, task settings · AC-4 install, target owner · AC-5 log path, cleanup checks · AC-6 `--quit` · AC-7 explorer restart · AC-8 Elevated column · AC-9 debug copy · AC-10 install and sign out layout
- Checked on 2026-10-28 before step 5: AC-1, AC-6, AC-7 (admin debug copy), AC-2 log line, AC-3 task settings, AC-5 files and Run key, AC-4 target owner
