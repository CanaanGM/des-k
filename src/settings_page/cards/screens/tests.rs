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
    assert_eq!(hit_at(&win, beyond), Some(Ctl::TileRemove));
    assert!(stops(&win, false).iter().any(|(c, _)| *c == Ctl::Tiles));
    assert!(!stops(&win, false).iter().any(|(c, _)| matches!(c, Ctl::WsTile(..) | Ctl::ShowTile(..))));
}
