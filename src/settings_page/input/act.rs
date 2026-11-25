// What a click or Space does to each control, and the key steps (spec 0012).
use super::*;
use crate::settings_page::cards::apps::{AppChange, app_edit};
use crate::settings_page::cards::colors::{ACCENT, WS_KEYS, path_of, reset_color, set_accent, ws_index};
use crate::settings_page::cards::indicator::show;
use crate::settings_page::cards::names::same_name;
use crate::settings_page::cards::place::{MODE, SPOT, is_place, reset_place, same_as_all, set_mode, set_place, show_there, step_place};
use crate::settings_page::cards::wallpaper::{IV_KEYS, tick_wall, wall_interval_edit, wall_path, wall_screen, wall_use_edit};
use crate::settings_page::choosers::{choose_screen, open_chooser};
use crate::settings_page::menu::{choose, open_menu};
use crate::settings_page::picker::{Anchor, How, pick_close, pick_open, pick_preset};
use crate::settings_page::recording::{Rec, give_key, remove, reset_keys, start_rec, stop_rec};
use crate::settings_page::rows::{Kind, Row, Section, entry, find_row, is_digits, ladder_step, name_of, shortcut, stepped, unused, value};

/// A caption link: scrolls there, and the Skip animations link focuses the switch.
fn follow(l: Link) {
    match l {
        Link::Skip => {
            jump(Some(Section::Motion));
            let _ = set(|w| w.focus = Some(Ctl::Toggle("fastest")));
            reveal();
        }
        Link::Indicator => jump(Some(Section::Indicator)),
        Link::Overview => jump(Some(Section::Overview)),
        Link::Workspaces => jump(Some(Section::Workspaces)),
    }
}

pub(super) fn act(ctl: Ctl) {
    if ctl.writes() && settings::store::parse_error().is_some() {
        return; // read only
    }
    let s = settings::get();
    match ctl {
        Ctl::Side(sec) => jump(sec),
        Ctl::Welcome => crate::welcome::show(),
        Ctl::WhatsNew => crate::whats_new::show_current(),
        Ctl::Open => crate::tray::open(&settings::store::path()),
        Ctl::Toggle(key) if unused(&s, key).is_empty() => save(key, 1 - value(&s, key)),
        Ctl::Toggle(_) => {} // greyed: kept but not in use
        Ctl::Choice(ACCENT, i) => set_accent(i == 1),
        Ctl::Choice(MODE, i) => set_mode(i == 1),
        Ctl::Minus(key) if is_place(key) => step_place(key, false),
        Ctl::Plus(key) if is_place(key) => step_place(key, true),
        Ctl::Reset(key) if key == ACCENT || ws_index(key).is_some() => reset_color(key),
        Ctl::Reset(key) if is_place(key) => reset_place(key),
        Ctl::Reset(WORDS) => commit(Edit::Remove(path_of(WORDS)), "Popup words"),
        Ctl::Hex(_) | Ctl::Words | Ctl::Name(_) | Ctl::Picture | Ctl::ScreenItem(usize::MAX) => {}
        Ctl::NameSame(k) => same_name(k),
        Ctl::Spot(i) => set_place(SPOT, i as f64),
        Ctl::Screens | Ctl::NameScreens | Ctl::WallScreens | Ctl::WallPick(_) | Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_) => open_chooser(ctl),
        Ctl::WallSame(k) => {
            if let Some(screen) = get(|w| wall_screen(w, &settings::get())).flatten() {
                commit(Edit::Remove(wall_path(Some(&screen), k)), &format!("Workspace {} wallpaper", k + 1));
            }
        }
        Ctl::AppX(r) => {
            if let Some((key, _)) = s.apps.get(r) {
                commit(app_edit(&settings::edit::app_key_in_file(key), AppChange::Remove), "Apps");
            }
        }
        Ctl::ScreenItem(i) => choose_screen(i),
        Ctl::WallTick(i) => tick_wall(i),
        Ctl::WallUse(k) => {
            let ids: Vec<String> = get(|w| w.pics.view.present.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
            if let Some(edit) = wall_use_edit(&settings::get(), &ids, k) {
                commit(edit, &format!("Workspace {} wallpaper", k + 1));
            }
        }
        Ctl::SameAll => same_as_all(),
        Ctl::ShowThere => show_there(),
        Ctl::Minus(key) => step_key(key, false),
        Ctl::Plus(key) => step_key(key, true),
        Ctl::Stepper(_) | Ctl::Pick(_) | Ctl::Tiles => {} // focus only: Left and Right change them
        Ctl::Choice(key, i) => match find_row(key).map(|r| r.kind) {
            Some(Kind::Pair(..)) => save(key, (i == 0) as i64),
            _ if !unused(&s, key).is_empty() => {}
            _ => save(key, i as i64),
        },
        Ctl::Reset("workspaces") => save("workspaces", DEFAULTS.workspaces as i64),
        Ctl::Reset(key) if shortcut(key).is_some() => reset_keys(key),
        Ctl::Reset(key) => save(key, value(&DEFAULTS, key)),
        Ctl::Link(l) => follow(l),
        Ctl::WsTile(row, i) | Ctl::ShowTile(row, i) => {
            if let Some(dev) = get(|w| w.pics.screens.get(row).map(|s| s.device.clone())).flatten() {
                let anchor = if matches!(ctl, Ctl::WsTile(..)) { Anchor::Tile(dev, i) } else { Anchor::Show(dev, i) };
                pick_open(WS_KEYS[i], anchor, None);
            }
        }
        Ctl::Swatch(key) => pick_open(key, Anchor::Swatch(key), Some(ctl)),
        Ctl::Preset(i) => pick_preset(i),
        Ctl::PickDone => pick_close(How::Keep(true)),
        Ctl::PickCancel => pick_close(How::Undo),
        Ctl::Ring | Ctl::Square | Ctl::PickCard => {} // the wheel acts on the press
        Ctl::TileAdd => step_key("workspaces", true),
        Ctl::TileRemove => step_key("workspaces", false),
        Ctl::Forget(i) => {
            if let Some(id) = get(|w| w.pics.view.waiting.get(i).map(|x| x.id.clone())).flatten() {
                crate::ws::screens::forget(&id);
                update_pictures(true);
            }
        }
        Ctl::KeyRow(key) | Ctl::AddKey(key) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, None) }),
        Ctl::Key(key, i) => start_rec(if is_digits(key) { Rec::Digits(key) } else { Rec::List(key, Some(i)) }),
        Ctl::KeyX(key, i) => remove(key, i),
        Ctl::WinSide(key, desk) => give_key(key, desk),
        Ctl::StopRec(_) => stop_rec(),
        Ctl::Move(_) => {
            if let Some(p) = set(|w| w.prompt.take()).flatten() {
                commit(p.edit, entry(name_of(p.key)).1);
            }
        }
        Ctl::Cancel(_) => {
            let _ = set(|w| w.prompt = None);
            refresh();
        }
        Ctl::Picker(row) => open_menu(row, 1, None),
        Ctl::Item(usize::MAX) => {}
        Ctl::Item(i) => choose(i),
        Ctl::Show => {
            if s.indicator_enabled {
                show();
            }
        }
        Ctl::Pad(Pad::Windows) => setup::use_gestures(),
        Ctl::Pad(Pad::ReadOn) => save("touchpad.enabled", 1),
        Ctl::Pad(Pad::GiveBack) => {
            crate::welcome::give();
            update_pictures(true);
        }
        Ctl::Chart => {}
        Ctl::Dismiss => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Ctl::DismissEdit => {
            let _ = set(|w| w.edited = None);
            refresh();
        }
    }
}

pub(super) fn step_key(key: &'static str, up: bool) {
    if let Some(k) = IV_KEYS.iter().position(|x| *x == key) {
        if let Some(edit) = wall_interval_edit(&settings::get(), k, up) {
            commit(edit, &format!("Workspace {} time per picture", k + 1));
        }
        return;
    }
    if let Some(Row { kind: Kind::Ladder(ladder), .. }) = find_row(key) {
        let v = value(&settings::get(), key);
        let n = ladder_step(ladder, v, up);
        if n != v {
            save(key, n);
        }
        return;
    }
    if let Some(Row { kind: Kind::Step { lo, hi, by, .. }, .. }) = find_row(key) {
        // Greyed: kept but not in use, so − and + do nothing (AC-SET-8).
        if !unused(&settings::get(), key).is_empty() {
            return;
        }
        let v = value(&settings::get(), key);
        let n = stepped(v, *lo, *hi, *by, up);
        if n != v {
            save(key, n);
        }
    }
}
