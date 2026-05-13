use anyhow::Result;
use serde::Serialize;

use crate::app::FeedbackNote;

pub fn to_markdown(notes: &[FeedbackNote]) -> String {
    let mut out = String::from("# Delta Review\n\n");

    for note in notes {
        let path = note.file.display();
        out.push_str(&format!("## `{}`\n\n", path));
        out.push_str(&format!("**Hunk:** `{}`\n\n", note.hunk_header));
        out.push_str("```\n");
        out.push_str(&note.hunk_content);
        out.push_str("\n```\n\n");
        out.push_str(&format!("**Feedback:** {}\n\n", note.note));
        out.push_str("---\n\n");
    }

    out
}

#[derive(Serialize)]
struct JsonExport<'a> {
    notes: Vec<JsonNote<'a>>,
}

#[derive(Serialize)]
struct JsonNote<'a> {
    file: String,
    hunk: &'a str,
    code: &'a str,
    note: &'a str,
}

pub fn to_json(notes: &[FeedbackNote]) -> Result<String> {
    let export = JsonExport {
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
