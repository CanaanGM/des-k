// Keeps every source file, tests included, at 1,000 lines or fewer (spec 0035), so each file
// does one job and fits in a sitting. Lines are counted with str::lines(), blank and comment
// lines too. It also keeps ws's Windows calls inside ws/win.rs.
use std::path::{Path, PathBuf};

const LIMIT: usize = 1000;

/// Files not split yet, each capped at its size when this test landed, so none can grow while it
/// waits. Take a file's line out once it is split: the list ends empty.
const NOT_SPLIT_YET: &[(&str, usize)] = &[];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Each source file under src/ with its line count, named by its path inside src/ with `/`.
fn sizes() -> Vec<(String, usize)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    files
        .iter()
        .map(|f| {
            let name = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            (name, std::fs::read_to_string(f).unwrap().lines().count())
        })
        .collect()
}

#[test]
fn no_source_file_passes_a_thousand_lines() {
    let over: Vec<String> = sizes()
        .into_iter()
        .filter_map(|(name, n)| {
            let cap = NOT_SPLIT_YET.iter().find(|(f, _)| *f == name).map_or(LIMIT, |&(_, cap)| cap);
            (n > cap).then(|| format!("{name}: {n} lines, more than {cap}"))
        })
        .collect();
    assert!(over.is_empty(), "split these files (spec 0035):\n{}", over.join("\n"));
}

/// ws decides and ws/win.rs calls Windows (spec 0035 AC-10): an `unsafe` anywhere else in ws
/// is a Windows call that skipped the adapter, or new unsafe code that belongs there.
#[test]
fn only_ws_win_holds_unsafe() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("ws.rs")];
    rs_files(&src.join("ws"), &mut files);
    let found: Vec<String> = files
        .iter()
        .filter(|f| **f != src.join("ws").join("win.rs"))
        .filter(|f| std::fs::read_to_string(f).unwrap().contains("unsafe"))
        .map(|f| f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(found.is_empty(), "move these Windows calls into ws/win.rs (spec 0035):\n{}", found.join("\n"));
}

#[test]
fn a_split_file_leaves_the_waiting_list() {
    let sizes = sizes();
    let stale: Vec<&str> = NOT_SPLIT_YET
        .iter()
        .filter(|(f, _)| sizes.iter().find(|(name, _)| name == f).is_none_or(|&(_, n)| n <= LIMIT))
        .map(|&(f, _)| f)
        .collect();
    assert!(stale.is_empty(), "take these off NOT_SPLIT_YET, they fit now:\n{}", stale.join("\n"));
}
