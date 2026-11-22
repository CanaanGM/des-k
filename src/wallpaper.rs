// Each screen's wallpaper as the overview's tiles show it (spec 0017 AC-OVR-3). Explorer's
// IDesktopWallpaper is read on a thread of its own, so a hung or restarting explorer never holds
// up the overview; the picture is decoded at tile size, fitted as Windows fits it, and kept for
// des-k's life, keyed per screen by the file, its time and the fit. Touches no `ws` state and
// no window: the overview asks what is cached and polls for news. It also decodes each
// workspace's own picture at tile size, fitted as des-k fits it (spec 0026 AC-9).
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use windows::Win32::{
    Foundation::RECT,
    Graphics::GdiPlus as gp,
    System::{
        Com::{CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize},
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    },
    UI::Shell::*,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::settings::Fit;
use crate::ui::Picture;

/// What a tile shows behind its windows.
#[derive(Clone)]
pub(crate) enum Face {
    Picture(Arc<Picture>),
    Color(u32),  // 0xRRGGBB: a solid color desktop, a slideshow, Spotlight, or a read that failed
    Card,        // even the background color could not be read
}

/// The file, its last write time and Windows' fit, plus the tile size it was decoded at.
type Key = (String, u64, i32, (i32, i32));

struct Entry {
    screen: RECT,
    size: (i32, i32), // the tiles' size it was read for: the overview's and the settings page's differ
    key: Option<Key>, // None: not a picture
    face: Face,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// A workspace's picture at tile size (spec 0026 AC-9): its file in the wallpapers folder, the
/// file's time, des-k's fit, the bar color, the screen's size and the tile's size. Undimmed:
/// the tile dims it when painted (AC-18).
type WsKey = (String, u64, i32, u32, (i32, i32), (i32, i32));

// None: that decode failed, so the tile shows the screen's face until the file changes.
static PICTURES: Mutex<Vec<(WsKey, Option<Arc<Picture>>)>> = Mutex::new(Vec::new());
static BUSY: AtomicBool = AtomicBool::new(false);
static FRESH: AtomicBool = AtomicBool::new(false);

/// Why one of your pictures can't show (spec 0026 AC-8).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Problem {
    Missing,     // no such file or subfolder in the wallpapers folder; for a set, none of its pictures
    Undecodable, // there, but not a picture GDI+ can read
    Empty,       // a subfolder with no picture in it (spec 0028 AC-8)
    Unreadable,  // a slideshow none of whose pictures can be read
}

impl Problem {
    /// The reason in your words, for the log line and the settings page row.
    pub(crate) fn why(self) -> &'static str {
        match self {
            Problem::Missing => "not in the folder",
            Problem::Undecodable => "can't be read as a picture",
            Problem::Empty => "no pictures in it",
            Problem::Unreadable => "no picture in it can be read",
        }
    }
}

// By a value's name in the wallpapers folder (a slideshow's by `backdrop::Source::key`); set by
// every decode of a picture or listing of a slideshow, cleared by its success.
static PROBLEMS: Mutex<BTreeMap<String, Problem>> = Mutex::new(BTreeMap::new());

/// Records how the last decode of `file` went: `None` is a success.
pub(crate) fn set_problem(file: &str, problem: Option<Problem>) {
    let mut all = PROBLEMS.lock().unwrap_or_else(|e| e.into_inner());
    match problem {
        Some(p) => all.insert(file.to_string(), p),
        None => all.remove(file),
    };
}

/// Why `file` can't show, if its last decode failed.
pub(crate) fn problem(file: &str) -> Option<Problem> {
    PROBLEMS.lock().unwrap_or_else(|e| e.into_inner()).get(file).copied()
}

/// What `screen`'s tiles show now, if it was ever read and its tiles are still `size`.
pub(crate) fn cached(screen: RECT, size: (i32, i32)) -> Option<Face> {
    // Never held across a call to explorer, so this never waits long.
    let cache = CACHE.lock().ok()?;
    cache.iter().find(|e| e.screen == screen && e.size == size).map(|e| e.face.clone())
}

/// Workspace picture `file` as a `size` tile on `screen` shows it, once decoded. While a new
/// decode is under way (the file, the fit or the bar color changed) the last one still serves.
pub(crate) fn cached_ws(screen: RECT, file: &str, size: (i32, i32)) -> Option<Arc<Picture>> {
    let all = PICTURES.lock().ok()?;
    let screen = (screen.right - screen.left, screen.bottom - screen.top);
    all.iter().rev().find(|(k, _)| k.0 == file && k.4 == screen && k.5 == size).and_then(|e| e.1.clone())
}

/// Whether a decode of workspace picture `file` for a `size` tile on `screen` with `fit` was
/// tried, even one that failed: the settings page asks for decodes only while this is false.
pub(crate) fn tried_ws(screen: RECT, file: &str, size: (i32, i32), fit: Fit) -> bool {
    let (screen, fit) = ((screen.right - screen.left, screen.bottom - screen.top), crate::backdrop::position(fit).0);
    PICTURES.lock().is_ok_and(|all| all.iter().any(|(k, _)| k.0 == file && k.2 == fit && k.4 == screen && k.5 == size))
}

/// Reads every screen's wallpaper again on its own thread, decoding only what changed. Does
/// nothing while an earlier read still runs (explorer may hang it for good). `screens`: each
/// screen's rect, its tiles' size in pixels, and the pictures its workspaces name, decoded
/// with `fit` (spec 0026 AC-9).
pub(crate) fn check(screens: Vec<(RECT, (i32, i32), Vec<String>)>, fit: Fit) {
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("wallpaper".into()).spawn(move || {
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        let faces: Vec<(RECT, (i32, i32))> = screens.iter().map(|s| (s.0, s.1)).collect();
        if let Err(e) = read(&faces) {
            warn!("wallpaper: not read ({e}), the tiles show the card color");
            for (screen, size) in &faces {
                store(*screen, *size, None, Face::Card);
            }
        }
        read_ws(&screens, fit);
        unsafe { CoUninitialize() };
        BUSY.store(false, Ordering::Release);
    });
    if let Err(e) = spawned {
        warn!("wallpaper: no thread to read it ({e})");
        BUSY.store(false, Ordering::Release);
    }
}

/// True once after a read changed what a tile shows.
pub(crate) fn fresh() -> bool {
    FRESH.swap(false, Ordering::AcqRel)
}

/// True while a read runs.
pub(crate) fn busy() -> bool {
    BUSY.load(Ordering::Acquire)
}

fn store(screen: RECT, size: (i32, i32), key: Option<Key>, face: Face) {
    let Ok(mut cache) = CACHE.lock() else { return };
    cache.retain(|e| e.screen != screen || e.size != size);
    cache.push(Entry { screen, size, key, face });
    FRESH.store(true, Ordering::Release);
}

fn rgb(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
}

/// Your Windows background color (Personalization's solid color behind a wallpaper) as
/// 0xRRGGBB: the bars around a des-k picture (spec 0026 AC-5). Asks explorer, so call it only
/// from a thread that may wait on it, with COM started.
pub(crate) fn background() -> Option<u32> {
    let dw: IDesktopWallpaper = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER) }.ok()?;
    unsafe { dw.GetBackgroundColor() }.ok().map(|c| rgb(c.0))
}

/// Windows Spotlight on the desktop: its picture changes on its own, like a slideshow.
fn spotlight() -> bool {
    let (mut v, mut size) = (0u32, 4u32);
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\DesktopSpotlight\Settings");
    let e = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("EnabledState"), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut size)) };
    e.is_ok() && v != 0
}

/// A string explorer allocated, taken and freed.
fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

fn read(screens: &[(RECT, (i32, i32))]) -> windows::core::Result<()> {
    let dw: IDesktopWallpaper = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_LOCAL_SERVER) }?;
    let bg = unsafe { dw.GetBackgroundColor() }.map(|c| rgb(c.0));
    let fit = unsafe { dw.GetPosition() }?;
    let moving = unsafe { dw.GetStatus() }.is_ok_and(|s| s.0 & DSS_SLIDESHOW.0 != 0) || spotlight();
    let mut monitors = Vec::new();
    for i in 0..unsafe { dw.GetMonitorDevicePathCount() }? {
        let id = take(unsafe { dw.GetMonitorDevicePathAt(i) }?);
        let id = HSTRING::from(id);
        // Fails for a monitor Windows remembers but is not attached.
        let Ok(rect) = (unsafe { dw.GetMonitorRECT(PCWSTR(id.as_ptr())) }) else { continue };
        let path = unsafe { dw.GetWallpaper(PCWSTR(id.as_ptr())) }.map(take).unwrap_or_default();
        monitors.push((rect, path));
    }
    // A spanned picture covers every screen together.
    let all = monitors.iter().map(|m| m.0).reduce(|a, b| RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) });
    let plain = |screen: RECT, size: (i32, i32)| match &bg {
        Ok(c) => store(screen, size, None, Face::Color(*c)),
        Err(_) => store(screen, size, None, Face::Card),
    };
    for &(screen, size) in screens {
        let path = monitors.iter().find(|m| m.0 == screen).map(|m| m.1.clone()).unwrap_or_default();
        if path.is_empty() || moving {
            plain(screen, size);
            continue;
        }
        let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
        let key = (path.clone(), time, fit.0, size);
        let same = CACHE.lock().is_ok_and(|c| c.iter().any(|e| e.screen == screen && e.size == size && e.key.as_ref() == Some(&key)));
        if same {
            continue;
        }
        let back = bg.as_ref().copied().unwrap_or(0);
        match decode(&path, screen, all.unwrap_or(screen), fit, size, back) {
            Some(p) => store(screen, size, Some(key), Face::Picture(Arc::new(p))),
            None => {
                warn!("wallpaper: {} not decoded, its tiles show the background color", std::path::Path::new(&path).file_name().unwrap_or_default().display());
                plain(screen, size);
            }
        }
    }
    Ok(())
}

/// Decodes each workspace picture the tiles name that is not decoded as it is now, then drops
/// the ones no tile of these sizes names any more; other sizes belong to the overview or the
/// settings page, whichever did not call. A failure only marks the file (AC-8): the backdrop decodes
/// the same files and logs the one line.
fn read_ws(screens: &[(RECT, (i32, i32), Vec<String>)], fit: Fit) {
    let folder = crate::ws::data_dir().join("wallpapers");
    let fit = crate::backdrop::position(fit);
    let back = if screens.iter().any(|s| !s.2.is_empty()) { background().unwrap_or(0) } else { 0 };
    let mut wanted = Vec::new();
    for (screen, size, files) in screens {
        let full = (screen.right - screen.left, screen.bottom - screen.top);
        for file in files {
            let path = folder.join(file);
            let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
            let key = (file.clone(), time, fit.0, back, full, *size);
            let done = PICTURES.lock().is_ok_and(|all| all.iter().any(|(k, _)| *k == key));
            if !done {
                let at = RECT { left: 0, top: 0, right: full.0, bottom: full.1 };
                let pic = decode(&path.to_string_lossy(), at, at, fit, *size, back).map(Arc::new);
                let problem = match pic {
                    Some(_) => None,
                    None if path.is_file() => Some(Problem::Undecodable),
                    None => Some(Problem::Missing),
                };
                set_problem(file, problem);
                if let Ok(mut all) = PICTURES.lock() {
                    // Pushed last, so it wins over an older decode of the same tile.
                    all.push((key.clone(), pic));
                }
                FRESH.store(true, Ordering::Release);
            }
            wanted.push(key);
        }
    }
    let sizes: Vec<(i32, i32)> = screens.iter().map(|s| s.1).collect();
    if let Ok(mut all) = PICTURES.lock() {
        all.retain(|(k, _)| wanted.contains(k) || !sizes.contains(&k.5));
    }
}

/// `path` as `screen` shows it with Windows' `fit`, made `size` pixels: a tile's face, or a
/// workspace's picture for the backdrop (spec 0026). `all` is every screen together, for a
/// spanned picture; `back` shows where the picture does not cover the screen.
pub(crate) fn decode(path: &str, screen: RECT, all: RECT, fit: DESKTOP_WALLPAPER_POSITION, (tw, th): (i32, i32), back: u32) -> Option<Picture> {
    if tw <= 0 || th <= 0 {
        return None;
    }
    let (sw, sh) = ((screen.right - screen.left) as f32, (screen.bottom - screen.top) as f32);
    let f = tw as f32 / sw.max(1.0); // tile pixels per screen pixel
    let mut px = vec![0xFF00_0000 | back; (tw * th) as usize];
    unsafe {
        let mut image = std::ptr::null_mut();
        if gp::GdipLoadImageFromFile(&HSTRING::from(path), &mut image).0 != 0 || image.is_null() {
            return None;
        }
        let (mut iw, mut ih) = (0u32, 0u32);
        gp::GdipGetImageWidth(image, &mut iw);
        gp::GdipGetImageHeight(image, &mut ih);
        let (iw, ih) = (iw as f32, ih as f32);
        let mut target = std::ptr::null_mut();
        gp::GdipCreateBitmapFromScan0(tw, th, tw * 4, 0x0026_200A, Some(px.as_mut_ptr().cast_const().cast()), &mut target); // 32 bpp ARGB
        let mut g = std::ptr::null_mut();
        if !target.is_null() {
            gp::GdipGetImageGraphicsContext(target.cast(), &mut g);
        }
        let ok = !g.is_null() && iw > 0.0 && ih > 0.0;
        if ok {
            gp::GdipSetInterpolationMode(g, gp::InterpolationModeHighQualityBicubic);
            gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHalf);
            // Where the picture lands, in screen pixels from the screen's top left.
            let centered = |k: f32, (ox, oy, w, h): (f32, f32, f32, f32)| (ox + (w - iw * k) / 2.0, oy + (h - ih * k) / 2.0, iw * k, ih * k);
            let one = (0.0, 0.0, sw, sh);
            let places: Vec<(f32, f32, f32, f32)> = match fit {
                DWPOS_TILE => {
                    let mut v = Vec::new();
                    let mut y = 0.0;
                    while y < sh {
                        let mut x = 0.0;
                        while x < sw {
                            v.push((x, y, iw, ih));
                            x += iw;
                        }
                        y += ih;
                    }
                    v
                }
                DWPOS_CENTER => vec![centered(1.0, one)],
                DWPOS_STRETCH => vec![one],
                DWPOS_FIT => vec![centered((sw / iw).min(sh / ih), one)],
                DWPOS_SPAN => {
                    let (aw, ah) = ((all.right - all.left) as f32, (all.bottom - all.top) as f32);
                    let span = (((all.left - screen.left) as f32), ((all.top - screen.top) as f32), aw, ah);
                    vec![centered((aw / iw).max(ah / ih), span)]
                }
                _ => vec![centered((sw / iw).max(sh / ih), one)], // fill, Windows' default
            };
            for (x, y, w, h) in places {
                gp::GdipDrawImageRect(g, image, x * f, y * f, w * f, h * f);
            }
            gp::GdipDeleteGraphics(g);
        }
        if !target.is_null() {
            gp::GdipDisposeImage(target.cast());
        }
        gp::GdipDisposeImage(image);
        ok.then_some(Picture { w: tw, h: th, px })
    }
}

/// A decoded picture for a test: workspace picture `file`, or with None the screen's face.
#[cfg(test)]
pub(crate) fn put_for_test(screen: RECT, file: Option<&str>, size: (i32, i32), pic: Picture) {
    match file {
        Some(f) => {
            let full = (screen.right - screen.left, screen.bottom - screen.top);
            PICTURES.lock().unwrap().push(((f.into(), 0, 0, 0, full, size), Some(Arc::new(pic))));
        }
        None => store(screen, size, None, Face::Picture(Arc::new(pic))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(file: &str, time: u64, screen: (i32, i32), tile: (i32, i32), w: i32) {
        let pic = Some(Arc::new(Picture { w, h: 1, px: Vec::new() }));
        PICTURES.lock().unwrap().push(((file.into(), time, 0, 0, screen, tile), pic));
    }

    // A tile must never show a picture fitted for another screen's shape, and after you swap
    // the file (or the fit) the newer decode must win, while the old one still serves until then.
    #[test]
    fn a_tile_takes_the_newest_picture_made_for_its_screen() {
        let screen = RECT { left: -1920, top: 0, right: 0, bottom: 1080 };
        assert!(cached_ws(screen, "cache-test.jpg", (192, 108)).is_none());
        put("cache-test.jpg", 1, (2560, 1440), (192, 108), 1);
        assert!(cached_ws(screen, "cache-test.jpg", (192, 108)).is_none(), "made for another screen size");
        put("cache-test.jpg", 1, (1920, 1080), (192, 108), 2);
        assert_eq!(cached_ws(screen, "cache-test.jpg", (192, 108)).map(|p| p.w), Some(2));
        put("cache-test.jpg", 2, (1920, 1080), (192, 108), 3);
        assert_eq!(cached_ws(screen, "cache-test.jpg", (192, 108)).map(|p| p.w), Some(3), "the newer decode wins");
        assert!(cached_ws(screen, "cache-test.jpg", (160, 90)).is_none(), "another tile size");
    }
}
