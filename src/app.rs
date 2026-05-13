use std::path::PathBuf;

use crate::diff::{ChangedFile, DiffFile, LineKind};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    FileList,
    DiffView,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Comment {
        hunk_idx: usize,
        input: String,
    },
}

#[derive(Debug, Clone)]
pub struct FeedbackNote {
    pub file: PathBuf,
    pub hunk_header: String,
    pub hunk_content: String,
    pub note: String,
}

pub struct App {
    pub base: String,
    pub files: Vec<ChangedFile>,
    pub selected_file: usize,
    pub focused_panel: Panel,
    pub current_diff: Option<DiffFile>,
    pub diff_scroll: usize,
    pub selected_hunk: usize,
    pub notes: Vec<FeedbackNote>,
    pub mode: Mode,
}

impl App {
    pub fn new(files: Vec<ChangedFile>, base: String) -> Self {
        Self {
            base,
            files,
            selected_file: 0,
            focused_panel: Panel::FileList,
            current_diff: None,
            diff_scroll: 0,
            selected_hunk: 0,
            notes: Vec::new(),
            mode: Mode::Normal,
        }
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
            self.diff_scroll = 0;
            self.selected_hunk = 0;
            self.current_diff = None;
        }
    }

    pub fn file_list_up(&mut self) {
        if self.selected_file > 0 {
            self.select_file(self.selected_file - 1);
        }
    }

    pub fn file_list_down(&mut self) {
        if self.selected_file + 1 < self.files.len() {
            self.select_file(self.selected_file + 1);
        }
    }

    pub fn diff_scroll_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(3);
    }

    pub fn diff_scroll_down(&mut self) {
        self.diff_scroll += 3;
    }

    pub fn next_hunk(&mut self) {
        if let Some(ref diff) = self.current_diff {
            if self.selected_hunk + 1 < diff.hunks.len() {
                self.selected_hunk += 1;
            }
        }
    }

    pub fn prev_hunk(&mut self) {
        self.selected_hunk = self.selected_hunk.saturating_sub(1);
    }

    pub fn start_comment(&mut self) {
        if self.current_diff.as_ref().map(|d| !d.hunks.is_empty()).unwrap_or(false) {
            self.mode = Mode::Comment {
                hunk_idx: self.selected_hunk,
                input: String::new(),
            };
        }
    }

    pub fn submit_comment(&mut self) {
        if let Mode::Comment { hunk_idx, ref input } = self.mode.clone() {
            let trimmed = input.trim().to_string();
            if !trimmed.is_empty() {
                if let Some(ref diff) = self.current_diff {
                    if let Some(hunk) = diff.hunks.get(hunk_idx) {
                        let hunk_content = hunk
                            .lines
                            .iter()
                            .map(|l| {
                                let prefix = match l.kind {
                                    LineKind::Added => "+",
                                    LineKind::Removed => "-",
                                    LineKind::Context => " ",
                                };
                                format!("{}{}", prefix, l.content)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        self.notes.push(FeedbackNote {
                            file: diff.file.path.clone(),
                            hunk_header: hunk.header.clone(),
                            hunk_content,
                            note: trimmed,
                        });
                    }
                }
            }
        }
        self.mode = Mode::Normal;
    }

    pub fn cancel_comment(&mut self) {
        self.mode = Mode::Normal;
    }
}
