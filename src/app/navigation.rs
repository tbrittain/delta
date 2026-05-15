use crate::filetree::TreeItem;
use super::{App, FILE_LIST_INNER_WIDTH};
use super::layout::{context_run_visual_lines, hunk_has_foldable_context, visual_rows_for_diff_line};

impl App {
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

    /// Select the first file in visual tree order and sync `file_tree_cursor` to match.
    /// Called once on startup so the highlighted file list entry, `selected_file`, and
    /// the initial diff load all agree — regardless of the order git returns files.
    pub fn select_first_tree_file(&mut self) {
        let tree = self.tree_items();
        if let Some((pos, idx)) = tree.iter().enumerate().find_map(|(pos, item)| {
            item.file_idx().map(|idx| (pos, idx))
        }) {
            self.selected_file = idx;
            self.file_tree_cursor = pos;
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

    /// Returns the `file_idx` of the next file after the current tree cursor,
    /// scanning in visual tree order. Returns `None` when already at the last visible file.
    pub fn next_file_in_tree(&self) -> Option<usize> {
        let tree = self.tree_items();
        (self.file_tree_cursor + 1..tree.len()).find_map(|i| tree[i].file_idx())
    }

    /// Returns the `file_idx` of the previous file before the current tree cursor,
    /// scanning in visual tree order. Returns `None` when already at the first visible file.
    pub fn prev_file_in_tree(&self) -> Option<usize> {
        let tree = self.tree_items();
        (0..self.file_tree_cursor).rev().find_map(|i| tree[i].file_idx())
    }

    /// True when `]` should cross to the next file: we are on the last hunk of the
    /// current file and there is at least one more visible file in the tree.
    pub fn at_last_hunk_boundary(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        !diff.hunks.is_empty()
            && self.selected_hunk + 1 >= diff.hunks.len()
            && self.next_file_in_tree().is_some()
    }

    /// True when `[` should cross to the previous file: we are on the first hunk of
    /// the current file and there is at least one earlier visible file in the tree.
    pub fn at_first_hunk_boundary(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        !diff.hunks.is_empty() && self.selected_hunk == 0 && self.prev_file_in_tree().is_some()
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
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Mode, FeedbackNote};
    use crate::app::test_helpers::*;
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use crate::git::WhitespaceMode;
    use std::path::PathBuf;

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

    // ── select_first_tree_file ────────────────────────────────────────────────

    #[test]
    fn test_select_first_tree_file_flat_list() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.select_first_tree_file();
        assert_eq!(app.selected_file, 0);
        assert_eq!(app.file_tree_cursor, 0);
    }

    #[test]
    fn test_select_first_tree_file_with_dir_at_top() {
        let files = vec![
            ChangedFile { path: PathBuf::from("src/a.rs"), status: FileStatus::Modified },
            ChangedFile { path: PathBuf::from("src/b.rs"), status: FileStatus::Modified },
        ];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.select_first_tree_file();
        assert_eq!(app.file_tree_cursor, 1);
        assert!(app.files[app.selected_file].path.ends_with("a.rs"));
    }

    #[test]
    fn test_select_first_tree_file_syncs_cursor_and_selected_file() {
        let files = vec![
            ChangedFile { path: PathBuf::from("src/a.rs"), status: FileStatus::Modified },
            ChangedFile { path: PathBuf::from("src/b.rs"), status: FileStatus::Modified },
        ];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.select_first_tree_file();
        let tree = app.tree_items();
        assert_eq!(tree[app.file_tree_cursor].file_idx(), Some(app.selected_file));
    }

    // ── File list navigation ──────────────────────────────────────────────────

    #[test]
    fn test_file_list_down_navigates() {
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
        let long_name = "a".repeat(36);
        let files = vec![ChangedFile { path: long_name.into(), status: FileStatus::Modified }];
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
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        assert_eq!(app.max_h_scroll(), 0, "short name should not allow any scroll");
        app.file_list_scroll_right();
        assert_eq!(app.file_list_h_scroll, 0, "scroll should not advance past max");
    }

    #[test]
    fn test_file_list_scroll_right_caps_at_overflow_amount() {
        let long_name = "a".repeat(36);
        let files = vec![ChangedFile {
            path: long_name.clone().into(),
            status: FileStatus::Modified,
        }];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        let cap = app.max_h_scroll();
        assert_eq!(cap, 10, "max scroll should equal the overflow amount");
        for _ in 0..10 { app.file_list_scroll_right(); }
        assert_eq!(app.file_list_h_scroll, cap, "h_scroll must not exceed the cap");
    }

    #[test]
    fn test_file_list_navigation_does_not_reset_diff_scroll() {
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
        app.next_hunk();
        assert_eq!(app.selected_hunk, 0);
    }

    // ── Cross-file hunk boundary ──────────────────────────────────────────────

    #[test]
    fn test_next_file_in_tree_returns_next_file() {
        let app = app_at_file(0);
        assert_eq!(app.next_file_in_tree(), Some(1));
    }

    #[test]
    fn test_next_file_in_tree_none_at_last_file() {
        let app = app_at_file(1);
        assert_eq!(app.next_file_in_tree(), None);
    }

    #[test]
    fn test_prev_file_in_tree_returns_prev_file() {
        let app = app_at_file(1);
        assert_eq!(app.prev_file_in_tree(), Some(0));
    }

    #[test]
    fn test_prev_file_in_tree_none_at_first_file() {
        let app = app_at_file(0);
        assert_eq!(app.prev_file_in_tree(), None);
    }

    #[test]
    fn test_at_last_hunk_boundary_true_when_last_hunk_and_more_files() {
        let files = make_files(2);
        let mut app = app_at_file(0);
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        app.selected_hunk = 0;
        assert!(app.at_last_hunk_boundary());
    }

    #[test]
    fn test_at_last_hunk_boundary_false_when_not_last_hunk() {
        let app = app_with_diff(3);
        assert!(!app.at_last_hunk_boundary());
    }

    #[test]
    fn test_at_last_hunk_boundary_false_when_last_file() {
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
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
        let mut app = app_at_file(1);
        app.current_diff = Some(DiffFile {
            file: files[1].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        assert!(app.at_first_hunk_boundary());
    }

    #[test]
    fn test_at_first_hunk_boundary_false_when_first_file() {
        let files = make_files(2);
        let mut app = app_at_file(0);
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@")],
        });
        assert!(!app.at_first_hunk_boundary(), "already at first file — should not cross");
    }

    #[test]
    fn test_at_first_hunk_boundary_false_when_not_first_hunk() {
        let files = make_files(2);
        let mut app = app_at_file(1);
        app.current_diff = Some(DiffFile {
            file: files[1].clone(),
            hunks: vec![make_hunk("@@ -1,1 +1,1 @@"), make_hunk("@@ -5,1 +5,1 @@")],
        });
        app.selected_hunk = 1;
        assert!(!app.at_first_hunk_boundary());
    }

    #[test]
    fn test_at_first_hunk_boundary_false_without_diff() {
        let app = app_at_file(1);
        assert!(!app.at_first_hunk_boundary());
    }

    #[test]
    fn test_next_file_in_tree_skips_dirs() {
        let files = vec![
            ChangedFile { path: PathBuf::from("src/a.rs"), status: FileStatus::Modified },
            ChangedFile { path: PathBuf::from("src/b.rs"), status: FileStatus::Modified },
        ];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 0; // cursor on Dir
        assert!(app.next_file_in_tree().is_some());
    }

    // ── Hunk scroll offset ────────────────────────────────────────────────────

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
        assert_eq!(app.diff_scroll, 5);
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
        // 1 hunk: 1 + 3 + 1 = 5 lines; viewport=3 → max_scroll=2
        app.diff_scroll_down(3);
        app.diff_scroll_down(3);
        app.diff_scroll_down(3);
        assert!(app.diff_scroll <= 2);
    }

    #[test]
    fn test_diff_scroll_down_no_scroll_when_content_fits() {
        let mut app = app_with_diff(1);
        // content=5 lines, viewport=20 → max_scroll=0
        app.diff_scroll_down(20);
        assert_eq!(app.diff_scroll, 0);
    }

    #[test]
    fn test_visual_rows_context_line_accounting_in_scroll() {
        let mut app = app_with_diff(0);
        let long_content = "x".repeat(75);
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
        // Without wrap: 1 header + 1 line + 0 notes + 1 blank = 3
        assert_eq!(app.diff_content_lines(), 3);
        // With wrap at 80: 1 header + 2 visual rows + 0 + 1 = 4
        app.diff_view_content_width = 80;
        assert_eq!(app.diff_content_lines(), 4);
    }

    // ── Context folding ───────────────────────────────────────────────────────

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

    // ── File tree ──────────────────────────────────────────────────────────────

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
        assert_eq!(tree.len(), 3);
        assert!(tree[0].is_dir());
    }

    #[test]
    fn test_toggle_dir_collapses_tree() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        assert_eq!(app.tree_items().len(), 3);
        app.file_tree_cursor = 0;
        app.toggle_dir_at_cursor();
        assert_eq!(app.tree_items().len(), 1);
    }

    #[test]
    fn test_toggle_dir_expands_tree() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 0;
        app.toggle_dir_at_cursor();
        app.toggle_dir_at_cursor();
        assert_eq!(app.tree_items().len(), 3);
    }

    #[test]
    fn test_toggle_dir_clamps_cursor_on_collapse() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 2;
        app.toggle_dir_at_cursor();
        assert_eq!(app.file_tree_cursor, 2);
    }

    #[test]
    fn test_toggle_dir_no_op_on_file() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = 1;
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
        app.expand_parents_of(0);
    }

    #[test]
    fn test_sync_tree_cursor_to_file() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        // tree: [Dir(0), File(a.rs)=1, File(b.rs)=2]
        app.selected_file = 1;
        app.sync_tree_cursor_to_file();
        assert_eq!(app.file_tree_cursor, 2);
    }

    #[test]
    fn test_file_list_down_skips_to_next_file_when_dir() {
        let mut app = App::new(dir_files(), "main".to_string(), "HEAD".to_string());
        app.file_list_down();
        assert_eq!(app.file_tree_cursor, 1);
        assert_eq!(app.selected_file, 0);
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
        if let crate::filetree::TreeItem::Dir { has_notes, .. } = &tree[0] {
            assert!(*has_notes);
        } else {
            panic!("expected dir as first item");
        }
    }
}
