# Verify: Check for updates from the tray · spec 0015 · updated 2026-11-03
_Steps derived from spec 0015 acceptance criteria. `/check verify` runs these; `/test` locks the durable ones._

## UI / manual
For the newer and newest steps: stop the installed des-k from the tray (Quit), then `cargo run` from a normal terminal. Put the installed one back afterwards by starting **des-k** from the Start menu.

- [ ] Set `version = "0.0.1"` in `Cargo.toml` (never commit it: the release job checks the version against the tag), `cargo run`, then tray → **Check for updates** → the menu closes at once, and reopening it while the check runs shows "Checking for updates…" grayed → AC-1, AC-6
- [ ] Same run: a balloon "des-k update" says "des-k 0.3.0 is out (you have 0.0.1). Click here to open its page." (or whatever release is newest) → AC-2
- [ ] Click that balloon → the release page `https://github.com/CanaanGM/des-k/releases/tag/v0.3.0` opens in your default browser, running as you (Task Manager, Details, the browser's Elevated column says No) → AC-2, AC-7
- [ ] Set the version back to the newest release, run again, check → balloon "des-k is up to date", "You have the newest des-k (0.3.0)."; clicking it opens nothing → AC-3
- [ ] Wi-Fi off, check → warning balloon "Could not check for updates", "No connection to GitHub. des-k carries on as before."; switch workspaces while it waits and it keeps working → AC-4, AC-1
- [ ] Start des-k and use it for a while without clicking the item → the log has no `update check` lines → AC-5
- [ ] After each check above, the log has an `update check: asking GitHub` line and one answer line (the HTTP status and Location, then the versions or the failure) → AC-8
- [ ] Nothing new appears in Downloads or `%LOCALAPPDATA%\des-k` after a check → AC-7
- [ ] Only if hiding is ever broken: with the "des-k cannot hide windows" warning showing, a check on a 0.0.1 build puts "Update to X.Y.Z (open page)" on the menu item and no balloon replaces the warning; clicking the item opens the page. Otherwise review `tray::answered` → AC-9

## Commands
- [ ] `cargo test update` → every update test passes: tag parsing, the redirect rules, version compare, pack and unpack, our own version readable → AC-2, AC-3, AC-4, AC-7
- [ ] `cargo test live -- --ignored --nocapture` → prints `HTTP 302, Location Some(".../releases/tag/vX.Y.Z")` and the newest version (one real request to github.com) → AC-4, value sourcing: URL, redirect policy, status, Location
- [ ] `cargo clippy --all-targets` → no warnings

## Value sourcing
- [ ] URL asked: the live test above reaches `github.com/CanaanGM/des-k/releases/latest`, and nothing in `settings.toml` can change it → spec value sourcing, fetch
- [ ] Redirects off: the live test prints status 302, not 200 (with redirects on, WinHTTP would follow to the tag page) → fetch, redirects
- [ ] Proxy: if you use a proxy, a check still works with it set only in Windows Settings, Network, Proxy → fetch, proxy
- [ ] Timeout: on a network that drops packets (or a firewall rule blocking des-k), the warning balloon comes within about 10 s per step, not never → fetch, timeouts
- [ ] Newest version: the balloon's version matches the newest release on the GitHub releases page → answer, newest version
- [ ] Your version: the balloon's "you have" matches `Cargo.toml`'s version for that build → answer, your version
- [ ] The URL opened is the tag page for the numbers shown, never anything else → balloon click

## Acceptance-criteria coverage
- AC-1: manual steps 1 and 5 · AC-2: steps 2 and 3, `cargo test update` · AC-3: step 4, `cargo test update` · AC-4: step 5, `cargo test update`, the live test · AC-5: step 6 · AC-6: step 1 · AC-7: steps 3 and 8, `cargo test update` · AC-8: step 7 · AC-9: step 9 (code review unless hiding breaks)
