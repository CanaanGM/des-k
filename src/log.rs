// The log: info!, warn! and error! write one line each, stamped with the local time, its UTC
// offset and a level. A line repeating the one before becomes a count, des-k.log rolls over at
// 5 MB, and every log opens with the version, the Windows build and admin (spec 0019).

use std::io::Write;
use std::os::windows::io::IntoRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, TryLockError};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, SYSTEMTIME},
    System::{
        Console::{GetConsoleWindow, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle},
        Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
        SystemInformation::{GetLocalTime, GetSystemTime},
    },
};
use windows::core::{PCWSTR, w};

macro_rules! info {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Info, format!($($t)*)) };
}
macro_rules! warn {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Warn, format!($($t)*)) };
}
macro_rules! error {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Error, format!($($t)*)) };
}

const LIMIT: u64 = 5 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// Where lines go: stderr (the console, or des-k.log once `open` points it there), or a test's list.
trait Sink {
    fn put(&mut self, line: &str);
    /// des-k.log becomes des-k.old.log and a fresh one takes its place; false when that failed.
    fn roll(&mut self, dir: &Path) -> bool;
}

struct Writer {
    last: Option<(Level, String)>,
    repeats: u32,
    bytes: u64,
    limit: u64,
    /// The data folder and the header, while lines go to des-k.log (no console): only then does it roll.
    file: Option<(PathBuf, String)>,
}

impl Writer {
    fn push(&mut self, sink: &mut impl Sink, stamp: &str, level: Level, msg: String) {
        let msg = one_line(msg);
        if self.last.as_ref().is_some_and(|(l, m)| *l == level && *m == msg) {
            self.repeats += 1;
            return;
        }
        self.flush(sink, stamp);
        if self.bytes >= self.limit && let Some((dir, header)) = self.file.clone() {
            // Reset either way: a roll that failed tries again after another 5 MB, not every line.
            self.bytes = 0;
            if sink.roll(&dir) {
                self.emit(sink, stamp, Level::Info, &header);
                self.emit(sink, stamp, Level::Info, "log rolled over at 5 MB, the earlier part is in des-k.old.log");
            }
        }
        self.emit(sink, stamp, level, &msg);
        self.last = Some((level, msg));
    }

    /// Writes the pending repeat count. After it, the same line again is written in full.
    fn flush(&mut self, sink: &mut impl Sink, stamp: &str) {
        if let Some((level, _)) = self.last.take()
            && self.repeats > 0
        {
            let n = std::mem::take(&mut self.repeats);
            self.emit(sink, stamp, level, &format!("(the line above repeated {n} more times)"));
        }
    }

    fn emit(&mut self, sink: &mut impl Sink, stamp: &str, level: Level, msg: &str) {
        let line = format!("{stamp} {} {msg}\n", level.tag());
        self.bytes += line.len() as u64;
        sink.put(&line);
    }
}

static WRITER: Mutex<Writer> = Mutex::new(Writer { last: None, repeats: 0, bytes: 0, limit: LIMIT, file: None });

/// A panic while writing must not silence the log for good.
fn lock() -> MutexGuard<'static, Writer> {
    WRITER.lock().unwrap_or_else(|e| e.into_inner())
}

struct Std;

impl Sink for Std {
    fn put(&mut self, line: &str) {
        // Rust's stderr looks the std handle up on every write, so it follows `point_std_at`.
        let _ = std::io::stderr().write_all(line.as_bytes());
    }

    fn roll(&mut self, dir: &Path) -> bool {
        let log = dir.join("des-k.log");
        let was = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        let _ = std::fs::rename(&log, dir.join("des-k.old.log"));
        let Ok(f) = crate::ws::create_fresh(&log) else { return false };
        point_std_at(f);
        if let Ok(h) = was
            && !h.is_invalid()
        {
            unsafe { let _ = CloseHandle(h); }
        }
        true
    }
}

/// stdout and stderr both: the default panic text and anything else on them land in the log too.
fn point_std_at(f: std::fs::File) {
    let h = HANDLE(f.into_raw_handle());
    unsafe {
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, h);
        let _ = SetStdHandle(STD_ERROR_HANDLE, h);
    }
}

/// Starts the log and writes its header. With no console (release started at login) lines go
/// to des-k.log, the run before's kept as des-k.old.log; with one, to the console.
/// `quit`: a `--quit` with nothing running, which adds to the log instead of starting a new one.
pub fn open(quit: bool) {
    let header = header(quit);
    let file = unsafe { GetConsoleWindow() }.is_invalid().then(crate::ws::data_dir).and_then(|dir| {
        let log = dir.join("des-k.log");
        let f = if quit {
            crate::ws::append_to(&log)
        } else {
            // The last run's log survives one restart: after a crash it still says why.
            let _ = std::fs::rename(&log, dir.join("des-k.old.log"));
            crate::ws::create_fresh(&log)
        };
        let f = f.ok()?;
        let size = f.metadata().map_or(0, |m| m.len());
        point_std_at(f);
        Some((dir, size))
    });
    let stamp = now();
    let mut w = lock();
    if let Some((dir, size)) = file {
        w.bytes = size;
        w.file = Some((dir, header.clone()));
    }
    w.push(&mut Std, &stamp, Level::Info, header);
}

/// What the macros call.
pub fn write(level: Level, msg: String) {
    let stamp = now();
    lock().push(&mut Std, &stamp, level, msg);
}

/// Writes a pending repeat count. Every exit path calls it, or the count dies with the process.
pub fn flush() {
    let stamp = now();
    lock().flush(&mut Std, &stamp);
}

/// The panic hook's line. A panic inside the writer, or on a thread holding it, would wait
/// forever on the lock: then the line goes straight to stderr.
pub fn panic(info: &std::panic::PanicHookInfo) {
    let at = info.location().map_or(String::new(), |l| format!(" at {}:{}", l.file(), l.line()));
    let msg = format!("panic: {}{at}", info.payload_as_str().unwrap_or("(no message)"));
    let stamp = now();
    let mut w = match WRITER.try_lock() {
        Ok(w) => w,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => {
            let _ = std::io::stderr().write_all(format!("{stamp} {} {}\n", Level::Error.tag(), one_line(msg)).as_bytes());
            return;
        }
    };
    w.push(&mut Std, &stamp, Level::Error, msg);
    w.flush(&mut Std, &stamp);
}

fn one_line(msg: String) -> String {
    if msg.contains(['\n', '\r']) { msg.replace("\r\n", " / ").replace(['\n', '\r'], " / ") } else { msg }
}

fn now() -> String {
    let local = unsafe { GetLocalTime() };
    let utc = unsafe { GetSystemTime() };
    stamp(&local, &utc)
}

/// `2026-11-07 17:05:33.123 +03:00`: the local time, then its offset from UTC, rounded to
/// 15 minutes since the two times are read one after the other.
fn stamp(local: &SYSTEMTIME, utc: &SYSTEMTIME) -> String {
    let ms = |t: &SYSTEMTIME| {
        days(t.wYear.into(), t.wMonth.into(), t.wDay.into()) * 86_400_000
            + i64::from(t.wHour) * 3_600_000
            + i64::from(t.wMinute) * 60_000
            + i64::from(t.wSecond) * 1000
            + i64::from(t.wMilliseconds)
    };
    let off = (ms(local) - ms(utc) + 450_000).div_euclid(900_000) * 15;
    let sign = if off < 0 { '-' } else { '+' };
    let off = off.abs();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {sign}{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond, local.wMilliseconds, off / 60, off % 60
    )
}

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's days_from_civil).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn header(quit: bool) -> String {
    let admin = match crate::elevated() {
        Ok(true) => "yes",
        Ok(false) => "no",
        Err(_) => "unknown",
    };
    let copy = if crate::install::installed() { "installed copy" } else { "not the installed copy" };
    let what = if quit { "--quit with nothing running" } else { "started" };
    format!("des-k {} {what}: {}, admin {admin}, {copy}", env!("CARGO_PKG_VERSION"), windows_build())
}

/// `Windows 25H2 build 26200.7019`. ProductName still says Windows 10 on 11, so it is not read.
fn windows_build() -> String {
    const KEY: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let text = |name: PCWSTR| {
        let mut buf = [0u16; 64];
        let mut size = size_of_val(&buf) as u32;
        let e = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, KEY, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if e.is_ok() { String::from_utf16_lossy(&buf[..len]) } else { "?".into() }
    };
    let mut ubr = 0u32;
    let mut size = 4;
    let e = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, KEY, w!("UBR"), RRF_RT_REG_DWORD, None, Some((&mut ubr as *mut u32).cast()), Some(&mut size)) };
    let ubr = if e.is_ok() { ubr.to_string() } else { "?".into() };
    format!("Windows {} build {}.{ubr}", text(w!("DisplayVersion")), text(w!("CurrentBuild")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects lines without the stamp; a roll shows as `<rolled>`.
    struct Lines(Vec<String>);

    impl Sink for Lines {
        fn put(&mut self, line: &str) {
            self.0.push(line.trim_end().to_string());
        }
        fn roll(&mut self, _: &Path) -> bool {
            self.0.push("<rolled>".into());
            true
        }
    }

    fn writer(limit: u64, file: bool) -> Writer {
        Writer { last: None, repeats: 0, bytes: 0, limit, file: file.then(|| (PathBuf::new(), "des-k started".into())) }
    }

    fn t(y: u16, mo: u16, d: u16, h: u16, mi: u16, s: u16, ms: u16) -> SYSTEMTIME {
        SYSTEMTIME { wYear: y, wMonth: mo, wDay: d, wHour: h, wMinute: mi, wSecond: s, wMilliseconds: ms, wDayOfWeek: 0 }
    }

    // A friend's log next to your clock: the offset is what makes their times comparable to yours.
    #[test]
    fn stamp_carries_the_local_time_and_its_offset() {
        assert_eq!(stamp(&t(2026, 11, 7, 17, 5, 33, 123), &t(2026, 11, 7, 14, 5, 33, 120)), "2026-11-07 17:05:33.123 +03:00");
        assert_eq!(stamp(&t(2026, 11, 7, 9, 35, 0, 0), &t(2026, 11, 7, 14, 5, 0, 1)), "2026-11-07 09:35:00.000 -04:30");
        assert_eq!(stamp(&t(2026, 11, 7, 12, 0, 0, 0), &t(2026, 11, 7, 12, 0, 0, 0)), "2026-11-07 12:00:00.000 +00:00");
    }

    // Local and UTC are read one after the other, and midnight (or a month or year end) can
    // fall between them: the offset must still come out right, not a day off.
    #[test]
    fn stamp_survives_a_date_change_between_the_two_reads() {
        assert_eq!(stamp(&t(2027, 1, 1, 2, 30, 0, 0), &t(2026, 12, 31, 23, 30, 0, 2)), "2027-01-01 02:30:00.000 +03:00");
        assert_eq!(stamp(&t(2026, 2, 28, 20, 0, 0, 0), &t(2026, 3, 1, 0, 0, 0, 0)), "2026-02-28 20:00:00.000 -04:00");
        assert_eq!(stamp(&t(2028, 2, 29, 23, 59, 59, 999), &t(2028, 3, 1, 4, 59, 59, 998)), "2028-02-29 23:59:59.999 -05:00");
    }

    // A failure in the 1 s timer must not write 86,400 lines a day, yet the log must still say how often.
    #[test]
    fn a_repeating_line_becomes_one_line_and_a_count() {
        let (mut w, mut s) = (writer(LIMIT, false), Lines(vec![]));
        for _ in 0..3 {
            w.push(&mut s, "S", Level::Warn, "tray: icon not shown".into());
        }
        w.push(&mut s, "S", Level::Info, "next".into());
        assert_eq!(s.0, ["S WARN  tray: icon not shown", "S WARN  (the line above repeated 2 more times)", "S INFO  next"]);
    }

    // Only the line right before counts, or two lines taking turns would lose their order.
    #[test]
    fn lines_taking_turns_are_all_written() {
        let (mut w, mut s) = (writer(LIMIT, false), Lines(vec![]));
        for m in ["A", "B", "A", "B"] {
            w.push(&mut s, "S", Level::Info, m.into());
        }
        assert_eq!(s.0.len(), 4);
    }

    // Same words at another level is a different event.
    #[test]
    fn the_same_words_at_another_level_are_not_a_repeat() {
        let (mut w, mut s) = (writer(LIMIT, false), Lines(vec![]));
        w.push(&mut s, "S", Level::Warn, "x".into());
        w.push(&mut s, "S", Level::Error, "x".into());
        assert_eq!(s.0, ["S WARN  x", "S ERROR x"]);
    }

    // On exit the count would die with the process, so flush writes it; the same line after
    // that is written in full again.
    #[test]
    fn flush_writes_the_pending_count() {
        let (mut w, mut s) = (writer(LIMIT, false), Lines(vec![]));
        w.push(&mut s, "S", Level::Info, "x".into());
        w.push(&mut s, "S", Level::Info, "x".into());
        w.flush(&mut s, "S");
        w.flush(&mut s, "S");
        w.push(&mut s, "S", Level::Info, "x".into());
        assert_eq!(s.0, ["S INFO  x", "S INFO  (the line above repeated 1 more times)", "S INFO  x"]);
    }

    // des-k runs for weeks: past the limit the log starts over, the pending count first and
    // the header again, so the new file still says which version and Windows wrote it.
    #[test]
    fn past_the_limit_the_log_rolls_and_opens_with_the_header() {
        let (mut w, mut s) = (writer(40, true), Lines(vec![]));
        w.push(&mut s, "S", Level::Info, "a line long enough to pass the limit".into());
        w.push(&mut s, "S", Level::Info, "a line long enough to pass the limit".into());
        w.push(&mut s, "S", Level::Info, "after".into());
        assert_eq!(
            s.0,
            [
                "S INFO  a line long enough to pass the limit",
                "S INFO  (the line above repeated 1 more times)",
                "<rolled>",
                "S INFO  des-k started",
                "S INFO  log rolled over at 5 MB, the earlier part is in des-k.old.log",
                "S INFO  after",
            ]
        );
        assert!(w.bytes < 200, "the count starts again after a roll");
    }

    // With a console there is no file to roll.
    #[test]
    fn a_console_never_rolls() {
        let (mut w, mut s) = (writer(1, false), Lines(vec![]));
        w.push(&mut s, "S", Level::Info, "a".into());
        w.push(&mut s, "S", Level::Info, "b".into());
        assert!(!s.0.contains(&"<rolled>".to_string()));
    }

    // A grep or a line count over the log must see one event per line.
    #[test]
    fn a_message_with_newlines_stays_one_line() {
        let (mut w, mut s) = (writer(LIMIT, false), Lines(vec![]));
        w.push(&mut s, "S", Level::Warn, "first\nsecond\r\nthird".into());
        assert_eq!(s.0, ["S WARN  first / second / third"]);
    }
}
