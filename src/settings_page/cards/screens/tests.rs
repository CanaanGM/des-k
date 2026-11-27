// Tests for clicks on the workspace tiles.
use super::*;
use crate::settings_page::input::keys::{hit_at, stops};
use crate::settings_page::tests::{SETTINGS, fake_win, measure, take};

// AC-1: a click on the part of the last tile's × box inside the tile opens the picker; only
// the part outside the tile removes a workspace. And the tiles are no keyboard stop of their own.
#[test]
fn a_click_inside_a_tile_never_removes_a_workspace() {
    let _one = SETTINGS.lock();
    settings::store::use_for_test("workspaces = 4\n");
    let mut win = fake_win(ui::LIGHT, 1.0);
    win.hover = Some(Ctl::WsTile(0, 3));
    let out = measure(&win);
    take(&mut win, out);
    let tile = win.geo.tiles.iter().find(|(d, i, _)| d == "DISPLAY1" && *i == 3).unwrap().2;
    let corner = (tile.0 + tile.2 - 2.0, tile.1 + 2.0); // inside the tile, inside the × box
    let beyond = (tile.0 + tile.2 + 4.0, tile.1 - 4.0); // in the × box, past the tile
    assert_eq!(hit_at(&win, corner), Some(Ctl::WsTile(0, 3)));
    assert_eq!(hit_at(&win, beyond), Some(Ctl::TileRemove(0)));
    assert!(stops(&win, false).iter().any(|(c, _)| *c == Ctl::Tiles(0)));
    assert!(!stops(&win, false).iter().any(|(c, _)| matches!(c, Ctl::WsTile(..) | Ctl::ShowTile(..))));
}

// Spec 0040 AC-9: a lone screen with no line of its own still changes `workspaces`, so a one
// screen setup's file looks as it always did; with two or more rows each row writes its own line,
// and a row going back to the shared count loses its line (the spec 0020 rule).
#[test]
fn each_row_writes_its_own_line_and_one_row_writes_the_shared_count() {
    assert_eq!(row_write(1, false, 5, 4), Write::Shared(5));
    assert_eq!(row_write(1, true, 5, 4), Write::Line(5), "a lone row with its own line keeps it");
    assert_eq!(row_write(2, false, 3, 4), Write::Line(3));
    assert_eq!(row_write(2, true, 4, 4), Write::Remove);
    let file = "workspaces = 4\n";
    let two = loaded_file(file);
    let added = settings::edit::apply_for_test(file, &row_edit(two, 2, "internal", 2));
    assert!(added.contains("[workspaces_per_screen]") && added.contains("internal = 2"), "{added}");
    assert_eq!(settings::parse::parse_for_test(&added).0.count_for("internal"), 2);
    let back = settings::edit::apply_for_test(&added, &row_edit(loaded_file(&added), 2, "internal", 4));
    assert_eq!(settings::parse::parse_for_test(&back).0.count_by_screen.len(), 0, "{back}");
    let one = settings::edit::apply_for_test(file, &row_edit(two, 1, "internal", 5));
    assert_eq!(one, "workspaces = 5\n");
}

// AC-1, AC-10 and your call during the build: a line equal to `workspaces` follows it, so a
// change to the shared count takes such a line out and that screen moves with it; a line that
// differs stays, and a change that lands on it writes nothing more.
#[test]
fn the_shared_count_carries_the_lines_that_follow_it() {
    let file = "workspaces = 5\n\n[workspaces_per_screen]\n\"internal\" = 5\n\"DEL-1\" = 3\n";
    let s = loaded_file(file);
    assert!(s.follows_shared("internal") && !s.follows_shared("DEL-1"));
    let lowered = settings::parse::parse_for_test(&settings::edit::apply_for_test(file, &shared_edit(s, 4))).0;
    assert_eq!((lowered.shared_count, lowered.count_for("internal"), lowered.count_for("DEL-1")), (4, 4, 3));
    let onto = settings::edit::apply_for_test(file, &shared_edit(s, 3));
    assert!(onto.contains("\"DEL-1\" = 3"), "{onto}");
    assert!(settings::parse::parse_for_test(&onto).0.follows_shared("DEL-1"));
    // A lone row whose line equals the shared count follows it: + changes `workspaces` and takes
    // the line out, or the screen would stay where it was.
    let lone = "workspaces = 4\n\n[workspaces_per_screen]\n\"internal\" = 4\n";
    let up = settings::parse::parse_for_test(&settings::edit::apply_for_test(lone, &row_edit(loaded_file(lone), 1, "internal", 5))).0;
    assert_eq!((up.shared_count, up.count_for("internal")), (5, 5));
    assert!(up.count_by_screen.is_empty());
}

fn loaded_file(text: &str) -> &'static Settings {
    Box::leak(Box::new(settings::parse::parse_for_test(text).0))
}
