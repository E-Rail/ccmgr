use crate::app::{App, Mode};
use crate::session::Scope;
use crate::time_fmt;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_top(f, app, chunks[0]);
    draw_list(f, app, chunks[1]);
    draw_bottom(f, app, chunks[2]);
}

fn draw_top(f: &mut Frame, app: &App, area: Rect) {
    let title = format!(" ccmgr — {} ", app.scope.label());
    let text = match &app.mode {
        Mode::Renaming { input } => format!("Rename: {input}_"),
        _ => format!("Search: {}_", app.query),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    f.render_widget(Paragraph::new(text).block(block), area);
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

            let mut details = time_fmt::relative(s.mtime);
            if app.scope == Scope::AllProjects {
                if let Some(cwd) = &s.cwd {
                    details.push_str("  ·  ");
                    details.push_str(cwd);
                }
            }

            ListItem::new(vec![
                Line::from(Span::styled(s.title.clone(), title_style)),
                Line::from(Span::styled(details, Style::default().fg(Color::DarkGray))),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Sessions "))
        .highlight_symbol("❯ ");

    let mut state = ListState::default();
    if !app.filtered.is_empty() {
        state.select(Some(app.selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_bottom(f: &mut Frame, app: &App, area: Rect) {
    let text = match &app.mode {
        Mode::ConfirmingDelete => "Delete this session? (y/n)".to_string(),
        Mode::Renaming { .. } => "Enter: save   Esc: cancel".to_string(),
        Mode::Browsing => app.status.clone().unwrap_or_else(|| {
            "↑/↓ move · type to search · Enter resume · Ctrl+R rename · Ctrl+D delete · Tab scope · Ctrl+C quit"
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
