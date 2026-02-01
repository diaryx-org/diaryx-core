//! UI layout and widget rendering for the navigation TUI

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_tree_widget::Tree;

use super::state::NavState;
use super::tree::tree_node_to_item;

/// Render the full UI
pub fn render(frame: &mut Frame, state: &mut NavState) {
    // Main layout: content area + help bar at bottom
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    // Content area: tree on left, preview on right
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(main_chunks[0]);

    render_tree(frame, state, content_chunks[0]);
    render_preview(frame, state, content_chunks[1]);
    render_help_bar(frame, main_chunks[1]);
}

/// Render the tree widget
fn render_tree(frame: &mut Frame, state: &mut NavState, area: Rect) {
    let items = vec![tree_node_to_item(&state.tree)];

    let tree_widget = Tree::new(&items)
        .expect("Tree creation should not fail")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Workspace ")
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(tree_widget, area, &mut state.tree_state);
}

/// Render the preview pane with title header and file content
fn render_preview(frame: &mut Frame, state: &NavState, area: Rect) {
    // Split preview into header and content
    let preview_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Header: title and path
    render_preview_header(frame, state, preview_chunks[0]);

    // Content: file body
    render_preview_content(frame, state, preview_chunks[1]);
}

/// Render the preview header with title and path
fn render_preview_header(frame: &mut Frame, state: &NavState, area: Rect) {
    let title = state
        .selected_title
        .clone()
        .unwrap_or_else(|| "(No selection)".to_string());

    let path_display = state
        .selected_path
        .as_ref()
        .and_then(|p| p.strip_prefix(&state.workspace_root).ok())
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let header_lines = vec![
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(title, Style::default().bold()),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(path_display, Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let header = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(header, area);
}

/// Render the preview content (file body without frontmatter)
fn render_preview_content(frame: &mut Frame, state: &NavState, area: Rect) {
    let content = Paragraph::new(state.preview_content.as_str())
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .wrap(Wrap { trim: false })
        .scroll((state.preview_scroll, 0));

    frame.render_widget(content, area);
}

/// Render the help bar at the bottom
fn render_help_bar(frame: &mut Frame, area: Rect) {
    let help_text = vec![
        Span::styled(" j/k", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": navigate  "),
        Span::styled("h/l", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": collapse/expand  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": open  "),
        Span::styled("J/K", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": scroll preview  "),
        Span::styled("q", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": quit"),
    ];

    let help_bar =
        Paragraph::new(Line::from(help_text)).style(Style::default().bg(Color::DarkGray));

    frame.render_widget(help_bar, area);
}
