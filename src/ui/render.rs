use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::app::{App, Mode, Panel, ViewMode};
use crate::diff::FileStatus;
use crate::filetree::TreeItem;
use crate::git::WhitespaceMode;
use super::{ACCENT, MUTED};
use super::diff_render::build_diff_text;
use super::split_render::build_split_diff_text;
use super::popup::render_comment_popup;

pub(super) fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let [file_area, right_area] = {
        let areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)])
            .split(vertical[0]);
        [areas[0], areas[1]]
    };

    render_file_list(frame, app, file_area);
    if !app.notes.is_empty() {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(10)])
            .split(right_area);
        render_diff_view(frame, app, areas[0]);
        render_notes_panel(frame, app, areas[1]);
    } else {
        render_diff_view(frame, app, right_area);
    }
    if matches!(app.mode, Mode::Comment { .. }) {
        render_comment_popup(frame, app, vertical[0]);
    }
    render_status_bar(frame, app, vertical[1]);
}

pub(super) fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::FileList;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };
    let h_scroll = app.file_list_h_scroll;
    let inner_width = area.width.saturating_sub(2) as usize;
    let tree = app.tree_items();
    let items: Vec<ListItem> = tree.iter().map(|item| {
        let depth = item.depth();
        let indent = "  ".repeat(depth);
        match item {
            TreeItem::Dir { display_name, file_count, collapsed, has_notes, .. } => {
                let arrow = if *collapsed { "▸" } else { "▾" };
                let note_marker = if *has_notes { " ●" } else { "" };
                let raw_spans: Vec<(String, Style)> = vec![
                    (indent, Style::default()),
                    (format!("{} ", arrow), Style::default().fg(ACCENT)),
                    (format!("{} ({}){}", display_name, file_count, note_marker), Style::default()),
                ];
                ListItem::new(Line::from(viewport_hscroll(raw_spans, h_scroll, inner_width)))
            }
            TreeItem::File { file_idx, display_name, has_notes, .. } => {
                let f = &app.files[*file_idx];
                let note_marker = if *has_notes { " ●" } else { "" };
                let status_color = match f.status {
                    FileStatus::Added    => Color::Green,
                    FileStatus::Modified => Color::Yellow,
                    FileStatus::Deleted  => Color::Red,
                    FileStatus::Renamed  => Color::Cyan,
                };
                let raw_spans: Vec<(String, Style)> = vec![
                    (indent, Style::default()),
                    (format!("[{}]", f.status.indicator()), Style::default().fg(status_color)),
                    (format!(" {}{}", display_name, note_marker), Style::default()),
                ];
                ListItem::new(Line::from(viewport_hscroll(raw_spans, h_scroll, inner_width)))
            }
        }
    }).collect();
    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(format!(" Files ({}) · {}..{} ", app.files.len(), app.from, app.to)))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.file_tree_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

pub(super) fn render_diff_view(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::DiffView;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };
    // Use the loaded file's path so the title stays in sync with the diff
    // content. Falling back to selected_file only when nothing is loaded yet.
    let file_name = app.current_rich_diff
        .as_ref()
        .map(|d| d.file.path.display().to_string())
        .or_else(|| app.files.get(app.selected_file).map(|f| f.path.display().to_string()))
        .unwrap_or_else(|| "Diff".to_string());
    let ws_label = app.whitespace_mode.label();
    let split_label = match app.view_mode {
        ViewMode::Inline     => "",
        ViewMode::SideBySide => " [split]",
    };
    let title = match &app.current_rich_diff {
        Some(diff) if !diff.hunks.is_empty() =>
            format!(" {} — {}/{}{}{} ", file_name, app.selected_hunk + 1, diff.hunks.len(), ws_label, split_label),
        _ => format!(" {}{}{} ", file_name, ws_label, split_label),
    };
    let note_max_chars = area.width.saturating_sub(6) as usize;
    let text = match app.view_mode {
        ViewMode::Inline     => build_diff_text(app, note_max_chars),
        ViewMode::SideBySide => build_split_diff_text(app, note_max_chars),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .border_type(border_type)
        .style(Style::default().bg(app.highlighter.panel_bg))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(text)
            .scroll((app.diff_scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        inner,
    );
    // Scroll-position indicator: only shown when the diff is taller than the viewport.
    let total = app.diff_content_lines();
    let viewport = inner.height as usize;
    if total > viewport {
        let mut scrollbar_state = ScrollbarState::new(total)
            .position(app.diff_scroll)
            .viewport_content_length(viewport);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut scrollbar_state,
        );
    }
}

pub(super) fn render_notes_panel(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::NotesView;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };
    let content_width = area.width.saturating_sub(2) as usize;
    let max_header = content_width.saturating_sub(2);
    let max_text   = content_width.saturating_sub(4);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.notes.is_empty() {
        lines.push(Line::from(Span::styled("No notes yet.", Style::default().fg(Color::DarkGray))));
    } else {
        for (i, note) in app.notes.iter().enumerate() {
            let is_selected = i == app.selected_note;
            let is_expanded = app.expanded_notes.contains(&i);
            let header_style = if is_selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else { Style::default().fg(MUTED) };
            let marker = if is_selected { "▶ " } else { "  " };
            let range_str = match &note.line_range {
                Some(r) => format!(" · L{}–{}", r.start, r.end),
                None    => String::new(),
            };
            let full_header = format!("{} · {}{}", note.file.display(), note.hunk_header, range_str);
            let header_text = if full_header.chars().count() > max_header {
                format!("{}…", full_header.chars().take(max_header.saturating_sub(1)).collect::<String>())
            } else { full_header };
            lines.push(Line::from(Span::styled(format!("{}{}", marker, header_text), header_style)));
            let note_style = Style::default().fg(Color::White);
            if is_expanded {
                for line_text in note.note.lines() {
                    lines.push(Line::from(vec![Span::raw("    "), Span::styled(line_text.to_string(), note_style)]));
                }
            } else {
                let first_line = note.note.lines().next().unwrap_or("");
                let truncated = if first_line.chars().count() > max_text {
                    format!("{}…", first_line.chars().take(max_text.saturating_sub(1)).collect::<String>())
                } else { first_line.to_string() };
                lines.push(Line::from(vec![Span::raw("    "), Span::styled(truncated, note_style)]));
            }
            lines.push(Line::raw(""));
        }
    }
    let para = Paragraph::new(Text::from(lines))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(format!(" Notes ({}) ", app.notes.len())))
        .scroll((app.notes_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

/// Apply a viewport horizontal scroll to a sequence of coloured spans.
/// Characters before `skip` are dropped (spread evenly across spans in order);
/// at most `max_width` characters are returned. Colours are preserved per span.
pub(super) fn viewport_hscroll(spans: Vec<(String, Style)>, skip: usize, max_width: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut remaining_skip = skip;
    let mut remaining_width = max_width;
    for (text, style) in spans {
        if remaining_width == 0 { break; }
        let chars: Vec<char> = text.chars().collect();
        let skip_here = remaining_skip.min(chars.len());
        remaining_skip -= skip_here;
        let visible = &chars[skip_here..];
        if visible.is_empty() { continue; }
        let take = remaining_width.min(visible.len());
        let s: String = visible[..take].iter().collect();
        remaining_width -= take;
        result.push(Span::styled(s, style));
    }
    result
}

pub(super) fn status_bar_text(app: &App) -> String {
    match app.mode {
        Mode::Comment { .. } => " Ctrl+S: submit   Ctrl+C/V/X: copy/paste/cut   Shift+arrows: select   Esc: cancel".to_string(),
        Mode::LineSelect { .. } => {
            let del = if app.selected_range_has_note() { "  d: delete" } else { "" };
            format!(" ↑↓: move selection   c: comment{}   Esc: cancel", del)
        }
        Mode::Normal => match app.focused_panel {
            Panel::FileList  => " Tab/Shift+Tab: navigate  ↑↓: items  ←/→: scroll names  Enter/Space: open/toggle  q: quit".to_string(),
            Panel::NotesView => " Tab/Shift+Tab: navigate  ↑↓: notes  Enter: jump  Space: expand  e: edit  d: delete  q: quit".to_string(),
            Panel::DiffView  => {
                let note_count = app.notes.len();
                let notes_str = if note_count == 1 { "  ● 1 note".to_string() }
                    else if note_count > 1 { format!("  ● {} notes", note_count) }
                    else { String::new() };
                let note_actions = if app.current_hunk_has_note() { "  e: edit  d: delete" } else { "  c: comment" };
                let fold_hint = if app.selected_hunk_is_foldable() {
                    if app.expanded_hunks.contains(&app.selected_hunk) { "  Space: fold" } else { "  Space: expand" }
                } else { "" };
                let ws_hint = match app.whitespace_mode {
                    WhitespaceMode::None          => "  w: whitespace".to_string(),
                    WhitespaceMode::IgnoreChanges => "  w: whitespace(-b)".to_string(),
                    WhitespaceMode::IgnoreAll     => "  w: whitespace(-w)".to_string(),
                };
                let split_hint = match app.view_mode {
                    ViewMode::Inline     => "  s: split",
                    ViewMode::SideBySide => "  s: inline",
                };
                format!(" Tab/Shift+Tab: navigate  ↑↓: scroll  []: hunk{}{}{}{}  q: quit{}", note_actions, fold_hint, ws_hint, split_hint, notes_str)
            }
        },
    }
}

pub(super) fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(status_bar_text(app)).style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, FeedbackNote, Mode, Panel};
    use crate::app::test_helpers::{make_rich_hunk, make_rich_line};
    use crate::diff::{ChangedFile, DiffLine, FileStatus, LineKind};
    use crate::git::WhitespaceMode;
    use crate::segment::{RichDiffFile, RichHunk};
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;

    fn make_app_with_hunks(hunk_count: usize) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified, old_path: None }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count).map(|i| make_rich_hunk(
                &format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1)
            )).collect(),
        });
        app
    }

    fn diff_view_rendered(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    fn app_with_single_line(content: &str) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified, old_path: None }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        let dl = DiffLine { old_lineno: None, new_lineno: Some(1), kind: LineKind::Added, content: content.to_string() };
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,1 +1,1 @@".to_string(), old_start: 1, new_start: 1,
                lines: vec![make_rich_line(dl)],
            }],
        });
        app
    }

    // ── Diff view wrap regression guard ──────────────────────────────────────

    #[test]
    fn test_diff_view_wraps_long_lines_full_content_visible() {
        let content = "Q".repeat(60);
        let rendered = diff_view_rendered(&app_with_single_line(&content), 40, 10);
        let q_count = rendered.chars().filter(|&c| c == 'Q').count();
        assert_eq!(q_count, 60,
            "diff view wrap: all 60 Q chars should be visible; got {} \
             (likely caused by Wrap being disabled — only the first ~32 fit per row)", q_count);
    }

    #[test]
    fn test_diff_view_short_line_fits_in_one_row() {
        let content = "Q".repeat(20);
        let rendered = diff_view_rendered(&app_with_single_line(&content), 40, 10);
        let q_count = rendered.chars().filter(|&c| c == 'Q').count();
        assert_eq!(q_count, 20, "short diff line should render all 20 chars");
    }

    fn notes_panel_rendered(app: &App) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_notes_panel(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    fn app_with_note(note_text: &str) -> App {
        let mut app = make_app_with_hunks(1);
        app.focused_panel = Panel::DiffView;
        app.mode = Mode::Comment { hunk_idx: 0, input: note_text.to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        app.focused_panel = Panel::NotesView;
        app
    }

    fn app_diff_view() -> App {
        let mut a = make_app_with_hunks(1);
        a.focused_panel = Panel::DiffView;
        a
    }

    // ── Whitespace mode in diff title ─────────────────────────────────────────

    fn diff_title(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, app, f.area())).unwrap();
        terminal.backend().buffer().content()[..(width as usize)]
            .iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn test_diff_title_no_ws_mode() {
        let app = make_app_with_hunks(1);
        let title = diff_title(&app, 80, 10);
        assert!(!title.contains("(-b)") && !title.contains("(-w)"),
            "no whitespace mode should show no label");
    }

    #[test]
    fn test_diff_title_shows_ws_mode_label() {
        let mut app = make_app_with_hunks(1);
        app.whitespace_mode = WhitespaceMode::IgnoreAll;
        let title = diff_title(&app, 80, 10);
        assert!(title.contains("(-w)"), "IgnoreAll should show (-w) in title; got: {}", title);
    }

    #[test]
    fn test_diff_title_shows_ignore_changes_label() {
        let mut app = make_app_with_hunks(1);
        app.whitespace_mode = WhitespaceMode::IgnoreChanges;
        let title = diff_title(&app, 80, 10);
        assert!(title.contains("(-b)"), "IgnoreChanges should show (-b) in title; got: {}", title);
    }

    // ── Scroll-position indicator ─────────────────────────────────────────────

    #[test]
    fn test_scroll_indicator_visible_when_content_exceeds_viewport() {
        let app = make_app_with_hunks(20);
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        let scrollbar_chars = ["│", "█", "▐", "▌"];
        let found = (1u16..11).any(|row| {
            scrollbar_chars.contains(&buf[(78u16, row)].symbol())
        });
        assert!(found, "scrollbar should appear in the right inner column when content is taller than viewport");
    }

    #[test]
    fn test_scroll_indicator_absent_when_content_fits() {
        let app = make_app_with_hunks(1);
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        let scrollbar_chars = ["│", "█", "▐", "▌"];
        let found = (1u16..19).any(|row| {
            scrollbar_chars.contains(&buf[(78u16, row)].symbol())
        });
        assert!(!found, "scrollbar should not appear when all content fits in the viewport");
    }

    // ── Status bar ────────────────────────────────────────────────────────────

    #[test]
    fn test_status_bar_comment_mode() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None, line_range: None };
        let text = status_bar_text(&app);
        assert!(text.contains("Ctrl+S: submit") && text.contains("Esc: cancel"));
    }

    #[test]
    fn test_status_bar_comment_mentions_clipboard() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None, line_range: None };
        assert!(status_bar_text(&app).contains("Ctrl+C/V/X"));
    }

    #[test]
    fn test_status_bar_diff_no_notes() {
        assert!(status_bar_text(&app_diff_view()).contains("c: comment"));
    }

    #[test]
    fn test_status_bar_diff_one_note() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: "a note".to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        assert!(status_bar_text(&app).contains("● 1 note"));
    }

    #[test]
    fn test_status_bar_diff_multiple_notes() {
        let mut app = make_app_with_hunks(2);
        app.focused_panel = Panel::DiffView;
        for hunk_idx in [0, 1] {
            app.mode = Mode::Comment { hunk_idx, input: "note".to_string(), cursor: 0, original: None, line_range: None };
            app.submit_comment();
            app.selected_hunk = hunk_idx;
        }
        app.selected_hunk = 0;
        assert!(status_bar_text(&app).contains("● 2 notes"));
    }

    #[test]
    fn test_status_bar_diff_shows_whitespace_hint() {
        let app = app_diff_view();
        let text = status_bar_text(&app);
        assert!(text.contains("w: whitespace"), "status bar should mention whitespace key");
    }

    #[test]
    fn test_status_bar_diff_shows_active_ws_mode() {
        let mut app = app_diff_view();
        app.whitespace_mode = WhitespaceMode::IgnoreAll;
        let text = status_bar_text(&app);
        assert!(text.contains("(-w)"), "active whitespace mode should appear in status bar");
    }

    #[test]
    fn test_status_bar_note_shows_edit_delete() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: "existing".to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        let text = status_bar_text(&app);
        assert!(text.contains("e: edit") && text.contains("d: delete") && !text.contains("c: comment"));
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

    #[test]
    fn test_notes_panel_empty() {
        let mut app = make_app_with_hunks(1);
        app.focused_panel = Panel::NotesView;
        assert!(notes_panel_rendered(&app).contains("No notes yet."));
    }

    #[test]
    fn test_notes_panel_short_note() {
        assert!(notes_panel_rendered(&app_with_note("short note")).contains("short note"));
    }

    #[test]
    fn test_notes_panel_long_note_truncated() {
        let long = "a".repeat(80);
        let r = notes_panel_rendered(&app_with_note(&long));
        assert!(r.contains("…") && !r.contains(&"a".repeat(75)));
    }

    #[test]
    fn test_notes_panel_expanded() {
        let long = "a".repeat(80);
        let mut app = app_with_note(&long);
        app.expanded_notes.insert(0);
        let r = notes_panel_rendered(&app);
        assert!(r.contains(&"a".repeat(74)) && !r.contains("…"));
    }

    #[test]
    fn test_notes_panel_selected_marker() {
        assert!(notes_panel_rendered(&app_with_note("my note")).contains("▶"));
    }

    #[test]
    fn test_notes_panel_hunk_header() {
        assert!(notes_panel_rendered(&app_with_note("note")).contains("@@"));
    }

    // ── File list / tree rendering ────────────────────────────────────────────

    fn file_list_rendered(app: &App) -> String {
        let backend = TestBackend::new(32, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    fn app_with_tree_files() -> App {
        let files = vec![
            ChangedFile { path: PathBuf::from("src/app.rs"),  status: FileStatus::Modified, old_path: None },
            ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Added,    old_path: None },
        ];
        App::new(files, "main".to_string(), "HEAD".to_string())
    }

    #[test]
    fn test_file_list_status_indicator_colour_does_not_bleed_into_space() {
        let files = vec![ChangedFile {
            path: PathBuf::from("main.rs"),
            status: FileStatus::Modified,
            old_path: None,
        }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        let backend = TestBackend::new(32, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        let indicator_fg = buf[(1, 1)].fg;
        let space_fg     = buf[(4, 1)].fg;
        assert_eq!(indicator_fg, Color::Yellow, "[M] bracket must be coloured yellow for Modified");
        assert_ne!(space_fg, Color::Yellow, "separator space after [M] must not be coloured");
    }

    #[test]
    fn test_file_list_hscroll_is_full_viewport() {
        let files = vec![ChangedFile {
            path: PathBuf::from("main.rs"),
            status: FileStatus::Modified,
            old_path: None,
        }];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.file_list_h_scroll = 4;
        let backend = TestBackend::new(32, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(1, 1)].symbol(), "m", "after scrolling past '[M] ', first char should be 'm' from 'main.rs'");
    }

    #[test]
    fn test_file_list_renders_dir_arrow() {
        let app = app_with_tree_files();
        let s = file_list_rendered(&app);
        assert!(s.contains("▾"), "expanded dir should show ▾ arrow");
    }

    #[test]
    fn test_file_list_renders_dir_name() {
        let app = app_with_tree_files();
        assert!(file_list_rendered(&app).contains("src/"));
    }

    #[test]
    fn test_file_list_renders_file_with_status() {
        let app = app_with_tree_files();
        let s = file_list_rendered(&app);
        assert!(s.contains("app.rs") && s.contains("main.rs"));
    }

    #[test]
    fn test_file_list_collapsed_dir_hides_files() {
        let mut app = app_with_tree_files();
        app.file_tree_cursor = 0;
        app.toggle_dir_at_cursor();
        let s = file_list_rendered(&app);
        assert!(s.contains("▸"), "collapsed dir should show ▸ arrow");
        assert!(!s.contains("app.rs"), "files should be hidden when dir is collapsed");
    }

    #[test]
    fn test_file_list_note_marker_on_file() {
        let mut app = app_with_tree_files();
        app.notes.push(FeedbackNote {
            file: PathBuf::from("src/app.rs"),
            hunk_header: "@@".to_string(),
            hunk_content: String::new(),
            note: "check this".to_string(),
            line_range: None,
        });
        assert!(file_list_rendered(&app).contains("●"));
    }

    #[test]
    fn test_status_bar_file_list_mentions_toggle() {
        let mut app = app_with_tree_files();
        app.focused_panel = Panel::FileList;
        assert!(status_bar_text(&app).contains("toggle"));
    }

    #[test]
    fn test_status_bar_file_list_mentions_scroll() {
        let mut app = app_with_tree_files();
        app.focused_panel = Panel::FileList;
        assert!(status_bar_text(&app).contains("scroll"));
    }

    #[test]
    fn test_viewport_hscroll_skips_and_caps() {
        let plain = Style::default();
        let result = viewport_hscroll(vec![("hello world".into(), plain)], 6, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "world");

        let yellow = Style::default().fg(Color::Yellow);
        let spans = vec![("[M]".into(), yellow), (" main.rs".into(), plain)];
        let result = viewport_hscroll(spans, 4, 20);
        assert_eq!(result.len(), 1, "first span fully skipped");
        assert_eq!(result[0].content, "main.rs");
        assert_eq!(result[0].style, plain, "remaining content has plain style");

        let result = viewport_hscroll(vec![("hello".into(), plain)], 0, 3);
        assert_eq!(result[0].content, "hel");
    }
}
