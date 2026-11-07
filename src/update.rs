// Check for updates, only when you click it in the tray (spec 0015): one WinHTTP request on a
// worker thread asks GitHub where releases/latest redirects, reads the version from that one
// header, and posts the answer to the tray window. Downloads nothing, runs nothing: the only
// thing des-k ever opens because of it is a release page URL it builds from three numbers.
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Networking::WinHttp::*,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};
use windows::core::{HSTRING, PCWSTR, w};

/// Window message to the tray window (a thread message is lost while a menu is open), wParam
/// and lParam from `pack`. Its own id: main's loop matches thread messages by id alone.
pub const WM_UPDATE: u32 = WM_APP + 13;

pub(crate) const HOST: PCWSTR = w!("github.com");
const LATEST: PCWSTR = w!("/CanaanGM/des-k/releases/latest");
const RELEASES: &str = "https://github.com/CanaanGM/des-k/releases";
const TAG: &str = "https://github.com/CanaanGM/des-k/releases/tag/";
pub(crate) const TIMEOUT_MS: i32 = 10_000;

/// A check or an install (spec 0018) is in flight: set by `check` or `install::start`, cleared
/// by that worker's guard on every exit (AC-6).
pub(crate) static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u16, pub u16, pub u16);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    NoConnection,
    Timeout,
    Certificate,
    ProxyLogin,
    Status(u16),
    NoRelease,
    Moved,
    BadTag,
    OurVersion,
    Crashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Newer(Version),
    /// GitHub's newest, not newer than the running des-k.
    Newest(Version),
    Failed(Fail),
}

impl Fail {
    /// The balloon's words; the details went to the log from the worker.
    pub fn words(self) -> String {
        self.reason() + " des-k carries on as before."
    }

    /// What went wrong, one sentence (the install's balloon reuses it, spec 0018).
    pub fn reason(self) -> String {
        match self {
            Fail::NoConnection => "No connection to GitHub.".to_string(),
            Fail::Timeout => "GitHub did not answer in time.".to_string(),
            Fail::Certificate => "The secure connection to GitHub failed.".to_string(),
            Fail::ProxyLogin => "Your proxy asks for a login.".to_string(),
            Fail::Status(n) => format!("GitHub answered with HTTP {n}."),
            Fail::NoRelease => "No des-k release is published yet.".to_string(),
            Fail::Moved => "The des-k page on GitHub has moved.".to_string(),
            Fail::BadTag => "The newest release has an unexpected name (the log has it).".to_string(),
            Fail::OurVersion => "This des-k's own version could not be read.".to_string(),
            Fail::Crashed => "The check stopped with an error (the log has it).".to_string(),
        }
    }

    /// The code `pack` carries, and the number beside it (the status for `Status`).
    pub(crate) fn code(self) -> (usize, u16) {
        match self {
            Fail::NoConnection => (0, 0),
            Fail::Timeout => (1, 0),
            Fail::Certificate => (2, 0),
            Fail::ProxyLogin => (3, 0),
            Fail::Status(n) => (4, n),
            Fail::NoRelease => (5, 0),
            Fail::Moved => (6, 0),
            Fail::BadTag => (7, 0),
            Fail::OurVersion => (8, 0),
            Fail::Crashed => (9, 0),
        }
    }

    pub(crate) fn from_code(code: usize, n: u16) -> Option<Fail> {
        Some(match code {
            0 => Fail::NoConnection,
            1 => Fail::Timeout,
            2 => Fail::Certificate,
            3 => Fail::ProxyLogin,
            4 => Fail::Status(n),
            5 => Fail::NoRelease,
            6 => Fail::Moved,
            7 => Fail::BadTag,
            8 => Fail::OurVersion,
            9 => Fail::Crashed,
            _ => return None,
        })
    }
}

/// Exactly `X.Y.Z`, each part plain digits that fit 16 bits.
pub fn parse_version(s: &str) -> Option<Version> {
    let mut parts = s.split('.').map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse::<u16>().ok() } else { None });
    let v = Version(parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// The redirect's target must be this repo's tag page and nothing after the version, so a
/// changed or odd answer fails loudly instead of naming a wrong version (AC-4, AC-7).
pub fn parse_location(loc: &str) -> Result<Version, Fail> {
    if loc == RELEASES {
        return Err(Fail::NoRelease);
    }
    let tag = loc.strip_prefix(TAG).ok_or(Fail::Moved)?;
    tag.strip_prefix('v').and_then(parse_version).ok_or(Fail::BadTag)
}

/// From the HTTP status and `Location` to GitHub's newest version.
fn redirect(status: u16, location: Option<&str>) -> Result<Version, Fail> {
    match (status, location) {
        (301 | 302, Some(loc)) => parse_location(loc),
        (407, _) => Err(Fail::ProxyLogin),
        (n, _) => Err(Fail::Status(n)),
    }
}

pub fn answer(newest: Version, ours: Version) -> Answer {
    if newest > ours { Answer::Newer(newest) } else { Answer::Newest(newest) }
}

/// The running des-k's version; the release job checks it equals the tag.
pub fn ours() -> Option<Version> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

/// Built from the three numbers, never the `Location` text (AC-7).
pub fn page(v: Version) -> String {
    format!("{TAG}v{v}")
}

/// wParam: kind (0 newer, 1 newest, 2 failed) | fail code << 8 | major << 16.
/// lParam: minor << 16 | patch, or the status for `Status`. Fits 32 bit params too.
pub fn pack(a: Answer) -> (WPARAM, LPARAM) {
    let ver = |kind: usize, v: Version| (WPARAM(kind | (v.0 as usize) << 16), LPARAM(((v.1 as u32) << 16 | v.2 as u32) as isize));
    match a {
        Answer::Newer(v) => ver(0, v),
        Answer::Newest(v) => ver(1, v),
        Answer::Failed(f) => {
            let (code, n) = f.code();
            (WPARAM(2 | code << 8), LPARAM(n as isize))
        }
    }
}

pub fn unpack(wp: WPARAM, lp: LPARAM) -> Option<Answer> {
    let l = lp.0 as u32;
    let v = Version((wp.0 >> 16) as u16, (l >> 16) as u16, l as u16);
    match wp.0 & 0xFF {
        0 => Some(Answer::Newer(v)),
        1 => Some(Answer::Newest(v)),
        2 => Fail::from_code((wp.0 >> 8) & 0xFF, l as u16).map(Answer::Failed),
        _ => None,
    }
}

pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// From the tray's menu, main thread. One check at a time (AC-6).
pub fn check(tray: HWND) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        eprintln!("update check: one is already running, this click is ignored");
        return;
    }
    println!("update check: asking GitHub for the newest des-k");
    let hwnd = tray.0 as isize; // an HWND is not Send
    let spawned = std::thread::Builder::new().name("update".into()).spawn(move || {
        let mut guard = Guard { hwnd, posted: false };
        guard.post(run());
    });
    if let Err(e) = spawned {
        eprintln!("update check: thread not started: {e}");
        Guard { hwnd, posted: false }.post(Answer::Failed(Fail::Crashed));
    }
}

/// Clears `RUNNING` and posts exactly one answer on every exit, a panic included.
struct Guard {
    hwnd: isize,
    posted: bool,
}

impl Guard {
    fn post(&mut self, a: Answer) {
        self.posted = true;
        RUNNING.store(false, Ordering::SeqCst);
        let (wp, lp) = pack(a);
        if let Err(e) = unsafe { PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_UPDATE, wp, lp) } {
            eprintln!("update check: answer not passed to the tray: {e}");
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.posted {
            self.post(Answer::Failed(Fail::Crashed));
        }
    }
}

/// The worker: logs every step's detail (AC-8), touches nothing but the log and its message.
fn run() -> Answer {
    let Some(ours) = ours() else {
        eprintln!("update check: this des-k's version {:?} is not X.Y.Z", env!("CARGO_PKG_VERSION"));
        return Answer::Failed(Fail::OurVersion);
    };
    let got = fetch().and_then(|(status, loc)| {
        println!("update check: GitHub answered HTTP {status}, Location {loc:?}");
        redirect(status, loc.as_deref())
    });
    match got {
        Ok(newest) => {
            println!("update check: the newest des-k is {newest}, you have {ours}");
            answer(newest, ours)
        }
        Err(f) => {
            eprintln!("update check failed: {f:?}");
            Answer::Failed(f)
        }
    }
}

/// A WinHTTP handle closed on drop, so every path closes all three.
pub(crate) struct Handle(pub(crate) *mut core::ffi::c_void);

impl Handle {
    pub(crate) fn new(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, Fail> {
        if h.is_null() { Err(fail(windows::core::Error::from_thread(), what)) } else { Ok(Handle(h)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { let _ = WinHttpCloseHandle(self.0); }
    }
}

/// A WinHTTP error into a `Fail`, its code into the log.
pub(crate) fn fail(e: windows::core::Error, what: &str) -> Fail {
    eprintln!("update check: {what} failed: {e}");
    let code = e.code().0 as u32 & 0xFFFF;
    match code {
        ERROR_WINHTTP_TIMEOUT => Fail::Timeout,
        ERROR_WINHTTP_SECURE_CERT_CN_INVALID
        | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
        | ERROR_WINHTTP_SECURE_CERT_REVOKED
        | ERROR_WINHTTP_SECURE_CERT_REV_FAILED
        | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE
        | ERROR_WINHTTP_SECURE_CHANNEL_ERROR
        | ERROR_WINHTTP_SECURE_FAILURE
        | ERROR_WINHTTP_SECURE_FAILURE_PROXY
        | ERROR_WINHTTP_SECURE_INVALID_CA
        | ERROR_WINHTTP_SECURE_INVALID_CERT => Fail::Certificate,
        // Name not resolved, cannot connect, and anything else.
        _ => Fail::NoConnection,
    }
}

/// One GET with redirects off; the body is never read. The HTTP status and `Location`.
fn fetch() -> Result<(u16, Option<String>), Fail> {
    unsafe {
        let agent = HSTRING::from(format!("des-k/{}", env!("CARGO_PKG_VERSION")));
        // Automatic proxy: Windows' and your own proxy settings, plus detection (8.1 and later).
        let session = Handle::new(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "opening WinHTTP")?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS).map_err(|e| fail(e, "setting timeouts"))?;
        let connect = Handle::new(WinHttpConnect(session.0, HOST, INTERNET_DEFAULT_HTTPS_PORT, 0), "connecting")?;
        let request = Handle::new(
            WinHttpOpenRequest(connect.0, w!("GET"), LATEST, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "opening the request",
        )?;
        let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
        WinHttpSetOption(Some(request.0), WINHTTP_OPTION_REDIRECT_POLICY, Some(&never)).map_err(|e| fail(e, "turning redirects off"))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| fail(e, "sending the request"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| fail(e, "receiving the answer"))?;

        let mut status = 0u32;
        let mut len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&raw mut status).cast()), &mut len, std::ptr::null_mut())
            .map_err(|e| fail(e, "reading the status"))?;
        let status = u16::try_from(status).unwrap_or(u16::MAX);

        // First call asks the size in bytes, the second reads it.
        let mut len = 0u32;
        let _ = WinHttpQueryHeaders(request.0, WINHTTP_QUERY_LOCATION, PCWSTR::null(), None, &mut len, std::ptr::null_mut());
        if len == 0 {
            return Ok((status, None)); // no Location header
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        let mut len = (buf.len() * 2) as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_LOCATION, PCWSTR::null(), Some(buf.as_mut_ptr().cast()), &mut len, std::ptr::null_mut())
            .map_err(|e| fail(e, "reading Location"))?;
        buf.truncate(len as usize / 2);
        Ok((status, Some(String::from_utf16_lossy(&buf))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "https://github.com/CanaanGM/des-k/releases/tag/";

    #[test]
    fn location_names_only_this_repos_plain_tag() {
        // Why: the version from this header becomes a balloon and a URL des-k opens; anything
        // but an exact vX.Y.Z tag page of this repo must fail, not name a wrong release.
        assert_eq!(parse_location(&format!("{T}v0.3.1")), Ok(Version(0, 3, 1)));
        assert_eq!(parse_location(&format!("{T}v10.0.65535")), Ok(Version(10, 0, 65535)));
        for bad in ["v0.3", "0.3.1", "v0.3.1-rc1", "v0.3.1/", "v0.3.1?x", "v65536.0.0", "v0..1", "v+1.0.0"] {
            assert_eq!(parse_location(&format!("{T}{bad}")), Err(Fail::BadTag), "{bad}");
        }
        assert_eq!(parse_location("https://github.com/CanaanGM/des-k/releases"), Err(Fail::NoRelease));
        assert_eq!(parse_location("https://github.com/someone/des-k/releases/tag/v1.0.0"), Err(Fail::Moved));
        assert_eq!(parse_location("https://GitHub.com/CanaanGM/des-k/releases/tag/v1.0.0"), Err(Fail::Moved));
        assert_eq!(parse_location("http://github.com/CanaanGM/des-k/releases/tag/v1.0.0"), Err(Fail::Moved));
    }

    #[test]
    fn only_a_redirect_with_location_gives_a_version() {
        // Why: GitHub's normal answer is a redirect; a 200 or an error page means the shape
        // changed, and a proxy login has its own words.
        assert_eq!(redirect(302, Some(&format!("{T}v1.2.3"))), Ok(Version(1, 2, 3)));
        assert_eq!(redirect(301, Some(&format!("{T}v1.2.3"))), Ok(Version(1, 2, 3)));
        assert_eq!(redirect(302, None), Err(Fail::Status(302)));
        assert_eq!(redirect(200, Some(&format!("{T}v1.2.3"))), Err(Fail::Status(200)));
        assert_eq!(redirect(407, None), Err(Fail::ProxyLogin));
        assert_eq!(redirect(404, None), Err(Fail::Status(404)));
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        // Why: as text "0.10.0" sorts before "0.9.9", which would hide a real update.
        assert_eq!(answer(Version(0, 10, 0), Version(0, 9, 9)), Answer::Newer(Version(0, 10, 0)));
        assert_eq!(answer(Version(0, 3, 0), Version(0, 3, 0)), Answer::Newest(Version(0, 3, 0)));
        assert_eq!(answer(Version(0, 2, 9), Version(0, 3, 0)), Answer::Newest(Version(0, 2, 9)));
    }

    #[test]
    fn every_answer_survives_the_message() {
        // Why: the answer crosses threads only inside wParam and lParam; a lossy pack would
        // show the wrong version or the wrong failure.
        let big = Version(65535, 65535, 65535);
        let mut all = vec![Answer::Newer(big), Answer::Newest(big), Answer::Newer(Version(0, 3, 1)), Answer::Newest(Version(0, 0, 0))];
        for f in [
            Fail::NoConnection,
            Fail::Timeout,
            Fail::Certificate,
            Fail::ProxyLogin,
            Fail::Status(407),
            Fail::Status(65535),
            Fail::NoRelease,
            Fail::Moved,
            Fail::BadTag,
            Fail::OurVersion,
            Fail::Crashed,
        ] {
            all.push(Answer::Failed(f));
        }
        for a in all {
            let (wp, lp) = pack(a);
            assert_eq!(unpack(wp, lp), Some(a));
        }
    }

    #[test]
    fn our_own_version_is_readable() {
        // Why: a release whose version the check cannot read would answer every check with a failure.
        assert!(ours().is_some());
    }

    /// One real request to GitHub: `cargo test live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_github_answers_with_a_tag() {
        let (status, loc) = fetch().expect("fetch");
        println!("HTTP {status}, Location {loc:?}");
        let newest = redirect(status, loc.as_deref()).expect("a release tag");
        println!("newest {newest}, ours {:?} -> {:?}", ours(), answer(newest, ours().expect("ours")));
    }

    #[test]
    fn the_page_is_built_from_the_numbers() {
        assert_eq!(page(Version(1, 20, 3)), "https://github.com/CanaanGM/des-k/releases/tag/v1.20.3");
    }
}
