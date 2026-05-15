use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::diff::{DiffFile, DiffLine, LineKind};
use crate::intraline::compute_intraline_map;
use crate::segment::{ByteRange, RichDiffFile, RichHunk, RichLine, Segment, apply_bg_ranges, apply_fg_ranges};

// ── Line-level background colors ─────────────────────────────────────────────

const ADDED_BG:   Color = Color::Rgb(0,  60,  0);
const REMOVED_BG: Color = Color::Rgb(70,  0,  0);

/// Brighter variants used for the changed characters within a paired line.
const ADDED_INTRALINE_BG:   Color = Color::Rgb(0,  100, 0);
const REMOVED_INTRALINE_BG: Color = Color::Rgb(120,  0, 0);

fn line_bg(kind: LineKind) -> Option<Color> {
    match kind {
        LineKind::Added   => Some(ADDED_BG),
        LineKind::Removed => Some(REMOVED_BG),
        LineKind::Context => None,
    }
}

fn fallback_fg(kind: LineKind) -> Color {
    match kind {
        LineKind::Added   => Color::Green,
        LineKind::Removed => Color::Red,
        LineKind::Context => Color::Gray,
    }
}

fn intraline_bg(kind: LineKind) -> Color {
    match kind {
        LineKind::Added   => ADDED_INTRALINE_BG,
        LineKind::Removed => REMOVED_INTRALINE_BG,
        LineKind::Context => ADDED_INTRALINE_BG, // unreachable in practice
    }
}

// ── SyntaxHighlighter ─────────────────────────────────────────────────────────

pub struct SyntaxHighlighter {
    /// syntect's built-in Sublime Text grammars (Rust, Python, HTML, …).
    default_set: SyntaxSet,
    /// two-face grammars for languages absent from the Sublime defaults
    /// (TypeScript, TSX, JSX, TOML, Dockerfile, …). Checked first so these
    /// grammars win when an extension appears in both sets.
    extra_set: SyntaxSet,
    theme: Theme,
    /// Theme background color, used to enforce the diff panel background.
    pub panel_bg: Color,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        let default_set = SyntaxSet::load_defaults_newlines();
        let extra_set = two_face::syntax::extra_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-ocean.dark"].clone();
        // Darker than the theme's own background so the panel feels closer to a
        // proper dark editor pane rather than the theme's medium-dark default.
        let panel_bg = Color::Rgb(18, 20, 26);
        Self { default_set, extra_set, theme, panel_bg }
    }

    /// Returns the best syntax and the SyntaxSet it belongs to.
    ///
    /// The extra set (TypeScript grammars) is checked first. `.jsx` is
    /// aliased to the TypeScriptReact grammar since it covers JSX syntax.
    fn find_syntax<'a>(&'a self, extension: &str) -> (&'a SyntaxReference, &'a SyntaxSet) {
        let lookup_ext = if extension == "jsx" { "tsx" } else { extension };

        if let Some(syn) = self.extra_set.find_syntax_by_extension(lookup_ext) {
            return (syn, &self.extra_set);
        }

        let syn = self
            .default_set
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| self.default_set.find_syntax_plain_text());
        (syn, &self.default_set)
    }

    /// Enrich `diff` into a `RichDiffFile` by running three enrichment passes:
    ///
    /// 1. **Base** — one segment per line with line-kind background and fallback fg.
    /// 2. **Syntax** — split segments at syntect token boundaries, applying fg colors.
    /// 3. **Intraline** — split segments at char-diff boundaries, applying brighter bg.
    pub fn enrich(&self, diff: &DiffFile) -> RichDiffFile {
        let extension = diff
            .file
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let (syntax, syntax_set) = self.find_syntax(extension);
        let intraline_map = compute_intraline_map(diff);

        let hunks = diff.hunks.iter().zip(intraline_map.iter()).map(|(hunk, hunk_intraline)| {
            // Fresh highlighter per hunk: hunks are non-contiguous excerpts so
            // grammar state from one hunk's last line is wrong for the next.
            let mut highlighter = HighlightLines::new(syntax, &self.theme);

            let lines = hunk.lines.iter().zip(hunk_intraline.iter()).map(|(dl, intraline)| {
                enrich_line(dl, intraline.as_deref(), &mut highlighter, syntax_set)
            }).collect();

            RichHunk {
                header: hunk.header.clone(),
                old_start: hunk.old_start,
                new_start: hunk.new_start,
                lines,
            }
        }).collect();

        RichDiffFile { file: diff.file.clone(), hunks }
    }
}

/// Enrich one `DiffLine` into a `RichLine` through the three passes.
fn enrich_line(
    dl: &DiffLine,
    intraline: Option<&[ByteRange]>,
    highlighter: &mut HighlightLines,
    syntax_set: &SyntaxSet,
) -> RichLine {
    // Pass 1 — base segment: full content, line-kind bg, fallback fg.
    let base_bg = line_bg(dl.kind);
    let base_fg = fallback_fg(dl.kind);
    let mut segments = vec![Segment {
        content: dl.content.clone(),
        fg: base_fg,
        bg: base_bg,
    }];

    // Pass 2 — syntax fg: convert syntect tokens to (ByteRange, Color) ranges.
    let line_with_newline = format!("{}\n", dl.content);
    if let Ok(tokens) = highlighter.highlight_line(&line_with_newline, syntax_set) {
        let syntax_ranges = syntax_tokens_to_ranges(&tokens);
        if !syntax_ranges.is_empty() {
            segments = apply_fg_ranges(segments, &syntax_ranges);
        }
    }

    // Pass 3 — intraline bg: apply brighter background to changed char ranges.
    if let Some(ranges) = intraline
        && !ranges.is_empty()
    {
        let bg_color = intraline_bg(dl.kind);
        let bg_ranges: Vec<(ByteRange, Color)> =
            ranges.iter().map(|r| (r.clone(), bg_color)).collect();
        segments = apply_bg_ranges(segments, &bg_ranges);
    }

    RichLine { diff_line: dl.clone(), segments }
}

/// Convert syntect `(Style, &str)` token pairs into `(ByteRange, Color)` ranges
/// over the line's byte space (excluding the trailing `\n` added for highlighting).
fn syntax_tokens_to_ranges(tokens: &[(syntect::highlighting::Style, &str)]) -> Vec<(ByteRange, Color)> {
    let mut ranges = Vec::with_capacity(tokens.len());
    let mut byte_offset = 0usize;
    for (style, text) in tokens {
        let content = text.trim_end_matches('\n');
        if content.is_empty() {
            continue;
        }
        let end = byte_offset + content.len();
        let color = Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        );
        ranges.push((ByteRange { start: byte_offset, end }, color));
        byte_offset = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};

    use super::*;

    fn make_diff(path: &str, lines: Vec<DiffLine>) -> DiffFile {
        DiffFile {
            file: ChangedFile {
                path: PathBuf::from(path),
                status: FileStatus::Modified,
            },
            hunks: vec![Hunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1,
                new_start: 1,
                lines,
            }],
        }
    }

    fn added(content: &str) -> DiffLine {
        DiffLine { old_lineno: None, new_lineno: Some(1), kind: LineKind::Added, content: content.to_string() }
    }

    fn removed(content: &str) -> DiffLine {
        DiffLine { old_lineno: Some(1), new_lineno: None, kind: LineKind::Removed, content: content.to_string() }
    }

    fn context(content: &str) -> DiffLine {
        DiffLine { old_lineno: Some(1), new_lineno: Some(1), kind: LineKind::Context, content: content.to_string() }
    }

    fn joined_content(rich: &RichDiffFile, hunk: usize, line: usize) -> String {
        rich.hunks[hunk].lines[line].segments.iter().map(|s| s.content.as_str()).collect()
    }

    // ── Structure ─────────────────────────────────────────────────────────────

    #[test]
    fn test_enrich_preserves_hunk_and_line_structure() {
        let hl = SyntaxHighlighter::new();
        let diff = DiffFile {
            file: ChangedFile { path: PathBuf::from("src/lib.rs"), status: FileStatus::Modified },
            hunks: vec![
                Hunk { header: "@@ -1,1 +1,2 @@".to_string(), old_start: 1, new_start: 1,
                    lines: vec![added("let x = 1;"), context("// comment")] },
                Hunk { header: "@@ -10,1 +10,1 @@".to_string(), old_start: 10, new_start: 10,
                    lines: vec![removed("old line")] },
            ],
        };
        let rich = hl.enrich(&diff);
        assert_eq!(rich.hunks.len(), 2);
        assert_eq!(rich.hunks[0].lines.len(), 2);
        assert_eq!(rich.hunks[1].lines.len(), 1);
    }

    #[test]
    fn test_enrich_content_fully_preserved() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("src/main.rs", vec![added("fn hello() {}")]);
        let rich = hl.enrich(&diff);
        assert_eq!(joined_content(&rich, 0, 0), "fn hello() {}");
    }

    // ── Background colors ─────────────────────────────────────────────────────

    #[test]
    fn test_added_line_gets_green_bg() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("f.rs", vec![added("x")]));
        assert!(rich.hunks[0].lines[0].segments.iter().all(|s| s.bg == Some(ADDED_BG)));
    }

    #[test]
    fn test_removed_line_gets_red_bg() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("f.rs", vec![removed("x")]));
        assert!(rich.hunks[0].lines[0].segments.iter().all(|s| s.bg == Some(REMOVED_BG) || s.bg == Some(REMOVED_INTRALINE_BG)));
    }

    #[test]
    fn test_context_line_has_no_bg() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("f.rs", vec![context("x")]));
        assert!(rich.hunks[0].lines[0].segments.iter().all(|s| s.bg.is_none()));
    }

    // ── Syntax highlighting ───────────────────────────────────────────────────

    #[test]
    fn test_rust_line_produces_multiple_segments() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("src/main.rs", vec![added("fn hello() {}")]));
        assert!(rich.hunks[0].lines[0].segments.len() > 1, "Rust line should be split into tokens");
    }

    #[test]
    fn test_unknown_extension_produces_one_segment() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("notes.xyz", vec![added("some random text")]));
        // Plain text grammar → single token
        assert_eq!(rich.hunks[0].lines[0].segments.len(), 1);
        assert_eq!(rich.hunks[0].lines[0].segments[0].content, "some random text");
    }

    #[test]
    fn test_ts_produces_multiple_segments() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("src/app.ts", vec![added("const x: number = 1;")]));
        assert!(rich.hunks[0].lines[0].segments.len() > 1, ".ts should produce multiple segments");
        assert_eq!(joined_content(&rich, 0, 0), "const x: number = 1;");
    }

    // ── Intraline highlighting ────────────────────────────────────────────────

    #[test]
    fn test_paired_lines_have_intraline_bg_on_changed_chars() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("f.rs", vec![
            removed("hello world"),
            added("hello earth"),
        ]);
        let rich = hl.enrich(&diff);
        // "world" → "earth": changed region should have brighter bg
        let added_segs = &rich.hunks[0].lines[1].segments;
        let has_bright_bg = added_segs.iter().any(|s| s.bg == Some(ADDED_INTRALINE_BG));
        assert!(has_bright_bg, "changed chars in added line should have intraline bg");
    }

    #[test]
    fn test_pure_addition_has_no_intraline_bg() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("f.rs", vec![added("brand new line")]));
        let segs = &rich.hunks[0].lines[0].segments;
        assert!(segs.iter().all(|s| s.bg != Some(ADDED_INTRALINE_BG)),
            "pure addition should not have intraline bg");
    }

    #[test]
    fn test_all_three_line_kinds() {
        let hl = SyntaxHighlighter::new();
        let rich = hl.enrich(&make_diff("src/app.rs", vec![
            added("let a = true;"),
            removed("let a = false;"),
            context("let b = 0;"),
        ]));
        assert!(!rich.hunks[0].lines[0].segments.is_empty());
        assert!(!rich.hunks[0].lines[1].segments.is_empty());
        assert!(!rich.hunks[0].lines[2].segments.is_empty());
    }
}
