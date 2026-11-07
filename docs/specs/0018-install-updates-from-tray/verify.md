# Verify: Install updates from the tray · spec 0018 · updated 2026-11-07
_Steps derived from spec 0018 acceptance criteria and its Value sourcing table. `/check verify` runs these; `/test` locks the durable ones._

Setup for most steps (recommended): set `version = "0.0.1"` in `Cargo.toml` (never commit it), run `cargo build --release`, then `iscc /DAppVersion=0.0.1 des-k.iss`, and install `target\setup\des-k-v0.0.1-setup.exe`. Put the version back afterwards.

## UI / manual
- [ ] Installed 0.0.1, tray → **Check for updates** → balloon "des-k update" says "des-k 0.4.1 is out (you have 0.0.1). Click here to install it." → AC-1
- [ ] Let that balloon go, open the menu → the item reads **Install 0.4.1**, line "Downloads and installs it" → AC-1
- [ ] Click the balloon or the item, open the menu at once → "Downloading des-k 0.4.1…", grayed; switching workspaces still works meanwhile → AC-3
- [ ] Moments later → "Installing des-k 0.4.1…", then des-k goes and comes back as 0.4.1 with your windows where they were, and no admin prompt or setup window shows → AC-6
- [ ] Afterwards, `<Program Files>\des-k\update\` holds only `setup.log`, and `schtasks /query /tn "des-k update"` finds nothing → AC-10
- [ ] The log has: install started, both downloads with HTTP status and size, the checksum match, task registered and started → AC-11
- [ ] `cargo run` with version 0.0.1 (not the installed copy), check → the balloon says "Click here to open its page." and opens the release page; the item reads "Update to 0.4.1" → AC-2
- [ ] An admin `target\release\des-k.exe` run (admin, but not in Program Files) → the page too → AC-2
- [ ] Installed 0.0.1, check, Wi-Fi off, click the balloon → "Could not install des-k 0.4.1" / "No connection to GitHub. Click here to open its page."; clicking opens the page; `update\` holds no `.part`; the item offers **Install 0.4.1** again → AC-5
- [ ] Failed setup: from an admin terminal, hold `<Program Files>\des-k\des-k.exe` open during the install (for example a PowerShell `[IO.File]::Open(path,'Open','Read','None')` started right after des-k stops) → the old des-k comes back with "des-k was not updated"; clicking opens `setup.log` → AC-8
- [ ] Killed setup: end `des-k-v0.4.1-setup.exe` from an admin Task Manager after des-k went away → des-k starts again (the task's second action) and says "des-k was not updated" → AC-6, AC-8
- [ ] Restart des-k after either of those → the balloon does not show a second time → AC-7, AC-8
- [ ] Once a release with this feature is out, update to it from the one before → "des-k updated" / "You now have des-k X.Y.Z.", once → AC-7
- [ ] Uninstall → `<Program Files>\des-k\update\` and the `des-k update` task are gone → AC-10

## Value sourcing
- [ ] Program Files and System32 come from Windows APIs: `grep -n "env::var" src/install.rs` finds nothing → can_install, task
- [ ] URLs come from the numbers: the log's download lines read `/CanaanGM/des-k/releases/download/v0.4.1/...` → download
- [ ] The expected hash is the exact setup line: covered by `cargo test install` (`the_checksum_comes_from_the_one_exact_line`) → verify
- [ ] The actual hash is BCrypt over the download: `cargo test live_release -- --ignored --nocapture` prints two equal hashes → verify
- [ ] The task's account is your SID: while installing, `schtasks /query /tn "des-k update" /xml` shows `<UserId>S-1-5-21-…</UserId>`, `HighestAvailable`, `PT0S`, two `Exec` actions → task
- [ ] The outcome is the version comparison: a failed setup (above) shows "not updated" even though the setup exited → outcome
- [ ] While "des-k cannot hide windows" shows, no install balloon replaces it → AC-9

## Commands
- [ ] `cargo test` → all pass, the `install::tests` ones included → AC-3 to AC-8
- [ ] `cargo test live_release -- --ignored --nocapture` → the setup's hash equals its `SHA256SUMS.txt` line, and a missing file reads as `Missing` → AC-4, AC-5
- [ ] `iscc /DAppVersion=0.0.1 des-k.iss` → compiles → AC-8, AC-10

## Acceptance-criteria coverage
- AC-1 steps 1, 2 · AC-2 steps 7, 8 · AC-3 step 3 · AC-4 the live test and URL check · AC-5 step 9 · AC-6 steps 4, 11 · AC-7 steps 12, 13 · AC-8 steps 10, 11, 12 · AC-9 value sourcing · AC-10 steps 5, 14 · AC-11 step 6
