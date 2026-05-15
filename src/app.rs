use std::collections::HashSet;
use std::path::PathBuf;

use crate::diff::{ChangedFile, DiffFile, DiffLine, LineKind};
use crate::filetree::{TreeItem, build_tree};
use crate::git::WhitespaceMode;

/// Context runs of this many lines or more are folded by default.
pub(crate) const FOLD_THRESHOLD: usize = 6;

/// Inner width (in terminal columns) of the file-list panel.
/// File list is rendered at Constraint::Length(32); minus 2 borders = 30.
pub(crate) const FILE_LIST_INNER_WIDTH: usize = 30;

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    FileList,
    DiffView,
    NotesView,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Comment {
        hunk_idx: usize,
        input: String,
        /// Byte offset of the insertion cursor, always on a char boundary.
        cursor: usize,
        /// The note being replaced, if this is an edit rather than a new comment.
        /// Restored on Esc so cancelling an edit never loses the original.
        original: Option<FeedbackNote>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackNote {
    pub file: PathBuf,
    pub hunk_header: String,
    pub hunk_content: String,
    pub note: String,
}

pub struct App {
    pub from: String,
    pub to: String,
    pub files: Vec<ChangedFile>,
    pub selected_file: usize,
    pub focused_panel: Panel,
    pub current_diff: Option<DiffFile>,
    pub current_highlights: Option<crate::highlight::DiffHighlights>,
    pub highlighter: crate::highlight::SyntaxHighlighter,
    pub diff_scroll: usize,
    pub selected_hunk: usize,
    pub notes: Vec<FeedbackNote>,
    pub mode: Mode,
    /// Hunk indices whose context runs have been expanded by the user.
    /// All other hunks have long context runs folded by default.
    pub expanded_hunks: HashSet<usize>,
    /// Which note is selected in the Notes panel.
    pub selected_note: usize,
    /// Which notes are expanded to show full text in the Notes panel.
    pub expanded_notes: HashSet<usize>,
    /// Scroll offset within the comment popup (logical lines).
    pub comment_scroll: usize,
    /// Byte offset of the selection anchor; `None` means no active selection.
    /// The selected range is `min(cursor, anchor)..max(cursor, anchor)`.
    pub comment_anchor: Option<usize>,
    /// Scroll offset (visual rows) for the notes panel. Updated after every navigation action
    /// so the selected note is always visible within the panel viewport.
    pub notes_scroll: usize,
    /// Inner width of the diff panel (terminal columns, excluding file-list panel and borders).
    /// Updated by the event loop before each draw. Used to compute accurate visual row counts
    /// when wrap is enabled. Zero means "no wrap accounting" (treats every line as 1 row).
    pub diff_view_content_width: usize,
    /// Directories the user has collapsed in the file tree. An empty set means all dirs are expanded.
    pub collapsed_dirs: HashSet<PathBuf>,
    /// Cursor position within the visible file tree (index into `tree_items()`).
    pub file_tree_cursor: usize,
    /// Horizontal scroll offset (in character columns) for the scrollable name portion
    /// of file-list items. Adjusted with ←/→ when the file-list panel is focused.
    pub file_list_h_scroll: usize,
    /// Active whitespace-sensitivity mode for `git diff`. Cycled with `w` in the diff view.
    pub whitespace_mode: WhitespaceMode,
}

impl App {
    pub fn new(files: Vec<ChangedFile>, from: String, to: String) -> Self {
        Self {
            from,
            to,
            files,
            selected_file: 0,
            focused_panel: Panel::FileList,
            current_diff: None,
            current_highlights: None,
            highlighter: crate::highlight::SyntaxHighlighter::new(),
            diff_scroll: 0,
            selected_hunk: 0,
            notes: Vec::new(),
            mode: Mode::Normal,
            expanded_hunks: HashSet::new(),
            selected_note: 0,
            expanded_notes: HashSet::new(),
            comment_scroll: 0,
            comment_anchor: None,
            notes_scroll: 0,
            diff_view_content_width: 0,
            collapsed_dirs: HashSet::new(),
            file_tree_cursor: 0,
            file_list_h_scroll: 0,
            whitespace_mode: WhitespaceMode::None,
        }
    }

    /// Build the visible file tree from the current files, notes, and collapsed state.
    pub fn tree_items(&self) -> Vec<TreeItem> {
        let noted: HashSet<PathBuf> = self.notes.iter().map(|n| n.file.clone()).collect();
        build_tree(&self.files, &noted, &self.collapsed_dirs)
    }

    /// Toggle the collapse/expand state of the directory at the current tree cursor.
    /// No-op when the cursor is on a file. Clamps `file_tree_cursor` after the tree changes.
    pub fn toggle_dir_at_cursor(&mut self) {
        let path_opt = self.tree_items().get(self.file_tree_cursor)
            .and_then(|i| i.dir_path()).cloned();
        if let Some(path) = path_opt {
            if self.collapsed_dirs.contains(&path) {
                self.collapsed_dirs.remove(&path);
            } else {
                self.collapsed_dirs.insert(path);
            }
            let new_len = self.tree_items().len();
            if new_len > 0 {
                self.file_tree_cursor = self.file_tree_cursor.min(new_len - 1);
            }
            self.sync_selected_file_from_cursor();
        }
    }

    /// Remove all ancestor directories of `file_idx` from `collapsed_dirs` so the file is visible.
    pub fn expand_parents_of(&mut self, file_idx: usize) {
        let path = match self.files.get(file_idx) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let mut current = path.as_path();
        while let Some(parent) = current.parent() {
            if parent == std::path::Path::new("") { break; }
            self.collapsed_dirs.remove(parent);
            current = parent;
        }
    }

    /// Move `file_tree_cursor` to the position of `selected_file` in the visible tree.
    /// Called after a jump-to-note so the file list cursor tracks the loaded file.
    pub fn sync_tree_cursor_to_file(&mut self) {
        let tree = self.tree_items();
        if let Some(pos) = tree.iter().position(|i| i.file_idx() == Some(self.selected_file)) {
            self.file_tree_cursor = pos;
        }
    }

    /// Update `selected_file` from the tree item at `file_tree_cursor`.
    /// No-op when the cursor is on a directory.
    fn sync_selected_file_from_cursor(&mut self) {
        let tree = self.tree_items();
        if let Some(item) = tree.get(self.file_tree_cursor) {
            if let Some(idx) = item.file_idx() {
                self.selected_file = idx;
            }
        }
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
            self.diff_scroll = 0;
            self.selected_hunk = 0;
            self.current_diff = None;
            self.current_highlights = None;
            self.expanded_hunks.clear();
        }
    }

    // ── Notes panel ──────────────────────────────────────────────────────────

    pub fn notes_up(&mut self) {
        self.selected_note = self.selected_note.saturating_sub(1);
    }

    pub fn notes_down(&mut self) {
        if self.selected_note + 1 < self.notes.len() {
            self.selected_note += 1;
        }
    }

    /// Adjust `notes_scroll` so the selected note is within the viewport.
    /// `viewport_height` is the inner content height of the notes panel (panel rows − 2 borders).
    pub fn scroll_notes_to_selected(&mut self, viewport_height: usize) {
        if self.notes.is_empty() || viewport_height == 0 { return; }
        let note_start: usize = self.notes[..self.selected_note].iter().enumerate()
            .map(|(i, n)| note_visual_rows(n, self.expanded_notes.contains(&i)))
            .sum();
        let note_h = note_visual_rows(
            &self.notes[self.selected_note],
            self.expanded_notes.contains(&self.selected_note),
        );
        if note_start < self.notes_scroll {
            self.notes_scroll = note_start;
        } else if note_start + note_h > self.notes_scroll + viewport_height {
            self.notes_scroll = (note_start + note_h).saturating_sub(viewport_height);
        }
    }

    pub fn toggle_note_expand(&mut self) {
        if self.expanded_notes.contains(&self.selected_note) {
            self.expanded_notes.remove(&self.selected_note);
        } else {
            self.expanded_notes.insert(self.selected_note);
        }
    }

    /// Returns the index into `self.files` for the note currently selected in the Notes panel.
    pub fn selected_note_file_idx(&self) -> Option<usize> {
        let note = self.notes.get(self.selected_note)?;
        self.files.iter().position(|f| f.path == note.file)
    }

    /// Deletes the note currently selected in the Notes panel.
    pub fn delete_selected_note(&mut self) {
        if self.selected_note >= self.notes.len() { return; }
        self.notes.remove(self.selected_note);
        self.expanded_notes.clear();
        self.notes_scroll = 0;
        if self.selected_note > 0 && self.selected_note >= self.notes.len() {
            self.selected_note -= 1;
        }
    }

    pub fn toggle_hunk_fold(&mut self) {
        if self.expanded_hunks.contains(&self.selected_hunk) {
            self.expanded_hunks.remove(&self.selected_hunk);
        } else {
            self.expanded_hunks.insert(self.selected_hunk);
        }
    }

    /// True if the selected hunk has any context run long enough to fold.
    pub fn selected_hunk_is_foldable(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        let Some(hunk) = diff.hunks.get(self.selected_hunk) else { return false };
        hunk_has_foldable_context(&hunk.lines)
    }

    pub fn file_list_up(&mut self) {
        if self.file_tree_cursor > 0 {
            self.file_tree_cursor -= 1;
            self.sync_selected_file_from_cursor();
        }
    }

    pub fn file_list_down(&mut self) {
        if self.file_tree_cursor + 1 < self.tree_items().len() {
            self.file_tree_cursor += 1;
            self.sync_selected_file_from_cursor();
        }
    }

    pub fn file_list_scroll_right(&mut self) {
        let cap = self.max_h_scroll();
        if self.file_list_h_scroll < cap {
            self.file_list_h_scroll = (self.file_list_h_scroll + 3).min(cap);
        }
    }

    pub fn file_list_scroll_left(&mut self) {
        self.file_list_h_scroll = self.file_list_h_scroll.saturating_sub(3);
    }

    /// Maximum useful horizontal scroll for the file list: the largest amount by
    /// which any visible item's content overflows the panel's inner width.
    /// Returns 0 when all names fit (scrolling would have no effect).
    fn max_h_scroll(&self) -> usize {
        self.tree_items().iter().map(|item| {
            match item {
                TreeItem::Dir { display_name, file_count, has_notes, depth, .. } => {
                    let note_w = if *has_notes { 2 } else { 0 };
                    let content_w = display_name.chars().count()
                        + 2  // " ("
                        + file_count.to_string().len()
                        + 1  // ")"
                        + note_w;
                    let prefix_w = depth * 2 + 2; // indent + arrow + space
                    prefix_w + content_w
                }
                TreeItem::File { display_name, has_notes, depth, .. } => {
                    let note_w = if *has_notes { 2 } else { 0 };
                    let content_w = display_name.chars().count() + note_w;
                    let prefix_w = depth * 2 + 4; // indent + "[X] "
                    prefix_w + content_w
                }
            }
        }).max().unwrap_or(0).saturating_sub(FILE_LIST_INNER_WIDTH)
    }

    /// Advance the whitespace mode one step: None → -b → -w → None.
    pub fn cycle_whitespace_mode(&mut self) {
        self.whitespace_mode = self.whitespace_mode.next();
    }

    pub fn diff_scroll_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(3);
    }

    /// Scroll the diff view down, capped so we never scroll past the content.
    /// `viewport_height` is the number of visible lines in the diff panel.
    pub fn diff_scroll_down(&mut self, viewport_height: usize) {
        let max_scroll = self.diff_content_lines().saturating_sub(viewport_height);
        self.diff_scroll = (self.diff_scroll + 3).min(max_scroll);
    }

    pub fn next_hunk(&mut self) {
        if let Some(ref diff) = self.current_diff {
            if self.selected_hunk + 1 < diff.hunks.len() {
                self.selected_hunk += 1;
                self.scroll_to_selected_hunk();
            }
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.selected_hunk > 0 {
            self.selected_hunk -= 1;
            self.scroll_to_selected_hunk();
        }
    }

    /// True when `]` should cross to the next file: we are on the last hunk of the
    /// current file and there is at least one more file in the list.
    pub fn at_last_hunk_boundary(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        !diff.hunks.is_empty()
            && self.selected_hunk + 1 >= diff.hunks.len()
            && self.selected_file + 1 < self.files.len()
    }

    /// True when `[` should cross to the previous file: we are on the first hunk of
    /// the current file and there is at least one earlier file in the list.
    pub fn at_first_hunk_boundary(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        !diff.hunks.is_empty() && self.selected_hunk == 0 && self.selected_file > 0
    }

    /// Scroll the diff view so the selected hunk is at the top.
    pub fn scroll_to_selected_hunk(&mut self) {
        self.diff_scroll = self.hunk_scroll_offset(self.selected_hunk);
    }

    /// Compute the rendered line offset of `target_hunk` within the diff view.
    /// Accounts for folded context runs so hunk-jump scrolls to the right position.
    fn hunk_scroll_offset(&self, target_hunk: usize) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        let pw = self.diff_view_content_width;
        let mut offset = 0;
        for (i, hunk) in diff.hunks.iter().enumerate() {
            if i >= target_hunk { break; }
            let is_expanded = self.expanded_hunks.contains(&i);
            let content_lines = if is_expanded {
                hunk.lines.iter().map(|l| visual_rows_for_diff_line(&l.content, pw)).sum()
            } else {
                context_run_visual_lines(&hunk.lines, pw)
            };
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == hunk.header)
                .count();
            offset += 1 + content_lines + note_count + 1;
        }
        offset
    }

    /// Total rendered line count for the current diff, used to cap scroll and drive the scrollbar.
    /// Accounts for folded context runs and per-line visual row counts when wrap is on.
    pub(crate) fn diff_content_lines(&self) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        let pw = self.diff_view_content_width;
        diff.hunks.iter().enumerate().map(|(i, h)| {
            let is_expanded = self.expanded_hunks.contains(&i);
            let content_lines = if is_expanded {
                h.lines.iter().map(|l| visual_rows_for_diff_line(&l.content, pw)).sum()
            } else {
                context_run_visual_lines(&h.lines, pw)
            };
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == h.header)
                .count();
            1 + content_lines + note_count + 1 // header + lines + notes + blank
        }).sum()
    }

    /// Adjust `comment_scroll` so the cursor stays visible within the popup viewport.
    /// `content_width` is the number of characters per visual line (popup width minus borders);
    /// it is used to compute the correct visual row when long lines wrap.
    pub fn scroll_comment_to_cursor(&mut self, viewport_height: usize, content_width: usize) {
        let cursor_visual_row = match &self.mode {
            Mode::Comment { input, cursor, .. } => visual_row_for_cursor(input, *cursor, content_width),
            _ => return,
        };
        if cursor_visual_row < self.comment_scroll {
            self.comment_scroll = cursor_visual_row;
        } else if viewport_height > 0 && cursor_visual_row + 1 > self.comment_scroll + viewport_height {
            self.comment_scroll = cursor_visual_row + 1 - viewport_height;
        }
    }

    /// Returns (file_path, hunk_header) for the currently selected hunk, or None.
    /// Used as a lookup key for notes on the current hunk.
    fn current_hunk_identity(&self) -> Option<(PathBuf, String)> {
        let diff = self.current_diff.as_ref()?;
        let hunk = diff.hunks.get(self.selected_hunk)?;
        Some((diff.file.path.clone(), hunk.header.clone()))
    }

    pub fn current_hunk_has_note(&self) -> bool {
        match self.current_hunk_identity() {
            Some((file, header)) => self.notes.iter().any(|n| n.file == file && n.hunk_header == header),
            None => false,
        }
    }

    pub fn delete_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        self.notes.retain(|n| !(n.file == file && n.hunk_header == header));
        self.expanded_notes.clear();
        self.notes_scroll = 0;
        if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
            self.selected_note = self.notes.len() - 1;
        }
    }

    /// Remove the existing note for the current hunk and re-open the comment
    /// input pre-populated with the old text so the user can revise it.
    pub fn edit_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        if let Some(original) = self.notes.iter()
            .find(|n| n.file == file && n.hunk_header == header)
            .cloned()
        {
            self.notes.retain(|n| !(n.file == file && n.hunk_header == header));
            self.expanded_notes.clear();
            if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
                self.selected_note = self.notes.len() - 1;
            }
            let cursor = original.note.len();
            let input = original.note.clone();
            self.comment_scroll = 0;
            self.comment_anchor = None;
            self.mode = Mode::Comment {
                hunk_idx: self.selected_hunk,
                input,
                cursor,
                original: Some(original),
            };
        }
    }

    pub fn start_comment(&mut self) {
        if self.current_diff.as_ref().map(|d| !d.hunks.is_empty()).unwrap_or(false) {
            if self.current_hunk_has_note() {
                self.edit_note_for_current_hunk();
            } else {
                self.comment_scroll = 0;
                self.comment_anchor = None;
                self.mode = Mode::Comment {
                    hunk_idx: self.selected_hunk,
                    input: String::new(),
                    cursor: 0,
                    original: None,
                };
            }
        }
    }

    pub fn submit_comment(&mut self) {
        if let Mode::Comment { hunk_idx, ref input, .. } = self.mode.clone() {
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
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }

    pub fn cancel_comment(&mut self) {
        // If editing an existing note, restore the original so Esc never loses it.
        if let Mode::Comment { ref original, .. } = self.mode.clone() {
            if let Some(note) = original.clone() {
                self.notes.push(note);
            }
        }
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }
}

/// Visual rows occupied by one note entry in the notes panel.
/// Collapsed: header + first-line-of-note + blank = 3 rows.
/// Expanded: header + all note lines + blank.
fn note_visual_rows(note: &FeedbackNote, expanded: bool) -> usize {
    if expanded {
        1 + note.note.lines().count().max(1) + 1
    } else {
        3
    }
}

/// Number of visual (screen) rows a single diff line occupies when the diff panel
/// wraps at `panel_width` columns. `panel_width` is the full inner width of the diff
/// panel including the 6-column gutter (4 lineno + 1 space + 1 prefix).
/// Returns 1 when `panel_width` is 0 (wrap accounting disabled).
pub(crate) fn visual_rows_for_diff_line(content: &str, panel_width: usize) -> usize {
    if panel_width == 0 { return 1; }
    let total = 6 + content.chars().count(); // gutter always occupies 6 chars on the first row
    (total + panel_width - 1) / panel_width
}

/// Count the visual lines a slice of diff lines occupies when context runs are folded.
/// Runs of context lines >= FOLD_THRESHOLD collapse to a single placeholder line.
/// `panel_width` is passed to `visual_rows_for_diff_line` for non-context (changed) lines;
/// context lines are assumed short and counted as 1 row each.
fn context_run_visual_lines(lines: &[DiffLine], panel_width: usize) -> usize {
    let mut count = 0;
    let mut ctx_run = 0;
    for line in lines {
        if line.kind == LineKind::Context {
            ctx_run += 1;
        } else {
            count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
            ctx_run = 0;
            count += visual_rows_for_diff_line(&line.content, panel_width);
        }
    }
    count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
    count
}

/// True if the given lines contain at least one context run long enough to fold.
fn hunk_has_foldable_context(lines: &[DiffLine]) -> bool {
    let mut ctx_run = 0;
    for line in lines {
        if line.kind == LineKind::Context {
            ctx_run += 1;
            if ctx_run >= FOLD_THRESHOLD {
                return true;
            }
        } else {
            ctx_run = 0;
        }
    }
    false
}

/// Returns the visual row index of `cursor` within `input`, accounting for line wrapping
/// at `content_width` characters. Used by `scroll_comment_to_cursor`.
pub(crate) fn visual_row_for_cursor(input: &str, cursor: usize, content_width: usize) -> usize {
    let cw = content_width.max(1);
    let mut visual_row = 0usize;
    let mut byte_pos = 0usize;
    for logical_line in input.split('\n') {
        let char_count = logical_line.chars().count();
        let line_byte_end = byte_pos + logical_line.len();
        if cursor >= byte_pos && cursor <= line_byte_end {
            let char_offset = logical_line[..cursor - byte_pos].chars().count();
            // Cursor at end of line (char_offset == char_count) belongs to the last visual row,
            // not the (out-of-range) row after it.
            let clamped = if char_count == 0 { 0 } else { char_offset.min(char_count - 1) };
            return visual_row + clamped / cw;
        }
        visual_row += if char_count == 0 { 1 } else { (char_count + cw - 1) / cw };
        byte_pos = line_byte_end + 1;
    }
    visual_row.saturating_sub(1)
}

/// Returns `Some((start, end))` where `start < end` if there is a non-empty selection,
/// `None` otherwise.
pub(crate) fn selected_range(cursor: usize, anchor: Option<usize>) -> Option<(usize, usize)> {
    let a = anchor?;
    let start = cursor.min(a);
    let end = cursor.max(a);
    if start < end { Some((start, end)) } else { None }
}

/// Delete the selected byte range from `input` and return `(new_input, new_cursor)`.
/// Returns `None` if there is no non-empty selection.
pub(crate) fn delete_selection(
    input: &str,
    cursor: usize,
    anchor: Option<usize>,
) -> Option<(String, usize)> {
    let (start, end) = selected_range(cursor, anchor)?;
    let mut new_input = input.to_string();
    new_input.drain(start..end);
    Some((new_input, start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use crate::git::WhitespaceMode;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_files(n: usize) -> Vec<ChangedFile> {
        (0..n)
            .map(|i| ChangedFile {
                path: PathBuf::from(format!("file_{}.rs", i)),
                status: FileStatus::Modified,
            })
            .collect()
    }

    fn make_hunk(header: &str) -> Hunk {
        Hunk {
            header: header.to_string(),
            old_start: 1,
            new_start: 1,
            lines: vec![
                DiffLine {
                    old_lineno: None,
                    new_lineno: Some(1),
                    kind: LineKind::Added,
                    content: "new line".to_string(),
                },
                DiffLine {
                    old_lineno: Some(1),
                    new_lineno: None,
                    kind: LineKind::Removed,
                    content: "old line".to_string(),
                },
                DiffLine {
                    old_lineno: Some(2),
                    new_lineno: Some(2),
                    kind: LineKind::Context,
                    content: "context".to_string(),
                },
            ],
        }
    }

    fn app_with_diff(hunk_count: usize) -> App {
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count)
                .map(|i| make_hunk(&format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1)))
                .collect(),
        });
        app
    }

    // ── Whitespace mode ───────────────────────────────────────────────────────

    #[test]
    fn test_cycle_whitespace_mode() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        assert_eq!(app.whitespace_mode, WhitespaceMode::None);
        app.cycle_whitespace_mode();
        assert_eq!(app.whitespace_mode, WhitespaceMode::IgnoreChanges);
        app.cycle_whitespace_mode();
        assert_eq!(app.whitespace_mode, WhitespaceMode::IgnoreAll);
        app.cycle_whitespace_mode();
        assert_eq!(app.whitespace_mode, WhitespaceMode::None);
    }

    // ── File list navigation ──────────────────────────────────────────────────

    #[test]
    fn test_file_list_down_navigates() {
        // make_files(3) → flat tree [file_0.rs, file_1.rs, file_2.rs]
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.file_list_down();
        assert_eq!(app.file_tree_cursor, 1);
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_down_clamps_at_end() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 2;
        app.file_list_down();
        assert_eq!(app.file_tree_cursor, 2);
    }

    #[test]
    fn test_file_list_up_navigates() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 2;
        app.file_list_up();
        assert_eq!(app.file_tree_cursor, 1);
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_up_clamps_at_start() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.file_list_up();
        assert_eq!(app.file_tree_cursor, 0);
    }

    #[test]
    fn test_file_list_scroll_right_increases_h_scroll() {
        // Need a name that overflows the 30-col inner panel.
        // depth-0 file prefix = 4 ("[M] "), available for name = 26. 36 chars overflows by 10.
        let long_name = "a".repeat(36);
        let files = vec![ChangedFile { path: long_name.into(), status: crate::diff::FileStatus::Modified }];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert_eq!(app.file_list_h_scroll, 0);
        app.file_list_scroll_right();
        assert_eq!(app.file_list_h_scroll, 3);
        app.file_list_scroll_right();
        assert_eq!(app.file_list_h_scroll, 6);
    }

    #[test]
    fn test_file_list_scroll_left_decreases_h_scroll() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.file_list_h_scroll = 6;
        app.file_list_scroll_left();
        assert_eq!(app.file_list_h_scroll, 3);
    }

    #[test]
    fn test_file_list_scroll_left_clamps_at_zero() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.file_list_scroll_left();
        assert_eq!(app.file_list_h_scroll, 0);
    }

    #[test]
    fn test_file_list_scroll_right_does_not_exceed_max_h_scroll() {
        // A short filename fits inside the 30-column inner width, so max_h_scroll() == 0
        // and scrolling right should be a no-op.
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        // make_files produces flat files named "file_0" etc. (6 chars), which fit easily.
        assert_eq!(app.max_h_scroll(), 0, "short name should not allow any scroll");
        app.file_list_scroll_right();
        assert_eq!(app.file_list_h_scroll, 0, "scroll should not advance past max");
    }

    #[test]
    fn test_file_list_scroll_right_caps_at_overflow_amount() {
        // Use a filename that overflows the panel width.
        // inner_width = 30, prefix for a depth-0 file = 4 ("[M] "), so content gets 26 cols.
        // A 36-char name overflows by 10. max_h_scroll() should return 10.
        let long_name = "a".repeat(36); // 36 chars > 26 available
        let files = vec![ChangedFile {
            path: long_name.clone().into(),
            status: crate::diff::FileStatus::Modified,
        }];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        let cap = app.max_h_scroll();
        assert_eq!(cap, 10, "max scroll should equal the overflow amount");
        // Scroll past the cap
        for _ in 0..10 { app.file_list_scroll_right(); }
        assert_eq!(app.file_list_h_scroll, cap, "h_scroll must not exceed the cap");
    }

    #[test]
    fn test_file_list_navigation_does_not_reset_diff_scroll() {
        // Navigating with ↑/↓ must NOT reset diff_scroll — only an explicit load does.
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.diff_scroll = 50;
        app.file_list_down();
        assert_eq!(app.diff_scroll, 50, "navigation alone must not reset diff_scroll");
    }

    #[test]
    fn test_select_file_resets_scroll_and_hunk() {
        let mut app = app_with_diff(3);
        app.diff_scroll = 10;
        app.selected_hunk = 2;
        app.select_file(0);
        assert_eq!(app.diff_scroll, 0);
        assert_eq!(app.selected_hunk, 0);
    }

    // ── Hunk navigation ───────────────────────────────────────────────────────

    #[test]
    fn test_next_hunk_advances() {
        let mut app = app_with_diff(3);
        app.next_hunk();
        assert_eq!(app.selected_hunk, 1);
    }

    #[test]
    fn test_next_hunk_clamps_at_last() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.next_hunk();
        assert_eq!(app.selected_hunk, 2);
    }

    #[test]
    fn test_prev_hunk_retreats() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 1);
    }

    #[test]
    fn test_prev_hunk_clamps_at_zero() {
        let mut app = app_with_diff(3);
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 0);
    }

    #[test]
    fn test_next_hunk_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.next_hunk(); // should not panic
        assert_eq!(app.selected_hunk, 0);
    }

    // ── Comment flow ──────────────────────────────────────────────────────────

    #[test]
    fn test_start_comment_enters_comment_mode() {
        let mut app = app_with_diff(2);
        app.start_comment();
        assert!(matches!(app.mode, Mode::Comment { hunk_idx: 0, .. }));
    }

    #[test]
    fn test_start_comment_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.start_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_start_comment_no_op_with_empty_hunks() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: make_files(1).remove(0),
            hunks: vec![],
        });
        app.start_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_start_comment_redirects_to_edit_when_note_exists() {
        let mut app = app_with_note_on_hunk(0);
        app.start_comment();
        // Should enter comment mode pre-populated with the existing note, not a blank input
        assert!(matches!(
            &app.mode,
            Mode::Comment { input, .. } if input == "original note"
        ));
    }

    #[test]
    fn test_start_comment_does_not_create_duplicate() {
        let mut app = app_with_note_on_hunk(0);
        app.start_comment(); // redirects to edit — old note removed
        if let Mode::Comment { ref mut input, .. } = app.mode {
            *input = "original note".to_string(); // re-submit same text
        }
        app.submit_comment();
        assert_eq!(app.notes.len(), 1, "should have exactly one note, not two");
    }

    #[test]
    fn test_cancel_comment_returns_to_normal() {
        let mut app = app_with_diff(1);
        app.start_comment();
        app.cancel_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_cancel_edit_restores_original_note() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Escape without submitting — original note must be restored
        app.cancel_comment();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "original note");
    }

    #[test]
    fn test_cancel_new_comment_does_not_create_note() {
        let mut app = app_with_diff(1);
        app.start_comment();
        app.cancel_comment();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_submit_comment_creates_note() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "This looks wrong".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "This looks wrong");
        assert_eq!(app.notes[0].file, PathBuf::from("file_0.rs"));
    }

    #[test]
    fn test_submit_comment_resets_to_normal() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "some note".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_submit_comment_ignores_blank_input() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "   ".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_submit_comment_captures_hunk_content() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "check this".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();

        let content = &app.notes[0].hunk_content;
        assert!(content.contains("+new line"));
        assert!(content.contains("-old line"));
        assert!(content.contains(" context"));
    }

    #[test]
    fn test_submit_comment_on_second_hunk() {
        let mut app = app_with_diff(2);
        app.selected_hunk = 1;
        app.mode = Mode::Comment {
            hunk_idx: 1,
            input: "note on second hunk".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].hunk_header.contains("11")); // second hunk starts at 11
    }

    // ── Cross-file hunk boundary ──────────────────────────────────────────────

    #[test]
    fn test_at_last_hunk_boundary_true_when_last_hunk_and_more_files() {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: make_files(2)[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        app.selected_file = 0;
        app.selected_hunk = 0; // only hunk → last hunk
        assert!(app.at_last_hunk_boundary());
    }

    #[test]
    fn test_at_last_hunk_boundary_false_when_not_last_hunk() {
        let mut app = app_with_diff(3); // 3 hunks, selected_hunk=0
        assert!(!app.at_last_hunk_boundary());
    }

    #[test]
    fn test_at_last_hunk_boundary_false_when_last_file() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: make_files(1)[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        app.selected_file = 0;
        app.selected_hunk = 0;
        assert!(!app.at_last_hunk_boundary(), "no next file — should not cross");
    }

    #[test]
    fn test_at_last_hunk_boundary_false_without_diff() {
        let app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        assert!(!app.at_last_hunk_boundary());
    }

    #[test]
    fn test_at_first_hunk_boundary_true_when_first_hunk_and_not_first_file() {
        let files = make_files(2);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.selected_file = 1;
        app.current_diff = Some(DiffFile {
            file: files[1].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        app.selected_hunk = 0;
        assert!(app.at_first_hunk_boundary());
    }

    #[test]
    fn test_at_first_hunk_boundary_false_when_first_file() {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: make_files(2)[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        app.selected_file = 0;
        app.selected_hunk = 0;
        assert!(!app.at_first_hunk_boundary(), "already at first file — should not cross");
    }

    #[test]
    fn test_at_first_hunk_boundary_false_when_not_first_hunk() {
        let files = make_files(2);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.selected_file = 1;
        app.current_diff = Some(DiffFile {
            file: files[1].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@"), make_hunk("@@ -5,1 +5,1 @@")],
        });
        app.selected_hunk = 1;
        assert!(!app.at_first_hunk_boundary());
    }

    #[test]
    fn test_at_first_hunk_boundary_false_without_diff() {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.selected_file = 1;
        assert!(!app.at_first_hunk_boundary());
    }

    // ── Hunk scroll offset ────────────────────────────────────────────────────

    // Each test hunk has: 1 header + 3 lines (added, removed, context) + 1 blank = 5 lines.

    #[test]
    fn test_hunk_scroll_offset_first_hunk_is_zero() {
        let app = app_with_diff(3);
        assert_eq!(app.hunk_scroll_offset(0), 0);
    }

    #[test]
    fn test_hunk_scroll_offset_second_hunk() {
        let app = app_with_diff(3);
        // hunk 0: 1 header + 3 lines + 0 notes + 1 blank = 5
        assert_eq!(app.hunk_scroll_offset(1), 5);
    }

    #[test]
    fn test_hunk_scroll_offset_third_hunk() {
        let app = app_with_diff(3);
        // hunk 0 + hunk 1 = 5 + 5 = 10
        assert_eq!(app.hunk_scroll_offset(2), 10);
    }

    #[test]
    fn test_hunk_scroll_offset_accounts_for_notes() {
        let mut app = app_with_diff(2);
        // Add a note on hunk 0
        app.mode = Mode::Comment { hunk_idx: 0, input: "a note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        // hunk 0: 1 header + 3 lines + 1 note + 1 blank = 6
        assert_eq!(app.hunk_scroll_offset(1), 6);
    }

    #[test]
    fn test_scroll_to_selected_hunk_sets_diff_scroll() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.scroll_to_selected_hunk();
        assert_eq!(app.diff_scroll, 10);
    }

    #[test]
    fn test_next_hunk_scrolls_view() {
        let mut app = app_with_diff(3);
        app.next_hunk();
        assert_eq!(app.selected_hunk, 1);
        assert_eq!(app.diff_scroll, 5); // scrolled to hunk 1
    }

    #[test]
    fn test_prev_hunk_scrolls_view() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.diff_scroll = 10;
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 1);
        assert_eq!(app.diff_scroll, 5);
    }

    // ── Diff scroll capping ───────────────────────────────────────────────────

    #[test]
    fn test_diff_content_lines() {
        let app = app_with_diff(3);
        // 3 hunks * (1 header + 3 lines + 0 notes + 1 blank) = 3 * 5 = 15
        assert_eq!(app.diff_content_lines(), 15);
    }

    #[test]
    fn test_diff_content_lines_no_diff() {
        let app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        assert_eq!(app.diff_content_lines(), 0);
    }

    #[test]
    fn test_diff_scroll_down_caps_at_content_boundary() {
        let mut app = app_with_diff(1);
        // 1 hunk: 1 + 3 + 1 = 5 lines of content
        // viewport of 3 → max_scroll = 5 - 3 = 2
        app.diff_scroll_down(3);
        app.diff_scroll_down(3);
        app.diff_scroll_down(3); // should be capped
        assert!(app.diff_scroll <= 2);
    }

    #[test]
    fn test_diff_scroll_down_no_scroll_when_content_fits() {
        let mut app = app_with_diff(1);
        // content = 5 lines, viewport = 20 → max_scroll = 0
        app.diff_scroll_down(20);
        assert_eq!(app.diff_scroll, 0);
    }

    // ── visual_row_for_cursor ─────────────────────────────────────────────────

    #[test]
    fn test_visual_row_no_wrap() {
        // Width > line length: no wrapping, rows == logical line indices.
        assert_eq!(visual_row_for_cursor("hello\nworld", 0,   100), 0); // start of "hello"
        assert_eq!(visual_row_for_cursor("hello\nworld", 5,   100), 0); // end of "hello"
        assert_eq!(visual_row_for_cursor("hello\nworld", 6,   100), 1); // start of "world"
        assert_eq!(visual_row_for_cursor("hello\nworld", 11,  100), 1); // end of "world"
    }

    #[test]
    fn test_visual_row_with_wrap() {
        // "hellothere" with width=5 wraps to ["hello"(row 0), "there"(row 1)]
        assert_eq!(visual_row_for_cursor("hellothere", 0, 5), 0);
        assert_eq!(visual_row_for_cursor("hellothere", 4, 5), 0); // in "hello"
        assert_eq!(visual_row_for_cursor("hellothere", 5, 5), 1); // start of "there"
        assert_eq!(visual_row_for_cursor("hellothere", 10, 5), 1); // end of "there"
    }

    #[test]
    fn test_visual_row_multiline_with_wrap() {
        // "hi\nhellothere" with width=5:
        //   row 0: "hi"        (bytes 0..2)
        //   row 1: "hello"     (bytes 3..7 in full input = chars 0..4 of "hellothere")
        //   row 2: "there"     (bytes 8..12 in full input = chars 5..9 of "hellothere")
        let input = "hi\nhellothere";
        assert_eq!(visual_row_for_cursor(input, 0,  5), 0); // 'h' of "hi"
        assert_eq!(visual_row_for_cursor(input, 2,  5), 0); // end of "hi"
        assert_eq!(visual_row_for_cursor(input, 3,  5), 1); // 'h' = first char of "hellothere"
        assert_eq!(visual_row_for_cursor(input, 7,  5), 1); // 'o' = char 4 of "hellothere"
        assert_eq!(visual_row_for_cursor(input, 8,  5), 2); // 't' = char 5 of "hellothere", first of "there"
        assert_eq!(visual_row_for_cursor(input, 13, 5), 2); // end of "hellothere"
    }

    // ── Comment popup scrolling ───────────────────────────────────────────────

    #[test]
    fn test_scroll_comment_to_cursor_scrolls_down_when_cursor_below_viewport() {
        let mut app = app_with_diff(1);
        let input = "a\nb\nc\nd\ne".to_string();
        let cursor = input.len(); // cursor on visual row 4 (width=100: no wrap)
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor, original: None };
        app.scroll_comment_to_cursor(3, 100);
        assert_eq!(app.comment_scroll, 2); // 4+1-3 = 2
    }

    #[test]
    fn test_scroll_comment_to_cursor_no_scroll_when_cursor_visible() {
        let mut app = app_with_diff(1);
        let input = "line1\nline2".to_string();
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor: 5, original: None };
        app.scroll_comment_to_cursor(5, 100);
        assert_eq!(app.comment_scroll, 0);
    }

    #[test]
    fn test_scroll_comment_to_cursor_scrolls_up_when_cursor_above_viewport() {
        let mut app = app_with_diff(1);
        let input = "a\nb\nc\nd\ne".to_string();
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor: 0, original: None };
        app.comment_scroll = 3;
        app.scroll_comment_to_cursor(3, 100);
        assert_eq!(app.comment_scroll, 0);
    }

    #[test]
    fn test_scroll_comment_to_cursor_no_op_outside_comment_mode() {
        let mut app = app_with_diff(1);
        app.comment_scroll = 5;
        app.scroll_comment_to_cursor(10, 100);
        assert_eq!(app.comment_scroll, 5);
    }

    #[test]
    fn test_scroll_comment_to_cursor_accounts_for_wrap() {
        // Single long line that wraps: "aaaaaaaaaa" (10 a's) with content_width=5
        // Visual rows: row 0 = "aaaaa", row 1 = "aaaaa"
        // cursor at byte 7 (in second visual row) with viewport=1 should scroll to row 1
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "aaaaaaaaaa".to_string(),
            cursor: 7,
            original: None,
        };
        app.scroll_comment_to_cursor(1, 5);
        assert_eq!(app.comment_scroll, 1);
    }

    // ── Edit / delete notes ───────────────────────────────────────────────────

    fn app_with_note_on_hunk(hunk_idx: usize) -> App {
        let mut app = app_with_diff(3);
        app.selected_hunk = hunk_idx;
        app.mode = Mode::Comment {
            hunk_idx,
            input: "original note".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        app.selected_hunk = hunk_idx;
        app
    }

    #[test]
    fn test_current_hunk_has_note_true_when_note_exists() {
        let app = app_with_note_on_hunk(0);
        assert!(app.current_hunk_has_note());
    }

    #[test]
    fn test_current_hunk_has_note_false_when_no_note() {
        let app = app_with_diff(2);
        assert!(!app.current_hunk_has_note());
    }

    #[test]
    fn test_current_hunk_has_note_false_without_diff() {
        let app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        assert!(!app.current_hunk_has_note());
    }

    #[test]
    fn test_delete_note_removes_it() {
        let mut app = app_with_note_on_hunk(0);
        app.delete_note_for_current_hunk();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_only_removes_current_hunk() {
        let mut app = app_with_diff(3);
        // Add notes on hunks 0 and 1
        for hunk_idx in [0, 1] {
            app.selected_hunk = hunk_idx;
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx), cursor: 0, original: None };
            app.submit_comment();
        }
        // Delete note on hunk 0 only
        app.selected_hunk = 0;
        app.delete_note_for_current_hunk();

        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].note.contains("note 1"));
    }

    #[test]
    fn test_delete_note_no_op_when_no_note() {
        let mut app = app_with_diff(2);
        app.delete_note_for_current_hunk(); // should not panic
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.delete_note_for_current_hunk(); // should not panic
    }

    #[test]
    fn test_edit_note_enters_comment_mode_with_existing_text() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        assert!(matches!(
            &app.mode,
            Mode::Comment { input, .. } if input == "original note"
        ));
    }

    #[test]
    fn test_edit_note_removes_old_note_before_editing() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Note should be gone — it will be re-added on submit
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_edit_note_no_op_when_no_note() {
        let mut app = app_with_diff(2);
        app.edit_note_for_current_hunk();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_edit_then_submit_updates_note() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Simulate the user clearing the input and typing a new note
        if let Mode::Comment { ref mut input, .. } = app.mode {
            *input = "revised note".to_string();
        }
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "revised note");
    }

    // ── Multi-line comment input ──────────────────────────────────────────────

    #[test]
    fn test_submit_comment_preserves_internal_newlines() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "line one\nline two\nline three".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert_eq!(app.notes[0].note, "line one\nline two\nline three");
    }

    #[test]
    fn test_submit_comment_trims_surrounding_newlines() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "\n\nline one\nline two\n\n".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert_eq!(app.notes[0].note, "line one\nline two");
    }

    #[test]
    fn test_submit_comment_blank_multiline_is_ignored() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "\n\n\n".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert!(app.notes.is_empty(), "all-whitespace multi-line input should not create a note");
    }

    // ── Context folding ───────────────────────────────────────────────────────

    fn make_lines(kinds: &[LineKind]) -> Vec<crate::diff::DiffLine> {
        kinds.iter().map(|k| crate::diff::DiffLine {
            old_lineno: Some(1),
            new_lineno: Some(1),
            kind: k.clone(),
            content: "x".to_string(),
        }).collect()
    }

    #[test]
    fn test_context_run_visual_lines_short_run_shown_as_is() {
        // 3 context lines < FOLD_THRESHOLD — should count as 3, not 1
        let lines = make_lines(&[LineKind::Context; 3]);
        assert_eq!(context_run_visual_lines(&lines, 0), 3);
    }

    #[test]
    fn test_context_run_visual_lines_long_run_folds_to_one() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert_eq!(context_run_visual_lines(&lines, 0), 1);
    }

    #[test]
    fn test_context_run_visual_lines_mixed() {
        // added + 2 context + added + FOLD_THRESHOLD context + added
        let mut kinds = vec![LineKind::Added];
        kinds.extend(vec![LineKind::Context; 2]);
        kinds.push(LineKind::Added);
        kinds.extend(vec![LineKind::Context; FOLD_THRESHOLD]);
        kinds.push(LineKind::Added);
        let lines = make_lines(&kinds);
        // visual: 1 + 2 + 1 + 1(fold) + 1 = 6
        assert_eq!(context_run_visual_lines(&lines, 0), 6);
    }

    #[test]
    fn test_hunk_has_foldable_context_false_when_below_threshold() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD - 1]);
        assert!(!hunk_has_foldable_context(&lines));
    }

    #[test]
    fn test_hunk_has_foldable_context_true_at_threshold() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert!(hunk_has_foldable_context(&lines));
    }

    // ── visual_rows_for_diff_line ─────────────────────────────────────────────

    #[test]
    fn test_visual_rows_zero_width_returns_one() {
        assert_eq!(visual_rows_for_diff_line("long line content", 0), 1);
    }

    #[test]
    fn test_visual_rows_short_line_fits_in_one_row() {
        // gutter=6, content=10 → total=16 ≤ panel_width=80 → 1 row
        assert_eq!(visual_rows_for_diff_line("0123456789", 80), 1);
    }

    #[test]
    fn test_visual_rows_exactly_fills_panel() {
        // total = 6 + 74 = 80 chars, panel=80 → 1 row
        let content = "x".repeat(74);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 1);
    }

    #[test]
    fn test_visual_rows_one_char_over_wraps_to_two() {
        // total = 6 + 75 = 81 chars, panel=80 → 2 rows
        let content = "x".repeat(75);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 2);
    }

    #[test]
    fn test_visual_rows_double_panel_width_gives_two_rows() {
        // total = 6 + 154 = 160 chars, panel=80 → 2 rows
        let content = "x".repeat(154);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 2);
    }

    #[test]
    fn test_visual_rows_context_line_accounting_in_scroll() {
        // Verify diff_content_lines accounts for wrap when diff_view_content_width is set.
        // Three hunks, each with one added line of 75 chars (would wrap at panel_width=80).
        use crate::diff::{DiffFile, DiffLine, Hunk, LineKind};
        let mut app = app_with_diff(0);
        let long_content = "x".repeat(75); // wraps to 2 visual rows at panel_width=80
        app.current_diff = Some(DiffFile {
            file: make_files(1).remove(0),
            hunks: vec![Hunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: vec![DiffLine {
                    old_lineno: None, new_lineno: Some(1),
                    kind: LineKind::Added, content: long_content,
                }],
            }],
        });
        // Without wrap accounting: 1 hunk = 1(header) + 1(line) + 0(notes) + 1(blank) = 3
        assert_eq!(app.diff_content_lines(), 3);
        // With wrap accounting: 1(header) + 2(visual rows) + 0 + 1 = 4
        app.diff_view_content_width = 80;
        assert_eq!(app.diff_content_lines(), 4);
    }

    #[test]
    fn test_toggle_hunk_fold_expands_then_collapses() {
        let mut app = app_with_diff(2);
        assert!(!app.expanded_hunks.contains(&0));
        app.toggle_hunk_fold();
        assert!(app.expanded_hunks.contains(&0));
        app.toggle_hunk_fold();
        assert!(!app.expanded_hunks.contains(&0));
    }

    #[test]
    fn test_select_file_clears_expanded_hunks() {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.expanded_hunks.insert(0);
        app.select_file(1);
        assert!(app.expanded_hunks.is_empty());
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

    fn app_with_two_file_notes() -> App {
        let mut app = app_with_diff(2);
        // Note on hunk 0
        app.mode = Mode::Comment { hunk_idx: 0, input: "first note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        // Note on hunk 1
        app.selected_hunk = 1;
        app.mode = Mode::Comment { hunk_idx: 1, input: "second note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        app.selected_note = 0;
        app
    }

    #[test]
    fn test_notes_down_navigates() {
        let mut app = app_with_two_file_notes();
        app.notes_down();
        assert_eq!(app.selected_note, 1);
    }

    #[test]
    fn test_notes_down_clamps_at_end() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.notes_down();
        assert_eq!(app.selected_note, 1);
    }

    #[test]
    fn test_notes_up_navigates() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.notes_up();
        assert_eq!(app.selected_note, 0);
    }

    #[test]
    fn test_notes_up_clamps_at_zero() {
        let mut app = app_with_two_file_notes();
        app.notes_up();
        assert_eq!(app.selected_note, 0);
    }

    // ── Notes panel scrolling ────────────────────────────────────────────────

    fn app_with_many_notes(n: usize) -> App {
        let mut app = app_with_diff(3);
        for hunk_idx in 0..n.min(3) {
            app.selected_hunk = hunk_idx;
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx), cursor: 0, original: None };
            app.submit_comment();
        }
        app.selected_note = 0;
        app
    }

    #[test]
    fn test_scroll_notes_no_op_when_selected_visible() {
        let mut app = app_with_many_notes(2);
        app.selected_note = 0;
        app.scroll_notes_to_selected(8); // both notes fit in 8 rows (3 rows each)
        assert_eq!(app.notes_scroll, 0);
    }

    #[test]
    fn test_scroll_notes_scrolls_down_when_below_viewport() {
        let mut app = app_with_many_notes(3);
        // 3 collapsed notes = 9 visual rows; viewport=6 can show 2 notes
        app.selected_note = 2; // note 2 starts at row 6
        app.scroll_notes_to_selected(6);
        // note 2 ends at row 9, viewport=6 → scroll = 9-6 = 3
        assert_eq!(app.notes_scroll, 3);
    }

    #[test]
    fn test_scroll_notes_scrolls_up_when_above_viewport() {
        let mut app = app_with_many_notes(3);
        app.notes_scroll = 6; // viewport shows rows 6+
        app.selected_note = 0; // note 0 starts at row 0
        app.scroll_notes_to_selected(6);
        assert_eq!(app.notes_scroll, 0);
    }

    #[test]
    fn test_note_visual_rows_collapsed() {
        let note = FeedbackNote {
            file: std::path::PathBuf::from("src/foo.rs"),
            hunk_header: "@@ -1,1 +1,1 @@".to_string(),
            hunk_content: String::new(),
            note: "single line".to_string(),
        };
        assert_eq!(note_visual_rows(&note, false), 3);
    }

    #[test]
    fn test_note_visual_rows_expanded_multiline() {
        let note = FeedbackNote {
            file: std::path::PathBuf::from("src/foo.rs"),
            hunk_header: "@@ -1,1 +1,1 @@".to_string(),
            hunk_content: String::new(),
            note: "line one\nline two\nline three".to_string(),
        };
        // header(1) + 3 lines + blank(1) = 5
        assert_eq!(note_visual_rows(&note, true), 5);
    }

    #[test]
    fn test_delete_selected_note_resets_scroll() {
        let mut app = app_with_many_notes(2);
        app.notes_scroll = 3;
        app.delete_selected_note();
        assert_eq!(app.notes_scroll, 0);
    }

    #[test]
    fn test_toggle_note_expand() {
        let mut app = app_with_two_file_notes();
        assert!(!app.expanded_notes.contains(&0));
        app.toggle_note_expand();
        assert!(app.expanded_notes.contains(&0));
        app.toggle_note_expand();
        assert!(!app.expanded_notes.contains(&0));
    }

    #[test]
    fn test_selected_note_file_idx_found() {
        let app = app_with_two_file_notes();
        // Both notes are on src/file_0.rs (same file in app_with_diff)
        assert_eq!(app.selected_note_file_idx(), Some(0));
    }

    #[test]
    fn test_selected_note_file_idx_none_when_no_notes() {
        let app = app_with_diff(1);
        assert_eq!(app.selected_note_file_idx(), None);
    }

    #[test]
    fn test_delete_selected_note_removes_it() {
        let mut app = app_with_two_file_notes();
        app.delete_selected_note();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "second note");
    }

    #[test]
    fn test_delete_selected_note_adjusts_index_when_at_end() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.delete_selected_note();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.selected_note, 0);
    }

    #[test]
    fn test_delete_selected_note_clears_expanded_notes() {
        let mut app = app_with_two_file_notes();
        app.expanded_notes.insert(0);
        app.delete_selected_note();
        assert!(app.expanded_notes.is_empty());
    }

    #[test]
    fn test_delete_selected_note_noop_when_empty() {
        let mut app = app_with_diff(1);
        app.delete_selected_note(); // should not panic
        assert!(app.notes.is_empty());
    }

    // ── selected_range ────────────────────────────────────────────────────────

    #[test]
    fn test_selected_range_forward() {
        assert_eq!(selected_range(8, Some(3)), Some((3, 8)));
    }

    #[test]
    fn test_selected_range_backward() {
        assert_eq!(selected_range(3, Some(8)), Some((3, 8)));
    }

    #[test]
    fn test_selected_range_no_anchor() {
        assert_eq!(selected_range(5, None), None);
    }

    #[test]
    fn test_selected_range_empty_when_cursor_equals_anchor() {
        assert_eq!(selected_range(5, Some(5)), None);
    }

    // ── delete_selection ─────────────────────────────────────────────────────

    #[test]
    fn test_delete_selection_forward_range() {
        // "hello world", delete bytes 3..8 = "lo wo"
        let (s, c) = delete_selection("hello world", 8, Some(3)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }

    #[test]
    fn test_delete_selection_backward_range() {
        let (s, c) = delete_selection("hello world", 3, Some(8)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }

    #[test]
    fn test_delete_selection_no_anchor_returns_none() {
        assert!(delete_selection("hello", 3, None).is_none());
    }

    #[test]
    fn test_delete_selection_empty_range_returns_none() {
        assert!(delete_selection("hello", 3, Some(3)).is_none());
    }

    #[test]
    fn test_delete_selection_full_text() {
        let (s, c) = delete_selection("hello", 5, Some(0)).unwrap();
        assert_eq!(s, "");
        assert_eq!(c, 0);
    }

    #[test]
    fn test_delete_selection_across_newline() {
        let input = "line1\nline2\nline3";
        // delete the '\n' at byte 5
        let (s, c) = delete_selection(input, 5, Some(6)).unwrap();
        assert_eq!(s, "line1line2\nline3");
        assert_eq!(c, 5);
    }

    #[test]
    fn test_delete_selection_multiline_span() {
        let input = "hello\nworld";
        // delete bytes 3..8 = "lo\nwo"
        let (s, c) = delete_selection(input, 8, Some(3)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }

    // ── File tree ──────────────────────────────────────────────────────────────

    fn dir_files() -> Vec<ChangedFile> {
        vec![
            ChangedFile { path: PathBuf::from("src/a.rs"), status: FileStatus::Modified },
            ChangedFile { path: PathBuf::from("src/b.rs"), status: FileStatus::Modified },
        ]
    }

    #[test]
    fn test_tree_items_flat_list() {
        let app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        let tree = app.tree_items();
        assert_eq!(tree.len(), 2);
        assert!(tree.iter().all(|i| i.file_idx().is_some()));
    }

    #[test]
    fn test_tree_items_with_dir() {
        let app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        let tree = app.tree_items();
        // Dir("src/") + 2 files = 3 items
        assert_eq!(tree.len(), 3);
        assert!(tree[0].is_dir());
    }

    #[test]
    fn test_toggle_dir_collapses_tree() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        assert_eq!(app.tree_items().len(), 3);
        app.file_tree_cursor = 0; // on Dir("src/")
        app.toggle_dir_at_cursor();
        assert_eq!(app.tree_items().len(), 1); // only dir node visible
    }

    #[test]
    fn test_toggle_dir_expands_tree() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 0;
        app.toggle_dir_at_cursor(); // collapse
        app.toggle_dir_at_cursor(); // expand
        assert_eq!(app.tree_items().len(), 3);
    }

    #[test]
    fn test_toggle_dir_clamps_cursor_on_collapse() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 2; // on File(b.rs)
        // cursor is not on a dir, so toggle_dir_at_cursor is a no-op
        app.toggle_dir_at_cursor();
        assert_eq!(app.file_tree_cursor, 2); // unchanged
    }

    #[test]
    fn test_toggle_dir_no_op_on_file() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 1; // File(a.rs)
        let len_before = app.tree_items().len();
        app.toggle_dir_at_cursor();
        assert_eq!(app.tree_items().len(), len_before);
    }

    #[test]
    fn test_expand_parents_of_removes_collapsed_dir() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.collapsed_dirs.insert(PathBuf::from("src"));
        app.expand_parents_of(0);
        assert!(!app.collapsed_dirs.contains(&PathBuf::from("src")));
    }

    #[test]
    fn test_expand_parents_of_root_file_no_op() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.expand_parents_of(0); // should not panic
    }

    #[test]
    fn test_sync_tree_cursor_to_file() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        // tree: [Dir(0), File(a.rs)=1, File(b.rs)=2]
        app.selected_file = 1;
        app.sync_tree_cursor_to_file();
        assert_eq!(app.file_tree_cursor, 2); // File(b.rs) is at tree index 2
    }

    #[test]
    fn test_file_list_down_skips_to_next_file_when_dir() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        // tree: [Dir("src/"), File(a), File(b)] — cursor=0 on Dir
        app.file_list_down(); // moves to File(a)
        assert_eq!(app.file_tree_cursor, 1);
        assert_eq!(app.selected_file, 0); // file_idx 0 = a.rs
    }

    #[test]
    fn test_tree_items_notes_propagate_to_dir() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.notes.push(FeedbackNote {
            file: PathBuf::from("src/a.rs"),
            hunk_header: "@@".to_string(),
            hunk_content: String::new(),
            note: "note".to_string(),
        });
        let tree = app.tree_items();
        // Dir should have has_notes=true
        if let crate::filetree::TreeItem::Dir { has_notes, .. } = &tree[0] {
            assert!(*has_notes);
        } else {
            panic!("expected dir as first item");
        }
    }
}
