use crate::app::{App, Mode};
use crate::fmt;
use crate::session::Scope;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph};
use ratatui::Frame;

const BACKGROUND: Color = Color::Rgb(40, 44, 52);
const ACCENT: Color = Color::Rgb(176, 185, 249);
const TEXT: Color = Color::Rgb(153, 153, 153);
const BORDER: Color = Color::Rgb(148, 150, 153);
const TITLE_START: Color = Color::Rgb(145, 185, 255);
const TITLE_END: Color = Color::Rgb(203, 181, 243);
const DANGER: Color = Color::Rgb(224, 108, 117);
const TEXT_STYLE: Style = Style::new().fg(TEXT);
const INDENT: &str = "  ";

pub fn draw(f: &mut Frame, app: &App) {
    let full = f.area();
    if full.is_empty() {
        return;
    }
    f.render_widget(Block::default().style(Style::default().bg(BACKGROUND)), full);

    // A uniform outer margin so nothing is ever drawn on the very top,
    // bottom, left, or rightmost row/column, at any terminal size.
    let area = full.inner(Margin { horizontal: 2, vertical: 1 });
    if area.is_empty() {
        return;
    }

    // ratatui doesn't guarantee *which* constraints get dropped when the
    // total exceeds available space, so on a small terminal that can't fit
    // everything, drop purely decorative rows (rule, spacers, title,
    // project label) explicitly, in that priority order, rather than
    // leaving it to chance. The bottom bar - which can carry the delete
    // confirmation prompt - is present in every tier. Each list item takes
    // 2 rows, so a tier is only worth using once it leaves at least 2 spare
    // rows for the list - otherwise it'd render a session list that's
    // always empty even though sessions exist, so that tier drops the list
    // entirely instead.
    if area.height >= 12 {
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
        draw_rule(f, chunks[0]);
        draw_title(f, chunks[1]);
        draw_input_box(f, app, chunks[3]);
        draw_project_label(f, app, chunks[4]);
        draw_list(f, app, chunks[6]);
        draw_bottom(f, app, chunks[8]);
    } else if area.height >= 8 {
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
        draw_title(f, chunks[0]);
        draw_input_box(f, app, chunks[1]);
        draw_project_label(f, app, chunks[2]);
        draw_list(f, app, chunks[3]);
        draw_bottom(f, app, chunks[4]);
    } else if area.height >= 6 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search / rename box
                Constraint::Min(0),    // session list
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_input_box(f, app, chunks[0]);
        draw_list(f, app, chunks[1]);
        draw_bottom(f, app, chunks[2]);
    } else if area.height >= 4 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search / rename box
                Constraint::Length(1), // hint / status
            ])
            .split(area);
        draw_input_box(f, app, chunks[0]);
        draw_bottom(f, app, chunks[1]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        draw_bottom(f, app, chunks[1]);
    }
}

fn draw_rule(f: &mut Frame, area: Rect) {
    f.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(ACCENT)),
        area,
    );
}

fn draw_title(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(gradient_line("Resume session", Modifier::BOLD)),
        area,
    );
}

fn draw_input_box(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if matches!(app.mode, Mode::Renaming { .. }) {
        ACCENT
    } else {
        BORDER
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
        Style::default().fg(BORDER)
    } else {
        TEXT_STYLE
    };

    let prefix_width = line_width(prefix);
    let input_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(prefix_width), Constraint::Min(0)])
        .split(inner);
    f.render_widget(
        Paragraph::new(Span::styled(prefix, Style::default().fg(BORDER))),
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

fn draw_project_label(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(app.project_label(), TEXT_STYLE),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    if app.filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(INDENT),
                Span::styled("No sessions found", TEXT_STYLE),
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
                    Span::styled("❯ ", Style::default().fg(ACCENT)),
                    Span::styled(
                        s.title.as_str(),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                ]
            } else {
                vec![Span::raw(INDENT), Span::styled(s.title.as_str(), TEXT_STYLE)]
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
                Line::from(vec![Span::raw(INDENT), Span::styled(details, TEXT_STYLE)]),
            ])
        })
        .collect();

    let list = List::new(items);

    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_bottom(f: &mut Frame, app: &App, area: Rect) {
    let (text, color) = match &app.mode {
        Mode::ConfirmingDelete => ("Delete this session? (y/n)".to_string(), DANGER),
        Mode::Renaming { .. } => ("Enter to save  ·  Esc to cancel".to_string(), TEXT),
        Mode::Browsing => match &app.status {
            Some(status) => (status.clone(), TEXT),
            None => (
                "Type to search  ·  ↑/↓ move  ·  Enter resume  ·  Ctrl+R rename  ·  Ctrl+D delete  ·  Tab scope  ·  Ctrl+C quit"
                    .to_string(),
                TEXT,
            ),
        },
    };
    let line = Line::from(vec![
        Span::raw(INDENT),
        Span::styled(text, Style::default().fg(color)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn line_width(s: &str) -> u16 {
    Line::from(s).width().min(u16::MAX as usize) as u16
}

fn gradient_line(text: &str, modifier: Modifier) -> Line<'static> {
    Line::from(gradient_spans(text, modifier))
}

fn gradient_spans(text: &str, modifier: Modifier) -> Vec<Span<'static>> {
    let characters: Vec<char> = text.chars().collect();
    let final_index = characters.len().saturating_sub(1);

    characters
        .into_iter()
        .enumerate()
        .map(|(index, character)| {
            let color = interpolate_color(TITLE_START, TITLE_END, index, final_index);
            Span::styled(
                character.to_string(),
                Style::default().fg(color).add_modifier(modifier),
            )
        })
        .collect()
}

fn interpolate_color(start: Color, end: Color, index: usize, final_index: usize) -> Color {
    let (Color::Rgb(start_r, start_g, start_b), Color::Rgb(end_r, end_g, end_b)) = (start, end)
    else {
        return start;
    };

    if final_index == 0 {
        return start;
    }

    let channel = |from: u8, to: u8| {
        let from = f64::from(from);
        let delta = f64::from(to) - from;
        (from + delta * index as f64 / final_index as f64).round() as u8
    };
    Color::Rgb(
        channel(start_r, end_r),
        channel(start_g, end_g),
        channel(start_b, end_b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn buffer_line(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer.cell((x, y)).expect("cell should exist").symbol());
        }
        line
    }

    fn app_with_session() -> App {
        let mut app = App::new(
            PathBuf::from("/definitely-not-a-real-claude-dir"),
            PathBuf::from("/tmp/ccmgr"),
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

    #[test]
    fn draw_matches_resume_session_layout_and_theme() {
        let app = app_with_session();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("drawing should succeed");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().bg, BACKGROUND);

        // Row 1 (after the 1-row top margin) is the rule.
        assert_eq!(buffer.cell((2, 1)).unwrap().symbol(), "─");
        assert_eq!(buffer.cell((2, 1)).unwrap().fg, ACCENT);

        assert!(buffer_line(buffer, 2).contains("Resume session"));
        assert_eq!(buffer.cell((2, 2)).unwrap().fg, TITLE_START);
        let title_end_x = 2 + line_width("Resume session") - 1;
        assert_eq!(buffer.cell((title_end_x, 2)).unwrap().fg, TITLE_END);
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
        assert_eq!(buffer.cell((2, 9)).unwrap().fg, ACCENT);
        assert_eq!(buffer.cell((4, 9)).unwrap().fg, ACCENT);
        assert_eq!(buffer.cell((4, 10)).unwrap().fg, TEXT);
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
    fn gradient_reaches_both_endpoint_colors() {
        let spans = gradient_spans("title", Modifier::BOLD);
        assert_eq!(spans.first().unwrap().style.fg, Some(TITLE_START));
        assert_eq!(spans.last().unwrap().style.fg, Some(TITLE_END));
    }

    #[test]
    fn gradient_interpolates_symmetrically_for_mixed_sign_deltas() {
        // start -> end has a positive delta on one channel and negative
        // deltas on the other two; truncating division used to round these
        // inconsistently at the same index.
        let start = Color::Rgb(0, 100, 100);
        let end = Color::Rgb(100, 0, 80);
        let Color::Rgb(r, g, b) = interpolate_color(start, end, 1, 2) else {
            panic!("expected Rgb");
        };
        assert_eq!((r, g, b), (50, 50, 90));
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
                .draw(|frame| draw(frame, &app))
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
                .draw(|frame| draw(frame, &app))
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
        let app = app_with_session();

        for (width, height) in [(40, 8), (40, 9), (40, 10), (40, 14)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("draw should succeed");

            let buffer = terminal.backend().buffer();
            assert!(
                (0..height).any(|y| buffer_line(buffer, y).contains("Claude Code")),
                "session title should be visible at {width}x{height}"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_on_the_outermost_edges() {
        let app = app_with_session();

        for (width, height) in [(30, 15), (80, 24), (100, 30)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal should initialize");
            terminal
                .draw(|frame| draw(frame, &app))
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
