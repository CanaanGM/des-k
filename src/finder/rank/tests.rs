// The order tests: recency, score then recency, the first pick, and the bold letters per column.
use super::*;
use crate::finder::tests::{filter, named, row, session};
use std::time::Duration;

fn ranks(rows: &[Row]) -> Vec<usize> {
    rows.iter().map(|r| r.rank).collect()
}

// AC-4: what you used last comes first (that is what makes Win+F, Enter flip back), then
// what you can see, then hidden windows in a fixed screen and workspace order.
#[test]
fn recency_beats_screen_order_and_hidden_windows_follow_screen_then_workspace() {
    let t = Instant::now();
    let mut rows = vec![
        row(true, 1, 1, None),
        row(false, 2, 3, None),
        row(false, 1, 2, Some(t)),
        row(false, 2, 1, None),
        row(true, 3, 1, Some(t + Duration::from_secs(1))),
        row(false, 1, 4, None),
    ];
    recency_rank(&mut rows);
    assert_eq!(ranks(&rows), [2, 5, 1, 4, 0, 3]);
}

// AC-5: a better match must win over recency, or typing would not find the window you mean;
// only equal scores fall back to the AC-4 order. Non matches are not listed at all.
#[test]
fn rank_puts_score_first_then_recency_order() {
    let mut rows = vec![row(true, 1, 1, None), row(true, 1, 1, None), row(true, 1, 1, None)];
    rows[0].rank = 2;
    rows[1].rank = 0;
    rows[2].rank = 1;
    assert_eq!(rank(&rows, &[Some(50), Some(20), Some(50)], &[]), [(2, 50), (0, 50), (1, 20)]);
    assert_eq!(rank(&rows, &[None, Some(1), None], &[]), [(1, 1)]);
}

// Win+F then Enter must land on the window you were in before, not the one you are in.
#[test]
fn empty_query_selects_the_second_row() {
    assert_eq!(first_pick("", 5), 1);
    assert_eq!(first_pick("  ", 2), 1);
    assert_eq!(first_pick("", 1), 0);
    assert_eq!(first_pick("code", 5), 0);
}

// AC-FND-6: the bold letters must be the ones that matched, each in the column it sits in;
// an index off by one would bold the letter next to it, or one in the other column.
#[test]
fn matched_letters_split_into_the_program_and_title_columns() {
    let mut s = session(vec![named("Google Chrome", "des-k issues", 0), named("Code", "notes", 1)]);
    filter(&mut s, "chrome");
    let (p, t) = split_marks(&s.rows[0]);
    assert_eq!((p, t), (vec![7, 8, 9, 10, 11, 12], vec![]));
    filter(&mut s, "code des");
    let (p, t) = split_marks(&s.rows[0]);
    let pick = |text: &str, at: &[usize]| at.iter().map(|&i| text.chars().nth(i).unwrap()).collect::<String>();
    assert_eq!(pick("Google Chrome", &p).to_lowercase().chars().filter(|c| "code".contains(*c)).count(), p.len());
    assert_eq!(pick("des-k issues", &t), "des");
    // A row the query no longer matches keeps no bold letters.
    filter(&mut s, "");
    assert!(s.rows.iter().all(|r| r.marks.is_empty()));
}
