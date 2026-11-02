# pdesk

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
powershell -File install.ps1   # from a NORMAL terminal: release build as you, one UAC prompt, install to Program Files, logon task starts it as admin, restart it
& "$env:ProgramFiles\pdesk\pdesk.exe" --quit   # clean stop from any terminal: shows every hidden window
```

`cargo test` covers settings parsing, the touchpad swipe recognizer and the settings page's number steps; everything else you verify by hand on real screens (Prototype workflow). Only one copy runs at a time: a second start prints `pdesk is already running` and exits, so stop the installed copy before `cargo run`. Release log and layout live in `%LOCALAPPDATA%\pdesk\` (`pdesk.log`, `state.txt`). `settings.toml` lives there too, one file shared by the installed copy and `cargo run` (spec 0008). pdesk checks it once a second and applies changes live, except `workspaces` and Restore, which wait for the next start (spec 0012).

## Specs

Stored in `docs/specs/`. Format: `docs/specs/NNNN-title.md`. Feature plan: `docs/scope/scope.md`.

## Rules

- Raw COM vtable calls live only in `src/cloak.rs`. Before calling an undocumented slot, check the layout (`GetThumbnailWindow` must return the same hwnd) and refuse on mismatch. Create a fresh `Shell` per operation, since explorer can restart.
- Only ever uncloak windows pdesk hid itself, tracked in `ws.rs` state. `DWMWA_CLOAKED == 2` looks the same for Windows' own desktops and suspended UWP windows.
- Threads: the keyboard hook thread (`src/keys.rs`) does no slow work; it posts `WM_APP` messages to the main thread. All window changes, the indicator, and saving happen on the main thread.
- The foreground win event hook (`on_foreground` in `src/main.rs`) runs on the main thread inside `GetMessageW` and only posts `WM_FOLLOW`; never touch `ws` state from a callback, its `Mutex` is not reentrant (spec 0002). Thread message ids: `WM_APP` switch, `WM_APP + 1` display change, `WM_APP + 2` follow, `WM_APP + 3` send (spec 0004), `WM_APP + 4` jump, `WM_APP + 5` send to (spec 0005), `WM_APP + 6` finder, `WM_APP + 7` overview, `WM_APP + 8` gesture and `WM_APP + 9` touchpads changed (spec 0011).
- Every exit path shows all hidden windows: `--quit`, the panic hook, the console Ctrl+C handler. A hard kill keeps `state.txt` so the next start adopts the layout (`install.ps1` relies on this on purpose).
- Hide windows bottom of z order first, show them top first. The reverse makes windows behind flash.
- Per window failures are logged with `eprintln!` and skipped, never fatal. Ignored results use `let _ =`.
- Input pdesk injects carries `MAGIC` in `dwExtraInfo` so the hook ignores it. Swallowed combos with Win or Alt send `VK_MASK` so neither the Start menu nor an app's menu bar opens.
- Settings (`src/settings.rs`) load once in `main` before `ws::load` and `keys::spawn`; everything else only reads `settings::get()`. After that only the main thread changes them: its 1 s watch timer calls `settings::reload`, and the settings page saves through `settings::write`, which reloads (spec 0012). A bad value resets just that key with a log line, never the whole file (spec 0001).
- Each file opens with a short comment saying what it owns; inline comments say why, not what.
- The installed pdesk runs as admin (spec 0008). Message filters let in only `WM_CLOSE` (indicator) and `WM_TRAY` plus `TaskbarCreated` (tray) from normal programs, nothing else. Nothing in `%LOCALAPPDATA%\pdesk` may ever name a program to run or code to load, since anything running as you can write there. Files the tray opens go through explorer (`IShellDispatch2::ShellExecute` in `src/tray.rs`) so the editor runs as you.

## Gotchas

- `windows` 0.62: `BOOL` lives in `windows::core`, and many hwnd parameters are `Option<HWND>`.
- A brand new window fails `GetViewForHwnd` (`0x8002802B`) for a moment; retry instead of giving up.
- The indicator window (class `pdesk-indicator`) doubles as the single instance marker and the `--quit` target, so it is created before anything else.
- With Num Lock on and Shift held, Windows turns numpad digits into navigation keys (End, Down, and so on) before the low level hook sees them, so a Shift combo never matches a numpad digit (spec 0005).
- A dev copy started from a normal terminal is not admin, so its hook hears nothing while an admin window has focus (UIPI); start `target\debug\pdesk.exe` from an admin terminal to test that, never `cargo run` there (the build would be owned by Administrators). Hard killing the installed pdesk also needs an admin terminal.
- PowerToys Keyboard Manager can remap F23 too; pdesk's Copilot key handling only wins if its hook is installed after PowerToys.

## Agent skills

Guidance only; where one disagrees with `## Rules` above, the rules win.
- [ferris-windows](.claude/skills/ferris-windows/): `mcapricorns/ferris-skills`, Win32, COM, handles, DPI and PowerShell contracts
- [memory-safety-patterns](.claude/skills/memory-safety-patterns/): `wshobson/agents`, ownership and RAII around `unsafe` and FFI
- [rust-best-practices](.claude/skills/rust-best-practices/): `apollographql/skills`, idiomatic Rust, error handling, clippy
- [rust-serde](.claude/skills/rust-serde/): `rewrite-rs/skills`, serde and parsing at the input boundary; `settings.rs` deliberately skips its fail the whole read advice (spec 0001)

## Context files

<!-- Nested AGENTS.md files are listed here as they are created -->

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
