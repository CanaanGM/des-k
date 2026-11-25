// Tests for the Overview card's live tiles.
use super::*;
use crate::settings_page::tests::{SETTINGS, fake_win, measure, take};

// AC-2: with 2 workspaces, the Overview picture's tiles 3 and 4 are empty slots that open
// nothing; 1 and 2 open the picker.
#[test]
fn overview_tiles_past_your_count_open_nothing() {
    let _one = SETTINGS.lock();
    settings::store::use_for_test("workspaces = 2\n");
    let mut win = fake_win(ui::LIGHT, 1.0);
    let out = measure(&win);
    take(&mut win, out);
    let live: Vec<(usize, usize)> = win.hits.iter().filter_map(|h| match h.ctl {
        Ctl::ShowTile(r, i) => Some((r, i)),
        _ => None,
    }).collect();
    assert_eq!(live, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    assert_eq!(win.geo.shows.len(), 4);
}
