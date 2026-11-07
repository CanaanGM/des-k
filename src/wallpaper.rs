// Each screen's wallpaper as the overview's tiles show it (spec 0017 AC-OVR-3). Explorer's
// IDesktopWallpaper is read on a thread of its own, so a hung or restarting explorer never holds
// up the overview; the picture is decoded at tile size, fitted as Windows fits it, and kept for
// des-k's life, keyed per screen by the file, its time and the fit. Touches no `ws` state and
// no window: the overview asks what is cached and polls for news.
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
    key: Option<Key>, // None: not a picture
    face: Face,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static BUSY: AtomicBool = AtomicBool::new(false);
static FRESH: AtomicBool = AtomicBool::new(false);

/// What `screen`'s tiles show now, if it was ever read and its tiles are still `size`.
pub(crate) fn cached(screen: RECT, size: (i32, i32)) -> Option<Face> {
    // Never held across a call to explorer, so this never waits long.
    let cache = CACHE.lock().ok()?;
    let e = cache.iter().find(|e| e.screen == screen)?;
    match (&e.face, &e.key) {
        (Face::Picture(_), Some(k)) if k.3 != size => None,
        _ => Some(e.face.clone()),
    }
}

/// Reads every screen's wallpaper again on its own thread, decoding only what changed. Does
/// nothing while an earlier read still runs (explorer may hang it for good). `screens`: each
/// screen's rect and its tiles' size in pixels.
pub(crate) fn check(screens: Vec<(RECT, (i32, i32))>) {
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("wallpaper".into()).spawn(move || {
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        if let Err(e) = read(&screens) {
            warn!("wallpaper: not read ({e}), the tiles show the card color");
            for (screen, _) in &screens {
                store(*screen, None, Face::Card);
            }
        }
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

fn store(screen: RECT, key: Option<Key>, face: Face) {
    let Ok(mut cache) = CACHE.lock() else { return };
    cache.retain(|e| e.screen != screen);
    cache.push(Entry { screen, key, face });
    FRESH.store(true, Ordering::Release);
}

fn rgb(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
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
    let plain = |screen: RECT| match &bg {
        Ok(c) => store(screen, None, Face::Color(*c)),
        Err(_) => store(screen, None, Face::Card),
    };
    for &(screen, size) in screens {
        let path = monitors.iter().find(|m| m.0 == screen).map(|m| m.1.clone()).unwrap_or_default();
        if path.is_empty() || moving {
            plain(screen);
            continue;
        }
        let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
        let key = (path.clone(), time, fit.0, size);
        let same = CACHE.lock().is_ok_and(|c| c.iter().any(|e| e.screen == screen && e.key.as_ref() == Some(&key)));
        if same {
            continue;
        }
        let back = bg.as_ref().copied().unwrap_or(0);
        match decode(&path, screen, all.unwrap_or(screen), fit, size, back) {
            Some(p) => store(screen, Some(key), Face::Picture(Arc::new(p))),
            None => {
                warn!("wallpaper: {} not decoded, its tiles show the background color", std::path::Path::new(&path).file_name().unwrap_or_default().display());
                plain(screen);
            }
        }
    }
    Ok(())
}

/// `path` as `screen` shows it with Windows' `fit`, made `size` pixels: a tile's face. `all`
/// is every screen together, for a spanned picture; `back` shows where the picture does not
/// cover the screen.
fn decode(path: &str, screen: RECT, all: RECT, fit: DESKTOP_WALLPAPER_POSITION, (tw, th): (i32, i32), back: u32) -> Option<Picture> {
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
