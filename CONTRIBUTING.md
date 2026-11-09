# Contributing

Thanks for looking. des-k is one person's tool, built and tried on their own screens, and it is
shared in case it helps you too. Bug reports are the most useful thing you can send.

## What fits

- **Yes**: bug fixes, Windows builds or screen setups des-k gets wrong, clearer docs.
- **Ask first** (open an issue): new features. des-k keeps windows floating and only hides and
  shows them; it will never tile or rearrange them.
- **No**: tiling, settings that name a program or a path to run (see [SECURITY.md](SECURITY.md)).

## Reporting a bug

Open a [bug report](https://github.com/CanaanGM/des-k/issues/new/choose) and attach the log
(tray → **Open log**, or `%LOCALAPPDATA%\des-k\des-k.log`, plus `des-k.old.log` after a crash).
The log names windows by their app, never their title, so it is safe to share.

## Building and testing

You need Windows 11, [Rust](https://rustup.rs) and Git.

```powershell
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
cargo test render -- --ignored   # draws each window's states to PNGs under target/
```

- Only one des-k runs at a time, so quit the installed copy first. Ctrl+C in `cargo run` shows
  every hidden window.
- A copy started from a normal terminal can't hear keys while an admin window has focus. To test
  that, start `target\debug\des-k.exe` from an admin terminal (never `cargo run` there).
- Most of des-k can only be checked by hand on real screens. Say in your PR what you tried and
  on which setup.

## How the code is built

[AGENTS.md](AGENTS.md) holds the rules every change follows (threads, which windows des-k may
show, logging, what may touch the network). Bigger features start as a spec in `docs/specs/`
before any code; open an issue and we'll write it together.

## Pull requests

Fork, branch from `main` and open the PR against `main`. Keep it to one change, with a short
description; CI runs clippy and the tests. By sending a PR you agree it is licensed under the
[GPL 3.0 or later](LICENSE), like the rest of des-k.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
