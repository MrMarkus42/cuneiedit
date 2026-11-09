/// What a visual row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowKind {
    /// A blank alignment row above the line.
    Spacer,
    /// The `n`-th visual row of the (possibly wrapped) line.
    Line(usize),
}

/// One line with something special: `above` spacer rows, `extra` wrapped rows and `hidden`
/// folded lines after it.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    line: usize,
    above: usize,
    extra: usize,
    hidden: usize,
}

impl Entry {
    fn delta(&self) -> isize {
        (self.above + self.extra) as isize - self.hidden as isize
    }
}

/// Maps document lines to visual rows, with blank spacer rows above some lines, extra rows
/// for wrapped lines and folded (hidden) lines.
#[derive(Debug, Default)]
pub(crate) struct RowMap {
    /// Sorted by line; hidden lines have no entries.
    entries: Vec<Entry>,
    /// Rows minus lines up to and including each entry.
    cumulative: Vec<isize>,
}

impl RowMap {
    /// `spacers` and `wraps` are `(line, count)` pairs in any order; `folds` are
    /// `(header line, hidden lines after it)`. Folds inside other folds are ignored.
    pub fn new(
        spacers: &[(usize, usize)],
        wraps: &[(usize, usize)],
        folds: &[(usize, usize)],
    ) -> Self {
        let mut sorted_folds: Vec<(usize, usize)> =
            folds.iter().copied().filter(|&(_, n)| n > 0).collect();
        sorted_folds.sort_unstable();
        let mut hidden: Vec<(usize, usize)> = Vec::with_capacity(sorted_folds.len());
        for (header, count) in sorted_folds {
            if hidden.last().is_some_and(|&(h, n)| header <= h + n) {
                continue;
            }
            hidden.push((header, count));
        }
        let is_hidden = |line: usize| {
            let i = hidden.partition_point(|&(h, _)| h < line);
            i > 0 && line <= hidden[i - 1].0 + hidden[i - 1].1
        };

        let sorted_wraps = wraps.windows(2).all(|w| w[0].0 < w[1].0);
        if spacers.is_empty() && hidden.is_empty() && sorted_wraps {
            return Self::from_entries(
                wraps
                    .iter()
                    .filter(|&&(_, extra)| extra > 0)
                    .map(|&(line, extra)| Entry {
                        line,
                        extra,
                        ..Entry::default()
                    })
                    .collect(),
            );
        }

        let mut all: Vec<Entry> = spacers
            .iter()
            .map(|&(line, above)| Entry {
                line,
                above,
                ..Entry::default()
            })
            .chain(wraps.iter().map(|&(line, extra)| Entry {
                line,
                extra,
                ..Entry::default()
            }))
            .filter(|e| e.above + e.extra > 0 && !is_hidden(e.line))
            .chain(hidden.iter().map(|&(line, count)| Entry {
                line,
                hidden: count,
                ..Entry::default()
            }))
            .collect();
        all.sort_unstable_by_key(|e| e.line);
        let mut entries: Vec<Entry> = Vec::with_capacity(all.len());
        for entry in all {
            match entries.last_mut() {
                Some(last) if last.line == entry.line => {
                    last.above += entry.above;
                    last.extra += entry.extra;
                    last.hidden += entry.hidden;
                }
                _ => entries.push(entry),
            }
        }
        Self::from_entries(entries)
    }

    fn from_entries(entries: Vec<Entry>) -> Self {
        let cumulative = entries
            .iter()
            .scan(0, |sum, entry| {
                *sum += entry.delta();
                Some(*sum)
            })
            .collect();
        Self {
            entries,
            cumulative,
        }
    }

    fn rows_before_entry(&self, index: usize) -> isize {
        index.checked_sub(1).map_or(0, |i| self.cumulative[i])
    }

    /// First visual row of `line` (below its spacers); for a hidden line, that of its fold.
    pub fn row(&self, line: usize) -> usize {
        let index = self.entries.partition_point(|e| e.line < line);
        if let Some(prev) = index.checked_sub(1).map(|i| self.entries[i])
            && prev.hidden > 0
            && line <= prev.line + prev.hidden
        {
            return self.row(prev.line);
        }
        let above = match self.entries.get(index) {
            Some(e) if e.line == line => e.above,
            _ => 0,
        };
        (line as isize + self.rows_before_entry(index)).max(0) as usize + above
    }

    pub fn total_rows(&self, line_count: usize) -> usize {
        (line_count as isize + self.cumulative.last().copied().unwrap_or(0)).max(0) as usize
    }

    /// Number of folded lines right after `line`.
    pub fn hidden_after(&self, line: usize) -> usize {
        let index = self.entries.partition_point(|e| e.line < line);
        match self.entries.get(index) {
            Some(e) if e.line == line => e.hidden,
            _ => 0,
        }
    }

    /// Line shown at `row`, and which part of it.
    pub fn line_at(&self, row: usize) -> (usize, RowKind) {
        let row = row as isize;
        let block_start = |i: usize| self.entries[i].line as isize + self.rows_before_entry(i);
        let (mut low, mut high) = (0, self.entries.len());
        while low < high {
            let mid = (low + high) / 2;
            if block_start(mid) <= row {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let Some(i) = low.checked_sub(1) else {
            return (row as usize, RowKind::Line(0));
        };
        let entry = self.entries[i];
        let first_row = block_start(i) + entry.above as isize;
        if row < first_row {
            (entry.line, RowKind::Spacer)
        } else if row <= first_row + entry.extra as isize {
            (entry.line, RowKind::Line((row - first_row) as usize))
        } else {
            ((row - self.cumulative[i]) as usize, RowKind::Line(0))
        }
    }

    /// Spacer blocks above lines in `lines`, as `(first row, count)`.
    pub fn blocks(
        &self,
        lines: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (usize, usize)> + '_ {
        let start = self.entries.partition_point(|e| e.line < lines.start);
        self.entries[start..]
            .iter()
            .take_while(move |e| e.line < lines.end)
            .filter(|e| e.above > 0)
            .map(move |e| (self.row(e.line) - e.above, e.above))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_spacers() {
        let rows = RowMap::new(&[(5, 1), (2, 2), (2, 1), (9, 0)], &[], &[]);
        // Rows: 0 1 S S S 2 3 4 S 5 6 ...
        assert_eq!(
            (0..7).map(|l| rows.row(l)).collect::<Vec<_>>(),
            vec![0, 1, 5, 6, 7, 9, 10]
        );
        assert_eq!(rows.total_rows(7), 11);
        assert_eq!(rows.line_at(1), (1, RowKind::Line(0)));
        assert_eq!(rows.line_at(2), (2, RowKind::Spacer));
        assert_eq!(rows.line_at(4), (2, RowKind::Spacer));
        assert_eq!(rows.line_at(5), (2, RowKind::Line(0)));
        assert_eq!(rows.line_at(8), (5, RowKind::Spacer));
        assert_eq!(rows.line_at(9), (5, RowKind::Line(0)));
        assert_eq!(rows.line_at(20), (16, RowKind::Line(0)));
        assert_eq!(rows.blocks(0..6).collect::<Vec<_>>(), vec![(2, 3), (8, 1)]);
        assert_eq!(rows.blocks(3..5).count(), 0);
    }

    #[test]
    fn maps_wrapped_lines() {
        // Line 1 takes 3 rows, line 3 has a spacer and 1 extra row.
        let rows = RowMap::new(&[(3, 1)], &[(1, 2), (3, 1)], &[]);
        // Rows: 0:L0 1:L1 2:L1 3:L1 4:L2 5:S 6:L3 7:L3 8:L4
        assert_eq!(
            (0..5).map(|l| rows.row(l)).collect::<Vec<_>>(),
            vec![0, 1, 4, 6, 8]
        );
        assert_eq!(rows.line_at(2), (1, RowKind::Line(1)));
        assert_eq!(rows.line_at(3), (1, RowKind::Line(2)));
        assert_eq!(rows.line_at(4), (2, RowKind::Line(0)));
        assert_eq!(rows.line_at(5), (3, RowKind::Spacer));
        assert_eq!(rows.line_at(7), (3, RowKind::Line(1)));
        assert_eq!(rows.line_at(8), (4, RowKind::Line(0)));
        assert_eq!(rows.total_rows(5), 9);
        assert_eq!(rows.blocks(0..5).collect::<Vec<_>>(), vec![(5, 1)]);
    }

    #[test]
    fn hides_folded_lines() {
        // Line 1 hides 2..=4 (and the nested fold at 2); the wrap of hidden line 3 is ignored.
        let rows = RowMap::new(&[], &[(6, 1), (3, 1)], &[(1, 3), (2, 1)]);
        // Rows: 0:L0 1:L1 2:L5 3:L6 4:L6 5:L7
        assert_eq!(
            (0..8).map(|l| rows.row(l)).collect::<Vec<_>>(),
            vec![0, 1, 1, 1, 1, 2, 3, 5]
        );
        assert_eq!(rows.line_at(1), (1, RowKind::Line(0)));
        assert_eq!(rows.line_at(2), (5, RowKind::Line(0)));
        assert_eq!(rows.line_at(4), (6, RowKind::Line(1)));
        assert_eq!(rows.line_at(5), (7, RowKind::Line(0)));
        assert_eq!(rows.total_rows(8), 6);
        assert_eq!(rows.hidden_after(1), 3);
        assert_eq!(rows.hidden_after(2), 0);
    }

    #[test]
    fn without_entries_rows_are_lines() {
        let rows = RowMap::default();
        assert_eq!(rows.row(7), 7);
        assert_eq!(rows.line_at(7), (7, RowKind::Line(0)));
        assert_eq!(rows.total_rows(3), 3);
    }
}
