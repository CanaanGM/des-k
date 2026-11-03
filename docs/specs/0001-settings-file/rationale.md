# Rationale: 0001. Settings file for workspaces, shortcuts and indicator

_Decision record for [spec 0001](index.md): why this design was chosen._

## Context

Every tunable in des-k is a constant in the code: `WORKSPACES = 4` (`src/ws.rs`), the Win+Ctrl+Left/Right match (`src/keys.rs`), `HOLD_MS` and the fade step (`src/indicator.rs`), and the Copilot remap is always on. Changing any of them means a rebuild and a run of `install.ps1`.

Forces:
- The file is edited by hand by one person, so comments and readable errors matter more than machine friendliness.
- The release build has no console. The only feedback channel is `des-k.log`, so every problem has to land there with enough detail to fix it.
- The keyboard hook runs on its own thread and must stay fast (AGENTS.md threading rule), so it can only read settings, never load them.
- `ws.rs` stores hidden windows in a fixed size array `[Vec<isize>; WORKSPACES]`. `state.txt` from an earlier run can hold workspace numbers that a smaller count no longer has, and today's loader skips those lines, which would leave the windows cloaked (hidden) forever.
- The touchpad swipe is a Windows Settings gesture that sends Win+Ctrl+Left/Right. Changing the shortcut without keeping that combo breaks the swipe.

## Options considered

### Option 1: TOML, parsed into a table and read key by key
Parse the text with the `toml` crate into a generic table, then read each known key with its own type and range check.
- **Pros**: per key fallback and unknown key warnings fall out naturally; TOML allows comments and gives line numbers on syntax errors; one small dependency.
- **Cons**: each key's read and check is written by hand (seven keys, so about seven short functions).

### Option 2: TOML, deserialized straight into a typed struct with serde derive
`#[derive(Deserialize)]` with `#[serde(default)]`, `try_from` validation and `deny_unknown_fields`, as the rust-serde skill recommends for config files.
- **Pros**: idiomatic, least code, and validation happens at the parse.
- **Cons**: one bad value fails the whole read, so a typo throws away every other setting (contradicts AC-10). Unknown keys become errors, not warnings (contradicts AC-12).

### Option 3: Hand parsed `key = value` lines, like `state.txt`
- **Pros**: no new crates.
- **Cons**: shortcut lists, quoting and comments become a grammar des-k invents, documents and debugs itself; errors have no line numbers unless written by hand.

### Option 4: Keep the constants, rebuild to change
- **Pros**: zero work.
- **Cons**: fails the feature's goal.

## Rationale

The file is hand edited and its only feedback channel is a log you read after the fact. So the costly failure is a small typo silently undoing all your other changes. Option 2 does exactly that, which is why the rust-serde skill's advice (fail the whole read, deny unknown keys) is deliberately not followed here: it fits machine input, and this is a human's file (AGENTS.md: project rules win over skills). Option 3 saves one dependency but makes des-k own a config grammar, which is a worse trade than adding the most common config crate in Rust. You picked TOML and the next to exe location; for the installed copy that is `%LOCALAPPDATA%\des-k\`, the same folder as the log and layout. Update 2026-10-28: spec 0008 moved the exe to Program Files, so the file now always lives in `%LOCALAPPDATA%\des-k`, for the installed copy and `cargo run` alike.

Recommended calls made in this spec (runner up in brackets):
- Hard coded defaults in `impl Default for Settings`, with the default file text written from the same values and a unit test proving they agree [derive defaults by parsing the default file text, which makes the fallback circular].
- Exact modifier match for shortcuts [today's looser rule that ignores Shift, which would make `Win+Ctrl+Shift+Left` and `Win+Ctrl+Left` impossible to tell apart].
- Send `VK_MASK` when a matched combo contains Win or Alt, so neither the Start menu nor an app's menu bar opens on release [only for Win, as today].
- Settings are loaded on the main thread before `keys::spawn` and stored in a `OnceLock`, so the hook thread only ever reads them [pass a copy into the hook thread].
