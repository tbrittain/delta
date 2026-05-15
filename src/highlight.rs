use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::diff::DiffFile;

pub struct HighlightedSpan {
    pub content: String,
    pub fg: Color,
}

/// Indexed as [hunk_idx][line_idx] → token spans for that line.
pub type DiffHighlights = Vec<Vec<Vec<HighlightedSpan>>>;

pub struct SyntaxHighlighter {
    default_set: SyntaxSet,
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
        // .jsx shares JSX syntax with .tsx; use TypeScriptReact for both.
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

    pub fn highlight_diff(&self, diff: &DiffFile) -> DiffHighlights {
        let extension = diff
            .file
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let (syntax, syntax_set) = self.find_syntax(extension);

        let mut h = HighlightLines::new(syntax, &self.theme);

        diff.hunks
            .iter()
            .map(|hunk| {
                hunk.lines
                    .iter()
                    .map(|dl| {
                        let line_with_newline = format!("{}\n", dl.content);
                        let ranges = h
                            .highlight_line(&line_with_newline, syntax_set)
                            .unwrap_or_default();
                        ranges
                            .iter()
                            .map(|(style, text)| {
                                let content = text.trim_end_matches('\n').to_string();
                                let fg = Color::Rgb(
                                    style.foreground.r,
                                    style.foreground.g,
                                    style.foreground.b,
                                );
                                HighlightedSpan { content, fg }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }
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
        DiffLine {
            old_lineno: None,
            new_lineno: Some(1),
            kind: LineKind::Added,
            content: content.to_string(),
        }
    }

    fn removed(content: &str) -> DiffLine {
        DiffLine {
            old_lineno: Some(1),
            new_lineno: None,
            kind: LineKind::Removed,
            content: content.to_string(),
        }
    }

    fn context(content: &str) -> DiffLine {
        DiffLine {
            old_lineno: Some(1),
            new_lineno: Some(1),
            kind: LineKind::Context,
            content: content.to_string(),
        }
    }

    #[test]
    fn test_highlight_rust_produces_spans() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("src/main.rs", vec![added("fn hello() {}")]);
        let highlights = hl.highlight_diff(&diff);

        assert!(!highlights[0][0].is_empty(), "Rust line should produce spans");
        let joined: String = highlights[0][0].iter().map(|s| s.content.as_str()).collect();
        assert_eq!(joined, "fn hello() {}");
    }

    #[test]
    fn test_highlight_unknown_extension_fallback() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("notes.xyz", vec![added("some random text here")]);
        let highlights = hl.highlight_diff(&diff);

        assert!(!highlights[0][0].is_empty(), "Unknown extension should still produce spans");
        let joined: String = highlights[0][0].iter().map(|s| s.content.as_str()).collect();
        assert_eq!(joined, "some random text here");
    }

    #[test]
    fn test_highlight_preserves_hunk_line_structure() {
        let hl = SyntaxHighlighter::new();
        let line_a = added("let x = 1;");
        let line_b = context("// comment");
        let diff = DiffFile {
            file: ChangedFile {
                path: PathBuf::from("src/lib.rs"),
                status: FileStatus::Modified,
            },
            hunks: vec![
                Hunk {
                    header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    new_start: 1,
                    lines: vec![line_a, line_b],
                },
                Hunk {
                    header: "@@ -10,1 +10,1 @@".to_string(),
                    old_start: 10,
                    new_start: 10,
                    lines: vec![removed("old line")],
                },
            ],
        };

        let highlights = hl.highlight_diff(&diff);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0].len(), 2);
        assert_eq!(highlights[1].len(), 1);
    }

    #[test]
    fn test_highlight_all_kinds() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff(
            "src/app.rs",
            vec![
                added("let a = true;"),
                removed("let a = false;"),
                context("let b = 0;"),
            ],
        );
        let highlights = hl.highlight_diff(&diff);
        assert!(!highlights[0][0].is_empty(), "added line should produce spans");
        assert!(!highlights[0][1].is_empty(), "removed line should produce spans");
        assert!(!highlights[0][2].is_empty(), "context line should produce spans");
    }

    // TypeScript-specific syntax (type annotation) produces multiple spans, confirming
    // the TypeScript grammar is active rather than the plain-text fallback.
    #[test]
    fn test_highlight_ts_produces_colored_spans() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("src/app.ts", vec![added("const x: number = 1;")]);
        let highlights = hl.highlight_diff(&diff);
        let spans = &highlights[0][0];
        assert!(spans.len() > 1, ".ts should produce multiple spans, got {}", spans.len());
        let joined: String = spans.iter().map(|s| s.content.as_str()).collect();
        assert_eq!(joined, "const x: number = 1;");
    }

    #[test]
    fn test_highlight_tsx_produces_colored_spans() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("src/App.tsx", vec![added("const x: number = 1;")]);
        let highlights = hl.highlight_diff(&diff);
        let spans = &highlights[0][0];
        assert!(spans.len() > 1, ".tsx should produce multiple spans, got {}", spans.len());
        let joined: String = spans.iter().map(|s| s.content.as_str()).collect();
        assert_eq!(joined, "const x: number = 1;");
    }

    #[test]
    fn test_highlight_jsx_produces_colored_spans() {
        let hl = SyntaxHighlighter::new();
        let diff = make_diff("src/Button.jsx", vec![added("const x = () => <div />;")]);
        let highlights = hl.highlight_diff(&diff);
        let spans = &highlights[0][0];
        assert!(spans.len() > 1, ".jsx should produce multiple spans, got {}", spans.len());
        let joined: String = spans.iter().map(|s| s.content.as_str()).collect();
        assert_eq!(joined, "const x = () => <div />;");
    }
}
