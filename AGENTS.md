# des-k

Per monitor virtual desktops for Windows 11: each screen switches its own workspaces, the others stay put. Floating windows only, never tiling. One person's tool, built and tried on their own screens.

## Stack

- **Language / Runtime**: Rust, edition 2024, Windows 11 only (verified on 25H2, build 26200)
- **Framework**: none, plain Win32 message loop
- **Key dependencies**: `windows` 0.62 (Win32 features listed in `Cargo.toml`); the undocumented shell `IApplicationView` called by raw vtable slot; `toml` reads `settings.toml`; `toml_edit` writes one value back in place, keeping comments (`settings::write`, spec 0012)
- **Package manager**: cargo

## Build approach

Tracer Bullet: each feature is built completely, from shortcut to window behavior to saved layout, and works before the next one starts.

## Commands

```bash
cargo build                    # debug build, keeps a console
cargo run                      # dev run; Ctrl+C shows every hidden window and exits
cargo test                     # unit tests (settings parsing, touchpad swipes, settings page steps)
cargo test render -- --ignored # draws each window's states to PNGs: target/indicator/, finder/, overview/, tray/, hints/, settings/, welcome/ (the first run)
powershell -File install.ps1   # from a NORMAL terminal: release build as you, one UAC prompt, install to Program Files, logon task starts it as admin, restart it
& "$env:ProgramFiles\des-k\des-k.exe" --quit   # clean stop from any terminal: shows every hidden window
iscc /DAppVersion=x.y.z des-k.iss  # after cargo build --release: the setup exe and its uninstaller in target\setup\ (Inno Setup 6, spec 0013)
```

`cargo test` covers settings parsing, the touchpad swipe recognizer, the settings page's number steps, and screen ids, `state.txt` lines and the screen check rules (spec 0016); everything else you verify by hand on real screens (Prototype workflow). Only one copy runs at a time: a second start prints `des-k is already running` and exits, so stop the installed copy before `cargo run`. Release log and layout live in `%LOCALAPPDATA%\des-k\` (`des-k.log`, the run before's `des-k.old.log`, `state.txt`). `settings.toml` lives there too, one file shared by the installed copy and `cargo run` (spec 0008). des-k checks it once a second and applies changes live, except Restore, which waits for the next start; a new `workspaces` count resizes every screen at once (`ws::resize`, spec 0012).

## Specs

Stored in `docs/specs/`. Format: `docs/specs/NNNN-title.md`. Feature plan: `docs/scope/scope.md`.

## Rules

- Raw COM vtable calls live only in `src/cloak.rs`. Before calling an undocumented slot, check the layout (`GetThumbnailWindow` must return the same hwnd) and refuse on mismatch. Create a fresh `Shell` per operation, since explorer can restart.
- Only ever uncloak windows des-k hid itself, tracked in `ws.rs` state. `DWMWA_CLOAKED == 2` looks the same for Windows' own desktops and suspended UWP windows.
- Threads: the keyboard hook thread (`src/keys.rs`) does no slow work; it posts `WM_APP` messages to the main thread. All window changes, the indicator, and saving happen on the main thread. The touchpad thread (`src/touchpad.rs`, spec 0011) works the same way: it only posts `WM_GESTURE` carrying the gesture (`gestures::pack`, never a rule index, which a reload can shift), and the main thread looks up its rule and runs it (`src/gestures.rs`). The keyboard thread also watches its own hook: raw input counts keys, and keys the hook never heard mean Windows removed it, so it is installed again.
- The foreground win event hook (`on_foreground` in `src/main.rs`) runs on the main thread inside `GetMessageW` and only posts `WM_FOLLOW`; never touch `ws` state from a callback, its `Mutex` is not reentrant (spec 0002). Thread message ids: `WM_APP` switch, `WM_APP + 1` display change, `WM_APP + 2` follow, `WM_APP + 3` send (spec 0004), `WM_APP + 4` jump, `WM_APP + 5` send to (spec 0005), `WM_APP + 6` finder, `WM_APP + 7` overview, `WM_APP + 8` gesture and `WM_APP + 9` touchpads changed (spec 0011), `WM_APP + 10` shortcut recorded (`WM_RECORDED`, spec 0012), `WM_APP + 12` pin (spec 0014). The tray's icon callback is a window message, `WM_APP + 11`, and so is the update check's answer to the tray window, `WM_APP + 13` (spec 0015), but the loop matches ids alone, so neither may share one; the loop ends only on a `WM_CLOSE` for the indicator or the tray, since a `WM_CLOSE` for des-k's own settings page or welcome screen comes through it too. `ws::app_windows` leaves des-k's own windows out, so no switch hides them and the finder and overview never list them.
- Every exit path shows all hidden windows: `--quit`, the panic hook, the console Ctrl+C handler. `--quit` with no des-k running shows what `state.txt` says it hid (`ws::rescue`); the uninstaller relies on it. The panic hook can run while its own thread holds the `ws` lock, so it uses `try_lock` and falls back to `cloak::ours()`, then ends the process. A window that may still be cloaked is never dropped from the lists: a failed uncloak keeps it there to try again, and a screen counts as gone only after `GONE_CHECKS` 1 s checks in a row. A hard kill keeps `state.txt` so the next start adopts the layout (`install.ps1` relies on this on purpose). Those same exit paths also write Windows' saved touchpad gestures back (`setup::write_back`, spec 0011); sign out, shutdown and a hard kill do not.
- A pinned window (`ws::State.pinned`, spec 0014) is never hidden by a switch; any send unpins it. Explorer restarting only sets a flag (`ws::resync_soon`, from the tray's `TaskbarCreated`); main's 1 s timer then hides every tracked window again (`ws::resync`), never inside the tray's message.
- Screens are keyed by the monitor's own id (spec 0016), never by Windows' `DISPLAYn`: `ws::State.monitors` holds present and remembered screens by id (EDID maker, product and serial read through SetupAPI in `restore::edid_id`, `internal` for a laptop panel, `#1`/`#2` left to right for twins), and `Monitor.device` is the `DISPLAYn` name only until the screen folds. The finder, the overview and the settings page still pass device names; `ws` converts with `id_of` and `mon_of`. A screen gone `GONE_CHECKS` checks (30 for 30 s after a wake: the indicator's `WM_POWERBROADCAST` only calls `ws::woke`, which has its own lock) folds: its hidden windows go hidden into the same workspace of the main display, and each window it had gets an away record (`A` line) that Quit and `--quit` keep. A window's spot is frozen while its screen is missing. Each `check_screens` runs claims, closed windows, fold and return, then spots, and shows its indicator note and balloon only after the `ws` lock is released. Any send, bring, overview drop, pin, or a move between two present screens drops a window's away record (`ws::claim`).
- Hide windows bottom of z order first, show them top first. The reverse makes windows behind flash.
- The main thread's COM is MTA on purpose: STA would pump messages inside outgoing calls made under the `ws` lock and re-enter des-k's own windows.
- Per window failures are logged with `eprintln!` and skipped, never fatal. Ignored results use `let _ =`.
- Input des-k injects carries `MAGIC` in `dwExtraInfo` so the hook ignores it. Swallowed combos with Win or Alt send `VK_MASK` so neither the Start menu nor an app's menu bar opens.
- Settings (`src/settings.rs`) load once in `main` before `ws::load` and `keys::spawn`; everything else only reads `settings::get()`. After that only the main thread changes them: its 1 s watch timer calls `settings::reload`, and the settings page and the first run save through `settings::write`, which reloads (spec 0012). The first run opens at start unless `first_run_seen` is true, and `main` writes true as it opens; a missing key means an upgrade from before 0.4.0, so keep it `Option<bool>` (spec 0017). A bad value resets just that key with a log line, never the whole file (spec 0001).
- Each file opens with a short comment saying what it owns; inline comments say why, not what.
- The installed des-k runs as admin (spec 0008). Message filters let in only `WM_CLOSE` (indicator) and `WM_TRAY` plus `TaskbarCreated` (tray) from normal programs, nothing else. Nothing in `%LOCALAPPDATA%\des-k` may ever name a program to run or code to load, since anything running as you can write there. For the same reason des-k never writes through a link there: `ws::data_dir` replaces a linked folder, and every file it writes goes through `ws::create_fresh`, `write_new` or `append_to`. Files the tray opens go through explorer (`IShellDispatch2::ShellExecute` in `src/tray.rs`) so the editor runs as you.
- The Start menu entry runs `des-k --start`, which runs the logon task (`schtasks /run`, allowed without admin), so des-k starts as admin; a plain shortcut to the exe would start a normal copy. Task Scheduler's restart on failure does not restart a task whose program exits with an error (tried 2026-11-03), so the task has none.
- The setup exe (`des-k.iss`, spec 0013) repeats `install.ps1`'s admin half: change their task definitions and Start menu entry together, keep `CloseApplications=no` (Restart Manager would quit des-k gently and show every window), and never change its `AppId` (a new one makes a second Apps entry instead of an update).
- Network only inside an update check you clicked in the tray (`src/update.rs`, spec 0015): no timer, no start up check, no setting turns one on. It runs on its own thread, touches no `ws` state, and posts one `WM_UPDATE`. Only three version numbers or a failure code leave it; the page it opens is a github.com URL des-k builds from them, never text from the answer, through explorer. Nothing is downloaded or run.
- Accepted risk (spec 0011): a gesture's key combo in `settings.toml` is typed by the admin des-k, so it reaches admin windows, and anything running as you can rewrite that file. Keep combos few and prefer action names; never let a setting carry a program or a path.

## Gotchas

- `windows` 0.62: `BOOL` lives in `windows::core`, and many hwnd parameters are `Option<HWND>`.
- A brand new window fails `GetViewForHwnd` (`0x8002802B`) for a moment; retry instead of giving up.
- The indicator window (class `des-k-indicator`) doubles as the single instance marker and the `--quit` target, so it is created before anything else.
- With Num Lock on and Shift held, Windows turns numpad digits into navigation keys (End, Down, and so on) before the low level hook sees them, so a Shift combo never matches a numpad digit (spec 0005).
- A dev copy started from a normal terminal is not admin, so its hook hears nothing while an admin window has focus (UIPI); start `target\debug\des-k.exe` from an admin terminal to test that, never `cargo run` there (the build would be owned by Administrators). Hard killing the installed des-k also needs an admin terminal.
- `schtasks /Create /XML` rejects UTF-8 task files: write plain ASCII with no `<?xml?>` line, or UTF-16.
- des-k was named pdesk until 2026-11-03. `ws::data_dir` moves `%LOCALAPPDATA%\pdesk` to `des-k` once, and both installers stop `pdesk.exe` and remove its task and Program Files folder; keep those `pdesk` names, they are cleanup on purpose. `pdesk-design` (beside the repo) and the finder's test titles keep the old name too.
- There is no maximize without activation: `ws::put` gives focus back after `SW_SHOWMAXIMIZED`, so a window going home never takes focus or triggers a follow (spec 0016).
- Right after a screen leaves, Windows may not have moved its windows yet, and `MonitorFromWindow` with `MONITOR_DEFAULTTONEAREST` then names a present screen they are not on. The screen check uses `MONITOR_DEFAULTTONULL` and skips a window that is on no screen, or Windows' own move would look like yours.
- PowerToys Keyboard Manager can remap F23 too; des-k's Copilot key handling only wins if its hook is installed after PowerToys.

## Agent skills

Guidance only; where one disagrees with `## Rules` above, the rules win.
- [ferris-windows](.claude/skills/ferris-windows/): `mcapricorns/ferris-skills`, Win32, COM, handles, DPI and PowerShell contracts
- [memory-safety-patterns](.claude/skills/memory-safety-patterns/): `wshobson/agents`, ownership and RAII around `unsafe` and FFI
- [rust-best-practices](.claude/skills/rust-best-practices/): `apollographql/skills`, idiomatic Rust, error handling, clippy
- [rust-serde](.claude/skills/rust-serde/): `rewrite-rs/skills`, serde and parsing at the input boundary; `settings.rs` deliberately skips its fail the whole read advice (spec 0001)

## Context files

<!-- Nested AGENTS.md files are listed here as they are created -->

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
