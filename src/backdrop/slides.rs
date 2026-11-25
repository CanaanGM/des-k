// The slideshows (spec 0028): each screen's workspace's place, listing a folder or set in
// name order, shuffle, the next picture and its counted seconds, and the notes and current
// picture the settings page and the overview read.
use super::*;
use super::paint::PAINTS;

/// A slideshow: one screen's workspace (spec 0028 AC-5).
pub(super) type ShowKey = (String, usize);

/// One slideshow's place (spec 0028). Pictures are paths relative to the wallpapers folder.
pub(super) struct Show {
    pub(super) source: Source,          // a Folder or a Set
    pub(super) current: Option<String>, // None: nothing to show yet, or nothing it can show (AC-8)
    pub(super) next: Option<String>,    // decoded ahead while its workspace shows (AC-7); None: one picture
    pub(super) bag: Vec<String>,        // shuffle's rest of this round (AC-3)
    seconds: u32,            // counted while its picture showed (AC-4)
}

impl Show {
    fn new(source: Source) -> Show {
        Show { source, current: None, next: None, bag: Vec::new(), seconds: 0 }
    }
}

/// The slideshows (spec 0028).
impl Backdrop {
    /// Keeps each slideshow whose value is the same, starts one whose value is new or changed,
    /// and drops one whose screen is in the wish without it. A screen missing from the wish
    /// keeps its slideshows until `forget_gone` (AC-9).
    pub(super) fn sync_shows(&mut self) {
        let (mut keep, mut fresh, mut renext) = (HashSet::new(), Vec::new(), Vec::new());
        for w in &self.want {
            for (i, f) in w.files.iter().enumerate() {
                let Some(src @ (Source::Folder(_) | Source::Set(_))) = f else { continue };
                let key = (w.id.clone(), i);
                keep.insert(key.clone());
                match self.shows.get_mut(&key) {
                    Some(s) if s.source == *src => {}
                    // A set edit keeps the picture showing now and its count when it is still in the set.
                    Some(s) if matches!((&s.source, src), (Source::Set(_), Source::Set(_))) && s.current.as_ref().is_some_and(|c| src.holds(c)) => {
                        s.source = src.clone();
                        if !s.next.as_ref().is_some_and(|n| src.holds(n)) {
                            renext.push(key);
                        }
                    }
                    _ => {
                        self.shows.insert(key.clone(), Show::new(src.clone()));
                        fresh.push(key);
                    }
                }
            }
        }
        let present: HashSet<&str> = self.want.iter().map(|w| w.id.as_str()).collect();
        let dropped: Vec<ShowKey> = self.shows.keys().filter(|k| present.contains(k.0.as_str()) && !keep.contains(*k)).cloned().collect();
        for k in &dropped {
            self.drop_show(k);
        }
        for k in &fresh {
            self.start(k);
        }
        for k in &renext {
            self.renext(k);
        }
    }

    pub(super) fn drop_show(&mut self, key: &ShowKey) {
        self.shows.remove(key);
        publish(key, None);
    }

    /// Drops the slideshows of a screen missing from the wish `GONE_CHECKS` checks in a row: one
    /// gone longer starts over when it comes back (AC-9).
    fn forget_gone(&mut self) {
        let present: HashSet<&str> = self.want.iter().map(|w| w.id.as_str()).collect();
        let away: HashSet<String> = self.shows.keys().filter(|k| !present.contains(k.0.as_str())).map(|k| k.0.clone()).collect();
        self.gone.retain(|id, _| away.contains(id));
        for id in away {
            let checks = self.gone.entry(id.clone()).or_insert(0);
            *checks += 1;
            if *checks >= GONE_CHECKS {
                self.gone.remove(&id);
                let keys: Vec<ShowKey> = self.shows.keys().filter(|k| k.0 == id).cloned().collect();
                for k in &keys {
                    self.drop_show(k);
                }
            }
        }
    }

    /// The slideshow's pictures that can show now, in AC-3's order: its folder's top level
    /// pictures or its set's files, less those that failed as they are now (AC-8). Logs a
    /// change in which ones can't be read, by count, never by name (AC-12).
    fn readable(&mut self, key: &ShowKey) -> Result<Vec<String>, Problem> {
        let Some(source) = self.shows.get(key).map(|s| s.source.clone()) else { return Err(Problem::Missing) };
        let listed = list(&self.folder, &source)?;
        let (ok, bad): (Vec<_>, Vec<_>) = listed.into_iter().partition(|(f, stamp)| self.bad.get(f) != Some(stamp));
        let bad: Vec<String> = bad.into_iter().map(|(f, _)| f).collect();
        let name = source.key();
        if self.skipped.get(&name) != Some(&bad) {
            let n = bad.len();
            if n > 0 {
                let what = match (&source, n) {
                    (Source::Set(_), _) => "of its pictures",
                    (_, 1) => "picture in its folder",
                    _ => "pictures in its folder",
                };
                warn!("backdrop: wallpaper.ws{}: {n} {what} can't be read, skipped", key.1 + 1);
            }
            self.skipped.insert(name, bad);
        }
        match ok.is_empty() {
            true => Err(Problem::Unreadable),
            false => Ok(ok.into_iter().map(|(f, _)| f).collect()),
        }
    }

    /// Starts a slideshow over (AC-9): its first picture by name, or the first of a new random
    /// order with shuffle. One with nothing to show shows your Windows wallpaper (AC-8).
    fn start(&mut self, key: &ShowKey) {
        let names = self.readable(key);
        let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
        let Some(show) = self.shows.get_mut(key) else { return };
        (show.current, show.next, show.seconds) = (None, None, 0);
        show.bag.clear();
        let problem = match names {
            Ok(names) => {
                show.current = if shuffle { draw(&mut show.bag, &names, None, rng) } else { names.first().cloned() };
                show.next = upcoming(&names, show.current.as_deref(), shuffle, &mut show.bag, rng);
                None
            }
            Err(p) => Some(p),
        };
        publish(key, show.current.as_ref());
        note(key.1, &show.source, problem);
    }

    /// Picks the slideshow's next picture again, listing its pictures afresh (AC-8).
    fn renext(&mut self, key: &ShowKey) {
        let names = self.readable(key);
        let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
        let Some(show) = self.shows.get_mut(key) else { return };
        show.next = names.ok().and_then(|n| upcoming(&n, show.current.as_deref(), shuffle, &mut show.bag, rng));
    }

    /// The 1 s check's part (AC-4): one second for each screen's shown slideshow while its
    /// window shows its picture, and a change once its interval is up and the next picture is
    /// ready (AC-7). A slideshow with nothing to show looks again, so a folder filled later
    /// shows within 2 s (AC-8).
    pub(super) fn slides(&mut self) {
        if self.shows.is_empty() {
            return; // AC-13
        }
        self.forget_gone();
        let idle: Vec<ShowKey> = self.shows.iter().filter(|(_, s)| s.current.is_none()).map(|(k, _)| k.clone()).collect();
        let mut moved = !idle.is_empty();
        for k in &idle {
            self.start(k);
        }
        for j in 0..self.want.len().min(self.screens.len()) {
            let w = &self.want[j];
            let (key, size) = ((w.id.clone(), w.shown), size(&w.rect));
            let limit = u32::from(w.intervals.get(w.shown).copied().unwrap_or(30)) * 60;
            let (hwnd, fading) = (self.screens[j].hwnd.0 as isize, self.screens[j].fade.is_some());
            let Some(show) = self.shows.get(&key) else { continue };
            let Some(current) = &show.current else { continue };
            let painted = PAINTS.with_borrow(|p| p.get(&hwnd).map(|p| p.pic.clone()));
            let decoded = |f: &String| self.pictures.get(&(f.clone(), size)).map(|(_, d)| d.clone());
            let on = self.status == Status::Shown && painted.zip(decoded(current)).is_some_and(|(a, b)| Rc::ptr_eq(&a, &b));
            if !on {
                continue; // not drawn: can't draw, or its picture is still decoding
            }
            let ready = show.next.as_ref().map(|n| decoded(n).is_some());
            let show = self.shows.get_mut(&key).expect("read above");
            show.seconds += 1;
            // A change due during a fade on that screen waits for its end.
            if show.seconds < limit || fading {
                continue;
            }
            match ready {
                Some(true) => {
                    (show.current, show.seconds) = (show.next.take(), 0);
                    publish(&key, show.current.as_ref());
                    self.stepped.insert(hwnd);
                    self.renext(&key);
                    moved = true;
                }
                Some(false) => {} // it changes as soon as the next picture is decoded (AC-7)
                None => {
                    // One picture: look for more each interval.
                    self.renext(&key);
                    if let Some(show) = self.shows.get_mut(&key).filter(|s| s.next.is_none()) {
                        show.seconds = 0;
                    }
                    moved = true;
                }
            }
        }
        if moved {
            self.prune();
            self.restamp(false);
            self.queue_decode();
            self.show();
        }
    }

    /// `file` can't be decoded as it is now: a slideshow showing it moves to the following
    /// picture, and one about to show it picks another (AC-8).
    pub(super) fn slide_failed(&mut self, file: &str) {
        let keys: Vec<ShowKey> = self.shows.iter().filter(|(_, s)| s.current.as_deref() == Some(file) || s.next.as_deref() == Some(file)).map(|(k, _)| k.clone()).collect();
        for key in &keys {
            let names = self.readable(key);
            let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
            let Some(show) = self.shows.get_mut(key) else { continue };
            if show.current.as_deref() != Some(file) {
                show.next = names.ok().and_then(|n| upcoming(&n, show.current.as_deref(), shuffle, &mut show.bag, rng));
                continue;
            }
            show.seconds = 0;
            let problem = match names {
                Ok(names) => {
                    show.current = if shuffle { draw(&mut show.bag, &names, Some(file), rng) } else { after(&names, file) };
                    show.next = upcoming(&names, show.current.as_deref(), shuffle, &mut show.bag, rng);
                    None
                }
                Err(p) => {
                    (show.current, show.next) = (None, None);
                    Some(p)
                }
            };
            publish(key, show.current.as_ref());
            note(key.1, &show.source, problem);
        }
        if !keys.is_empty() {
            self.prune();
            self.restamp(false);
        }
    }
}

/// Sets slideshow `key`'s picture now for the overview and the settings page.
fn publish(key: &ShowKey, current: Option<&String>) {
    let mut all = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    let changed = match current {
        Some(c) => all.insert(key.clone(), c.clone()).as_ref() != Some(c),
        None => all.remove(key).is_some(),
    };
    if changed {
        CURRENT_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Files why slideshow `source` of workspace `ws` (0 based) can't show, for its card row, and
/// logs it once per change by key, never by name (AC-8, AC-12). Screens showing the same value
/// share one note, so the line is not repeated per screen.
fn note(ws: usize, source: &Source, problem: Option<Problem>) {
    let name = source.key();
    if crate::wallpaper::problem(&name) == problem {
        return;
    }
    let ws = ws + 1;
    let set = matches!(source, Source::Set(_));
    match (problem, set) {
        (None, _) => {}
        (Some(Problem::Missing), false) => warn!("backdrop: the folder for wallpaper.ws{ws} is not in the wallpapers folder, that workspace shows your Windows wallpaper"),
        (Some(Problem::Missing), true) => warn!("backdrop: none of the pictures for wallpaper.ws{ws} are in the wallpapers folder, that workspace shows your Windows wallpaper"),
        (Some(Problem::Unreadable), true) => warn!("backdrop: none of the pictures for wallpaper.ws{ws} can be read, that workspace shows your Windows wallpaper"),
        (Some(p), _) => warn!("backdrop: the folder for wallpaper.ws{ws} has {}, that workspace shows your Windows wallpaper", match p {
            Problem::Empty => "no pictures in it",
            _ => "no picture that can be read",
        }),
    }
    crate::wallpaper::set_problem(&name, problem);
}

/// A slideshow's pictures as paths relative to the wallpapers folder, with their stamps, in
/// AC-3's order: a folder's top level files with a picture's name, or a set's names that are
/// files there now. Lists afresh each time, so pictures added or removed count (AC-8).
fn list(folder: &std::path::Path, source: &Source) -> Result<Vec<(String, Stamp)>, Problem> {
    let mut out: Vec<(String, Stamp)> = match source {
        Source::Picture(_) => Vec::new(),
        Source::Folder(f) => {
            let dir = std::fs::read_dir(folder.join(f)).map_err(|_| Problem::Missing)?;
            dir.flatten()
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok().filter(|n| crate::settings::parse::is_picture_name(n))?;
                    let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
                    Some((format!("{f}\\{name}"), meta.modified().ok()))
                })
                .collect()
        }
        Source::Set(names) => names
            .iter()
            .filter_map(|n| {
                let meta = std::fs::metadata(folder.join(n)).ok().filter(std::fs::Metadata::is_file)?;
                Some((n.clone(), meta.modified().ok()))
            })
            .collect(),
    };
    if out.is_empty() {
        return Err(if matches!(source, Source::Folder(_)) { Problem::Empty } else { Problem::Missing });
    }
    out.sort_by(|a, b| name_order(&a.0, &b.0));
    Ok(out)
}

/// The picture after `current` in AC-3's order among `names` (sorted that way), wrapping. A
/// `current` no longer there still has its place by name.
fn after(names: &[String], current: &str) -> Option<String> {
    names.iter().find(|n| name_order(n, current).is_gt()).or(names.first()).cloned()
}

/// The picture to show after `current`, or None when there is no other (AC-7).
fn upcoming(names: &[String], current: Option<&str>, shuffle: bool, bag: &mut Vec<String>, rng: &mut u64) -> Option<String> {
    let current = current?;
    if !names.iter().any(|n| n != current) {
        return None;
    }
    if !shuffle {
        return after(names, current);
    }
    match draw(bag, names, Some(current), rng)? {
        n if n == current => draw(bag, names, Some(current), rng),
        n => Some(n),
    }
}

/// Shuffle's next picture (AC-3): the rest of this round, skipping pictures gone since; an
/// empty round refills with a new random order whose first isn't `current`.
fn draw(bag: &mut Vec<String>, names: &[String], current: Option<&str>, rng: &mut u64) -> Option<String> {
    bag.retain(|n| names.contains(n));
    if bag.is_empty() {
        *bag = names.to_vec();
        for i in (1..bag.len()).rev() {
            bag.swap(i, (xorshift(rng) % (i as u64 + 1)) as usize);
        }
        if bag.len() >= 2 && current.is_some_and(|c| bag[0] == c) {
            let j = 1 + (xorshift(rng) % (bag.len() as u64 - 1)) as usize;
            bag.swap(0, j);
        }
    }
    (!bag.is_empty()).then(|| bag.remove(0))
}

/// A small random number generator: plenty for an order of pictures, and no crate.
fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[cfg(test)]
mod tests;
