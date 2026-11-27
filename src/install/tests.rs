// Tests for the tray install (spec 0018): the checksum line, signatures, the task XML, outcomes,
// and the live release checks that only run when asked.
use super::*;

const NAME: &str = "des-k-v0.4.2-setup.exe";
const H: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn the_checksum_comes_from_the_one_exact_line() {
    // Why: this hash decides whether an admin des-k runs a downloaded program; any line
    // that is not exactly the setup's own must never be taken for it.
    let sums = format!("\u{FEFF}{}  des-k-v0.4.2.zip\r\n{H}  {NAME}\r\n{}  des-k-v0.4.2-setup.exe.bak\r\n", "0".repeat(64), "1".repeat(64));
    assert_eq!(hex(&sums_line(&sums, NAME).unwrap()), H);
    let upper = format!("{}  {NAME}\n", H.to_uppercase());
    assert_eq!(hex(&sums_line(&upper, NAME).unwrap()), H);
    for bad in [
        format!("{H}  des-k-v0.4.1-setup.exe\n"),
        format!("{H}  {NAME}.bak\n"),
        format!("{}  {NAME}\n", &H[1..]),
        format!("{H}0  {NAME}\n"),
        format!("{H} {NAME}\n"),
        format!("{}g  {NAME}\n", &H[1..]),
        format!("{H}  {NAME}\n{H}  {NAME}\n"),
        format!("{H}\n"),
        String::new(),
    ] {
        assert_eq!(sums_line(&bad, NAME), Err(InstallFail::NoLine), "{bad:?}");
    }
}

// Fixtures (spec 0031 build step 1): minisign 0.12 signatures of MSG. TEST is a throwaway
// key made for these tests only; MAIN and BACKUP are signed by the two keys in KEYS.
const MSG: &[u8] = b"des-k test\n";
const TEST_KEY: &str = "RWST8dK4CSKTJN0pzkI+3nVrnE7QHr63VxH25F0Lh6bxWPPPmetC2qdX";
const TEST_GOOD: &str = "untrusted comment: signature from minisign secret key
RUST8dK4CSKTJDOxTW8COwx2LhH9vcvlFTyxDwBYlta0JvOUUFWj8MVsXae4/aP3zU71rjCwGqxQXUOcNuU5nrZte2pLtqmjqAA=
trusted comment: des-k 0.0.1
+PieHYA2bE3g4JMnuu5X3KabBTRJ4AF1zk2C247JuGP8I3QycgQjj+AYQR+mvbLZ1t/VrfrSRjiH5B9wIYOxAw==
";
const TEST_V2: &str = "untrusted comment: signature from minisign secret key
RUST8dK4CSKTJDOxTW8COwx2LhH9vcvlFTyxDwBYlta0JvOUUFWj8MVsXae4/aP3zU71rjCwGqxQXUOcNuU5nrZte2pLtqmjqAA=
trusted comment: des-k 0.0.2
R8YxXXl9FkRVTJ76XS6lwu/nPH2S8Tvb4lZvGCpE+5fgxOf+X6UFRuvsQpUZPn5LfzTS4Y0DmGBf+ZM7037gAw==
";
const TEST_LEGACY: &str = "untrusted comment: signature from minisign secret key
RWST8dK4CSKTJP9JjghzkXiJP1X4bmGjpF+odPTdwaDyI92UjDcDzHrjDs1PcvnHLhvcPoAm4YrMxwVGTyYM2XfsOawiCMPWDAk=
trusted comment: des-k 0.0.1
oTNTondAyVNsh81X+uiYeWX2uRioH7albLoQk9RgAtfErELON3q3epizUw/tl5UX18omQOlmwOHT0pqkxGW2Dw==
";
const MAIN_SIG: &str = "untrusted comment: signature from minisign secret key
RUThunwqMhrF8RGuxI+LesW2Zr0AiwFNX+N/T7yLzoNLZERxgREWVOmh53E4qNC+obFXyfDVJQLsyfg2GCszGWjP8lZzjM3+2AI=
trusted comment: des-k 0.0.1
GlTWa97e6z0yTvm49ZmdLhGEX49K+twx5MiGlOmkAHeDNNtdUUQlnh6bgQwQAz+3VXLQ4nxM4ADba7huy9niCg==
";
const BACKUP_SIG: &str = "untrusted comment: signature from minisign secret key
RUTZ2ng2Y7g+ON7YgtzOeFozn6liy8Z5DFENdyscSpejzNDzqtycPD9bL2A9PDGhm7VRtxJUUxYr96VaaS1UJnmDNq9fsE+tvAo=
trusted comment: des-k 0.0.1
efDS0QSvmV8vGxpsgsWjyF+sMwzXr/UHEDfqaSzq7ApsyYS+TwJ2rmYl8IbuWjcTKHGDCoWk/l3o8VKJUJeYBg==
";
const V001: Version = Version(0, 0, 1);

#[test]
fn only_a_signature_by_our_key_for_this_version_passes() {
    // Why: this check is the only thing between whoever controls the GitHub release and an
    // admin des-k running their program; anything but our exact proof must refuse (AC-1, AC-3).
    let test = [("test", TEST_KEY)];
    assert_eq!(check_with(&test, MSG, TEST_GOOD.as_bytes(), V001), Ok("test"));
    // CRLF, as a Windows editor or download might leave it.
    assert_eq!(check_with(&test, MSG, TEST_GOOD.replace('\n', "\r\n").as_bytes(), V001), Ok("test"));
    let bad = |keys: &[(&'static str, &str)], sums: &[u8], sig: &[u8], v| assert_eq!(check_with(keys, sums, sig, v), Err(InstallFail::BadSignature));
    bad(&test, b"des-k tesT\n", TEST_GOOD.as_bytes(), V001); // one changed checksum byte
    bad(&KEYS, MSG, TEST_GOOD.as_bytes(), V001); // a key des-k does not have
    bad(&test, MSG, TEST_V2.as_bytes(), V001); // signed for another version
    bad(&test, MSG, TEST_GOOD.as_bytes(), Version(0, 0, 2)); // an old release passed off as new
    bad(&test, MSG, TEST_LEGACY.as_bytes(), V001); // the old non prehashed algorithm
    bad(&test, MSG, TEST_GOOD.replace("des-k 0.0.1", "des-k 0.0.1 ").as_bytes(), V001); // comment changed after signing
    bad(&test, MSG, b"not a signature", V001); // garbled
    bad(&test, MSG, &[0xFF, 0xFE, 0x00], V001); // not UTF-8
    bad(&test, MSG, b"", V001); // empty
}

#[test]
fn both_built_in_keys_work() {
    // Why: a typo in KEYS would refuse every release, and only a reinstall by hand would fix
    // it; the backup must work before the day it is needed (AC-1, AC-8, AC-11).
    assert_eq!(check_sums(MSG, MAIN_SIG.as_bytes(), V001), Ok("main"));
    assert_eq!(check_sums(MSG, BACKUP_SIG.as_bytes(), V001), Ok("backup"));
}

#[test]
fn a_missing_or_huge_signature_is_refused_but_the_network_is_not() {
    // Why: a release without our proof is refused, while a dropped connection must stay
    // an ordinary failure, never shown as an attack (AC-2, AC-3, AC-7).
    assert_eq!(sig_fail(InstallFail::Missing), InstallFail::Unsigned);
    assert_eq!(sig_fail(InstallFail::TooBig), InstallFail::BadSignature);
    assert_eq!(sig_fail(InstallFail::Net(Fail::Timeout)), InstallFail::Net(Fail::Timeout));
    for f in [InstallFail::Unsigned, InstallFail::BadSignature, InstallFail::Mismatch] {
        assert!(f.refused(), "{f:?}");
    }
    for f in [InstallFail::Net(Fail::Timeout), InstallFail::Missing, InstallFail::NoLine, InstallFail::TooBig, InstallFail::Folder, InstallFail::Task, InstallFail::SetupStopped, InstallFail::Slow, InstallFail::Crashed] {
        assert!(!f.refused(), "{f:?}");
    }
}

#[test]
fn the_refused_release_is_remembered_with_its_kind() {
    // Why: the menu and every install start read it; a lossy pack would let the refused
    // version be installed, or show the wrong line (AC-5).
    assert_eq!(unpack_refused(0), None);
    for f in [InstallFail::Unsigned, InstallFail::BadSignature, InstallFail::Mismatch] {
        for v in [Version(65535, 65535, 65535), V001, Version(0, 0, 0)] {
            assert_eq!(unpack_refused(pack_refused(v, f)), Some((v, f)));
        }
    }
}

#[test]
fn sha256_is_the_standard_one() {
    // Why: a wrong hash would refuse every good setup, or worse, agree with a bad one.
    assert_eq!(hex(&sha256(b"abc").unwrap()), H);
}

#[test]
fn downloads_are_built_from_the_numbers() {
    // Why: the response never names what des-k fetches or runs (AC-4).
    let v = Version(0, 4, 2);
    assert_eq!(release_path(v, &setup_name(v)), "/CanaanGM/des-k/releases/download/v0.4.2/des-k-v0.4.2-setup.exe");
    assert_eq!(release_path(v, "SHA256SUMS.txt"), "/CanaanGM/des-k/releases/download/v0.4.2/SHA256SUMS.txt");
}

#[test]
fn the_task_runs_the_setup_then_des_k() {
    // Why: schtasks rejects UTF-8 and an <?xml?> line; the second action must follow the
    // setup on every path, which a time limit would cut (AC-6).
    let xml = task_xml("S-1-5-21-1-2-3-1001", Path::new(r"C:\Program Files\des-k\update\des-k-v0.4.2-setup.exe"), Path::new(r"C:\Program Files\des-k\update\setup.log"), Path::new(r"C:\WINDOWS\system32\schtasks.exe"));
    assert!(xml.is_ascii() && !xml.contains("<?xml"));
    assert!(xml.contains(r#"/LOG="C:\Program Files\des-k\update\setup.log""#));
    assert!(xml.contains(r"<UserId>S-1-5-21-1-2-3-1001</UserId>") && xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>") && !xml.contains("<Triggers>"));
    let setup = xml.find(r"<Command>C:\Program Files\des-k\update\des-k-v0.4.2-setup.exe</Command>").expect("setup action");
    let again = xml.find(r"<Command>C:\WINDOWS\system32\schtasks.exe</Command><Arguments>/run /tn des-k</Arguments>").expect("restart action");
    assert!(setup < again && xml.matches("<Exec>").count() == 2);
    assert!(task_xml("é&", Path::new("a"), Path::new("b"), Path::new("c")).contains("<UserId>&#233;&amp;</UserId>"));
}

#[test]
fn the_outcome_compares_versions_only() {
    // Why: whether des-k was updated is the version now running, never the setup's exit code (AC-8).
    let ours = Version(0, 4, 2);
    assert_eq!(rule(Some(Version(0, 4, 2)), ours), Outcome::Updated(ours));
    assert_eq!(rule(Some(Version(0, 5, 0)), ours), Outcome::NotUpdated(Version(0, 5, 0)));
    assert_eq!(rule(Some(Version(0, 4, 1)), ours), Outcome::Nothing);
    assert_eq!(rule(None, ours), Outcome::Nothing);
    assert_eq!(setup_version(NAME), Some(ours));
    // Why: a shown outcome is renamed so it shows once; a download in flight is no outcome.
    assert_eq!(setup_version("des-k-v0.4.2-setup.exe.done"), None);
    assert_eq!(setup_version("des-k-v0.4.2-setup.exe.part"), None);
}

/// Downloads a real release's checksum file and setup into memory, runs nothing:
/// `cargo test live_release -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_release_matches_its_checksum() {
    let v = Version(0, 4, 1);
    let mut sums = Vec::new();
    download(&release_path(v, "SHA256SUMS.txt"), SUMS_CAP, |b| {
        sums.extend_from_slice(b);
        Ok(())
    })
    .expect("SHA256SUMS.txt");
    let want = sums_line(std::str::from_utf8(&sums).unwrap(), &setup_name(v)).expect("its line");
    let mut sha = Sha::new().unwrap();
    let n = download(&release_path(v, &setup_name(v)), SETUP_CAP, |b| sha.add(b)).expect("the setup");
    let got = sha.finish().unwrap();
    println!("{n} bytes, {} vs {}", hex(&got), hex(&want));
    assert_eq!(got, want);
    assert_eq!(download(&release_path(v, "nope.exe"), SUMS_CAP, |_| Ok(())), Err(InstallFail::Missing));
}

/// Checks the newest real release's signature the way the tray install does, downloading
/// only its checksum file and signature into memory: `cargo test live_release -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_release_is_signed() {
    // Why: a release published without sign-release.ps1, or a check that disagrees with
    // minisign, would refuse every update with no page link (AC-1, AC-8).
    let v = update::newest().expect("the newest release");
    let get = |file: &str, cap: u64| {
        let mut bytes = Vec::new();
        download(&release_path(v, file), cap, |b| {
            bytes.extend_from_slice(b);
            Ok(())
        })
        .expect(file);
        bytes
    };
    let sums = get("SHA256SUMS.txt", SUMS_CAP);
    let sig = get("SHA256SUMS.txt.minisig", SIG_CAP);
    let key = check_sums(&sums, &sig, v);
    println!("des-k {v}: {key:?}");
    assert!(key.is_ok());
    assert!(sums_line(std::str::from_utf8(&sums).unwrap(), &setup_name(v)).is_ok());
}

#[test]
fn every_install_message_survives() {
    // Why: the outcome crosses threads only in wParam and lParam; a lossy pack shows the
    // wrong version or the wrong reason (AC-3, AC-5, AC-6).
    let big = Version(65535, 65535, 65535);
    let mut all = vec![Msg::Started];
    for f in [
        InstallFail::Net(Fail::NoConnection),
        InstallFail::Net(Fail::Timeout),
        InstallFail::Net(Fail::Certificate),
        InstallFail::Net(Fail::ProxyLogin),
        InstallFail::Net(Fail::Status(65535)),
        InstallFail::Missing,
        InstallFail::NoLine,
        InstallFail::TooBig,
        InstallFail::Mismatch,
        InstallFail::Folder,
        InstallFail::Task,
        InstallFail::SetupStopped,
        InstallFail::Slow,
        InstallFail::Crashed,
        InstallFail::Unsigned,
        InstallFail::BadSignature,
    ] {
        all.push(Msg::Failed(f));
    }
    for m in all {
        for v in [big, Version(0, 4, 2), Version(0, 0, 0)] {
            let (wp, lp) = pack(m, v);
            assert_eq!(unpack(wp, lp), Some((m, v)), "{m:?} {v}");
        }
    }
    for p in [Phase::Idle, Phase::Downloading(big), Phase::Installing(big), Phase::Downloading(Version(0, 0, 1))] {
        assert_eq!(unpack_phase(pack_phase(p)), p);
    }
}
