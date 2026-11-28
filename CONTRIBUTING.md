# Contributing

Thanks for looking. des-k is one person's tool, built and tried on their own screens, and it is
shared in case it helps you too. Bug reports are the most useful thing you can send.

## What fits

- **Yes**: bug fixes, Windows builds or screen setups des-k gets wrong, clearer docs.
- **Ask first** (open an issue): new features. des-k keeps windows floating and only hides and
  shows them; it will never tile or rearrange them.
- **No**: tiling, settings that name a program or a path to run (see [SECURITY.md](SECURITY.md)).

## Reporting a bug

Open a [bug report](https://github.com/CanaanGM/des-k/issues/new/choose). The guide says
[what to put in it](https://canaangm.github.io/des-k/#report-a-bug).

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

[Where is the code?](https://canaangm.github.io/des-k/how-it-works.html#where-is-the-code) maps each file to what it owns. Every change keeps
these rules:

- des-k only ever shows again the windows it hid itself. Every exit path (Quit, `--quit`, a
  panic, Ctrl+C) shows them all.
- The keyboard hook and touchpad threads do no slow work: they post a message, and the main
  thread changes windows.
- Raw COM vtable calls live only in `src/cloak.rs`.
- Log only with `info!`, `warn!` and `error!`; name a window by its exe, never its title.
- No network outside the update check you click in the tray, and nothing in
  `%LOCALAPPDATA%\des-k` may name a program to run.

For a bigger feature, open an issue first so we can agree on the design before any code.

## Writing the docs

The two site pages are built from `site-src/`: edit the source there, never `site/`, then run
`pwsh -File site.ps1 build`. In CI, `pwsh -File site.ps1 check` fails on a stale page, a broken
`#link`, a dash used as punctuation or a private reference, and warns on long sentences.

- One idea per paragraph, and sentences mostly under 20 words.
- Headings say a task or a question.
- Explain a word the first time it shows. Plain words come before technical ones.
- No dash, and no spaced hyphen, as punctuation: use a comma, a period or parentheses.
- Say each fact once and link to it elsewhere. User facts live in the guide, developer facts on
  the how it works page or here.
- Never name spec numbers, review ids, or notes and files that are not in this repo.
- After a visible change to the indicator, finder, overview, tray menu or hints window, run
  `cargo test render -- --ignored`, then `pwsh -File site.ps1 pictures`.

## Pull requests

Fork, branch from `main` and open the PR against `main`. Keep it to one change, with a short
description; CI checks the docs, then runs clippy and the tests. By sending a PR you agree it is licensed under the
[GPL 3.0 or later](LICENSE), like the rest of des-k.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
