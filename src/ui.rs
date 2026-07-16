use crate::app::{App, Mode};
use crate::fmt;
use crate::session::Scope;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top rule
            Constraint::Length(1), // title
            Constraint::Length(1), // spacer
            Constraint::Length(3), // search / rename box
            Constraint::Length(1), // spacer
            Constraint::Min(1),    // session list
            Constraint::Length(1), // spacer
            Constraint::Length(1), // hint / status
        ])
        .split(f.area());

    draw_rule(f, chunks[0]);
    draw_title(f, app, chunks[1]);
    draw_input_box(f, app, chunks[3]);
    draw_list(f, app, chunks[5]);
    draw_bottom(f, app, chunks[7]);
}

fn draw_rule(f: &mut Frame, area: Rect) {
    f.render_widget(Block::default().borders(Borders::BOTTOM), area);
}

fn draw_title(f: &mut Frame, app: &App, area: Rect) {
    let title = format!(" ccmgr — {}", app.scope.label());
    let style = Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(title).style(style), area);
}

fn draw_input_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (prefix, text, is_placeholder) = match &app.mode {
        Mode::Renaming { input } => ("Rename: ", input.as_str(), false),
        _ if app.query.is_empty() => ("⌕ ", "Search...", true),
        _ => ("⌕ ", app.query.as_str(), false),
    };

    let text_style = if is_placeholder {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };

    let line = Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::DarkGray)),
        Span::styled(text, text_style),
    ]);
    f.render_widget(Paragraph::new(line), inner);

    if !matches!(app.mode, Mode::ConfirmingDelete) {
        let typed_len = match &app.mode {
            Mode::Renaming { input } => input.chars().count(),
            _ => app.query.chars().count(),
        };
        let cursor_x = inner.x + prefix.chars().count() as u16 + typed_len as u16;
        f.set_cursor_position((cursor_x, inner.y));
    }
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let s = &app.sessions[i];
            let selected = row == app.selected;

            let title_style = if selected {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
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
                Line::from(Span::styled(s.title.clone(), title_style)),
                Line::from(Span::styled(details, Style::default().fg(Color::DarkGray))),
            ])
        })
        .collect();

    let list = List::new(items).highlight_symbol("❯ ");

    let mut state = ListState::default();
    if !app.filtered.is_empty() {
        state.select(Some(app.selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_bottom(f: &mut Frame, app: &App, area: Rect) {
    let text = match &app.mode {
        Mode::ConfirmingDelete => "Delete this session? (y/n)".to_string(),
        Mode::Renaming { .. } => "Enter to save · Esc to cancel".to_string(),
        Mode::Browsing => app.status.clone().unwrap_or_else(|| {
            "Type to search · ↑/↓ move · Enter resume · Ctrl+R rename · Ctrl+D delete · Tab scope · Ctrl+C quit"
                .to_string()
        }),
    };
    let style = if matches!(app.mode, Mode::ConfirmingDelete) {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Paragraph::new(text).style(style), area);
}
