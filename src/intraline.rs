use similar::{DiffTag, TextDiff};

use crate::diff::{DiffFile, Hunk, LineKind};
use crate::segment::ByteRange;

/// Per-line intraline highlight data: `Some(ranges)` means this line has
/// character-level changed regions; `None` means no intraline highlight
/// (unpaired pure additions/deletions, or context lines).
pub type LineIntraline = Option<Vec<ByteRange>>;

/// Indexed as `[hunk_idx][line_idx]` → changed byte ranges for that line,
/// or `None` when no intraline highlight applies.
pub type IntralineMap = Vec<Vec<LineIntraline>>;

/// Compute character-level changed byte ranges for every line in `diff`.
///
/// Within each hunk, consecutive runs of removed lines followed by added
/// lines are paired 1:1 (up to `min(removed_count, added_count)`). Each
/// pair is diffed at the character level using `similar`. Unpaired lines
/// (pure additions or pure deletions beyond the shorter run) receive `None`.
pub fn compute_intraline_map(diff: &DiffFile) -> IntralineMap {
    diff.hunks.iter().map(compute_hunk_intraline).collect()
}

fn compute_hunk_intraline(hunk: &Hunk) -> Vec<LineIntraline> {
    let mut line_ranges: Vec<LineIntraline> = vec![None; hunk.lines.len()];
    for (removed_idx, added_idx) in pair_hunk_lines(hunk) {
        let old_content = &hunk.lines[removed_idx].content;
        let new_content = &hunk.lines[added_idx].content;
        let (old_ranges, new_ranges) = char_diff_ranges(old_content, new_content);
        if !old_ranges.is_empty() {
            line_ranges[removed_idx] = Some(old_ranges);
        }
        if !new_ranges.is_empty() {
            line_ranges[added_idx] = Some(new_ranges);
        }
    }
    line_ranges
}

/// Pair consecutive removed/added line runs within a hunk.
///
/// Scans linearly through the hunk lines. When a run of `Removed` lines is
/// immediately followed by a run of `Added` lines, the first `min(N, M)` are
/// paired by position. Context lines reset the search.
///
/// Returns a list of `(removed_line_idx, added_line_idx)` pairs — indices
/// into `hunk.lines`.
pub(crate) fn pair_hunk_lines(hunk: &Hunk) -> Vec<(usize, usize)> {
    let lines = &hunk.lines;
    let mut pairs = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].kind == LineKind::Context {
            i += 1;
            continue;
        }

        // Collect a run of Removed lines.
        let removed_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Removed {
            i += 1;
        }
        let removed_end = i;

        // Collect the immediately following run of Added lines.
        let added_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Added {
            i += 1;
        }
        let added_end = i;

        // Only pair when we have both sides.
        if removed_start < removed_end && added_start < added_end {
            let pair_count = (removed_end - removed_start).min(added_end - added_start);
            for k in 0..pair_count {
                pairs.push((removed_start + k, added_start + k));
            }
        }
        // If we stalled (no removed and no added at current position), skip
        // the current line to avoid an infinite loop on Added-only runs.
        if i == removed_start {
            i += 1;
        }
    }

    pairs
}

/// Diff `old` vs `new` at character granularity and return the byte ranges of
/// changed characters in the old and new strings respectively.
///
/// Ranges are computed from `similar`'s char-index ops, then converted to byte
/// offsets so callers can use them directly with `apply_bg_ranges`.
pub(crate) fn char_diff_ranges(old: &str, new: &str) -> (Vec<ByteRange>, Vec<ByteRange>) {
    let old_char_to_byte = char_to_byte_map(old);
    let new_char_to_byte = char_to_byte_map(new);

    let diff = TextDiff::from_chars(old, new);
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();

    for op in diff.ops() {
        match op.tag() {
            DiffTag::Delete | DiffTag::Replace => {
                old_ranges.push(char_range_to_byte_range(
                    &old_char_to_byte, old, op.old_range(),
                ));
            }
            _ => {}
        }
        match op.tag() {
            DiffTag::Insert | DiffTag::Replace => {
                new_ranges.push(char_range_to_byte_range(
                    &new_char_to_byte, new, op.new_range(),
                ));
            }
            _ => {}
        }
    }

    (old_ranges, new_ranges)
}

/// Build a table mapping char index → byte offset for `s`.
/// The entry at index `char_count` maps to `s.len()` (one-past-end sentinel).
fn char_to_byte_map(s: &str) -> Vec<usize> {
    let mut map: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    map.push(s.len());
    map
}

fn char_range_to_byte_range(
    char_to_byte: &[usize],
    s: &str,
    range: std::ops::Range<usize>,
) -> ByteRange {
    let start = char_to_byte.get(range.start).copied().unwrap_or(s.len());
    let end   = char_to_byte.get(range.end).copied().unwrap_or(s.len());
    ByteRange { start, end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use std::path::PathBuf;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn dl(kind: LineKind, content: &str) -> DiffLine {
        DiffLine {
            old_lineno: Some(1),
            new_lineno: Some(1),
            kind,
            content: content.to_string(),
        }
    }

    fn hunk_from_lines(lines: Vec<DiffLine>) -> Hunk {
        Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            old_start: 1,
            new_start: 1,
            lines,
        }
    }

    fn diff_with_hunks(hunks: Vec<Hunk>) -> DiffFile {
        DiffFile {
            file: ChangedFile { path: PathBuf::from("f.rs"), status: FileStatus::Modified },
            hunks,
        }
    }

    // ── pair_hunk_lines ───────────────────────────────────────────────────────

    #[test]
    fn test_pair_simple_removed_then_added() {
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Removed, "old"),
            dl(LineKind::Added,   "new"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![(0, 1)]);
    }

    #[test]
    fn test_pair_more_removed_than_added() {
        // 3 removed, 2 added → 2 pairs; third removed is unpaired
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Removed, "r1"),
            dl(LineKind::Removed, "r2"),
            dl(LineKind::Removed, "r3"),
            dl(LineKind::Added,   "a1"),
            dl(LineKind::Added,   "a2"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![(0, 3), (1, 4)]);
    }

    #[test]
    fn test_pair_more_added_than_removed() {
        // 2 removed, 3 added → 2 pairs; third added is unpaired
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Removed, "r1"),
            dl(LineKind::Removed, "r2"),
            dl(LineKind::Added,   "a1"),
            dl(LineKind::Added,   "a2"),
            dl(LineKind::Added,   "a3"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![(0, 2), (1, 3)]);
    }

    #[test]
    fn test_pair_context_resets_runs() {
        // Two separate removed/added pairs separated by context
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Removed, "r1"),
            dl(LineKind::Added,   "a1"),
            dl(LineKind::Context, "ctx"),
            dl(LineKind::Removed, "r2"),
            dl(LineKind::Added,   "a2"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![(0, 1), (3, 4)]);
    }

    #[test]
    fn test_pair_pure_addition_not_paired() {
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Added, "new line"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![]);
    }

    #[test]
    fn test_pair_pure_deletion_not_paired() {
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Removed, "old line"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![]);
    }

    #[test]
    fn test_pair_only_context_produces_no_pairs() {
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Context, "c1"),
            dl(LineKind::Context, "c2"),
        ]);
        assert_eq!(pair_hunk_lines(&hunk), vec![]);
    }

    #[test]
    fn test_pair_added_before_removed_not_paired() {
        // Added then Removed (reversed order) — should not pair
        let hunk = hunk_from_lines(vec![
            dl(LineKind::Added,   "a1"),
            dl(LineKind::Removed, "r1"),
        ]);
        // No removed-then-added run, so no pairs
        assert_eq!(pair_hunk_lines(&hunk), vec![]);
    }

    // ── char_diff_ranges ──────────────────────────────────────────────────────

    #[test]
    fn test_char_diff_identical_strings_no_ranges() {
        let (old, new) = char_diff_ranges("hello", "hello");
        assert!(old.is_empty());
        assert!(new.is_empty());
    }

    #[test]
    fn test_char_diff_one_char_changed() {
        // "hello" → "hXllo": 'e' replaced by 'X'
        let (old, new) = char_diff_ranges("hello", "hXllo");
        assert_eq!(old, vec![ByteRange { start: 1, end: 2 }]);
        assert_eq!(new, vec![ByteRange { start: 1, end: 2 }]);
    }

    #[test]
    fn test_char_diff_word_changed() {
        let (old, new) = char_diff_ranges("foo bar", "foo baz");
        // 'r' → 'z' at byte 6
        assert!(!old.is_empty());
        assert!(!new.is_empty());
        // Changed region must be within the word
        for r in &old { assert!(r.start >= 4); }
        for r in &new { assert!(r.start >= 4); }
    }

    #[test]
    fn test_char_diff_multibyte_change() {
        // "café" → "cafe": 'é'(2 bytes) replaced by 'e'(1 byte)
        let (old, new) = char_diff_ranges("café", "cafe");
        // Old: 'é' at bytes [3,5)
        assert!(old.iter().any(|r| r.start == 3 && r.end == 5));
        // New: 'e' at bytes [3,4)
        assert!(new.iter().any(|r| r.start == 3 && r.end == 4));
    }

    #[test]
    fn test_char_diff_fully_different_strings() {
        let (old, new) = char_diff_ranges("abc", "xyz");
        // All characters changed — ranges cover the full strings
        let old_bytes: usize = old.iter().map(|r| r.end - r.start).sum();
        let new_bytes: usize = new.iter().map(|r| r.end - r.start).sum();
        assert_eq!(old_bytes, 3);
        assert_eq!(new_bytes, 3);
    }

    // ── compute_intraline_map ─────────────────────────────────────────────────

    #[test]
    fn test_intraline_map_paired_lines_get_ranges() {
        let diff = diff_with_hunks(vec![hunk_from_lines(vec![
            dl(LineKind::Removed, "hello world"),
            dl(LineKind::Added,   "hello earth"),
        ])]);
        let map = compute_intraline_map(&diff);
        // Both lines are paired → both should have Some ranges
        assert!(map[0][0].is_some(), "removed line should have intraline ranges");
        assert!(map[0][1].is_some(), "added line should have intraline ranges");
    }

    #[test]
    fn test_intraline_map_pure_addition_is_none() {
        let diff = diff_with_hunks(vec![hunk_from_lines(vec![
            dl(LineKind::Added, "brand new line"),
        ])]);
        let map = compute_intraline_map(&diff);
        assert!(map[0][0].is_none(), "pure addition should have no intraline ranges");
    }

    #[test]
    fn test_intraline_map_context_is_none() {
        let diff = diff_with_hunks(vec![hunk_from_lines(vec![
            dl(LineKind::Context, "unchanged"),
        ])]);
        let map = compute_intraline_map(&diff);
        assert!(map[0][0].is_none());
    }

    #[test]
    fn test_intraline_map_identical_pair_produces_no_ranges() {
        // If old == new (e.g. whitespace-only diff edge case), no changed chars
        let diff = diff_with_hunks(vec![hunk_from_lines(vec![
            dl(LineKind::Removed, "same line"),
            dl(LineKind::Added,   "same line"),
        ])]);
        let map = compute_intraline_map(&diff);
        // No changed chars → both lines stay None (empty ranges become None)
        assert!(map[0][0].is_none());
        assert!(map[0][1].is_none());
    }

    #[test]
    fn test_intraline_map_structure_matches_diff() {
        let diff = diff_with_hunks(vec![
            hunk_from_lines(vec![dl(LineKind::Removed, "r"), dl(LineKind::Added, "a")]),
            hunk_from_lines(vec![dl(LineKind::Context, "c")]),
        ]);
        let map = compute_intraline_map(&diff);
        assert_eq!(map.len(), 2);
        assert_eq!(map[0].len(), 2);
        assert_eq!(map[1].len(), 1);
    }
}
