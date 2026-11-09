//! Line-based comparison of two documents, with character-level detail for changed lines.
//!
//! Uses Myers' O(ND) algorithm in linear space. A per-split budget keeps very different
//! inputs fast; the result is then still correct, just not always minimal.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;

use crate::Document;

/// Longer lines are marked as changed as a whole instead of char by char.
const MAX_INLINE_CHARS: usize = 5_000;
/// Edit distance explored per split before falling back to splitting in the middle.
const MAX_EDIT_DISTANCE: isize = 1_024;

/// `count` blank rows shown above `line`, as `(line, count)`.
pub type Spacer = (usize, usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DiffOptions {
    pub ignore_whitespace: bool,
    pub ignore_case: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkKind {
    /// Lines only on the left.
    Removed,
    /// Lines only on the right.
    Added,
    Changed,
}

/// A block of differing lines; `left` and `right` are line ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub kind: HunkKind,
    pub left: Range<usize>,
    pub right: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    pub hunks: Vec<Hunk>,
    /// Changed chars within changed lines, as sorted char ranges.
    pub left_changes: Vec<Range<usize>>,
    pub right_changes: Vec<Range<usize>>,
}

impl Comparison {
    pub fn is_identical(&self) -> bool {
        self.hunks.is_empty()
    }

    /// Blank rows that make equal lines line up, as `(before line, count)` for left and right.
    pub fn spacers(&self) -> (Vec<Spacer>, Vec<Spacer>) {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for hunk in &self.hunks {
            let (l, r) = (hunk.left.len(), hunk.right.len());
            if l < r {
                left.push((hunk.left.end, r - l));
            } else if r < l {
                right.push((hunk.right.end, l - r));
            }
        }
        (left, right)
    }
}

pub fn compare(left: &Document, right: &Document, options: DiffOptions) -> Comparison {
    let a = line_keys(left, options);
    let b = line_keys(right, options);
    let matches = matching_pairs(&a, &b);

    let mut comparison = Comparison::default();
    let (mut i, mut j) = (0, 0);
    for (x, y) in matches.into_iter().chain([(a.len(), b.len())]) {
        if x > i || y > j {
            let kind = match (x > i, y > j) {
                (true, false) => HunkKind::Removed,
                (false, true) => HunkKind::Added,
                _ => HunkKind::Changed,
            };
            comparison.hunks.push(Hunk {
                kind,
                left: i..x,
                right: j..y,
            });
        }
        (i, j) = (x + 1, y + 1);
    }

    let Comparison {
        hunks,
        left_changes,
        right_changes,
    } = &mut comparison;
    for hunk in hunks.iter().filter(|hunk| hunk.kind == HunkKind::Changed) {
        for (l, r) in hunk.left.clone().zip(hunk.right.clone()) {
            let changes = (&mut *left_changes, &mut *right_changes);
            inline_changes(left, right, l, r, options, changes);
        }
    }
    comparison
}

/// Hashes of all lines, to compare a document with an earlier state of itself cheaply.
pub fn line_hashes(doc: &Document) -> Vec<u64> {
    line_keys(doc, DiffOptions::default())
}

/// Line ranges of the current document that differ from `baseline`; both are
/// [`line_hashes`]. Removed lines leave no range behind.
pub fn changed_lines(baseline: &[u64], current: &[u64]) -> Vec<Range<usize>> {
    let mut changed = Vec::new();
    let mut next = 0;
    for (_, y) in matching_pairs(baseline, current)
        .into_iter()
        .chain([(baseline.len(), current.len())])
    {
        if y > next {
            changed.push(next..y);
        }
        next = y + 1;
    }
    changed
}

fn line_keys(doc: &Document, options: DiffOptions) -> Vec<u64> {
    let rope = doc.text();
    (0..rope.len_lines())
        .map(|line| {
            let mut hasher = DefaultHasher::new();
            for ch in rope.line(line).chars().filter(|&c| c != '\n') {
                if options.ignore_whitespace && ch.is_whitespace() {
                    continue;
                }
                if options.ignore_case {
                    ch.to_lowercase().for_each(|c| c.hash(&mut hasher));
                } else {
                    ch.hash(&mut hasher);
                }
            }
            hasher.finish()
        })
        .collect()
}

fn line_chars(doc: &Document, line: usize, ignore_case: bool) -> Vec<char> {
    doc.text()
        .line(line)
        .chars()
        .filter(|&c| c != '\n')
        .map(|c| {
            if ignore_case {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

fn inline_changes(
    left: &Document,
    right: &Document,
    l: usize,
    r: usize,
    options: DiffOptions,
    (left_changes, right_changes): (&mut Vec<Range<usize>>, &mut Vec<Range<usize>>),
) {
    let a = line_chars(left, l, options.ignore_case);
    let b = line_chars(right, r, options.ignore_case);
    let (a_start, b_start) = (left.text().line_to_char(l), right.text().line_to_char(r));
    if a.len() > MAX_INLINE_CHARS || b.len() > MAX_INLINE_CHARS {
        left_changes.push(a_start..a_start + a.len());
        right_changes.push(b_start..b_start + b.len());
        return;
    }
    let (mut i, mut j) = (0, 0);
    for (x, y) in matching_pairs(&a, &b)
        .into_iter()
        .chain([(a.len(), b.len())])
    {
        if x > i {
            left_changes.push(a_start + i..a_start + x);
        }
        if y > j {
            right_changes.push(b_start + j..b_start + y);
        }
        (i, j) = (x + 1, y + 1);
    }
}

/// Index pairs of equal elements on a shortest edit path, in ascending order.
pub(crate) fn matching_pairs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let mut myers = Myers {
        a,
        b,
        forward: Vec::new(),
        backward: Vec::new(),
        pairs: Vec::new(),
    };
    myers.compare(0, a.len(), 0, b.len());
    myers.pairs
}

struct Myers<'a, T> {
    a: &'a [T],
    b: &'a [T],
    /// Furthest x per diagonal, indexed by `k + offset`.
    forward: Vec<isize>,
    backward: Vec<isize>,
    pairs: Vec<(usize, usize)>,
}

impl<T: PartialEq> Myers<'_, T> {
    fn compare(&mut self, mut a_lo: usize, mut a_hi: usize, mut b_lo: usize, mut b_hi: usize) {
        while a_lo < a_hi && b_lo < b_hi && self.a[a_lo] == self.b[b_lo] {
            self.pairs.push((a_lo, b_lo));
            a_lo += 1;
            b_lo += 1;
        }
        let mut suffix = 0;
        while a_lo < a_hi && b_lo < b_hi && self.a[a_hi - 1] == self.b[b_hi - 1] {
            a_hi -= 1;
            b_hi -= 1;
            suffix += 1;
        }
        if a_lo < a_hi && b_lo < b_hi {
            let (x, y) = self.split(a_lo, a_hi, b_lo, b_hi);
            let progress = (x, y) != (a_lo, b_lo) && (x, y) != (a_hi, b_hi);
            if progress {
                self.compare(a_lo, x, b_lo, y);
                self.compare(x, a_hi, y, b_hi);
            }
        }
        self.pairs.extend((0..suffix).map(|i| (a_hi + i, b_hi + i)));
    }

    /// A point on a shortest edit path through the box, found from both ends (the middle snake).
    fn split(&mut self, a_lo: usize, a_hi: usize, b_lo: usize, b_hi: usize) -> (usize, usize) {
        let n = (a_hi - a_lo) as isize;
        let m = (b_hi - b_lo) as isize;
        let delta = n - m;
        let odd = delta % 2 != 0;
        let max = ((n + m + 1) / 2 + 1).min(MAX_EDIT_DISTANCE);
        let offset = max;
        let size = (2 * max + 2) as usize;
        self.forward.clear();
        self.forward.resize(size, 0);
        self.backward.clear();
        self.backward.resize(size, 0);

        for d in 0..max {
            for k in (-d..=d).step_by(2) {
                let i = (k + offset) as usize;
                let mut x = if k == -d || (k != d && self.forward[i - 1] < self.forward[i + 1]) {
                    self.forward[i + 1]
                } else {
                    self.forward[i - 1] + 1
                };
                let mut y = x - k;
                while x < n && y < m && self.a[a_lo + x as usize] == self.b[b_lo + y as usize] {
                    x += 1;
                    y += 1;
                }
                self.forward[i] = x;
                let kb = delta - k;
                if odd && (-(d - 1)..=d - 1).contains(&kb) {
                    let xb = self.backward[(kb + offset) as usize];
                    if x + xb >= n {
                        return (a_lo + x as usize, b_lo + y as usize);
                    }
                }
            }
            for k in (-d..=d).step_by(2) {
                let i = (k + offset) as usize;
                let mut x = if k == -d || (k != d && self.backward[i - 1] < self.backward[i + 1]) {
                    self.backward[i + 1]
                } else {
                    self.backward[i - 1] + 1
                };
                let mut y = x - k;
                while x < n
                    && y < m
                    && self.a[a_hi - 1 - x as usize] == self.b[b_hi - 1 - y as usize]
                {
                    x += 1;
                    y += 1;
                }
                self.backward[i] = x;
                let kf = delta - k;
                if !odd && (-d..=d).contains(&kf) {
                    let xf = self.forward[(kf + offset) as usize];
                    if x + xf >= n {
                        return (a_hi - x as usize, b_hi - y as usize);
                    }
                }
            }
        }
        // Budget exhausted: split in the middle, which is correct but may not be minimal.
        (a_lo + n as usize / 2, b_lo + m as usize / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    fn hunk(kind: HunkKind, left: Range<usize>, right: Range<usize>) -> Hunk {
        Hunk { kind, left, right }
    }

    #[test]
    fn finds_changed_lines_against_a_baseline() {
        let baseline = line_hashes(&doc("a\nb\nc\nd"));
        let current = line_hashes(&doc("a\nB\nc\nx\ny"));
        assert_eq!(changed_lines(&baseline, &current), vec![1..2, 3..5]);
        assert!(changed_lines(&baseline, &baseline).is_empty());
        let removed = line_hashes(&doc("a\nd"));
        assert!(changed_lines(&baseline, &removed).is_empty());
    }

    /// Length of the longest common subsequence, by dynamic programming.
    fn lcs(a: &[u8], b: &[u8]) -> usize {
        let mut table = vec![vec![0; b.len() + 1]; a.len() + 1];
        for i in 0..a.len() {
            for j in 0..b.len() {
                table[i + 1][j + 1] = if a[i] == b[j] {
                    table[i][j] + 1
                } else {
                    table[i][j + 1].max(table[i + 1][j])
                };
            }
        }
        table[a.len()][b.len()]
    }

    #[test]
    fn finds_minimal_matches() {
        let mut seed = 12345u64;
        let mut random = |len: usize| -> Vec<u8> {
            (0..len)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    (seed >> 60) as u8 % 4
                })
                .collect()
        };
        for round in 0..300 {
            let a = random(round % 23);
            let b = random(round % 17 + 1);
            let pairs = matching_pairs(&a, &b);
            assert_eq!(pairs.len(), lcs(&a, &b), "{a:?} / {b:?}");
            assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
            assert!(pairs.iter().all(|&(x, y)| a[x] == b[y]));
        }
    }

    #[test]
    fn identical_documents_have_no_hunks() {
        let comparison = compare(&doc("a\nb\n"), &doc("a\nb\n"), DiffOptions::default());
        assert!(comparison.is_identical());
    }

    #[test]
    fn classifies_hunks() {
        let left = doc("same\nold\nkeep\ngone\nend");
        let right = doc("same\nnew\nkeep\nend\nextra");
        let comparison = compare(&left, &right, DiffOptions::default());
        assert_eq!(
            comparison.hunks,
            vec![
                hunk(HunkKind::Changed, 1..2, 1..2),
                hunk(HunkKind::Removed, 3..4, 3..3),
                hunk(HunkKind::Added, 5..5, 4..5),
            ]
        );
        assert_eq!(comparison.spacers(), (vec![(5, 1)], vec![(3, 1)]));
    }

    #[test]
    fn marks_changed_chars() {
        let comparison = compare(
            &doc("x\nlet a = 1;"),
            &doc("x\nlet b = 12;"),
            DiffOptions::default(),
        );
        assert_eq!(comparison.left_changes, vec![6..7]);
        assert_eq!(comparison.right_changes, vec![6..7, 11..12]);
    }

    #[test]
    fn can_ignore_whitespace_and_case() {
        let (left, right) = (doc("Hello  World"), doc("hello world"));
        assert!(!compare(&left, &right, DiffOptions::default()).is_identical());
        let options = DiffOptions {
            ignore_whitespace: true,
            ignore_case: true,
        };
        assert!(compare(&left, &right, options).is_identical());
    }

    #[test]
    fn completely_different_files_stay_fast() {
        let left: String = (0..20_000).map(|i| format!("left {i}\n")).collect();
        let right: String = (0..20_000).map(|i| format!("right {i}\n")).collect();
        let start = std::time::Instant::now();
        let comparison = compare(&doc(&left), &doc(&right), DiffOptions::default());
        assert_eq!(comparison.hunks.len(), 1);
        assert!(start.elapsed().as_secs() < 10);
    }
}
