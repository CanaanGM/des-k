# Verify: Restore layout after reboot · spec 0003 · updated 2026-10-26
_Steps derived from spec 0003 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

First, you may want to run `install.ps1` and confirm `des-k.exe` is running (`Get-Process des-k`). Then switch once so `state.txt` gets the new format (an `L` line, tabs on `W` lines, `V` lines). Every result shows up in `%LOCALAPPDATA%\des-k\des-k.log`, in lines that start with `restore:`.

## UI / manual
- [ ] Hide Notepad on workspace 3 of the laptop screen, then reboot with "restart apps" on → Notepad reopens and vanishes within a second, and switching to workspace 3 shows it → AC-1
- [ ] Before the reboot, leave screens on different workspaces → each present screen comes back on the same one → AC-2
- [ ] Hide an app that starts at login (it opens before des-k does) → it still goes back to its workspace → AC-3
- [ ] Use three Edge windows with different titles: two hidden on workspaces 2 and 4, one visible. Reboot → each one goes back where it was → AC-4, AC-5
- [ ] Use two saved Edge windows whose titles changed completely → both stay visible, and the log shows `unclear match` once for each → AC-4, AC-11
- [ ] Open Notepad by hand 5 minutes after you sign in → it stays visible, and the log shows `restore: done` before that → AC-6
- [ ] Run `install.ps1` while windows are hidden → the log shows `loaded saved layout` and no `restore:` line → AC-7
- [ ] Hide windows on an external screen, then boot without it → they stay visible, and the log says `is not connected` → AC-8
- [ ] Lower `workspaces` below a saved workspace, then reboot → that window stays visible, and the log says `no longer exists` → AC-8
- [ ] When a reopened window takes focus and restore hides it → focus lands on a visible window, the screen stays put, and the log may show `follow: restore just hid` → AC-9
- [ ] Set `[restore] enabled = false`, then reboot → no `restore:` lines. Set `minutes = 0` → the log shows the settings warning and `restore.minutes=3` → AC-10
- [ ] Check the log after any reboot → one line per hidden window, the unclear ones, and a `restore: done, N hidden, N kept visible, N ... not matched` summary. Nothing pops up → AC-11
- [ ] Reboot again within the restore period → the unmatched entries are still in `state.txt` (`W ... 0 0` lines) and get matched after the second boot → AC-12
- [ ] Shut down with Fast Startup on, and separately sign out and back in → matching runs both times → AC-13
- [ ] The first start of the new build on an old `state.txt` → loads with no errors → AC-14
- [ ] Hard kill des-k (`Stop-Process -Name des-k -Force`), change the id in the `L` line, then start des-k more than 5 minutes after you signed in → the log shows `restore: signed in N min ago, skipped` → AC-15
- [ ] Run `install.ps1` one minute after you sign in → the log shows `restore: resuming,` and the remaining entries get matched → AC-16
- [ ] Sign out with windows hidden. Before des-k starts again (or from the log right after), `state.txt` still has its `W` lines → AC-17

## Value sourcing
- [ ] Logon session id: after a sign out and back in, the hex id in the `L` line changes → new sign in detected
- [ ] Logon time: the second number in the `L` line converts to your real sign in time (`[DateTimeOffset]::FromUnixTimeSeconds(<n>).LocalDateTime`) → the late start check uses the right clock
- [ ] `app` for a normal program: `W`/`V` lines show the lowercased full exe path
- [ ] `app` for a Store app (Calculator, Settings): check whether the line shows an app id such as `microsoft.windowscalculator_...!app` rather than `applicationframehost.exe`. This path has not been seen working yet
- [ ] Screen now: open a saved app on the wrong screen during the period, then drag it to the saved screen → it gets hidden after you move it, not before
- [ ] Period length: set `minutes = 1` → `restore: done` is logged about a minute after start

## Commands
- [ ] `cargo test` → 16 pass, including the 10 `restore::tests` for the matching rule → AC-4, AC-5, AC-10

## Acceptance-criteria coverage
- AC-1 to AC-17: one UI / manual step each, as tagged above. AC-4, AC-5 and AC-10 are also covered by `cargo test`.
