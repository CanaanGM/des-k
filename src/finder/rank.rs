// The finder's order: recency first (AC-4), then the match score (AC-5), the row chosen as it
// opens, and which matched letters fall in each column.
use super::*;

/// AC-4: windows used since start, most recent first; then unused ones on screen, front to
/// back; then unused hidden ones by screen, workspace and saved order. `rows` come in
/// `ws::windows()` order, which breaks the remaining ties.
pub(super) fn recency_rank(rows: &mut [Row]) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&rows[a], &rows[b]);
        rb.used
            .cmp(&ra.used)
            .then(rb.on_screen.cmp(&ra.on_screen))
            .then_with(|| if ra.on_screen { std::cmp::Ordering::Equal } else { (ra.screen, ra.workspace).cmp(&(rb.screen, rb.workspace)) })
            .then(a.cmp(&b))
    });
    for (rank, i) in order.into_iter().enumerate() {
        rows[i].rank = rank;
    }
}

/// AC-5: the matching rows, best score first, equal scores in the AC-4 order. Then the rows
/// that match only by their workspace's name (`by_name`), in the AC-4 order, so a window you
/// typed the title of always comes first (spec 0023 AC-9).
pub(super) fn rank(rows: &[Row], scores: &[Option<u32>], by_name: &[bool]) -> Vec<(usize, u32)> {
    let mut shown: Vec<(usize, u32)> = scores.iter().enumerate().filter_map(|(i, s)| Some((i, (*s)?))).collect();
    shown.sort_by(|a, b| b.1.cmp(&a.1).then(rows[a.0].rank.cmp(&rows[b.0].rank)));
    let mut named: Vec<(usize, u32)> = (0..rows.len()).filter(|&i| by_name.get(i) == Some(&true) && scores[i].is_none()).map(|i| (i, 0)).collect();
    named.sort_by_key(|&(i, _)| rows[i].rank);
    shown.extend(named);
    shown
}

/// With nothing typed the second row, so Win+F then Enter flips back like Alt+Tab.
pub(super) fn first_pick(query: &str, shown: usize) -> usize {
    if query.trim().is_empty() && shown >= 2 { 1 } else { 0 }
}

/// A row's matched letters split back into its two columns, as char indexes (AC-FND-6). nucleo
/// counts a letter with combining marks as one; a column where that happened gets none.
pub(super) fn split_marks(r: &Row) -> (Vec<usize>, Vec<usize>) {
    let mut buf = Vec::new();
    let plen = Utf32Str::new(&r.program, &mut buf).len();
    let tlen = Utf32Str::new(&r.title, &mut buf).len();
    let (pok, tok) = (plen == r.program.chars().count(), tlen == r.title.chars().count());
    let (mut p, mut t) = (vec![], vec![]);
    for &i in &r.marks {
        let i = i as usize;
        if i < plen {
            if pok {
                p.push(i);
            }
        } else if i > plen && i - plen - 1 < tlen && tok {
            t.push(i - plen - 1);
        }
    }
    (p, t)
}


#[cfg(test)]
mod tests;
