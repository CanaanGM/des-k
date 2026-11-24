# Releasing des-k

How a des-k release is made, signed and checked. Everything in this file is public; no secret is in it.

## Why releases are signed

The installed des-k runs as admin and can install an update from the tray with one click. It runs that update only when the release's `SHA256SUMS.txt` carries a minisign signature from one of the two keys below, made for exactly that version, and the setup matches its line in that file. CI builds each release but cannot sign it. Signing happens on the publisher's own PC, so someone who takes over the GitHub account still cannot make your des-k run their program.

## What you need to publish

- **minisign 0.12** by jedisct1: `winget install jedisct1.minisign`. Version 0.10 or newer works; older ones make legacy signatures, which des-k refuses.
- **GitHub CLI** (`gh`), logged in with push access to the repo.
- **PowerShell 7.4** or newer.
- **Bitwarden CLI** (`bw`), optional: without it, `sign-release.ps1` asks you to paste the key.

## Release steps

1. Set the new version in `Cargo.toml` and run `cargo build`, so `Cargo.lock` follows. Add the release to `CHANGELOG.md`, and add its cards to `NEWS` in `src/whats_new.rs` if it has news.
2. Commit, push `main`, and wait for CI to pass.
3. Tag that commit and push the tag: `git tag vX.Y.Z`, then `git push origin vX.Y.Z`. CI builds the setup and the zip and creates a **draft** release with them and `SHA256SUMS.txt`. Nobody else, and no copy of des-k, can download a draft's files.
4. With your checkout on that commit and up to date, unlock Bitwarden if you use it (`$env:BW_SESSION = bw unlock --raw`), then run:

   ```powershell
   pwsh -File sign-release.ps1 X.Y.Z
   ```

   It stops unless `Cargo.toml`'s version, the tag `vX.Y.Z`, your `HEAD`, `origin/main` and the CI run that made the draft all name the same commit, and unless every file in the draft matches its line in `SHA256SUMS.txt`. It shows the commit, the CI run and each file's size, and waits for you to type the version. minisign then asks for the key's password. The script checks the new signature with the key in `src/install.rs`, uploads `SHA256SUMS.txt.minisig`, and publishes the release.
5. Recommended: check the published release by hand (below), and install it from the tray on a copy that has the previous release.

If the script stops, the release stays a draft. Fix what it names and run it again. If a draft's files are wrong, delete the draft and the tag on GitHub, then tag and push again.

## The public keys

des-k carries both keys (`KEYS` in `src/install.rs`) and accepts a release signed by either one.

| Key | Key id | Public key |
|---|---|---|
| main | `F1C51A322A7CBAE1` | `RWThunwqMhrF8Ty7LmKeLKoczPJyRjIQoPDPAYyEyLpGhgPnHDr8nBkJ` |
| backup | `383EB8633678DAD9` | `RWTZ2ng2Y7g+OCJHvqDtAUchdqToYuz2sd4zArQdh/UCryBUNv+USVkz` |

## Where the private keys live

- **Main**: in your Bitwarden vault, as the note `des-k minisign main`. The key is encrypted with its own password, which lives only in your head. It signs every normal release.
- **Backup**: offline and outside Bitwarden, with its password kept offline too. It signs only when the main key is lost or leaks.
- Neither key ever goes to GitHub, CI, a server or a cloud sync folder. `sign-release.ps1` writes the main key to a temp file on your PC only while it signs, and deletes it after.

## Check a download by hand

Releases after 0.9.0 carry `SHA256SUMS.txt.minisig`; 0.9.0 and older have no signature. To check one yourself:

1. Download the setup (or the zip), `SHA256SUMS.txt` and `SHA256SUMS.txt.minisig` from the same release into one folder.
2. Check the signature with the main key:

   ```powershell
   minisign -Vm SHA256SUMS.txt -P RWThunwqMhrF8Ty7LmKeLKoczPJyRjIQoPDPAYyEyLpGhgPnHDr8nBkJ
   ```

   If it says the key ids differ, run it again with the backup key. It must print `Signature and comment signature verified` and `Trusted comment: des-k X.Y.Z`, with the version of the release you downloaded.
3. Hash the file and compare it with its line in `SHA256SUMS.txt`:

   ```powershell
   Get-FileHash des-k-vX.Y.Z-setup.exe
   ```

If any step fails, do not run the file, and please open an issue.

## When a key is lost or leaks

Every installed copy keeps trusting both of its keys until it installs a release that carries new ones. So you replace one key with a release signed by the other.

After you put a new key in `KEYS`, make its test fixture too, or `cargo test` fails: sign the text `des-k test` plus one newline with the trusted comment `des-k 0.0.1`, and put the whole signature file in `MAIN_SIG` or `BACKUP_SIG` in the tests of `src/install.rs`.

```powershell
[IO.File]::WriteAllText("$PWD\msg.txt", "des-k test`n")
minisign -S -s <new key file> -m msg.txt -t "des-k 0.0.1" -x msg.minisig
```

- **Main key lost** (gone, but nobody else has it): make a new main key (`minisign -G`) and store it like the old one. Put its public key in `KEYS` and in the table above, then sign that release with the backup: `sign-release.ps1 X.Y.Z -Key backup`. Sign with the new main key from then on.
- **Main key leaks** (someone else may have it): do the same, as soon as you can, and say in the release notes why people should update. A copy keeps trusting the leaked key until it installs that release. The key alone cannot publish a release, which also needs your GitHub account, so if you think both are exposed, secure the account first.
- **Backup key lost or leaks**: make a new backup key, store it offline, put it in `KEYS` and the table, and sign that release with the main key.
- **Both keys lost or leaked**: no installed copy can be told to trust a new key. Make two new keys, put them in `KEYS` and the table, and sign the release with the new main key. Every installed copy refuses it with "des-k refused X.Y.Z", so each person installs that one release by hand, after checking it with the new keys as above. Announce the new keys somewhere besides this repo too, so people can tell your change from an attacker's. After that, updates from the tray work again.
