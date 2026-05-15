use ratatui::style::Color;

use crate::diff::{ChangedFile, DiffLine};

/// The minimum unit of styled terminal output: a string fragment with fg and bg colors.
/// `bg: None` means "inherit the terminal default" — no background override applied.
#[derive(Debug, Clone)]
pub struct Segment {
    pub content: String,
    pub fg: Color,
    pub bg: Option<Color>,
}

/// A named byte range within a content string, used to specify where color overrides apply.
/// Always refers to byte indices (not char indices) into the full line content string.
#[derive(Debug, Clone, PartialEq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

/// A parsed diff line paired with its pre-computed rendering segments.
/// Segments tile the full `diff_line.content` string without gaps.
#[derive(Debug, Clone)]
pub struct RichLine {
    pub diff_line: DiffLine,
    pub segments: Vec<Segment>,
}

/// A diff hunk with pre-computed rendering for each line.
#[derive(Debug, Clone)]
pub struct RichHunk {
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<RichLine>,
}

/// A complete diff for one file, with all lines pre-enriched for rendering.
#[derive(Debug, Clone)]
pub struct RichDiffFile {
    pub file: ChangedFile,
    pub hunks: Vec<RichHunk>,
}

/// Split `segments` at `ranges` boundaries, applying a new foreground color to
/// sub-segments that fall within a range. Segments must tile the full string
/// without gaps. Ranges must be non-overlapping byte ranges in that same string.
pub fn apply_fg_ranges(segments: Vec<Segment>, ranges: &[(ByteRange, Color)]) -> Vec<Segment> {
    apply_ranges(segments, ranges, |seg, color| seg.fg = color)
}

/// Split `segments` at `ranges` boundaries, applying a new background color to
/// sub-segments that fall within a range.
pub fn apply_bg_ranges(segments: Vec<Segment>, ranges: &[(ByteRange, Color)]) -> Vec<Segment> {
    apply_ranges(segments, ranges, |seg, color| seg.bg = Some(color))
}

/// Core algorithm shared by `apply_fg_ranges` and `apply_bg_ranges`.
///
/// Walks the segments in order, tracking each segment's absolute byte offset
/// in the full string. For each segment, collects range boundary points that
/// fall within it, splits at those points, then applies the color closure to
/// sub-segments that fall inside a range.
fn apply_ranges(
    segments: Vec<Segment>,
    ranges: &[(ByteRange, Color)],
    apply: impl Fn(&mut Segment, Color),
) -> Vec<Segment> {
    if ranges.is_empty() {
        return segments;
    }

    let mut result = Vec::with_capacity(segments.len());
    let mut seg_offset = 0usize;

    for seg in segments {
        let seg_end = seg_offset + seg.content.len();

        // Collect split points (relative to segment start) from range boundaries
        // that land strictly inside this segment — boundaries at the segment
        // edges don't require a split.
        let mut splits: Vec<usize> = Vec::new();
        for (range, _) in ranges {
            if range.start > seg_offset && range.start < seg_end {
                splits.push(range.start - seg_offset);
            }
            if range.end > seg_offset && range.end < seg_end {
                splits.push(range.end - seg_offset);
            }
        }
        splits.sort_unstable();
        splits.dedup();
        splits.push(seg.content.len()); // sentinel: always produce the trailing sub-segment

        let fg = seg.fg;
        let bg = seg.bg;
        let content = seg.content;
        let mut prev = 0usize;

        for split in splits {
            if split == prev {
                continue; // skip zero-width sub-segments
            }
            let sub_content = content[prev..split].to_string();
            let sub_abs_start = seg_offset + prev;
            let sub_abs_end = seg_offset + split;

            let mut s = Segment { content: sub_content, fg, bg };
            // Apply the first range that fully contains this sub-segment.
            for (range, color) in ranges {
                if range.start <= sub_abs_start && sub_abs_end <= range.end {
                    apply(&mut s, *color);
                    break;
                }
            }
            result.push(s);
            prev = split;
        }

        seg_offset = seg_end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn seg(content: &str, fg: Color, bg: Option<Color>) -> Segment {
        Segment { content: content.to_string(), fg, bg }
    }

    fn br(start: usize, end: usize) -> ByteRange {
        ByteRange { start, end }
    }

    fn range(start: usize, end: usize, color: Color) -> (ByteRange, Color) {
        (br(start, end), color)
    }

    fn contents(segs: &[Segment]) -> Vec<&str> {
        segs.iter().map(|s| s.content.as_str()).collect()
    }

    // ── apply_fg_ranges ───────────────────────────────────────────────────────

    #[test]
    fn test_fg_empty_ranges_returns_segments_unchanged() {
        let input = vec![seg("hello world", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[]);
        assert_eq!(contents(&result), vec!["hello world"]);
        assert_eq!(result[0].fg, Color::Gray);
    }

    #[test]
    fn test_fg_range_covers_full_segment() {
        let input = vec![seg("hello", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[range(0, 5, Color::Red)]);
        assert_eq!(contents(&result), vec!["hello"]);
        assert_eq!(result[0].fg, Color::Red);
    }

    #[test]
    fn test_fg_range_within_segment_splits_into_three() {
        let input = vec![seg("hello world", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[range(2, 7, Color::Red)]);
        assert_eq!(contents(&result), vec!["he", "llo w", "orld"]);
        assert_eq!(result[0].fg, Color::Gray);
        assert_eq!(result[1].fg, Color::Red);
        assert_eq!(result[2].fg, Color::Gray);
    }

    #[test]
    fn test_fg_range_at_segment_start() {
        let input = vec![seg("hello world", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[range(0, 5, Color::Red)]);
        assert_eq!(contents(&result), vec!["hello", " world"]);
        assert_eq!(result[0].fg, Color::Red);
        assert_eq!(result[1].fg, Color::Gray);
    }

    #[test]
    fn test_fg_range_at_segment_end() {
        let input = vec![seg("hello world", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[range(6, 11, Color::Red)]);
        assert_eq!(contents(&result), vec!["hello ", "world"]);
        assert_eq!(result[0].fg, Color::Gray);
        assert_eq!(result[1].fg, Color::Red);
    }

    #[test]
    fn test_fg_range_spanning_two_segments() {
        let input = vec![
            seg("hello ", Color::Gray, None),
            seg("world",  Color::Gray, None),
        ];
        // range covers "o world" → bytes 4..11
        let result = apply_fg_ranges(input, &[range(4, 9, Color::Red)]);
        // seg1 [0,6): split at 4 → "hell"[Gray], "o "[Red]
        // seg2 [6,11): split at 9-6=3 → "wor"[Red], "ld"[Gray]
        assert_eq!(contents(&result), vec!["hell", "o ", "wor", "ld"]);
        assert_eq!(result[0].fg, Color::Gray);
        assert_eq!(result[1].fg, Color::Red);
        assert_eq!(result[2].fg, Color::Red);
        assert_eq!(result[3].fg, Color::Gray);
    }

    #[test]
    fn test_fg_multiple_non_overlapping_ranges() {
        let input = vec![seg("abcdefghij", Color::Gray, None)];
        let result = apply_fg_ranges(input, &[
            range(0, 3, Color::Red),
            range(6, 9, Color::Blue),
        ]);
        // "abc"[Red] "def"[Gray] "ghi"[Blue] "j"[Gray]
        assert_eq!(contents(&result), vec!["abc", "def", "ghi", "j"]);
        assert_eq!(result[0].fg, Color::Red);
        assert_eq!(result[1].fg, Color::Gray);
        assert_eq!(result[2].fg, Color::Blue);
        assert_eq!(result[3].fg, Color::Gray);
    }

    #[test]
    fn test_fg_preserves_existing_bg() {
        let bg = Some(Color::Rgb(0, 60, 0));
        let input = vec![seg("hello", Color::Gray, bg)];
        let result = apply_fg_ranges(input, &[range(0, 5, Color::White)]);
        assert_eq!(result[0].bg, bg);
        assert_eq!(result[0].fg, Color::White);
    }

    // ── apply_bg_ranges ───────────────────────────────────────────────────────

    #[test]
    fn test_bg_range_sets_background_on_covered_part() {
        let input = vec![seg("hello world", Color::White, None)];
        let result = apply_bg_ranges(input, &[range(0, 5, Color::Rgb(0, 60, 0))]);
        // "hello"[bg=green], " world"[bg=None]
        assert_eq!(contents(&result), vec!["hello", " world"]);
        assert_eq!(result[0].bg, Some(Color::Rgb(0, 60, 0)));
        assert_eq!(result[1].bg, None);
    }

    #[test]
    fn test_bg_range_preserves_fg() {
        let input = vec![seg("hello", Color::Green, None)];
        let result = apply_bg_ranges(input, &[range(0, 5, Color::Rgb(0, 100, 0))]);
        assert_eq!(result[0].fg, Color::Green);
        assert_eq!(result[0].bg, Some(Color::Rgb(0, 100, 0)));
    }

    #[test]
    fn test_bg_none_preserved_for_uncovered_parts() {
        let input = vec![seg("hello world", Color::White, None)];
        let result = apply_bg_ranges(input, &[range(6, 11, Color::Rgb(0, 60, 0))]);
        assert_eq!(result[0].bg, None);        // "hello "
        assert_eq!(result[1].bg, Some(Color::Rgb(0, 60, 0))); // "world"
    }

    // ── Multi-byte character handling ─────────────────────────────────────────

    #[test]
    fn test_multibyte_split_at_char_boundary() {
        // "café" = b'c'(1) b'a'(1) b'f'(1) b'\xc3\xa9'(2) = 5 bytes total
        let input = vec![seg("café", Color::Gray, None)];
        // Range covering "caf" = bytes [0, 3)
        let result = apply_fg_ranges(input, &[range(0, 3, Color::Red)]);
        assert_eq!(contents(&result), vec!["caf", "é"]);
        assert_eq!(result[0].fg, Color::Red);
        assert_eq!(result[1].fg, Color::Gray);
    }

    #[test]
    fn test_multibyte_range_covering_multibyte_char() {
        // "héllo" = b'h'(1) b'\xc3\xa9'(2) b'l'(1) b'l'(1) b'o'(1) = 6 bytes
        let input = vec![seg("héllo", Color::Gray, None)];
        // Range covering "é" = bytes [1, 3)
        let result = apply_fg_ranges(input, &[range(1, 3, Color::Red)]);
        assert_eq!(contents(&result), vec!["h", "é", "llo"]);
        assert_eq!(result[1].fg, Color::Red);
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn test_empty_segments_list() {
        let result = apply_fg_ranges(vec![], &[range(0, 5, Color::Red)]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_range_beyond_content_does_not_panic() {
        let input = vec![seg("hi", Color::Gray, None)];
        // Range extends past content — just covers what overlaps
        let result = apply_fg_ranges(input, &[range(0, 100, Color::Red)]);
        assert_eq!(contents(&result), vec!["hi"]);
        assert_eq!(result[0].fg, Color::Red);
    }

    #[test]
    fn test_multiple_segments_no_ranges() {
        let input = vec![
            seg("foo", Color::Red,  None),
            seg("bar", Color::Blue, None),
        ];
        let result = apply_fg_ranges(input, &[]);
        assert_eq!(contents(&result), vec!["foo", "bar"]);
        assert_eq!(result[0].fg, Color::Red);
        assert_eq!(result[1].fg, Color::Blue);
    }

    #[test]
    fn test_range_exactly_matches_segment_boundary() {
        let input = vec![
            seg("hello", Color::Gray, None),
            seg(" world", Color::Gray, None),
        ];
        // Range exactly covers the first segment [0, 5)
        let result = apply_fg_ranges(input, &[range(0, 5, Color::Red)]);
        // No split needed — boundary at 5 is already a segment boundary
        assert_eq!(contents(&result), vec!["hello", " world"]);
        assert_eq!(result[0].fg, Color::Red);
        assert_eq!(result[1].fg, Color::Gray);
    }
}
