// The slideshow tests: listing, name order, shuffle, a new wish, a missing screen and an
// unreadable picture.
use super::*;

/// A wallpapers folder of its own for one test, filled with empty files (listing reads
/// names only), and removed again when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(test: &str, files: &[&str]) -> Folder {
        let root = std::env::temp_dir().join(format!("des-k-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let path = root.join(f);
            std::fs::create_dir_all(path.parent().expect("inside root")).expect("temp folder");
            if !f.ends_with('\\') {
                std::fs::write(&path, b"x").expect("temp file");
            }
        }
        std::fs::create_dir_all(&root).expect("temp folder");
        Folder(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(list: &[(String, Stamp)]) -> Vec<&str> {
    list.iter().map(|(f, _)| f.as_str()).collect()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn want(id: &str, shown: usize, files: Vec<Option<Source>>) -> Want {
    let n = files.len();
    Want { rect: RECT::default(), id: id.into(), shown, files, intervals: vec![1; n] }
}

// Spec 0028 AC-1, AC-3, AC-8: a folder plays its top level pictures by name with case
// ignored, never a text file or a picture in a folder inside it; a set plays the names
// that are files there now. A missing folder and an empty one get their own notes.
#[test]
fn a_slideshow_lists_only_its_own_pictures_by_name() {
    let dir = Folder::new("list", &["beach\\b.jpg", "beach\\A.png", "beach\\c.JPG", "beach\\notes.txt", "beach\\deep\\d.jpg", "empty\\read me.txt", "x.jpg", "y.png"]);
    let beach = list(&dir.0, &Source::Folder("beach".into())).expect("pictures");
    assert_eq!(names(&beach), ["beach\\A.png", "beach\\b.jpg", "beach\\c.JPG"]);
    assert_eq!(list(&dir.0, &Source::Folder("trips".into())).err(), Some(Problem::Missing));
    assert_eq!(list(&dir.0, &Source::Folder("empty".into())).err(), Some(Problem::Empty));
    let set = list(&dir.0, &Source::Set(strings(&["gone.jpg", "x.jpg", "y.png"]))).expect("two there");
    assert_eq!(names(&set), ["x.jpg", "y.png"]);
    assert_eq!(list(&dir.0, &Source::Set(strings(&["gone.jpg", "lost.png"]))).err(), Some(Problem::Missing));
    // A picture named like the folder but outside it is not one of its pictures.
    assert!(Source::Folder("beach".into()).holds("beach\\b.jpg") && !Source::Folder("bea".into()).holds("beach\\b.jpg"));
}

// AC-3: name order walks on and wraps; a picture removed while it showed keeps its place,
// so the next one is the one after it by name. One picture has nothing to change to.
#[test]
fn name_order_wraps_and_keeps_a_removed_pictures_place() {
    let list = strings(&["a.jpg", "C.jpg", "d.jpg"]);
    let (mut bag, mut rng) = (Vec::new(), 1u64);
    assert_eq!(upcoming(&list, Some("a.jpg"), false, &mut bag, &mut rng).as_deref(), Some("C.jpg"));
    assert_eq!(upcoming(&list, Some("d.jpg"), false, &mut bag, &mut rng).as_deref(), Some("a.jpg"));
    assert_eq!(upcoming(&list, Some("b.jpg"), false, &mut bag, &mut rng).as_deref(), Some("C.jpg"), "b.jpg was removed");
    assert_eq!(upcoming(&strings(&["a.jpg"]), Some("a.jpg"), false, &mut bag, &mut rng), None);
}

// AC-3: shuffle shows every picture once per round, and a new round never starts with the
// picture just shown, so nothing shows twice in a row.
#[test]
fn shuffle_shows_each_picture_once_a_round_and_never_twice_in_a_row() {
    let list = strings(&["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
    for seed in [1u64, 7, 12345, 0xDEAD_BEEF] {
        let (mut bag, mut rng) = (Vec::new(), seed);
        let mut shown = vec![draw(&mut bag, &list, None, &mut rng).expect("a first picture")];
        for _ in 0..(5 * 20 - 1) {
            let next = upcoming(&list, shown.last().map(String::as_str), true, &mut bag, &mut rng).expect("five pictures");
            assert_ne!(Some(&next), shown.last(), "seed {seed}");
            shown.push(next);
        }
        for round in shown.chunks(5) {
            let mut round = round.to_vec();
            round.sort();
            assert_eq!(round, list, "seed {seed}");
        }
    }
}

// AC-5, AC-9: each screen keeps its own slideshow; the same value keeps the picture and its
// count; another folder starts over; a set edit keeps the picture showing now while it is
// still ticked, else starts over; a workspace whose value is gone drops its slideshow.
#[test]
fn a_new_wish_keeps_or_restarts_slideshows_as_promised() {
    let dir = Folder::new("wish", &["beach\\a.jpg", "beach\\b.jpg", "trips\\t.jpg", "x.jpg", "y.jpg", "z.jpg"]);
    let mut b = Backdrop::new(dir.0.clone());
    let beach = || Some(Source::Folder("beach".into()));
    b.want = vec![want("T-wish-1", 0, vec![None, beach()]), want("T-wish-2", 1, vec![None, beach()])];
    b.sync_shows();
    let key = |id: &str, i| (id.to_string(), i);
    assert_eq!(b.shows[&key("T-wish-1", 1)].current.as_deref(), Some("beach\\a.jpg"));
    assert_eq!(b.shows[&key("T-wish-1", 1)].next.as_deref(), Some("beach\\b.jpg"));
    assert_eq!(current("T-wish-2", 1).as_deref(), Some("beach\\a.jpg"), "the overview reads it");
    b.shows.get_mut(&key("T-wish-2", 1)).expect("screen 2").seconds = 40;
    b.sync_shows();
    assert_eq!(b.shows[&key("T-wish-2", 1)].seconds, 40, "the same value keeps its count");
    assert_eq!(b.shows[&key("T-wish-1", 1)].seconds, 0, "screens count on their own");

    b.want[1].files[1] = Some(Source::Folder("trips".into()));
    b.sync_shows();
    assert_eq!((b.shows[&key("T-wish-2", 1)].current.as_deref(), b.shows[&key("T-wish-2", 1)].seconds), (Some("trips\\t.jpg"), 0));
    assert_eq!(b.shows[&key("T-wish-2", 1)].next, None, "one picture never changes");

    b.want[0].files[1] = Some(Source::Set(strings(&["x.jpg", "y.jpg"])));
    b.sync_shows();
    let set = |b: &Backdrop| (b.shows[&key("T-wish-1", 1)].current.clone(), b.shows[&key("T-wish-1", 1)].seconds);
    assert_eq!(set(&b), (Some("x.jpg".into()), 0));
    b.shows.get_mut(&key("T-wish-1", 1)).expect("set").seconds = 30;
    b.want[0].files[1] = Some(Source::Set(strings(&["x.jpg", "y.jpg", "z.jpg"])));
    b.sync_shows();
    assert_eq!(set(&b), (Some("x.jpg".into()), 30), "ticking another keeps the picture and its count");
    b.want[0].files[1] = Some(Source::Set(strings(&["y.jpg", "z.jpg"])));
    b.sync_shows();
    assert_eq!(set(&b), (Some("y.jpg".into()), 0), "unticking the one showing starts over");

    b.want[0].files[1] = Some(Source::Picture("x.jpg".into()));
    b.sync_shows();
    assert!(!b.shows.contains_key(&key("T-wish-1", 1)) && current("T-wish-1", 1).is_none());
}

// AC-9: a screen missing from the wish keeps its slideshows through a monitor's display
// sleep, and loses them after 30 checks in a row.
#[test]
fn a_missing_screen_keeps_its_slideshows_for_30_checks() {
    let dir = Folder::new("gone", &["beach\\a.jpg", "beach\\b.jpg"]);
    let mut b = Backdrop::new(dir.0.clone());
    b.want = vec![want("T-gone", 0, vec![Some(Source::Folder("beach".into()))])];
    b.sync_shows();
    b.want.clear();
    b.sync_shows();
    for _ in 0..GONE_CHECKS - 1 {
        b.forget_gone();
    }
    assert!(b.shows.contains_key(&("T-gone".to_string(), 0)));
    b.want = vec![want("T-gone", 0, vec![Some(Source::Folder("beach".into()))])];
    b.forget_gone();
    b.want.clear();
    for _ in 0..GONE_CHECKS - 1 {
        b.forget_gone();
    }
    assert!(b.shows.contains_key(&("T-gone".to_string(), 0)), "coming back resets the count");
    b.forget_gone();
    assert!(b.shows.is_empty() && current("T-gone", 0).is_none());
}

// AC-8: a picture that can't be read is skipped for the next one; when none can, the
// slideshow shows your Windows wallpaper and its row gets the note.
#[test]
fn an_unreadable_picture_is_skipped_and_none_readable_is_a_note() {
    let dir = Folder::new("bad", &["bad\\a.jpg", "bad\\b.jpg", "bad\\c.jpg"]);
    let mut b = Backdrop::new(dir.0.clone());
    b.want = vec![want("T-bad", 0, vec![Some(Source::Folder("bad".into()))])];
    b.sync_shows();
    let key = ("T-bad".to_string(), 0);
    let fail = |b: &mut Backdrop, f: &str| {
        let stamp = std::fs::metadata(dir.0.join(f)).and_then(|m| m.modified()).ok();
        b.bad.insert(f.to_string(), stamp);
        b.slide_failed(f);
    };
    fail(&mut b, "bad\\a.jpg");
    assert_eq!((b.shows[&key].current.as_deref(), b.shows[&key].next.as_deref()), (Some("bad\\b.jpg"), Some("bad\\c.jpg")));
    fail(&mut b, "bad\\c.jpg");
    assert_eq!((b.shows[&key].current.as_deref(), b.shows[&key].next.as_deref()), (Some("bad\\b.jpg"), None), "the next is picked again");
    fail(&mut b, "bad\\b.jpg");
    assert_eq!(b.shows[&key].current, None);
    assert_eq!(crate::wallpaper::problem("bad"), Some(Problem::Unreadable));
    assert_eq!(b.skipped["bad"].len(), 3, "counted, for one log line per change");
}
