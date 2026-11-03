// Windows' own touchpad gestures and touchpad.txt (spec 0011, 0011-first-run-setup): whether
// Windows' three and four finger gestures all read off in the registry (des-k acts on a
// gesture only then), your saved values, and the state and actions the welcome screen
// (src/welcome.rs) shows. des-k cannot switch Windows' gestures live, so you set them in
// Settings and des-k follows what the registry says.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use windows::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
    System::{
        Registry::{HKEY_CURRENT_USER, REG_DWORD, REG_VALUE_TYPE, RRF_RT_ANY, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW},
        RemoteDesktop::{WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSFreeMemory, WTSINFOW, WTSQuerySessionInformationW, WTSSessionInfo},
    },
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

const KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad");
/// The values Settings writes for three and four finger gestures, with their "Nothing" numbers,
/// from the check step (not documented by Microsoft). A missing value counts as on.
const VALUES: [(&str, u32); 13] = [
    ("ThreeFingerTapEnabled", 0),
    ("CustomThreeFingerTap", 0),
    ("ThreeFingerUp", 0),
    ("ThreeFingerDown", 0),
    ("ThreeFingerLeft", 0),
    ("ThreeFingerRight", 0),
    ("ThreeFingerSlideEnabled", 0xffff),
    ("FourFingerTapEnabled", 0),
    ("CustomFourFingerTap", 0),
    ("FourFingerUp", 0),
    ("FourFingerDown", 0),
    ("FourFingerLeft", 0),
    ("FourFingerRight", 0),
];
/// The welcome screen's rows: each Settings control and the values it sets, as indexes into
/// VALUES. Checked control by control in Settings on 2026-11-01 (build step 5, Q1): no
/// change touched values of two rows.
pub const ROWS: [(&str, &[usize]); 4] = [
    ("3-finger swipes", &[2, 3, 4, 5, 6]),
    ("3-finger taps", &[0, 1]),
    ("4-finger swipes", &[9, 10, 11, 12]),
    ("4-finger taps", &[7, 8]),
];
/// The largest number touchpad.txt may hold, so a planted file can write nothing odd.
const MAX: u32 = 0xffff;
const SETTINGS_PAGE: &str = "ms-settings:devices-touchpad";

/// One row as the registry has it now.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Row {
    Nothing,
    StillOn,
    CantRead,
}

/// One row after "Give gestures back": how many of its values were written.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Wrote {
    All,
    Part,
    NotOne,
}

/// What "Give gestures back" did. Shown from this, not from a registry read: the registry
/// now holds your values, but Windows keeps its gestures off until you sign in again.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GaveBack {
    pub wrote: [Wrote; 4], // per row of ROWS
    pub cleared: bool,     // touchpad.txt forgot your values
}

impl GaveBack {
    /// A write failed, so your values stay saved and the screen offers to try again.
    pub fn failed(&self) -> bool {
        self.wrote.iter().any(|w| *w != Wrote::All)
    }
}

/// What the welcome screen shows, read fresh.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct State {
    pub pad: bool,     // a precision touchpad is there
    pub enabled: bool, // `[touchpad] enabled`
    pub rows: [Row; 4],
    pub off: bool,   // windows_off(): des-k runs the gestures
    pub saved: bool, // touchpad.txt holds your values
}

/// One registry value as read.
#[derive(Clone, PartialEq, Debug)]
enum Read {
    Dword(u32),
    Missing,
    Other(String), // another type, or a read error: why, worded after the value's name
}

/// Each value as you had it: a number, or None when it did not exist.
type Saved = [Option<u32>; 13];

/// What touchpad.txt holds.
#[derive(Default, PartialEq, Debug)]
struct File {
    saved: Option<Saved>, // all 13 or none, so des-k never writes back half of your settings
    restored: Option<u64>,
}

// touchpad.txt is read, changed and written whole; Ctrl+C and the panic hook write back
// from other threads.
static LOCK: Mutex<()> = Mutex::new(());

fn read(name: &str) -> Read {
    let (mut kind, mut data, mut size) = (REG_VALUE_TYPE::default(), 0u32, 4u32);
    let e = unsafe { RegGetValueW(HKEY_CURRENT_USER, KEY, &HSTRING::from(name), RRF_RT_ANY, Some(&mut kind), Some((&mut data as *mut u32).cast()), Some(&mut size)) };
    match e {
        ERROR_SUCCESS if kind == REG_DWORD && size == 4 => Read::Dword(data),
        ERROR_FILE_NOT_FOUND => Read::Missing,
        ERROR_SUCCESS => Read::Other("is not a number".into()),
        e => Read::Other(format!("cannot be read ({})", why(e))),
    }
}

/// Writes one value back, or deletes it when you had none.
fn write(name: &str, v: Option<u32>) -> Result<(), String> {
    let name = HSTRING::from(name);
    let e = unsafe {
        match v {
            Some(v) => RegSetKeyValueW(HKEY_CURRENT_USER, KEY, &name, REG_DWORD.0, Some((&v as *const u32).cast()), 4),
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, &name),
        }
    };
    match e {
        ERROR_SUCCESS => Ok(()),
        ERROR_FILE_NOT_FOUND if v.is_none() => Ok(()), // already gone
        e => Err(why(e)),
    }
}

/// Writes all 13; a failure is named and the rest still run. Returns the failures: the
/// value's index in VALUES, and its name and why.
fn write_all(values: [Option<u32>; 13]) -> Vec<(usize, String)> {
    let each = VALUES.iter().zip(values).enumerate();
    each.filter_map(|(i, ((name, _), v))| write(name, v).err().map(|e| (i, format!("{name} ({e})")))).collect()
}

fn names(failed: &[(usize, String)]) -> String {
    failed.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ")
}

fn why(e: WIN32_ERROR) -> String {
    windows::core::Error::from(e.to_hresult()).message()
}

/// This Windows sign in's logon time: tells a later start in the same sign in, when Windows
/// still has its gestures off live, from one after a sign in, when Windows read them again.
fn logon_time() -> Result<u64, String> {
    let (mut buf, mut size) = (PWSTR::null(), 0u32);
    unsafe { WTSQuerySessionInformationW(Some(WTS_CURRENT_SERVER_HANDLE), WTS_CURRENT_SESSION, WTSSessionInfo, &mut buf, &mut size) }.map_err(|e| e.message())?;
    let t = match size as usize >= size_of::<WTSINFOW>() {
        true => unsafe { buf.0.cast::<WTSINFOW>().read_unaligned() }.LogonTime,
        false => 0,
    };
    unsafe { WTSFreeMemory(buf.0.cast()) };
    u64::try_from(t).ok().filter(|t| *t > 0).ok_or_else(|| "Windows gave none".into())
}

fn reads() -> [Read; 13] {
    VALUES.map(|(name, _)| read(name))
}

/// Ok when every one of the 13 reads off; else the first that does not, worded for the log.
/// A read error counts as not off: acting while Windows acts too would do each gesture twice.
pub fn windows_off() -> Result<(), String> {
    first_not_off(&reads())
}

fn first_not_off(reads: &[Read; 13]) -> Result<(), String> {
    for ((name, off), r) in VALUES.iter().zip(reads) {
        match r {
            Read::Dword(v) if v == off => {}
            Read::Dword(_) => return Err(format!("{name} is on")),
            Read::Missing => return Err(format!("{name} is missing (Windows may have renamed it)")),
            Read::Other(why) => return Err(format!("{name} {why}")),
        }
    }
    Ok(())
}

/// Each row: Nothing when every value in it reads off, Can't read when any read fails or has
/// another type, else Still on (an absent value is on).
fn rows(reads: &[Read; 13]) -> [Row; 4] {
    ROWS.map(|(_, values)| {
        if values.iter().any(|i| matches!(reads[*i], Read::Other(_))) {
            Row::CantRead
        } else if values.iter().all(|i| reads[*i] == Read::Dword(VALUES[*i].1)) {
            Row::Nothing
        } else {
            Row::StillOn
        }
    })
}

/// Each row's share of the writes that worked, from the failed values' indexes.
fn wrote(failed: &[usize]) -> [Wrote; 4] {
    ROWS.map(|(_, values)| match values.iter().filter(|i| failed.contains(i)).count() {
        0 => Wrote::All,
        n if n == values.len() => Wrote::NotOne,
        _ => Wrote::Part,
    })
}

/// Your values as they are now, to save; a value des-k could not write back stops the save.
fn to_saved(reads: &[Read; 13]) -> Result<Saved, String> {
    let mut saved = [None; 13];
    for (((name, _), r), s) in VALUES.iter().zip(reads).zip(&mut saved) {
        *s = match r {
            Read::Dword(v) if *v <= MAX => Some(*v),
            Read::Dword(v) => return Err(format!("{name} is {v:#x}, more than des-k saves")),
            Read::Missing => None,
            Read::Other(why) => return Err(format!("{name} {why}")),
        };
    }
    Ok(saved)
}

/// Whether the registry holds exactly your saved values, absent ones included.
fn equals_saved(reads: &[Read; 13], saved: &Saved) -> bool {
    reads.iter().zip(saved).all(|(r, s)| match (r, s) {
        (Read::Dword(v), Some(n)) => v == n,
        (Read::Missing, None) => true,
        _ => false,
    })
}

fn parse(text: &str) -> (File, Vec<String>) {
    let mut warn = vec![];
    let mut saved: [Option<Option<u32>>; 13] = [None; 13];
    let mut named = false; // any of the 13 names seen, good or bad
    let mut restored = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some((k, v)) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) else {
            warn.push(format!("skipped \"{line}\": not name=value"));
            continue;
        };
        if k == "restored" {
            match v.parse::<u64>() {
                Ok(t) if restored.is_none() => restored = Some(t),
                Ok(_) => warn.push(format!("skipped \"{line}\": restored twice")),
                Err(_) => warn.push(format!("skipped \"{line}\": not a number")),
            }
            continue;
        }
        let Some(i) = VALUES.iter().position(|(name, _)| *name == k) else {
            warn.push(format!("skipped \"{line}\": not a gesture value des-k knows"));
            continue;
        };
        named = true;
        let value = if v == "absent" { Some(None) } else { v.parse::<u32>().ok().filter(|n| *n <= MAX).map(Some) };
        match (value, saved[i]) {
            (_, Some(_)) => warn.push(format!("skipped \"{line}\": {k} twice")),
            (Some(value), None) => saved[i] = Some(value),
            (None, None) => warn.push(format!("skipped \"{line}\": not absent or a number from 0 to {MAX}")),
        }
    }
    let saved = match saved.iter().all(Option::is_some) {
        true => Some(saved.map(Option::unwrap_or_default)),
        false if named => {
            warn.push("the saved gesture values are incomplete, so none are kept (des-k never writes back half of them)".into());
            None
        }
        false => None,
    };
    (File { saved, restored }, warn)
}

fn format(f: &File) -> String {
    let mut out = String::new();
    if let Some(saved) = &f.saved {
        for ((name, _), v) in VALUES.iter().zip(saved) {
            match v {
                Some(v) => out += &format!("{name}={v}\n"),
                None => out += &format!("{name}=absent\n"),
            }
        }
    }
    if let Some(t) = f.restored {
        out += &format!("restored={t}\n");
    }
    out
}

fn path() -> PathBuf {
    crate::ws::data_dir().join("touchpad.txt")
}

/// None: no touchpad.txt yet, the first start.
fn load() -> Option<File> {
    match std::fs::read_to_string(path()) {
        Ok(text) => {
            let (f, warn) = parse(&text);
            for w in warn {
                eprintln!("touchpad.txt: {w}");
            }
            Some(f)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            // Not a first start: the box would open at every start otherwise.
            eprintln!("touchpad.txt: cannot read it ({e}), so no saved values");
            Some(File::default())
        }
    }
}

/// A hard kill mid write must never lose your saved values.
fn save(f: &File) -> bool {
    let tmp = crate::ws::data_dir().join("touchpad.tmp");
    match crate::ws::write_new(&tmp, &format(f)).and_then(|()| std::fs::rename(&tmp, path())) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("touchpad.txt: cannot write it: {e}");
            false
        }
    }
}

/// Main thread, before the touchpad thread starts: logs the setup state. Returns whether this
/// is the first start, when the welcome screen opens.
pub fn start() -> bool {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load();
    if let Some(f) = &mut file {
        after_write_back(f);
    }
    let off = windows_off();
    if !crate::settings::get().touchpad_enabled && off.is_ok() {
        eprintln!(
            "touchpad reading is off in settings.toml, but Windows' three and four finger gestures are set to Nothing, so they do nothing; set them back in Settings, Touchpad"
        );
    }
    let gestures = match &off {
        Ok(()) => "Windows' gestures all off".to_string(),
        Err(why) => format!("Windows' gestures on ({why})"),
    };
    let saved = match &file {
        None => "no touchpad.txt yet (first start)",
        Some(File { saved: Some(_), .. }) => "your values saved",
        Some(_) => "no values saved",
    };
    println!("touchpad setup: {gestures}, {saved}");
    file.is_none()
}

/// An exit wrote your values back (the `restored` line). If that was earlier in this sign in,
/// Windows still has its gestures off live, so the registry is set off again to match it.
fn after_write_back(f: &mut File) {
    let Some(t) = f.restored.take() else { return };
    save(f);
    let Some(saved) = &f.saved else { return };
    match logon_time() {
        Ok(now) if now == t && equals_saved(&reads(), saved) => {
            if crate::settings::get().touchpad_enabled {
                eprintln!(
                    "Quit wrote your Windows gestures back earlier in this sign in; setting them off again. If you changed them in Settings since, every gesture now acts twice; press Give gestures back."
                );
                let failed = write_all(VALUES.map(|(_, off)| Some(off)));
                if !failed.is_empty() {
                    eprintln!("touchpad setup: could not set {} off again", names(&failed));
                }
            } else {
                eprintln!("Windows' gestures come back at your next sign in; until then three and four finger gestures do nothing");
            }
        }
        Ok(_) => {} // you signed in since, or changed them yourself
        Err(e) => eprintln!("touchpad setup: cannot read this sign in's logon time ({e}), so the earlier write back is left as it is"),
    }
}

/// Quit, `--quit`, a panic and Ctrl+C (never a sign out, shutdown or hard kill): hands Windows
/// your gestures back from your next sign in. Only while they still read off: if you changed
/// one in Settings since, your settings are left alone.
pub fn write_back() {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut f) = load() else { return };
    let Some(saved) = f.saved else { return };
    if let Err(why) = windows_off() {
        println!("touchpad setup: nothing written back, Windows' gestures are not all off ({why})");
        return;
    }
    let t = match logon_time() {
        Ok(t) => t,
        Err(e) => return eprintln!("touchpad setup: nothing written back, cannot read this sign in's logon time ({e})"),
    };
    // First, so a crash partway leaves a line the next start can act on.
    f.restored = Some(t);
    if !save(&f) {
        return eprintln!("touchpad setup: nothing written back, since touchpad.txt could not record it");
    }
    match write_all(saved).as_slice() {
        [] => println!("touchpad setup: wrote your Windows gesture values back; Windows uses them from your next sign in"),
        failed => eprintln!("touchpad setup: could not write back {}", names(failed)),
    }
}

/// For the welcome screen, read fresh each time it refreshes.
pub fn state() -> State {
    let reads = reads();
    let saved = LOCK.lock().map(|_| load()).unwrap_or_default().is_some_and(|f| f.saved.is_some());
    State {
        pad: crate::touchpad::present(),
        enabled: crate::settings::get().touchpad_enabled,
        rows: rows(&reads),
        off: first_not_off(&reads).is_ok(),
        saved,
    }
}

pub fn open_settings() {
    crate::tray::open(Path::new(SETTINGS_PAGE));
}

/// Skip on the first start: an empty touchpad.txt, so the screen never opens on its own again.
pub fn skip() {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if load().is_none() {
        save(&File::default());
    }
}

/// "Use these gestures" (and "Choose my own"): saves your values the first time, then opens Settings at Touchpad.
pub fn use_gestures() {
    {
        let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = load().unwrap_or_default();
        let reads = reads();
        if f.saved.is_none() && first_not_off(&reads).is_err() {
            match to_saved(&reads) {
                Ok(s) => {
                    f.saved = Some(s);
                    println!("touchpad setup: your Windows gesture values saved");
                }
                Err(why) => eprintln!("touchpad setup: your Windows gesture values not saved, {why}"),
            }
        }
        save(&f); // also when nothing changed: the screen never opens on its own again
    }
    crate::tray::open(Path::new(SETTINGS_PAGE));
}

/// "Give gestures back to Windows": writes your saved values back (Windows uses them from your
/// next sign in) and forgets them only if every write worked. Then opens Settings at Touchpad.
/// None when there was nothing saved to give back.
pub fn give_back() -> Option<GaveBack> {
    let done = {
        let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = load().unwrap_or_default();
        let saved = f.saved?;
        let failed = write_all(saved);
        let wrote = wrote(&failed.iter().map(|(i, _)| *i).collect::<Vec<_>>());
        let cleared = if failed.is_empty() {
            f.saved = None;
            f.restored = None;
            println!("touchpad setup: gave Windows its gestures back, from your next sign in");
            save(&f)
        } else {
            eprintln!("touchpad setup: could not give back {}, your values stay saved", names(&failed));
            false
        };
        GaveBack { wrote, cleared }
    };
    open_settings();
    Some(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_OFF: [u32; 13] = [0, 0, 0, 0, 0, 0, 0xffff, 0, 0, 0, 0, 0, 0];

    fn off_reads() -> [Read; 13] {
        ALL_OFF.map(Read::Dword)
    }

    // The gate: des-k acts only when Windows does nothing, or one gesture does two things.
    #[test]
    fn only_all_thirteen_off_is_off() {
        assert_eq!(first_not_off(&off_reads()), Ok(()));
        let mut r = off_reads();
        r[11] = Read::Dword(0xffff); // FourFingerLeft set to a custom shortcut
        assert_eq!(first_not_off(&r), Err("FourFingerLeft is on".into()));
        // ThreeFingerSlideEnabled is off at 0xffff, not 0.
        let mut r = off_reads();
        r[6] = Read::Dword(0);
        assert!(first_not_off(&r).is_err());
    }

    // Absent is Windows' default, which is on; a read error must not let des-k act.
    #[test]
    fn missing_or_unreadable_is_not_off() {
        let mut r = off_reads();
        r[0] = Read::Missing;
        assert_eq!(first_not_off(&r), Err("ThreeFingerTapEnabled is missing (Windows may have renamed it)".into()));
        let mut r = off_reads();
        r[12] = Read::Other("is not a number".into());
        assert_eq!(first_not_off(&r), Err("FourFingerRight is not a number".into()));
    }

    // What Use saves must be exactly what give back writes, absent ones included.
    #[test]
    fn saved_values_round_trip_through_the_file() {
        let saved = [None, None, None, None, None, None, None, None, None, Some(2), Some(3), Some(0xffff), Some(0xffff)];
        let f = File { saved: Some(saved), restored: Some(133_000_000_000_000_000) };
        assert_eq!(parse(&format(&f)), (f, vec![]));
        assert_eq!(parse(""), (File::default(), vec![]));
    }

    // Writing back half of your settings would leave Windows in a state you never set.
    #[test]
    fn one_bad_or_missing_saved_line_drops_them_all() {
        let full = format(&File { saved: Some([Some(1); 13]), restored: None });
        for bad in ["FourFingerUp=1\n", "FourFingerUp=65536\n", "FourFingerUp=-1\n", "FourFingerUp=on\n"] {
            let text = full.replace("FourFingerUp=1\n", if bad == "FourFingerUp=1\n" { "" } else { bad });
            let (f, warn) = parse(&text);
            assert_eq!(f.saved, None, "{bad}");
            assert!(warn.last().unwrap().contains("none are kept"), "{bad}");
        }
    }

    // The same sign in rewrite sets gestures off again only over exactly what Quit wrote: any
    // other value means you changed them in Settings, and des-k must leave that alone.
    #[test]
    fn only_the_values_quit_wrote_count_as_unchanged() {
        let saved = [None, None, None, None, None, None, None, None, None, Some(2), Some(3), Some(0xffff), Some(0xffff)];
        let mut r = saved.map(|v| v.map_or(Read::Missing, Read::Dword));
        assert!(equals_saved(&r, &saved));
        r[0] = Read::Dword(0); // you set the three finger tap to Nothing since
        assert!(!equals_saved(&r, &saved));
        let mut r = saved.map(|v| v.map_or(Read::Missing, Read::Dword));
        r[9] = Read::Missing;
        assert!(!equals_saved(&r, &saved));
        r[9] = Read::Other("cannot be read (denied)".into());
        assert!(!equals_saved(&r, &saved));
    }

    // A planted line naming another value must never reach the registry.
    #[test]
    fn unknown_names_and_bad_lines_are_skipped() {
        let mut text = format(&File { saved: Some([None; 13]), restored: None });
        text += "Shell=cmd.exe\nnonsense\nrestored=soon\n";
        let (f, warn) = parse(&text);
        assert_eq!(f.saved, Some([None; 13]));
        assert_eq!(f.restored, None);
        assert_eq!(warn.len(), 3);
    }

    // Each value belongs to exactly one row, or a row could read Nothing while Windows still
    // runs a gesture that only that value controls (Q1, 2026-11-01).
    #[test]
    fn every_value_is_in_exactly_one_row() {
        let mut seen = [0; 13];
        for (_, values) in ROWS {
            for i in values {
                seen[*i] += 1;
            }
        }
        assert_eq!(seen, [1; 13]);
    }

    // A row says Nothing only when Windows does nothing for that control; a value des-k cannot
    // read must never look like Nothing, and an absent one is Windows' default, which is on.
    #[test]
    fn a_row_is_nothing_only_when_all_its_values_read_off() {
        assert_eq!(rows(&off_reads()), [Row::Nothing; 4]);
        let mut r = off_reads();
        r[6] = Read::Dword(0); // ThreeFingerSlideEnabled: off is 0xffff
        r[7] = Read::Missing; // FourFingerTapEnabled
        r[12] = Read::Other("cannot be read (denied)".into());
        assert_eq!(rows(&r), [Row::StillOn, Row::Nothing, Row::CantRead, Row::StillOn]);
        // Can't read wins over Still on in the same row.
        r[9] = Read::Dword(2);
        assert_eq!(rows(&r)[2], Row::CantRead);
    }

    // After a give back the rows say what was written, row by row, so you can tell which
    // gestures come back at your next sign in.
    #[test]
    fn written_rows_follow_the_failed_values() {
        assert_eq!(wrote(&[]), [Wrote::All; 4]);
        assert_eq!(wrote(&[2, 0, 1]), [Wrote::Part, Wrote::NotOne, Wrote::All, Wrote::All]);
        let every: Vec<usize> = (0..13).collect();
        assert_eq!(wrote(&every), [Wrote::NotOne; 4]);
        assert!(!GaveBack { wrote: [Wrote::All; 4], cleared: false }.failed());
        assert!(GaveBack { wrote: [Wrote::All, Wrote::Part, Wrote::All, Wrote::All], cleared: false }.failed());
    }

    // Only a value des-k can write back as it was is saved.
    #[test]
    fn a_value_des_k_cannot_write_back_stops_the_save() {
        let mut r = off_reads();
        r[0] = Read::Missing;
        assert_eq!(to_saved(&r).unwrap()[0], None);
        r[1] = Read::Dword(0x1_0000);
        assert!(to_saved(&r).is_err());
        let mut r = off_reads();
        r[2] = Read::Other("cannot be read (denied)".into());
        assert!(to_saved(&r).is_err());
    }
}
