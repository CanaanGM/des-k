// Install a newer release from the tray (spec 0018), only in the installed copy running as
// admin. A worker thread downloads the release's SHA256SUMS.txt and setup into the admin only
// <Program Files>\des-k\update\, checks the setup's SHA-256, and runs it silently through its
// own one shot task `des-k update`: a child of des-k would sit in the des-k task's job and die
// with it. The setup stops des-k, installs and starts it again; the next des-k says how it went.
use crate::update::{self, Fail, Handle, Version, fail};
use std::convert::Infallible;
use std::ffi::OsString;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, HWND, LPARAM, LocalFree, NTSTATUS, WPARAM},
    Networking::WinHttp::*,
    Security::{
        Authorization::ConvertSidToStringSidW,
        Cryptography::{BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData},
        GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::{
        Com::CoTaskMemFree,
        Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS},
        SystemInformation::GetSystemDirectoryW,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
    UI::{
        Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
        WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

/// Window message to the tray window, wParam and lParam from `pack`. Its own id: main's loop
/// matches ids alone, and `WM_APP + 14` is `keys::WM_HINTS`.
pub const WM_INSTALL: u32 = WM_APP + 15;

// `pack` puts the version's 48 bits in lParam.
const _: () = assert!(size_of::<isize>() == 8, "des-k builds for x64 only");

const SUMS_CAP: u64 = 64 * 1024;
const SETUP_CAP: u64 = 64 * 1024 * 1024;
const TASK: &str = "des-k update";
const XML: &str = "des-k-update.xml";
const LOG: &str = "setup.log";
const START_WAIT: Duration = Duration::from_secs(60);
const SLOW: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_millis(500);
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Downloading(Version),
    Installing(Version),
}

/// Kind in the top 16 bits, the version in the low 48, so the menu never reads one without the other.
static PHASE: AtomicU64 = AtomicU64::new(0);
/// The start up cleanup still runs: an install now could lose its files or task to it.
static CLEANING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallFail {
    Net(Fail),
    Missing,
    NoLine,
    TooBig,
    Mismatch,
    Folder,
    Task,
    SetupStopped,
    Slow,
    Crashed,
}

impl InstallFail {
    /// The "Could not install" balloon's sentence; `SetupStopped` and `Slow` have their own balloon.
    pub fn reason(self) -> String {
        match self {
            InstallFail::Net(f) => f.reason(),
            InstallFail::Missing => "This release has no setup or checksum file.".into(),
            InstallFail::NoLine => "The checksum file does not list the setup.".into(),
            InstallFail::TooBig => "The setup is larger than expected.".into(),
            InstallFail::Mismatch => "The download does not match its checksum.".into(),
            InstallFail::Folder => "The update folder could not be used.".into(),
            InstallFail::Task => "Windows did not start the setup.".into(),
            InstallFail::SetupStopped | InstallFail::Slow | InstallFail::Crashed => "The install stopped with an error (the log has it).".into(),
        }
    }

    fn code(self) -> usize {
        match self {
            InstallFail::Net(_) => 0,
            InstallFail::Missing => 1,
            InstallFail::NoLine => 2,
            InstallFail::TooBig => 3,
            InstallFail::Mismatch => 4,
            InstallFail::Folder => 5,
            InstallFail::Task => 6,
            InstallFail::SetupStopped => 7,
            InstallFail::Slow => 8,
            InstallFail::Crashed => 9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    Started,
    Failed(InstallFail),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Updated(Version),
    NotUpdated(Version),
    Nothing,
}

fn v48(v: Version) -> u64 {
    (v.0 as u64) << 32 | (v.1 as u64) << 16 | v.2 as u64
}

fn from48(n: u64) -> Version {
    Version((n >> 32) as u16, (n >> 16) as u16, n as u16)
}

fn pack_phase(p: Phase) -> u64 {
    match p {
        Phase::Idle => 0,
        Phase::Downloading(v) => 1 << 48 | v48(v),
        Phase::Installing(v) => 2 << 48 | v48(v),
    }
}

fn unpack_phase(n: u64) -> Phase {
    let v = from48(n & 0xFFFF_FFFF_FFFF);
    match n >> 48 {
        1 => Phase::Downloading(v),
        2 => Phase::Installing(v),
        _ => Phase::Idle,
    }
}

pub fn phase() -> Phase {
    unpack_phase(PHASE.load(Ordering::SeqCst))
}

fn set_phase(p: Phase) {
    PHASE.store(pack_phase(p), Ordering::SeqCst);
}

/// wParam: kind (0 started, 1 failed) | fail code << 8 | the network failure's code << 16 |
/// its number (the HTTP status) << 24. lParam: the version's 48 bits.
pub fn pack(m: Msg, v: Version) -> (WPARAM, LPARAM) {
    let wp = match m {
        Msg::Started => 0,
        Msg::Failed(f) => {
            let (net, n) = if let InstallFail::Net(e) = f { e.code() } else { (0, 0) };
            1 | f.code() << 8 | net << 16 | (n as usize) << 24
        }
    };
    (WPARAM(wp), LPARAM(v48(v) as isize))
}

pub fn unpack(wp: WPARAM, lp: LPARAM) -> Option<(Msg, Version)> {
    let v = from48(lp.0 as u64 & 0xFFFF_FFFF_FFFF);
    let f = match (wp.0 >> 8) & 0xFF {
        0 => InstallFail::Net(Fail::from_code((wp.0 >> 16) & 0xFF, (wp.0 >> 24) as u16)?),
        1 => InstallFail::Missing,
        2 => InstallFail::NoLine,
        3 => InstallFail::TooBig,
        4 => InstallFail::Mismatch,
        5 => InstallFail::Folder,
        6 => InstallFail::Task,
        7 => InstallFail::SetupStopped,
        8 => InstallFail::Slow,
        9 => InstallFail::Crashed,
        _ => return None,
    };
    match wp.0 & 0xFF {
        0 => Some((Msg::Started, v)),
        1 => Some((Msg::Failed(f), v)),
        _ => None,
    }
}

fn setup_name(v: Version) -> String {
    format!("des-k-v{v}-setup.exe")
}

/// The download paths on github.com, built from the numbers only (AC-4).
fn release_path(v: Version, file: &str) -> String {
    format!("/CanaanGM/des-k/releases/download/v{v}/{file}")
}

/// The version of a verified setup's file name, `des-k-vX.Y.Z-setup.exe` exactly: a `.done`
/// or `.part` never counts as an outcome.
fn setup_version(name: &str) -> Option<Version> {
    name.strip_prefix("des-k-v")?.strip_suffix("-setup.exe").and_then(update::parse_version)
}

/// The hash on the one line that is 64 hex digits, two spaces and exactly `name` (the format
/// build.yml writes). None, or two such lines, is `NoLine`.
pub fn sums_line(text: &str, name: &str) -> Result<[u8; 32], InstallFail> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut found = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some((hex, rest)) = line.split_at_checked(64) else { continue };
        if rest.strip_prefix("  ") != Some(name) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if found.is_some() {
            warn!("install: SHA256SUMS.txt names {name} twice");
            return Err(InstallFail::NoLine);
        }
        let mut hash = [0u8; 32];
        for (i, b) in hash.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| InstallFail::NoLine)?;
        }
        found = Some(hash);
    }
    found.ok_or_else(|| {
        warn!("install: SHA256SUMS.txt has no line for {name}");
        InstallFail::NoLine
    })
}

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// An incremental BCrypt SHA-256, its handle destroyed on every path.
struct Sha(BCRYPT_HASH_HANDLE);

fn nt(s: NTSTATUS, what: &str) -> Result<(), InstallFail> {
    if s.is_ok() {
        Ok(())
    } else {
        warn!("install: {what} failed: NTSTATUS {:#010x}", s.0);
        Err(InstallFail::Crashed)
    }
}

impl Sha {
    fn new() -> Result<Sha, InstallFail> {
        let mut h = BCRYPT_HASH_HANDLE::default();
        nt(unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut h, None, None, 0) }, "starting SHA-256")?;
        Ok(Sha(h))
    }

    fn add(&mut self, bytes: &[u8]) -> Result<(), InstallFail> {
        nt(unsafe { BCryptHashData(self.0, bytes, 0) }, "hashing")
    }

    fn finish(self) -> Result<[u8; 32], InstallFail> {
        let mut out = [0u8; 32];
        nt(unsafe { BCryptFinishHash(self.0, &mut out, 0) }, "finishing SHA-256")?;
        Ok(out)
    }
}

impl Drop for Sha {
    fn drop(&mut self) {
        unsafe { let _ = BCryptDestroyHash(self.0); }
    }
}

#[cfg(test)]
fn sha256(bytes: &[u8]) -> Result<[u8; 32], InstallFail> {
    let mut s = Sha::new()?;
    s.add(bytes)?;
    s.finish()
}

/// `<Program Files>` from Windows, never the environment des-k inherits (anything running as
/// you can set that).
fn program_files() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None).inspect_err(|e| warn!("install: Program Files not found: {e}")).ok()?;
        let path = PathBuf::from(OsString::from_wide(p.as_wide()));
        CoTaskMemFree(Some(p.0 as *const _));
        Some(path)
    }
}

fn update_dir() -> Option<PathBuf> {
    program_files().map(|p| p.join(r"des-k\update"))
}

/// `setup.log` in the update folder, when the last setup wrote one (AC-8's click).
pub fn setup_log() -> Option<PathBuf> {
    update_dir().map(|d| d.join(LOG)).filter(|p| p.is_file())
}

/// Admin, and running as `<Program Files>\des-k\des-k.exe` (AC-2). Read once: neither changes.
pub fn can_install() -> bool {
    static CAN: OnceLock<bool> = OnceLock::new();
    *CAN.get_or_init(|| {
        let admin = crate::elevated().inspect_err(|e| warn!("install: cannot tell whether des-k runs as admin: {e}")).unwrap_or(false);
        let same = installed();
        if !(admin && same) {
            info!("install: this des-k (admin {admin}, installed copy {same}) is not the installed one, so updates open their page");
        }
        admin && same
    })
}

/// Running as `<Program Files>\des-k\des-k.exe`; false when either path can't be read.
/// Logs nothing: the log's header calls it before the log is open.
pub fn installed() -> bool {
    let (Some(pf), Ok(exe)) = (program_files(), std::env::current_exe()) else { return false };
    exe.to_string_lossy().to_lowercase() == pf.join(r"des-k\des-k.exe").to_string_lossy().to_lowercase()
}

/// `<System32>\schtasks.exe`, never `%SystemRoot%` from the environment.
fn schtasks_exe() -> Option<PathBuf> {
    let mut buf = [0u16; 260];
    let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        warn!("install: System32 not found: {}", windows::core::Error::from_thread());
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..n])).join("schtasks.exe"))
}

/// Runs schtasks with no window; false, with its words in the log, when it fails.
fn schtasks(args: &[&std::ffi::OsStr]) -> bool {
    let Some(exe) = schtasks_exe() else { return false };
    match std::process::Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let say = String::from_utf8_lossy(&o.stderr);
            warn!("install: schtasks {:?} failed ({}): {}", args, o.status, say.trim());
            false
        }
        Err(e) => {
            warn!("install: schtasks did not run: {e}");
            false
        }
    }
}

/// Logs what Task Scheduler says about the update task, before a failure deletes it: its
/// status and last result are the only word on why the setup never started.
fn log_task_state() {
    let Some(exe) = schtasks_exe() else { return };
    let Ok(o) = std::process::Command::new(exe).args(["/query", "/tn", TASK, "/v", "/fo", "list"]).creation_flags(CREATE_NO_WINDOW).output() else {
        return warn!("install: the task's state not read");
    };
    for line in String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.starts_with("Status") || l.starts_with("Last R")) {
        warn!("install: task {TASK:?} {}", line.split_whitespace().collect::<Vec<_>>().join(" "));
    }
}

fn task_exists() -> bool {
    let Some(exe) = schtasks_exe() else { return false };
    std::process::Command::new(exe).args(["/query", "/tn", TASK]).creation_flags(CREATE_NO_WINDOW).output().is_ok_and(|o| o.status.success())
}

/// Ends and deletes the `des-k update` task. False when it is still there.
fn drop_task() -> bool {
    if !task_exists() {
        return true;
    }
    let _ = schtasks(&["/end".as_ref(), "/tn".as_ref(), TASK.as_ref()]);
    schtasks(&["/delete".as_ref(), "/tn".as_ref(), TASK.as_ref(), "/f".as_ref()])
}

/// Removes one file, retrying for 2 s: an ended setup lets go of its exe a moment later.
fn remove(path: &Path) -> std::io::Result<()> {
    let mut tries = 10;
    loop {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) if tries > 0 => {
                tries -= 1;
                std::thread::sleep(Duration::from_millis(200));
            }
            r => return r,
        }
    }
}

/// The task, then every file in `update\` but `setup.log` (AC-10). The reasons it could not.
fn clear(dir: &Path) -> Result<(), String> {
    let mut why = Vec::new();
    if !drop_task() {
        why.push(format!("the {TASK:?} task is still there"));
    }
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for e in entries.flatten() {
                let path = e.path();
                if e.file_name() == LOG || !e.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                if let Err(err) = remove(&path) {
                    why.push(format!("{} not deleted: {err}", path.display()));
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => why.push(format!("{} not read: {e}", dir.display())),
    }
    if why.is_empty() { Ok(()) } else { Err(why.join("; ")) }
}

/// A link there is refused, never followed: nothing but an admin may name what des-k runs.
fn is_plain_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0)
}

/// The update folder, made if missing (it inherits Program Files' admin only rights), with
/// whatever an earlier run left cleared away.
fn prepare(dir: &Path) -> Result<(), InstallFail> {
    if let Err(e) = std::fs::create_dir(dir)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        warn!("install: {} not created: {e}", dir.display());
        return Err(InstallFail::Folder);
    }
    if !is_plain_dir(dir) {
        warn!("install: {} is a link or not a folder, refused", dir.display());
        return Err(InstallFail::Folder);
    }
    clear(dir).map_err(|why| {
        warn!("install: the last update's leftovers not cleared: {why}");
        InstallFail::Folder
    })
}

/// des-k's own account as a SID string, so no account name is pasted into the task XML.
fn user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        if let Err(e) = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) {
            warn!("install: own token not opened: {e}");
            return None;
        }
        let mut len = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)]; // 8 byte aligned for TOKEN_USER
        let r = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len);
        let _ = CloseHandle(token);
        if let Err(e) = r {
            warn!("install: own account not read: {e}");
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        if let Err(e) = ConvertSidToStringSidW(user.User.Sid, &mut s) {
            warn!("install: own account not turned into a SID string: {e}");
            return None;
        }
        let sid = String::from_utf16_lossy(s.as_wide());
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        Some(sid)
    }
}

/// XML text, and anything past ASCII as a character reference: schtasks rejects UTF-8 files.
fn xml_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("&#{};", c as u32)),
        }
    }
    out
}

/// The one shot task: the setup, then the des-k task again on every path (AC-6). Plain ASCII,
/// no `<?xml?>` line. No time limit: it would end the second action too, leaving des-k down.
pub fn task_xml(user: &str, setup: &Path, log: &Path, schtasks: &Path) -> String {
    let [user, setup, log, schtasks] = [user, &setup.to_string_lossy(), &log.to_string_lossy(), &schtasks.to_string_lossy()].map(xml_text);
    format!(
        concat!(
            r#"<Task version="1.3" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">"#,
            r#"<Principals><Principal id="Author"><UserId>{user}</UserId>"#,
            "<LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>",
            "<Settings><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><Priority>4</Priority></Settings>",
            r#"<Actions Context="Author">"#,
            r#"<Exec><Command>{setup}</Command><Arguments>/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="{log}"</Arguments></Exec>"#,
            "<Exec><Command>{schtasks}</Command><Arguments>/run /tn des-k</Arguments></Exec>",
            "</Actions></Task>"
        ),
        user = user,
        setup = setup,
        log = log,
        schtasks = schtasks
    )
}

/// The outcome at start: the setup des-k ran, against the version now running (AC-7, AC-8).
fn rule(setup: Option<Version>, ours: Version) -> Outcome {
    match setup {
        Some(v) if v == ours => Outcome::Updated(v),
        Some(v) if v > ours => Outcome::NotUpdated(v),
        _ => Outcome::Nothing,
    }
}

/// From the tray, main thread: starts the install of `v`, which a check you started found.
/// One check or install at a time (AC-3).
pub fn start(tray: HWND, v: Version) {
    if CLEANING.load(Ordering::SeqCst) {
        warn!("install: the last update's files are still being cleared, this click is ignored");
        return;
    }
    if update::RUNNING.swap(true, Ordering::SeqCst) {
        warn!("install: a check or an install is already running, this click is ignored");
        return;
    }
    set_phase(Phase::Downloading(v));
    info!("install: started, des-k {v}");
    let hwnd = tray.0 as isize; // an HWND is not Send
    let spawned = std::thread::Builder::new().name("install".into()).spawn(move || {
        let mut guard = Guard { hwnd, v, dir: None, done: false };
        let Err(f) = steps(&mut guard);
        guard.fail(f);
    });
    if let Err(e) = spawned {
        warn!("install: thread not started: {e}");
        Guard { hwnd, v, dir: None, done: false }.fail(InstallFail::Crashed);
    }
}

/// On every failed exit, a panic included: deletes this install's files and task, frees the
/// item, and posts the one final message.
struct Guard {
    hwnd: isize,
    v: Version,
    dir: Option<PathBuf>,
    done: bool,
}

impl Guard {
    fn post(&self, m: Msg) {
        let (wp, lp) = pack(m, self.v);
        if let Err(e) = unsafe { PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_INSTALL, wp, lp) } {
            warn!("install: {m:?} not passed to the tray: {e}");
        }
    }

    fn fail(&mut self, f: InstallFail) {
        self.done = true;
        warn!("install of des-k {} failed: {f:?}", self.v);
        if let Some(dir) = &self.dir
            && let Err(why) = clear(dir)
        {
            warn!("install: cleanup after the failure incomplete, the next start tries again: {why}");
        }
        set_phase(Phase::Idle);
        update::RUNNING.store(false, Ordering::SeqCst);
        self.post(Msg::Failed(f));
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.done {
            self.fail(InstallFail::Crashed);
        }
    }
}

/// One GET over HTTPS, redirects followed (GitHub sends files from another host; WinHTTP's
/// default never goes from HTTPS to HTTP), each chunk to `each`. The byte count.
fn download(path: &str, cap: u64, mut each: impl FnMut(&[u8]) -> Result<(), InstallFail>) -> Result<u64, InstallFail> {
    let net = |e, what| InstallFail::Net(fail(e, what));
    let t = update::TIMEOUT_MS;
    unsafe {
        let agent = HSTRING::from(format!("des-k/{}", env!("CARGO_PKG_VERSION")));
        let session = Handle::new(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "install: opening WinHTTP").map_err(InstallFail::Net)?;
        WinHttpSetTimeouts(session.0, t, t, t, t).map_err(|e| net(e, "install: setting timeouts"))?;
        let connect = Handle::new(WinHttpConnect(session.0, update::HOST, INTERNET_DEFAULT_HTTPS_PORT, 0), "install: connecting").map_err(InstallFail::Net)?;
        let object = HSTRING::from(path);
        let request = Handle::new(
            WinHttpOpenRequest(connect.0, w!("GET"), &object, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "install: opening the request",
        )
        .map_err(InstallFail::Net)?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| net(e, "install: sending the request"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| net(e, "install: receiving the answer"))?;

        let mut status = 0u32;
        let mut len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&raw mut status).cast()), &mut len, std::ptr::null_mut())
            .map_err(|e| net(e, "install: reading the status"))?;
        info!("install: {path} answered HTTP {status}");
        match status {
            200 => {}
            404 => return Err(InstallFail::Missing),
            407 => return Err(InstallFail::Net(Fail::ProxyLogin)),
            n => return Err(InstallFail::Net(Fail::Status(u16::try_from(n).unwrap_or(u16::MAX)))),
        }
        let mut size = 0u64;
        let mut len = size_of::<u64>() as u32;
        let known = WinHttpQueryHeaders(request.0, WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64, PCWSTR::null(), Some((&raw mut size).cast()), &mut len, std::ptr::null_mut()).is_ok();
        if known && size > cap {
            warn!("install: {path} is {size} bytes, more than the {cap} allowed");
            return Err(InstallFail::TooBig);
        }

        let mut total = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut avail).map_err(|e| net(e, "install: waiting for data"))?;
            if avail == 0 {
                break;
            }
            let want = (avail as usize).min(buf.len());
            let mut read = 0u32;
            WinHttpReadData(request.0, buf.as_mut_ptr().cast(), want as u32, &mut read).map_err(|e| net(e, "install: reading data"))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > cap {
                warn!("install: {path} is past the {cap} bytes allowed, stopped");
                return Err(InstallFail::TooBig);
            }
            each(&buf[..read as usize])?;
        }
        Ok(total)
    }
}

/// Whether the setup runs: a process whose image is exactly this exe. Never opens the file: an
/// open landing as Task Scheduler starts the setup fails that start with a sharing violation
/// (0x80070020, tried 2026-11-07). Names come from the snapshot; only a match is opened.
fn running(exe: &Path) -> bool {
    let Some(name) = exe.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    let want = exe.to_string_lossy().to_lowercase();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        warn!("install: the process list not read");
        return false;
    };
    let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut found = false;
    let mut more = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
    while more && !found {
        let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
        if String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase() == name {
            found = crate::restore::exe_path(e.th32ProcessID).is_some_and(|p| p.to_lowercase() == want);
        }
        more = unsafe { Process32NextW(snap, &mut e) }.is_ok();
    }
    unsafe { let _ = CloseHandle(snap); }
    found
}

/// The worker. Returns only on a failure: on success the setup ends des-k.
fn steps(g: &mut Guard) -> Result<Infallible, InstallFail> {
    let v = g.v;
    let dir = update_dir().ok_or(InstallFail::Folder)?;
    prepare(&dir)?;
    g.dir = Some(dir.clone());
    let name = setup_name(v);

    let mut sums = Vec::new();
    let n = download(&release_path(v, "SHA256SUMS.txt"), SUMS_CAP, |b| {
        sums.extend_from_slice(b);
        Ok(())
    })?;
    info!("install: SHA256SUMS.txt downloaded, {n} bytes");
    let text = std::str::from_utf8(&sums).map_err(|_| {
        warn!("install: SHA256SUMS.txt is not UTF-8");
        InstallFail::NoLine
    })?;
    let want = sums_line(text, &name)?;

    let part = dir.join(format!("{name}.part"));
    let folder = |e: std::io::Error| {
        warn!("install: writing in {} failed: {e}", dir.display());
        InstallFail::Folder
    };
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&part).map_err(folder)?;
    let mut sha = Sha::new()?;
    let n = download(&release_path(v, &name), SETUP_CAP, |b| {
        sha.add(b)?;
        file.write_all(b).map_err(folder)
    })?;
    file.sync_all().map_err(folder)?;
    drop(file);
    let got = sha.finish()?;
    if got != want {
        warn!("install: the setup's SHA-256 {} is not the checksum file's {}", hex(&got), hex(&want));
        return Err(InstallFail::Mismatch);
    }
    info!("install: setup downloaded, {n} bytes, its SHA-256 matches ({})", hex(&got));
    let exe = dir.join(&name);
    std::fs::rename(&part, &exe).map_err(folder)?;

    let (Some(sid), Some(schtasks_path)) = (user_sid(), schtasks_exe()) else { return Err(InstallFail::Task) };
    let xml = dir.join(XML);
    std::fs::write(&xml, task_xml(&sid, &exe, &dir.join(LOG), &schtasks_path)).map_err(folder)?;
    if !schtasks(&["/create".as_ref(), "/tn".as_ref(), TASK.as_ref(), "/xml".as_ref(), xml.as_os_str(), "/f".as_ref()]) {
        return Err(InstallFail::Task);
    }
    set_phase(Phase::Installing(v));
    if !schtasks(&["/run".as_ref(), "/tn".as_ref(), TASK.as_ref()]) {
        return Err(InstallFail::Task);
    }
    info!("install: task {TASK:?} registered and started, waiting for the setup");

    let t0 = Instant::now();
    while !running(&exe) {
        if t0.elapsed() > START_WAIT {
            warn!("install: the setup did not start within {} s", START_WAIT.as_secs());
            log_task_state();
            return Err(InstallFail::Task);
        }
        std::thread::sleep(POLL);
    }
    info!("install: the setup runs; it stops des-k, installs, and starts des-k again");
    g.post(Msg::Started);
    let t1 = Instant::now();
    loop {
        std::thread::sleep(POLL);
        if !running(&exe) {
            warn!("install: the setup ended and this des-k still runs, so it did not install (setup.log says why)");
            return Err(InstallFail::SetupStopped);
        }
        if t1.elapsed() > SLOW {
            warn!("install: the setup still runs after {} min, given up on", SLOW.as_secs() / 60);
            return Err(InstallFail::Slow);
        }
    }
}

/// At start, main thread, after `tray::init`: the setup des-k ran last, if any, and whether
/// this des-k is its version. Its exe becomes `.done` so the balloon shows once (renaming works
/// while the setup still runs).
pub fn outcome() -> Outcome {
    let (true, Some(dir), Some(ours)) = (can_install(), update_dir(), update::ours()) else { return Outcome::Nothing };
    if !is_plain_dir(&dir) {
        return Outcome::Nothing; // no update folder yet, or a link, never read
    }
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("install: {} not read: {e}", dir.display());
            return Outcome::Nothing;
        }
    };
    let Some((v, path)) = entries.flatten().find_map(|e| setup_version(&e.file_name().to_string_lossy()).map(|v| (v, e.path()))) else { return Outcome::Nothing };
    let o = rule(Some(v), ours);
    info!("install: found the setup for {v}, this des-k is {ours}: {o:?}");
    let mut done = path.clone().into_os_string();
    done.push(".done");
    if let Err(e) = std::fs::rename(&path, &done) {
        warn!("install: {} not renamed, so its balloon may show again: {e}", path.display());
    }
    o
}

/// At start, after `outcome`: a short thread deletes what the last install left, the setup's
/// exe once the setup has let go of it (every 1 s, up to 2 min), then the XML and the task (AC-10).
pub fn cleanup() {
    if !can_install() {
        return;
    }
    CLEANING.store(true, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("install cleanup".into()).spawn(|| {
        if let Some(dir) = update_dir().filter(|d| is_plain_dir(d)) {
            sweep(&dir);
        } else if !drop_task() {
            warn!("install: the {TASK:?} task not deleted, the next start tries again");
        }
        CLEANING.store(false, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        warn!("install: cleanup thread not started: {e}");
        CLEANING.store(false, Ordering::SeqCst);
    }
}

fn sweep(dir: &Path) {
    let leftover = |e: &std::fs::DirEntry| e.file_name() != LOG && e.file_name() != XML && e.file_type().is_ok_and(|t| t.is_file());
    let t0 = Instant::now();
    loop {
        let mut left = 0;
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(leftover) {
            if std::fs::remove_file(e.path()).is_err() {
                left += 1; // the setup still runs from it
            }
        }
        if left == 0 {
            break;
        }
        if t0.elapsed() > Duration::from_secs(120) {
            return warn!("install: the setup still holds its files after 2 min, the next start tries again");
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let xml = dir.join(XML);
    let had = xml.exists() || task_exists();
    if let Err(e) = remove(&xml) {
        warn!("install: {} not deleted: {e}", xml.display());
    }
    if !drop_task() {
        warn!("install: the {TASK:?} task not deleted, the next start tries again");
    } else if had {
        info!("install: the last update's files and task are deleted");
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_macros)] // live_release prints what it checked (spec 0019 exception)
mod tests {
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
}
