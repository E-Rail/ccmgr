use crate::app::{App, Mode};
use crate::config::Theme;
use crate::fmt;
use crate::preview::{PreviewCache, PreviewDisplayLine, PreviewRole, SessionPreview};
use crate::session::Scope;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph};
use ratatui::Frame;
use std::collections::VecDeque;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const TITLE_TEXT: &str = "ccmgr - Claude Code Session Manager";
const LIST_ITEM_HEIGHT: u16 = 3; // title + details + blank separator
const MAX_PREVIEW_LINES: usize = 20_000;
const INDENT: &str = "  ";

pub fn draw(f: &mut Frame, app: &mut App) {
    let theme = app.config.theme.clone();
    let full = f.area();
    if full.is_empty() {
        return;
    }
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        full,
    );

    // A uniform outer margin so nothing is ever drawn on the very top,
    // bottom, left, or rightmost row/column, at any terminal size.
    let area = full.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    if area.is_empty() {
        return;
    }
    let preview_status = app.status.clone();
    if let Mode::Previewing { preview } = &mut app.mode {
        draw_preview(f, preview, preview_status.as_deref(), area, &theme);
        return;
    }

    // ratatui doesn't guarantee *which* constraints get dropped when the
    // total exceeds available space, so on a small terminal that can't fit
    // everything, drop purely decorative rows (rule, spacers, title,
    // project label) explicitly, in that priority order, rather than
    // leaving it to chance. The bottom bar - which can carry the delete
    // confirmation prompt - is present in every tier. Each list item takes
    // LIST_ITEM_HEIGHT rows, so a tier is only worth using once it leaves
    // at least that many spare rows for the list - otherwise it'd render a
    // session list that's always empty even though sessions exist, so that
    // tier drops the list entirely instead.
    if area.height >= 10 + LIST_ITEM_HEIGHT {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // rule
                Constraint::Length(1), // title
                Constraint::Length(1), // spacer
                Constraint::Length(3), // search / rename box
                Constraint::Length(1), // project label
                Constraint::Length(1), // spacer
                Constraint::Min(0),    // session list
                Constraint::Length(1), // spacer
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_rule(f, chunks[0], &theme);
        draw_title(f, chunks[1], &theme);
        draw_input_box(f, app, chunks[3], &theme);
        draw_project_label(f, app, chunks[4], &theme);
        draw_list(f, app, chunks[6], &theme);
        draw_bottom(f, app, chunks[8], &theme);
    } else if area.height >= 6 + LIST_ITEM_HEIGHT {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(3), // search / rename box
                Constraint::Length(1), // project label
                Constraint::Min(0),    // session list
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_title(f, chunks[0], &theme);
        draw_input_box(f, app, chunks[1], &theme);
        draw_project_label(f, app, chunks[2], &theme);
        draw_list(f, app, chunks[3], &theme);
        draw_bottom(f, app, chunks[4], &theme);
    } else if area.height >= 4 + LIST_ITEM_HEIGHT {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search / rename box
                Constraint::Min(0),    // session list
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_input_box(f, app, chunks[0], &theme);
        draw_list(f, app, chunks[1], &theme);
        draw_bottom(f, app, chunks[2], &theme);
    } else if area.height >= 4 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search / rename box
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_input_box(f, app, chunks[0], &theme);
        draw_bottom(f, app, chunks[1], &theme);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        draw_bottom(f, app, chunks[1], &theme);
    }
}

fn draw_rule(f: &mut Frame, area: Rect, theme: &Theme) {
    f.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.accent)),
        area,
    );
}

fn draw_title(f: &mut Frame, area: Rect, theme: &Theme) {
    let style = Style::default().fg(theme.title).add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(Span::styled(TITLE_TEXT, style)), area);
}

fn draw_input_box(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let border_color = if matches!(app.mode, Mode::Renaming { .. }) {
        theme.accent
    } else {
        theme.border
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.is_empty() {
        return;
    }

    let (prefix, text, is_placeholder) = match &app.mode {
        Mode::Renaming { input } => ("Rename: ", input.as_str(), false),
        _ if app.query.is_empty() => ("⌕ ", "Search...", true),
        _ => ("⌕ ", app.query.as_str(), false),
    };

    let text_style = if is_placeholder {
        Style::default().fg(theme.border)
    } else {
        Style::default().fg(theme.text)
    };

    let prefix_width = line_width(prefix);
    let input_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(prefix_width), Constraint::Min(0)])
        .split(inner);
    f.render_widget(
        Paragraph::new(Span::styled(prefix, Style::default().fg(theme.border))),
        input_chunks[0],
    );

    let typed_width = if is_placeholder { 0 } else { line_width(text) };
    let max_cursor_offset = input_chunks[1].width.saturating_sub(1);
    let horizontal_scroll = typed_width.saturating_sub(max_cursor_offset);
    f.render_widget(
        Paragraph::new(Span::styled(text, text_style)).scroll((0, horizontal_scroll)),
        input_chunks[1],
    );

    if !matches!(app.mode, Mode::ConfirmingDelete) && !input_chunks[1].is_empty() {
        let cursor_offset = typed_width
            .saturating_sub(horizontal_scroll)
            .min(max_cursor_offset);
        f.set_cursor_position((input_chunks[1].x + cursor_offset, input_chunks[1].y));
    }
}

fn draw_project_label(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(app.project_label(), Style::default().fg(theme.text)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_list(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    if app.filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(INDENT),
                Span::styled("No sessions found", Style::default().fg(theme.text)),
            ])),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let s = &app.sessions[i];
            let selected = row == app.selected;

            let title = if selected {
                vec![
                    Span::styled("❯ ", Style::default().fg(theme.accent)),
                    Span::styled(
                        s.title.as_str(),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]
            } else {
                vec![
                    Span::raw(INDENT),
                    Span::styled(s.title.as_str(), Style::default().fg(theme.text)),
                ]
            };

            let mut parts = vec![fmt::relative(s.mtime)];
            if let Some(branch) = &s.git_branch {
                parts.push(branch.clone());
            }
            parts.push(fmt::human_size(s.size_bytes));
            if app.scope == Scope::AllProjects {
                if let Some(cwd) = &s.cwd {
                    parts.push(cwd.clone());
                }
            }
            let details = parts.join("  ·  ");

            ListItem::new(vec![
                Line::from(title),
                Line::from(vec![
                    Span::raw(INDENT),
                    Span::styled(details, Style::default().fg(theme.text)),
                ]),
                Line::from(""),
            ])
        })
        .collect();

    let list = List::new(items);

    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_bottom(f: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    let (text, color) = match &app.mode {
        Mode::ConfirmingDelete => (
            "Delete this session? (y/n)".to_string(),
            theme.danger,
        ),
        Mode::Renaming { .. } => (
            "Enter to save  ·  Esc to cancel".to_string(),
            theme.text,
        ),
        Mode::Browsing => match &app.status {
            Some(status) => (status.clone(), theme.text),
            None => (
                "Type to search  ·  ↑/↓ move  ·  Space preview  ·  Enter resume  ·  Ctrl+R rename  ·  Ctrl+D delete  ·  Tab scope  ·  Ctrl+C quit"
                    .to_string(),
                theme.text,
            ),
        },
        Mode::Previewing { .. } => return,
    };
    let line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(text, Style::default().fg(color)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_preview(
    f: &mut Frame,
    preview: &mut SessionPreview,
    status: Option<&str>,
    area: Rect,
    theme: &Theme,
) {
    if area.height >= 8 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // rule
                Constraint::Length(1), // title
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // preview body
                Constraint::Length(1), // spacer
                Constraint::Length(1), // controls
            ])
            .split(area);
        draw_rule(f, chunks[0], theme);
        let title_style = Style::default().fg(theme.title).add_modifier(Modifier::BOLD);
        f.render_widget(
            Paragraph::new(Span::styled("Preview session", title_style)),
            chunks[1],
        );
        draw_preview_body(f, preview, chunks[3], theme);
        draw_preview_bottom(f, status, chunks[5], theme);
    } else if area.height >= 2 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        draw_preview_body(f, preview, chunks[0], theme);
        draw_preview_bottom(f, status, chunks[1], theme);
    } else {
        draw_preview_bottom(f, status, area, theme);
    }
}

fn draw_preview_body(f: &mut Frame, preview: &mut SessionPreview, area: Rect, theme: &Theme) {
    let block = Block::default()
        .title(format!(" {} ", preview.title))
        .title_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    ensure_preview_cache(preview, inner.width.saturating_sub(2));
    let cache = preview
        .cache
        .as_ref()
        .expect("preview cache should be initialized");
    let max_scroll = cache.lines.len().saturating_sub(inner.height as usize);
    preview.scroll_from_bottom = preview.scroll_from_bottom.min(max_scroll);
    let top = max_scroll.saturating_sub(preview.scroll_from_bottom);
    let bottom = top
        .saturating_add(inner.height as usize)
        .min(cache.lines.len());
    let visible = cache.lines[top..bottom]
        .iter()
        .map(|line| display_line(line, theme))
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(visible), inner);
}

fn ensure_preview_cache(preview: &mut SessionPreview, content_width: u16) {
    let width = content_width.max(1);
    if preview
        .cache
        .as_ref()
        .is_some_and(|cache| cache.width == width)
    {
        return;
    }

    let mut lines = VecDeque::with_capacity(MAX_PREVIEW_LINES.min(1024));
    let mut wrapped_content_omitted = false;
    if preview.messages.is_empty() {
        push_preview_line(
            &mut lines,
            &mut wrapped_content_omitted,
            PreviewDisplayLine::Text("No readable messages in this session".to_string()),
        );
    } else {
        for message in &preview.messages {
            push_preview_line(
                &mut lines,
                &mut wrapped_content_omitted,
                PreviewDisplayLine::Role(message.role),
            );
            for wrapped in wrap_text(&message.text, width) {
                push_preview_line(
                    &mut lines,
                    &mut wrapped_content_omitted,
                    PreviewDisplayLine::Text(wrapped),
                );
            }
            push_preview_line(
                &mut lines,
                &mut wrapped_content_omitted,
                PreviewDisplayLine::Blank,
            );
        }
    }

    if preview.total_messages > preview.messages.len() {
        push_preview_line(
            &mut lines,
            &mut wrapped_content_omitted,
            PreviewDisplayLine::Muted(format!(
                "Showing latest {} of {} messages",
                preview.messages.len(),
                preview.total_messages
            )),
        );
    }
    if preview.history_incomplete {
        push_preview_line(
            &mut lines,
            &mut wrapped_content_omitted,
            PreviewDisplayLine::Muted("Earlier history is unavailable".to_string()),
        );
    }
    if wrapped_content_omitted {
        push_preview_line(
            &mut lines,
            &mut wrapped_content_omitted,
            PreviewDisplayLine::Muted(
                "Earlier wrapped content was omitted from this preview".to_string(),
            ),
        );
    }

    preview.cache = Some(PreviewCache {
        width,
        lines: lines.into(),
    });
}

fn push_preview_line(
    lines: &mut VecDeque<PreviewDisplayLine>,
    content_omitted: &mut bool,
    line: PreviewDisplayLine,
) {
    if lines.len() == MAX_PREVIEW_LINES {
        lines.pop_front();
        *content_omitted = true;
    }
    lines.push_back(line);
}

fn display_line<'a>(line: &'a PreviewDisplayLine, theme: &Theme) -> Line<'a> {
    match line {
        PreviewDisplayLine::Role(role) => {
            let (marker, label, color) = match role {
                PreviewRole::User => ("❯ ", "You", theme.accent),
                PreviewRole::Assistant => (INDENT, "Claude", theme.title),
            };
            Line::from(vec![
                Span::styled(marker, Style::default().fg(color)),
                Span::styled(
                    label,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ])
        }
        PreviewDisplayLine::Text(text) => Line::from(vec![
            Span::raw(INDENT),
            Span::styled(text.as_str(), Style::default().fg(theme.text)),
        ]),
        PreviewDisplayLine::Muted(text) => Line::from(Span::styled(
            text.as_str(),
            Style::default().fg(theme.border),
        )),
        PreviewDisplayLine::Blank => Line::default(),
    }
}

fn draw_preview_bottom(f: &mut Frame, status: Option<&str>, area: Rect, theme: &Theme) {
    let (text, color) = match status {
        Some(status) => (status, theme.danger),
        None => (
            "Esc close  ·  Enter resume  ·  ↑/↓ scroll  ·  PgUp/PgDn page",
            theme.text,
        ),
    };
    let line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(text, Style::default().fg(color)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn wrap_text(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();

    for logical_line in text.split('\n') {
        if logical_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width: u16 = 0;
        for grapheme in logical_line.graphemes(true) {
            let grapheme_width = grapheme.width().min(u16::MAX as usize) as u16;
            if !current.is_empty() && current_width.saturating_add(grapheme_width) > width {
                wrapped.push(std::mem::take(&mut current));
                current_width = 0;
            }
            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
        }
        if !current.is_empty() {
            wrapped.push(current);
        }
    }
    wrapped
}

fn line_width(s: &str) -> u16 {
    Line::from(s).width().min(u16::MAX as usize) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::session::Session;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use ratatui::Terminal;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::SystemTime;

    fn buffer_line(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer.cell((x, y)).expect("cell should exist").symbol());
        }
        line
    }

    fn text_position(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
        for y in 0..buffer.area.height {
            let line = buffer_line(buffer, y);
            if let Some(byte_index) = line.find(text) {
                let x = line[..byte_index].chars().count() as u16;
                return Some((x, y));
            }
        }
        None
    }

    fn app_with_config(config: Config) -> App {
        let mut app = App::new(
            PathBuf::from("/definitely-not-a-real-claude-dir"),
            PathBuf::from("/tmp/ccmgr"),
            Arc::new(config),
        );
        app.sessions = vec![Session {
            id: "session-id".to_string(),
            title: "Claude Code session manager".to_string(),
            cwd: Some("/tmp/ccmgr".to_string()),
            git_branch: Some("main".to_string()),
            size_bytes: 2_831_155,
            mtime: SystemTime::now(),
            path: PathBuf::from("/tmp/session-id.jsonl"),
        }];
        app.filtered = vec![0];
        app
    }

    fn app_with_session() -> App {
        app_with_config(Config::default())
    }

    #[test]
    fn draw_matches_resume_session_layout_and_theme() {
        let mut app = app_with_session();
        let theme = app.config.theme.clone();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("drawing should succeed");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().bg, theme.background);

        // Row 1 (after the 1-row top margin) is the rule.
        assert_eq!(buffer.cell((2, 1)).unwrap().symbol(), "─");
        assert_eq!(buffer.cell((2, 1)).unwrap().fg, theme.accent);

        assert!(buffer_line(buffer, 2).contains(TITLE_TEXT));
        assert_eq!(buffer.cell((2, 2)).unwrap().fg, theme.title);
        let title_last_x = 2 + line_width(TITLE_TEXT) - 1;
        assert_eq!(buffer.cell((title_last_x, 2)).unwrap().fg, theme.title);
        assert!(buffer
            .cell((2, 2))
            .unwrap()
            .modifier
            .contains(Modifier::BOLD));

        assert_eq!(buffer.cell((2, 4)).unwrap().symbol(), "╭");
        assert_eq!(buffer.cell((77, 4)).unwrap().symbol(), "╮");
        assert_eq!(buffer.cell((2, 6)).unwrap().symbol(), "╰");
        assert_eq!(buffer.cell((77, 6)).unwrap().symbol(), "╯");
        assert_eq!(buffer.cell((4, 5)).unwrap().symbol(), "⌕");
        assert!(buffer_line(buffer, 7).contains("ccmgr"));

        assert_eq!(buffer.cell((2, 9)).unwrap().symbol(), "❯");
        assert_eq!(buffer.cell((2, 9)).unwrap().fg, theme.accent);
        assert_eq!(buffer.cell((4, 9)).unwrap().fg, theme.accent);
        assert_eq!(buffer.cell((4, 10)).unwrap().fg, theme.text);
        assert!((0..24).any(|y| buffer_line(buffer, y).contains("Type to search")));
    }

    #[test]
    fn project_label_changes_with_scope() {
        let mut app = app_with_session();
        assert_eq!(app.project_label(), "ccmgr");

        app.scope = Scope::AllProjects;
        assert_eq!(app.project_label(), "All projects");
    }

    #[test]
    fn draw_handles_tiny_and_zero_size_terminals_without_panicking() {
        let mut app = app_with_session();
        app.query = "a very long query ".repeat(20);
        app.filtered.clear();

        for (width, height) in [(0, 0), (1, 1), (4, 4), (12, 8), (20, 10)] {
            let backend = TestBackend::new(width.max(1), height.max(1));
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw should never panic regardless of terminal size");
        }
    }

    #[test]
    fn delete_confirmation_prompt_stays_visible_on_small_terminals() {
        let mut app = app_with_session();
        app.mode = Mode::ConfirmingDelete;

        for (width, height) in [(40, 5), (40, 8), (80, 24)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw should succeed");

            let buffer = terminal.backend().buffer();
            assert!(
                (0..height).any(|y| buffer_line(buffer, y).contains("Delete this session?")),
                "delete-confirmation prompt should stay visible at {width}x{height}"
            );
        }
    }

    #[test]
    fn session_list_stays_visible_once_a_terminal_can_fit_one_item() {
        let mut app = app_with_session();

        for (width, height) in [(40, 9), (40, 11), (40, 15), (40, 20)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw should succeed");

            let buffer = terminal.backend().buffer();
            assert!(
                (0..height).any(|y| buffer_line(buffer, y).contains("Claude Code")),
                "session title should be visible at {width}x{height}"
            );
        }
    }

    #[test]
    fn sessions_are_separated_by_a_blank_line() {
        let mut app = app_with_session();
        app.sessions.push(Session {
            id: "session-id-2".to_string(),
            title: "A second session".to_string(),
            cwd: Some("/tmp/ccmgr".to_string()),
            git_branch: None,
            size_bytes: 512,
            mtime: SystemTime::now(),
            path: PathBuf::from("/tmp/session-id-2.jsonl"),
        });
        app.filtered = vec![0, 1];

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer();
        // First item occupies rows 9 (title) and 10 (details); row 11 must
        // be a blank separator before the second item's title on row 12.
        assert!(buffer_line(buffer, 9).contains("Claude Code"));
        assert!(buffer_line(buffer, 11).trim().is_empty());
        assert!(buffer_line(buffer, 12).contains("A second session"));
    }

    #[test]
    fn every_theme_color_is_used_by_the_renderer() {
        let mut config = Config::default();
        config.theme.background = Color::Rgb(1, 2, 3);
        config.theme.accent = Color::Rgb(4, 5, 6);
        config.theme.text = Color::Rgb(7, 8, 9);
        config.theme.border = Color::Rgb(10, 11, 12);
        config.theme.title = Color::Rgb(13, 14, 15);
        config.theme.danger = Color::Rgb(19, 20, 21);
        let mut app = app_with_config(config);
        let theme = app.config.theme.clone();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().bg, theme.background);
        assert_eq!(buffer.cell((2, 1)).unwrap().fg, theme.accent);
        assert_eq!(buffer.cell((2, 2)).unwrap().fg, theme.title);
        let title_last_x = 2 + line_width(TITLE_TEXT) - 1;
        assert_eq!(buffer.cell((title_last_x, 2)).unwrap().fg, theme.title);
        assert_eq!(buffer.cell((2, 4)).unwrap().fg, theme.border);
        assert_eq!(buffer.cell((4, 10)).unwrap().fg, theme.text);

        app.mode = Mode::ConfirmingDelete;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let prompt = text_position(buffer, "Delete this session?").expect("prompt should render");
        assert_eq!(buffer.cell(prompt).unwrap().fg, theme.danger);
    }

    #[test]
    fn preview_renders_roles_content_and_controls_without_a_search_cursor() {
        let mut app = app_with_session();
        let theme = app.config.theme.clone();
        app.mode = Mode::Previewing {
            preview: SessionPreview {
                title: "Preview me".to_string(),
                messages: vec![
                    crate::preview::PreviewMessage {
                        role: PreviewRole::User,
                        text: "Please inspect this session".to_string(),
                    },
                    crate::preview::PreviewMessage {
                        role: PreviewRole::Assistant,
                        text: "The preview is ready".to_string(),
                    },
                ],
                total_messages: 2,
                history_incomplete: false,
                scroll_from_bottom: 0,
                cache: None,
            },
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        assert!(text_position(buffer, "Preview session").is_some());
        assert!(text_position(buffer, "Preview me").is_some());
        assert!(text_position(buffer, "Please inspect this session").is_some());
        assert!(text_position(buffer, "The preview is ready").is_some());
        assert!(text_position(buffer, "Enter resume").is_some());
        assert!(text_position(buffer, "Search...").is_none());
        let user = text_position(buffer, "You").unwrap();
        let assistant = text_position(buffer, "Claude").unwrap();
        assert_eq!(buffer.cell(user).unwrap().fg, theme.accent);
        assert_eq!(buffer.cell(assistant).unwrap().fg, theme.title);
    }

    #[test]
    fn preview_scroll_is_anchored_to_the_newest_content() {
        let mut preview = SessionPreview {
            title: "Long preview".to_string(),
            messages: (0..20)
                .map(|index| crate::preview::PreviewMessage {
                    role: PreviewRole::User,
                    text: format!("message {index}"),
                })
                .collect(),
            total_messages: 20,
            history_incomplete: false,
            scroll_from_bottom: 0,
            cache: None,
        };
        ensure_preview_cache(&mut preview, 60);
        let max_scroll = preview
            .cache
            .as_ref()
            .unwrap()
            .lines
            .len()
            .saturating_sub(8);
        assert!(max_scroll > 0);
        assert_eq!(
            max_scroll.saturating_sub(preview.scroll_from_bottom.min(max_scroll)),
            max_scroll
        );

        let mut older = preview;
        older.scroll_from_bottom = 5;
        assert_eq!(
            max_scroll.saturating_sub(older.scroll_from_bottom.min(max_scroll)),
            max_scroll - 5
        );
    }

    #[test]
    fn preview_content_remains_visible_at_the_compact_layout_breakpoint() {
        let mut app = app_with_session();
        app.mode = Mode::Previewing {
            preview: SessionPreview {
                title: "Compact preview".to_string(),
                messages: vec![crate::preview::PreviewMessage {
                    role: PreviewRole::User,
                    text: "Visible at nine rows".to_string(),
                }],
                total_messages: 1,
                history_incomplete: false,
                scroll_from_bottom: 0,
                cache: None,
            },
        };
        let backend = TestBackend::new(60, 9);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        assert!(text_position(terminal.backend().buffer(), "Visible at nine rows").is_some());
    }

    #[test]
    fn preview_surfaces_status_and_omission_notices() {
        let mut app = app_with_session();
        app.status = Some("Can't resume: project directory no longer exists".to_string());
        app.mode = Mode::Previewing {
            preview: SessionPreview {
                title: "Incomplete preview".to_string(),
                messages: vec![crate::preview::PreviewMessage {
                    role: PreviewRole::Assistant,
                    text: "Newest answer".to_string(),
                }],
                total_messages: 12,
                history_incomplete: true,
                scroll_from_bottom: 0,
                cache: None,
            },
        };
        let theme = app.config.theme.clone();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        assert!(text_position(buffer, "Showing latest 1 of 12 messages").is_some());
        assert!(text_position(buffer, "Earlier history is unavailable").is_some());
        let status = text_position(buffer, "Can't resume").expect("status should be visible");
        assert_eq!(buffer.cell(status).unwrap().fg, theme.danger);
    }

    #[test]
    fn preview_cache_is_bounded_and_keeps_the_newest_rows() {
        let mut preview = SessionPreview {
            title: "Large preview".to_string(),
            messages: vec![crate::preview::PreviewMessage {
                role: PreviewRole::User,
                text: (0..MAX_PREVIEW_LINES + 10)
                    .map(|index| format!("line {index}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }],
            total_messages: 1,
            history_incomplete: false,
            scroll_from_bottom: 0,
            cache: None,
        };

        ensure_preview_cache(&mut preview, 80);

        let cache = preview.cache.as_ref().unwrap();
        assert_eq!(cache.lines.len(), MAX_PREVIEW_LINES);
        assert!(matches!(
            cache.lines.last(),
            Some(PreviewDisplayLine::Muted(message))
                if message.contains("wrapped content was omitted")
        ));
        assert!(cache
            .lines
            .iter()
            .any(|line| matches!(line, PreviewDisplayLine::Text(text) if text == "line 20009")));
    }

    #[test]
    fn wrapping_keeps_unicode_graphemes_intact() {
        assert_eq!(
            wrap_text("a👩‍💻b", 2),
            vec!["a".to_string(), "👩‍💻".to_string(), "b".to_string()]
        );
        assert_eq!(
            wrap_text("e\u{301}x", 1),
            vec!["e\u{301}".to_string(), "x".to_string()]
        );
    }

    #[test]
    fn nothing_is_drawn_on_the_outermost_edges() {
        let mut app = app_with_session();

        for (width, height) in [(30, 15), (80, 24), (100, 30)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw should succeed");

            let buffer = terminal.backend().buffer();
            let last_x = width - 1;
            let last_y = height - 1;
            for x in 0..width {
                assert_eq!(
                    buffer.cell((x, 0)).unwrap().symbol(),
                    " ",
                    "top row should be blank at x={x}"
                );
                assert_eq!(
                    buffer.cell((x, last_y)).unwrap().symbol(),
                    " ",
                    "bottom row should be blank at x={x}"
                );
            }
            for y in 0..height {
                assert_eq!(
                    buffer.cell((0, y)).unwrap().symbol(),
                    " ",
                    "left column should be blank at y={y}"
                );
                assert_eq!(
                    buffer.cell((last_x, y)).unwrap().symbol(),
                    " ",
                    "right column should be blank at y={y}"
                );
            }
        }
    }
}
