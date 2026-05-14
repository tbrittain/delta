use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{App, Mode, selected_range};
use super::ACCENT;
use super::cursor::{compute_visual_lines, visual_row_and_col, line_spans};

pub(super) fn comment_popup_area(total_width: u16, total_height: u16) -> Rect {
    let width  = (total_width  * 70 / 100).max(40).min(total_width.saturating_sub(4));
    let height = (total_height * 40 / 100).max(8).min(total_height.saturating_sub(4));
    Rect {
        x: (total_width.saturating_sub(width)) / 2,
        y: (total_height.saturating_sub(height)) / 2,
        width, height,
    }
}

/// Content width (characters) of the popup's text area for a given terminal size.
pub(super) fn comment_popup_content_width(term_width: u16, term_height: u16) -> usize {
    comment_popup_area(term_width, term_height.saturating_sub(1)).width.saturating_sub(2) as usize
}

pub(super) fn render_comment_popup(frame: &mut Frame, app: &App, area: Rect) {
    let Mode::Comment { ref input, cursor, hunk_idx, .. } = app.mode else { return };

    let rel = comment_popup_area(area.width, area.height);
    let popup = Rect { x: area.x + rel.x, y: area.y + rel.y, width: rel.width, height: rel.height };
    frame.render_widget(Clear, popup);

    let hunk_header = app.current_diff.as_ref()
        .and_then(|d| d.hunks.get(hunk_idx))
        .map(|h| h.header.clone())
        .unwrap_or_default();
    let max_title = popup.width.saturating_sub(4) as usize;
    let title_hunk: String = if hunk_header.chars().count() > max_title {
        format!("{}…", hunk_header.chars().take(max_title.saturating_sub(1)).collect::<String>())
    } else { hunk_header };
    let title = if title_hunk.is_empty() { " Comment ".to_string() } else { format!(" {} ", title_hunk) };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(app.highlighter.panel_bg))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height < 2 { return; }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let content_area = layout[0];
    let help_area    = layout[1];

    let content_width = content_area.width as usize;
    let selection = selected_range(cursor, app.comment_anchor);
    let visual_lines = compute_visual_lines(input, content_width);
    let (cursor_vrow, cursor_vcol) = visual_row_and_col(cursor, &visual_lines);

    let mut rendered: Vec<Line<'static>> = visual_lines.iter()
        .map(|vl| Line::from(line_spans(&vl.text, vl.byte_start, selection)))
        .collect();
    if rendered.is_empty() { rendered.push(Line::raw("")); }

    frame.render_widget(Paragraph::new(Text::from(rendered)).scroll((app.comment_scroll as u16, 0)), content_area);

    if cursor_vrow >= app.comment_scroll {
        let visible_row = (cursor_vrow - app.comment_scroll) as u16;
        if visible_row < content_area.height {
            let x = (content_area.x + cursor_vcol as u16).min(content_area.x + content_area.width.saturating_sub(1));
            frame.set_cursor_position((x, content_area.y + visible_row));
        }
    }

    frame.render_widget(
        Paragraph::new(" Ctrl+S: submit   Esc: cancel")
            .style(Style::default().add_modifier(Modifier::REVERSED)),
        help_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Mode};
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;

    fn make_app(hunk_count: usize) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = crate::app::Panel::DiffView;
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count).map(|i| Hunk {
                header: format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1),
                old_start: (i * 10 + 1) as u32, new_start: (i * 10 + 1) as u32,
                lines: vec![
                    DiffLine { old_lineno: None,    new_lineno: Some(1), kind: LineKind::Added,   content: "new line".to_string() },
                    DiffLine { old_lineno: Some(1), new_lineno: None,    kind: LineKind::Removed, content: "old line".to_string() },
                ],
            }).collect(),
        });
        app
    }

    fn popup_str(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_comment_popup(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn test_popup_renders_text() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "review comment".to_string(), cursor: 0, original: None };
        assert!(popup_str(&app).contains("review comment"));
    }

    #[test]
    fn test_popup_no_block_cursor() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "hello".to_string(), cursor: 3, original: None };
        assert!(!popup_str(&app).contains("█"));
    }

    #[test]
    fn test_popup_help_line() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        let s = popup_str(&app);
        assert!(s.contains("Ctrl+S") && s.contains("Esc"));
    }

    #[test]
    fn test_popup_title_has_hunk_header() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(popup_str(&app).contains("@@"));
    }

    #[test]
    fn test_popup_not_shown_in_normal_mode() {
        assert!(!popup_str(&make_app(1)).contains("Comment"));
    }

    #[test]
    fn test_popup_multiline() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "line one\nline two".to_string(), cursor: 0, original: None };
        let s = popup_str(&app);
        assert!(s.contains("line one") && s.contains("line two"));
    }
}
