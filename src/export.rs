use anyhow::Result;
use serde::Serialize;

use crate::app::FeedbackNote;

pub fn to_markdown(notes: &[FeedbackNote], from: &str, to: &str) -> String {
    let mut out = format!(
        "The following are code review notes from a human reviewer. \
        Please address each item before proceeding.\n\n\
        Diff range: `{from}..{to}`\n\n---\n\n",
    );

    for note in notes {
        out.push_str(&format!(
            "## `{}` · `{}`\n\n",
            note.file.display(),
            note.hunk_header,
        ));
        out.push_str("```diff\n");
        out.push_str(&note.hunk_content);
        out.push_str("\n```\n\n");
        // Multi-line notes: each line needs a > prefix to stay inside the blockquote.
        let mut note_lines = note.note.lines();
        if let Some(first) = note_lines.next() {
            out.push_str(&format!("> **Human:** {}\n", first));
        }
        for line in note_lines {
            out.push_str(&format!("> {}\n", line));
        }
        out.push('\n');
        out.push_str("---\n\n");
    }

    out
}

#[derive(Serialize)]
struct JsonExport<'a> {
    range: String,
    notes: Vec<JsonNote<'a>>,
}

#[derive(Serialize)]
struct JsonNote<'a> {
    file: String,
    hunk: &'a str,
    code: &'a str,
    note: &'a str,
}

pub fn to_json(notes: &[FeedbackNote], from: &str, to: &str) -> Result<String> {
    let export = JsonExport {
        range: format!("{}..{}", from, to),
        notes: notes
            .iter()
            .map(|n| JsonNote {
                file: n.file.display().to_string(),
                hunk: &n.hunk_header,
                code: &n.hunk_content,
                note: &n.note,
            })
            .collect(),
    };

    Ok(serde_json::to_string_pretty(&export)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_note(file: &str, hunk: &str, code: &str, note: &str) -> FeedbackNote {
        FeedbackNote {
            file: PathBuf::from(file),
            hunk_header: hunk.to_string(),
            hunk_content: code.to_string(),
            note: note.to_string(),
        }
    }

    // ── Markdown export ───────────────────────────────────────────────────────

    #[test]
    fn test_markdown_contains_file_path() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+token log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("src/auth.rs"));
    }

    #[test]
    fn test_markdown_contains_hunk_header() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+token log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("@@ -1,3 +1,4 @@"));
    }

    #[test]
    fn test_markdown_contains_code() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+token log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("+token log"));
    }

    #[test]
    fn test_markdown_contains_feedback() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+token log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("too verbose"));
    }

    #[test]
    fn test_markdown_multiple_notes_all_present() {
        let notes = vec![
            make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log token", "sensitive"),
            make_note("src/db.rs", "@@ -10,5 +10,6 @@", "+raw query", "use parameterized"),
        ];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("src/auth.rs"));
        assert!(md.contains("src/db.rs"));
        assert!(md.contains("sensitive"));
        assert!(md.contains("use parameterized"));
    }

    #[test]
    fn test_markdown_has_preamble() {
        let md = to_markdown(&[], "HEAD^", "HEAD");
        assert!(md.contains("code review notes from a human reviewer"));
        assert!(md.contains("Please address each item"));
    }

    #[test]
    fn test_markdown_contains_range() {
        let md = to_markdown(&[], "main", "HEAD");
        assert!(md.contains("main..HEAD"), "markdown should include the diff range");
    }

    #[test]
    fn test_json_has_range_field() {
        let json = to_json(&[], "main", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["range"], "main..HEAD");
    }

    #[test]
    fn test_markdown_empty_notes_has_no_human_label() {
        let md = to_markdown(&[], "HEAD^", "HEAD");
        assert!(!md.contains("**Human:**"));
    }

    #[test]
    fn test_markdown_uses_human_label_not_feedback() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("> **Human:**"));
        assert!(!md.contains("**Feedback:**"));
    }

    #[test]
    fn test_markdown_human_note_is_blockquote() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("> **Human:** too verbose"));
    }

    #[test]
    fn test_markdown_file_and_hunk_on_same_line() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        // File path and hunk header should appear on the same header line
        assert!(md.contains("`src/auth.rs` · `@@ -1,3 +1,4 @@`"));
    }

    #[test]
    fn test_markdown_uses_diff_code_fence() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "too verbose")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("```diff\n"));
    }

    // ── JSON export ───────────────────────────────────────────────────────────

    #[test]
    fn test_json_is_valid() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "issue")];
        let json = to_json(&notes, "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn test_json_has_notes_array() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "issue")];
        let json = to_json(&notes, "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["notes"].is_array());
        assert_eq!(parsed["notes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_json_note_fields() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log token", "sensitive data")];
        let json = to_json(&notes, "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let note = &parsed["notes"][0];

        assert_eq!(note["file"], "src/auth.rs");
        assert_eq!(note["hunk"], "@@ -1,3 +1,4 @@");
        assert_eq!(note["code"], "+log token");
        assert_eq!(note["note"], "sensitive data");
    }

    #[test]
    fn test_json_multiple_notes() {
        let notes = vec![
            make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "issue one"),
            make_note("src/db.rs", "@@ -5,2 +5,3 @@", "+query", "issue two"),
        ];
        let json = to_json(&notes, "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["notes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_json_empty_notes() {
        let json = to_json(&[], "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["notes"].as_array().unwrap().len(), 0);
    }

    // ── Multi-line note formatting ─────────────────────────────────────────────

    #[test]
    fn test_markdown_multiline_note_first_line_has_human_label() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "line one\nline two")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("> **Human:** line one"));
    }

    #[test]
    fn test_markdown_multiline_note_continuation_has_blockquote_prefix() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "line one\nline two\nline three")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        assert!(md.contains("> line two"));
        assert!(md.contains("> line three"));
    }

    #[test]
    fn test_markdown_multiline_note_no_bare_human_label_on_continuation() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "line one\nline two")];
        let md = to_markdown(&notes, "HEAD^", "HEAD");
        // Only the first line should have **Human:**
        let human_count = md.matches("**Human:**").count();
        assert_eq!(human_count, 1);
    }

    #[test]
    fn test_json_multiline_note_preserved() {
        let notes = vec![make_note("src/auth.rs", "@@ -1,3 +1,4 @@", "+log", "line one\nline two")];
        let json = to_json(&notes, "HEAD^", "HEAD").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["notes"][0]["note"].as_str().unwrap().contains('\n'));
    }
}
