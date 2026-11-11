//! Line diff for the undo history view: Myers' algorithm in linear space
//! (it finds the middle of the shortest edit path from both ends and splits
//! the problem there), with a time limit after which the remaining middle is
//! reported as replaced instead of searched further.

use std::collections::HashMap;
use std::time::{Duration, Instant};

const TIME_LIMIT: Duration = Duration::from_millis(25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Equal,
    /// Only on the old side.
    Delete,
    /// Only on the new side.
    Insert,
}

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub(crate) op: Op,
    /// Old-side line number; for an insert, the old line it goes before.
    pub(crate) old: usize,
    /// New-side line number; for a delete, the new line it would go before.
    pub(crate) new: usize,
    /// Bytes of the line that changed, when it pairs with a similar line
    /// on the other side.
    pub(crate) changed: Option<(usize, usize)>,
}

pub(crate) struct Hunk {
    pub(crate) lines: Vec<Line>,
}

pub(crate) struct Diff {
    pub(crate) old: String,
    pub(crate) new: String,
    /// Byte ranges of the lines diffed, from line `first` on (both texts
    /// share everything before it).
    old_lines: Vec<(usize, usize)>,
    new_lines: Vec<(usize, usize)>,
    first: usize,
    pub(crate) hunks: Vec<Hunk>,
}

/// Each line as a number, equal lines getting equal numbers.
fn tokens<'a>(
    ids: &mut HashMap<&'a str, u32>,
    text: &'a str,
    lines: &[(usize, usize)],
) -> Vec<u32> {
    lines
        .iter()
        .map(|(start, end)| {
            let next = ids.len() as u32;
            *ids.entry(&text[*start..*end]).or_insert(next)
        })
        .collect()
}

/// The lines of `text[from..to]` (a final line break does not start
/// another line).
fn line_ranges(text: &str, from: usize, to: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = from;
    for index in memchr::memchr_iter(b'\n', &text.as_bytes()[from..to]) {
        ranges.push((start, from + index));
        start = from + index + 1;
    }
    if start < to {
        ranges.push((start, to));
    }
    ranges
}

fn shared_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 64 <= n && a[i..i + 64] == b[i..i + 64] {
        i += 64;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

fn shared_suffix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 64 <= n && a[a.len() - i - 64..a.len() - i] == b[b.len() - i - 64..b.len() - i] {
        i += 64;
    }
    while i < n && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
        i += 1;
    }
    i
}

/// Where two texts differ, widened to whole lines plus `context` lines on
/// each side: `(start, old_end, new_end)` in bytes. Before `start` and after
/// the ends both texts are the same.
fn changed_region(old: &str, new: &str, context: usize) -> (usize, usize, usize) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let prefix = shared_prefix(a, b);
    let suffix = shared_suffix(&a[prefix..], &b[prefix..]);
    let line_start = |end: usize| a[..end].iter().rposition(|c| *c == b'\n').map_or(0, |i| i + 1);
    let mut start = line_start(prefix);
    for _ in 0..context {
        if start == 0 {
            break;
        }
        start = line_start(start - 1);
    }
    let shared = &a[a.len() - suffix..];
    let line_end = |from: usize| {
        shared[from..]
            .iter()
            .position(|c| *c == b'\n')
            .map_or(shared.len(), |i| from + i + 1)
    };
    let mut extra = line_end(0);
    for _ in 0..context {
        if extra >= shared.len() {
            break;
        }
        extra = line_end(extra);
    }
    (start, a.len() - suffix + extra, b.len() - suffix + extra)
}

impl Diff {
    pub(crate) fn new(old: String, new: String, context: usize) -> Self {
        // Only the part that differs is split into lines and diffed: what
        // both share before and after it is just counted.
        let (start, old_end, new_end) = changed_region(&old, &new, context);
        let first = memchr::memchr_iter(b'\n', &old.as_bytes()[..start]).count();
        let old_lines = line_ranges(&old, start, old_end);
        let new_lines = line_ranges(&new, start, new_end);
        let ops = {
            let mut ids: HashMap<&str, u32> = HashMap::new();
            let a = tokens(&mut ids, &old, &old_lines);
            let b = tokens(&mut ids, &new, &new_lines);
            diff_ops(&a, &b, Instant::now() + TIME_LIMIT)
        };
        let mut diff = Self {
            old,
            new,
            old_lines,
            new_lines,
            first,
            hunks: Vec::new(),
        };
        let lines = diff.lines_of(&ops);
        diff.hunks = group_hunks(lines, context);
        diff
    }

    pub(crate) fn old_line(&self, index: usize) -> &str {
        self.old_lines
            .get(index.wrapping_sub(self.first))
            .map(|(start, end)| &self.old[*start..*end])
            .unwrap_or("")
    }

    pub(crate) fn new_line(&self, index: usize) -> &str {
        self.new_lines
            .get(index.wrapping_sub(self.first))
            .map(|(start, end)| &self.new[*start..*end])
            .unwrap_or("")
    }

    /// The text of a diff line (from the side it belongs to).
    pub(crate) fn text(&self, line: &Line) -> &str {
        match line.op {
            Op::Insert => self.new_line(line.new),
            _ => self.old_line(line.old),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }

    /// Ops (one per line) into numbered lines, each change block ordered as
    /// its deletes then its inserts, with the changed part of paired lines.
    fn lines_of(&self, ops: &[Op]) -> Vec<Line> {
        let mut lines = Vec::with_capacity(ops.len());
        let (mut old, mut new) = (self.first, self.first);
        let mut index = 0;
        while index < ops.len() {
            if ops[index] == Op::Equal {
                lines.push(Line {
                    op: Op::Equal,
                    old,
                    new,
                    changed: None,
                });
                old += 1;
                new += 1;
                index += 1;
                continue;
            }
            let block_end = ops[index..]
                .iter()
                .position(|op| *op == Op::Equal)
                .map_or(ops.len(), |offset| index + offset);
            let deletes = ops[index..block_end]
                .iter()
                .filter(|op| **op == Op::Delete)
                .count();
            let inserts = block_end - index - deletes;
            let first_delete = lines.len();
            for offset in 0..deletes {
                lines.push(Line {
                    op: Op::Delete,
                    old: old + offset,
                    new,
                    changed: None,
                });
            }
            let first_insert = lines.len();
            for offset in 0..inserts {
                lines.push(Line {
                    op: Op::Insert,
                    old: old + deletes,
                    new: new + offset,
                    changed: None,
                });
            }
            for pair in 0..deletes.min(inserts) {
                let before = self.old_line(old + pair);
                let after = self.new_line(new + pair);
                if let Some((was, now)) = changed_parts(before, after) {
                    lines[first_delete + pair].changed = Some(was);
                    lines[first_insert + pair].changed = Some(now);
                }
            }
            old += deletes;
            new += inserts;
            index = block_end;
        }
        lines
    }
}

/// The differing middle of two similar lines (by characters), or None when
/// they share nothing at either end.
fn changed_parts(before: &str, after: &str) -> Option<((usize, usize), (usize, usize))> {
    let prefix: usize = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    if prefix + suffix == 0 {
        return None;
    }
    Some((
        (prefix, before.len() - suffix),
        (prefix, after.len() - suffix),
    ))
}

fn group_hunks(lines: Vec<Line>, context: usize) -> Vec<Hunk> {
    let changed: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.op != Op::Equal)
        .map(|(index, _)| index)
        .collect();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for index in changed {
        let start = index.saturating_sub(context);
        let end = (index + context + 1).min(lines.len());
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }
    ranges
        .into_iter()
        .map(|(start, end)| Hunk {
            lines: lines[start..end].to_vec(),
        })
        .collect()
}

/// One op per line turning `a` into `b`.
pub(crate) fn diff_ops(a: &[u32], b: &[u32], deadline: Instant) -> Vec<Op> {
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    let max_d = max_d(a.len(), b.len());
    let mut vf = V::new(max_d);
    let mut vb = V::new(max_d);
    conquer(a, 0..a.len(), b, 0..b.len(), &mut vf, &mut vb, deadline, &mut out);
    out
}

fn max_d(len1: usize, len2: usize) -> usize {
    (len1 + len2 + 1) / 2 + 1
}

struct V {
    offset: isize,
    v: Vec<usize>,
}

impl V {
    fn new(max_d: usize) -> Self {
        Self {
            offset: max_d as isize,
            v: vec![0; 2 * max_d + 2],
        }
    }
}

impl std::ops::Index<isize> for V {
    type Output = usize;
    fn index(&self, index: isize) -> &usize {
        &self.v[(index + self.offset) as usize]
    }
}

impl std::ops::IndexMut<isize> for V {
    fn index_mut(&mut self, index: isize) -> &mut usize {
        &mut self.v[(index + self.offset) as usize]
    }
}

fn common_prefix(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

fn common_suffix(a: &[u32], b: &[u32]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The start of the middle snake of the shortest edit path (Myers 1986,
/// section 4b), or None when the time limit ran out.
fn middle_snake(
    a: &[u32],
    ar: std::ops::Range<usize>,
    b: &[u32],
    br: std::ops::Range<usize>,
    vf: &mut V,
    vb: &mut V,
    deadline: Instant,
) -> Option<(usize, usize)> {
    let n = ar.len();
    let m = br.len();
    let delta = n as isize - m as isize;
    let odd = delta & 1 == 1;
    vf[1] = 0;
    vb[1] = 0;
    let d_max = max_d(n, m) as isize;
    for d in 0..d_max {
        if Instant::now() > deadline {
            return None;
        }
        // Forward.
        for k in (-d..=d).rev().step_by(2) {
            let mut x = if k == -d || (k != d && vf[k - 1] < vf[k + 1]) {
                vf[k + 1]
            } else {
                vf[k - 1] + 1
            };
            let y = (x as isize - k) as usize;
            let (x0, y0) = (x, y);
            if x < n && y < m {
                x += common_prefix(&a[ar.start + x..ar.end], &b[br.start + y..br.end]);
            }
            vf[k] = x;
            if odd && (k - delta).abs() <= d - 1 && vf[k] + vb[-(k - delta)] >= n {
                return Some((x0 + ar.start, y0 + br.start));
            }
        }
        // Backward.
        for k in (-d..=d).rev().step_by(2) {
            let mut x = if k == -d || (k != d && vb[k - 1] < vb[k + 1]) {
                vb[k + 1]
            } else {
                vb[k - 1] + 1
            };
            let mut y = (x as isize - k) as usize;
            if x < n && y < m {
                let advance =
                    common_suffix(&a[ar.start..ar.start + n - x], &b[br.start..br.start + m - y]);
                x += advance;
                y += advance;
            }
            vb[k] = x;
            if !odd && (k - delta).abs() <= d && vb[k] + vf[-(k - delta)] >= n {
                return Some((n - x + ar.start, m - y + br.start));
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn conquer(
    a: &[u32],
    mut ar: std::ops::Range<usize>,
    b: &[u32],
    mut br: std::ops::Range<usize>,
    vf: &mut V,
    vb: &mut V,
    deadline: Instant,
    out: &mut Vec<Op>,
) {
    let prefix = common_prefix(&a[ar.clone()], &b[br.clone()]);
    out.extend(std::iter::repeat_n(Op::Equal, prefix));
    ar.start += prefix;
    br.start += prefix;
    let suffix = common_suffix(&a[ar.clone()], &b[br.clone()]);
    ar.end -= suffix;
    br.end -= suffix;
    if ar.is_empty() && br.is_empty() {
    } else if br.is_empty() {
        out.extend(std::iter::repeat_n(Op::Delete, ar.len()));
    } else if ar.is_empty() {
        out.extend(std::iter::repeat_n(Op::Insert, br.len()));
    } else if let Some((x, y)) = middle_snake(a, ar.clone(), b, br.clone(), vf, vb, deadline) {
        conquer(a, ar.start..x, b, br.start..y, vf, vb, deadline, out);
        conquer(a, x..ar.end, b, y..br.end, vf, vb, deadline, out);
    } else {
        out.extend(std::iter::repeat_n(Op::Delete, ar.len()));
        out.extend(std::iter::repeat_n(Op::Insert, br.len()));
    }
    out.extend(std::iter::repeat_n(Op::Equal, suffix));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(a: &[u32], b: &[u32], ops: &[Op]) -> Vec<u32> {
        let (mut i, mut j) = (0, 0);
        let mut out = Vec::new();
        for op in ops {
            match op {
                Op::Equal => {
                    assert_eq!(a[i], b[j]);
                    out.push(a[i]);
                    i += 1;
                    j += 1;
                }
                Op::Delete => i += 1,
                Op::Insert => {
                    out.push(b[j]);
                    j += 1;
                }
            }
        }
        assert_eq!((i, j), (a.len(), b.len()));
        out
    }

    fn lcs(a: &[u32], b: &[u32]) -> usize {
        let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i][j] = if a[i] == b[j] {
                    table[i + 1][j + 1] + 1
                } else {
                    table[i + 1][j].max(table[i][j + 1])
                };
            }
        }
        table[0][0]
    }

    #[test]
    fn random_diffs_rebuild_the_new_side_with_the_fewest_changes() {
        let mut seed = 99_u64;
        let mut random = |limit: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % limit
        };
        for _ in 0..3000 {
            let alphabet = 1 + random(5) as u32;
            let a: Vec<u32> = (0..random(30)).map(|_| random(alphabet as usize) as u32).collect();
            let b: Vec<u32> = (0..random(30)).map(|_| random(alphabet as usize) as u32).collect();
            let ops = diff_ops(&a, &b, Instant::now() + Duration::from_secs(5));
            assert_eq!(apply(&a, &b, &ops), b);
            let edits = ops.iter().filter(|op| **op != Op::Equal).count();
            assert_eq!(edits, a.len() + b.len() - 2 * lcs(&a, &b), "{a:?} → {b:?}");
        }
    }

    #[test]
    fn hunks_show_context_and_the_changed_words() {
        let old = "## Hoy\n- [ ] comprar leche\n- [x] llamar al banco\n".to_string();
        let new = "## Hoy\n- [ ] comprar yerba\n- [x] llamar al banco\n".to_string();
        let diff = Diff::new(old, new, 3);
        assert_eq!(diff.hunks.len(), 1);
        let ops: Vec<Op> = diff.hunks[0].lines.iter().map(|line| line.op).collect();
        assert_eq!(ops, [Op::Equal, Op::Delete, Op::Insert, Op::Equal]);
        let delete = &diff.hunks[0].lines[1];
        let (start, end) = delete.changed.unwrap();
        assert_eq!(&diff.text(delete)[start..end], "leche");
        let insert = &diff.hunks[0].lines[2];
        let (start, end) = insert.changed.unwrap();
        assert_eq!(&diff.text(insert)[start..end], "yerba");
        assert_eq!(insert.old, 2, "an insert knows where it goes on the old side");
    }

    #[test]
    fn diffs_of_big_texts_only_split_the_part_that_changed() {
        let body: String = (0..200_000).map(|n| format!("line {n}\n")).collect();
        let mut changed = body.clone();
        let at = changed.find("line 150000\n").unwrap();
        changed.replace_range(at..at + "line 150000".len(), "line CHANGED");
        let started = Instant::now();
        let diff = Diff::new(body.clone(), changed, 3);
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(diff.hunks.len(), 1);
        let lines = &diff.hunks[0].lines;
        assert_eq!(lines.len(), 8, "3 lines of context on each side");
        assert_eq!(lines[0].old, 149_997);
        assert_eq!(diff.text(&lines[3]), "line 150000");
        assert_eq!(diff.text(&lines[4]), "line CHANGED");
        assert_eq!(lines[4].op, Op::Insert);
        // Same text, and changes at either end.
        assert!(Diff::new(body.clone(), body.clone(), 3).is_empty());
        let diff = Diff::new(body.clone(), format!("new first\n{body}last\n"), 1);
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.text(&diff.hunks[0].lines[0]), "new first");
        assert_eq!(diff.text(diff.hunks[1].lines.last().unwrap()), "last");
        assert_eq!(diff.hunks[1].lines.last().unwrap().new, 200_001);
    }

    #[test]
    fn random_texts_diff_to_the_same_lines_as_the_whole_text() {
        let mut seed = 5_u64;
        let mut random = |limit: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % limit
        };
        for _ in 0..2000 {
            let line = |random: &mut dyn FnMut(usize) -> usize| ["a", "b", "c", ""][random(4)].to_string();
            let old: Vec<String> = (0..random(12)).map(|_| line(&mut random)).collect();
            let mut new = old.clone();
            for _ in 0..random(4) {
                let at = random(new.len() + 1);
                match random(3) {
                    0 if at < new.len() => {
                        new.remove(at);
                    }
                    1 => new.insert(at, line(&mut random)),
                    _ if at < new.len() => new[at] = line(&mut random),
                    _ => {}
                }
            }
            let ending = ["", "\n"][random(2)];
            let old_text = old.join("\n") + ending;
            let new_text = new.join("\n") + ["", "\n"][random(2)];
            let diff = Diff::new(old_text.clone(), new_text.clone(), 2);
            // Rebuild the new text's lines from the old one and the hunks.
            let old_lines: Vec<&str> = line_ranges(&old_text, 0, old_text.len())
                .into_iter()
                .map(|(s, e)| &old_text[s..e])
                .collect();
            let new_lines: Vec<&str> = line_ranges(&new_text, 0, new_text.len())
                .into_iter()
                .map(|(s, e)| &new_text[s..e])
                .collect();
            let mut rebuilt: Vec<&str> = Vec::new();
            let mut next_old = 0;
            for hunk in &diff.hunks {
                for line in &hunk.lines {
                    while next_old < line.old && next_old < old_lines.len() {
                        rebuilt.push(old_lines[next_old]);
                        next_old += 1;
                    }
                    match line.op {
                        Op::Equal => {
                            assert_eq!(diff.text(line), old_lines[line.old]);
                            rebuilt.push(old_lines[line.old]);
                            next_old = line.old + 1;
                        }
                        Op::Delete => next_old = line.old + 1,
                        Op::Insert => rebuilt.push(diff.text(line)),
                    }
                }
            }
            rebuilt.extend(&old_lines[next_old.min(old_lines.len())..]);
            assert_eq!(rebuilt, new_lines, "{old_text:?} → {new_text:?}");
        }
    }

    #[test]
    fn a_huge_replacement_stays_fast() {
        let old: String = (0..20_000).map(|n| format!("old line {n}\n")).collect();
        let new: String = (0..20_000).map(|n| format!("new line {n}\n")).collect();
        let started = Instant::now();
        let diff = Diff::new(old, new, 3);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(diff.hunks.len(), 1);
    }
}
