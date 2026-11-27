// Test only: the hits and UI Automation entries that a layout pass of the settings page, the
// first run and What's new produces, held against the hashes kept in snapshot/, so merging their
// widgets into one module can't move a click area or change what Narrator reads (spec 0035
// AC-7). The boxes come from this PC's fonts, as the render PNGs do, so the tests run by hand:
// `cargo test snapshot -- --ignored`.
use crate::uia::Node;
use std::fmt::Write;
use windows::Win32::Foundation::RECT;

/// A box in dip, to a tenth.
pub(crate) fn bx((x, y, w, h): crate::ui::canvas::Bx) -> String {
    format!("{x:.1} {y:.1} {w:.1} {h:.1}")
}

/// Everything a UI Automation entry tells Narrator, on one line.
pub(crate) fn node(n: &Node) -> String {
    let mut s = format!("{} {:?}", n.role.0, n.name);
    if !n.help.is_empty() {
        let _ = write!(s, " help {:?}", n.help);
    }
    for (on, word) in [(n.greyed, " greyed"), (n.focusable, " focusable"), (n.focused, " focused")] {
        if on {
            s.push_str(word);
        }
    }
    if let Some(on) = n.toggled {
        let _ = write!(s, " toggled {on}");
    }
    if let Some(v) = &n.value {
        let _ = write!(s, " value {v:?}");
    }
    if let Some(on) = n.selected {
        let _ = write!(s, " selected {on}");
    }
    if n.rect != RECT::default() {
        let r = n.rect;
        let _ = write!(s, " rect {} {} {} {}", r.left, r.top, r.right, r.bottom);
    }
    if !n.kids.is_empty() {
        let _ = write!(s, " [{}]", n.kids.iter().map(node).collect::<Vec<_>>().join("; "));
    }
    s
}

/// FNV-1a: a stable hash, the same on every build, unlike std's.
fn hash(s: &str) -> u64 {
    s.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
}

/// Compares each state of a screen's snapshot (a block that opens with `== state theme`) with
/// the hash kept for it in snapshot/<name>.txt, and names every state that differs. The whole
/// text is too big to keep, so this run's goes to target/snapshot/<name>.txt to diff against a
/// run from before, and its hashes to target/snapshot/<name>.hashes.txt, to copy over the kept
/// ones after a change you meant.
pub(crate) fn check(name: &str, got: &str) {
    let kept = match name {
        "settings_page" => include_str!("snapshot/settings_page.txt"),
        "welcome" => include_str!("snapshot/welcome.txt"),
        "whats_new" => include_str!("snapshot/whats_new.txt"),
        _ => panic!("no kept snapshot named {name}"),
    };
    let mut blocks: Vec<(&str, String)> = vec![];
    for line in got.lines() {
        match (line.strip_prefix("== "), blocks.last_mut()) {
            (Some(title), _) => blocks.push((title, String::new())),
            (None, Some((_, body))) => {
                body.push_str(line);
                body.push('\n');
            }
            (None, None) => panic!("{name} snapshot starts without a state: {line}"),
        }
    }
    let hashes: String = blocks.iter().map(|(title, body)| format!("{:016x} {title}\n", hash(body))).collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("snapshot");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.txt")), got).unwrap();
    std::fs::write(dir.join(format!("{name}.hashes.txt")), &hashes).unwrap();
    // Git may check the kept copy out with CRLF line ends; lines() drops the CR.
    let (was, now): (Vec<&str>, Vec<&str>) = (kept.lines().collect(), hashes.lines().collect());
    let differ: Vec<&str> = now.iter().filter(|l| !was.contains(l)).chain(was.iter().filter(|l| !now.contains(l))).map(|l| l.get(17..).unwrap_or(l)).collect();
    assert!(
        differ.is_empty() && was.len() == now.len(),
        "{name} snapshot differs in these states (this run is in target/snapshot/):\n{}",
        differ.join("\n")
    );
}
