//! UI layout and widget rendering for the navigation TUI

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_tree_widget::Tree;

use super::state::{InputMode, NavState};
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
    render_help_bar(frame, state, main_chunks[1]);
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

/// Render the help bar at the bottom, adapting to current input mode
fn render_help_bar(frame: &mut Frame, state: &NavState, area: Rect) {
    let line = match &state.mode {
        InputMode::Normal => render_normal_help(state),
        InputMode::TextInput {
            prompt,
            buffer,
            cursor,
            ..
        } => render_text_input_help(prompt, buffer, *cursor),
        InputMode::Confirm { message, .. } => render_confirm_help(message),
        InputMode::NodePick { prompt, .. } => render_node_pick_help(prompt),
    };

    let help_bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(help_bar, area);
}

/// Normal mode help bar — shows status message if active, otherwise key hints
fn render_normal_help(state: &NavState) -> Line<'static> {
    // Show status message if active
    if let Some((msg, _, is_error)) = &state.status_message {
        let color = if *is_error { Color::Red } else { Color::Green };
        return Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(msg.clone(), Style::default().fg(color).bold()),
        ]);
    }

    Line::from(vec![
        Span::styled(" j/k", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": nav  "),
        Span::styled("h/l", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": tree  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": edit  "),
        Span::styled("a", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": add  "),
        Span::styled("x", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": del  "),
        Span::styled("r", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": rename  "),
        Span::styled("p", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": dup  "),
        Span::styled("m", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": move  "),
        Span::styled("M", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": merge  "),
        Span::styled("q", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": quit"),
    ])
}

/// Text input help bar — shows prompt and editable text buffer
fn render_text_input_help(prompt: &str, buffer: &str, cursor: usize) -> Line<'static> {
    let before = &buffer[..cursor];
    let cursor_char = buffer.get(cursor..cursor + 1).unwrap_or(" ");
    let after = if cursor < buffer.len() {
        &buffer[cursor + 1..]
    } else {
        ""
    };

    Line::from(vec![
        Span::styled(
            format!(" {}: ", prompt),
            Style::default().fg(Color::Yellow).bold(),
        ),
        Span::raw(before.to_string()),
        Span::styled(
            cursor_char.to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(after.to_string()),
        Span::styled("  Esc", Style::default().fg(Color::DarkGray)),
        Span::styled(": cancel", Style::default().fg(Color::DarkGray)),
    ])
}

/// Confirmation help bar — shows yes/no prompt
fn render_confirm_help(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {} ", message),
            Style::default().fg(Color::Yellow).bold(),
        ),
        Span::styled("y", Style::default().fg(Color::Green).bold()),
        Span::raw("/"),
        Span::styled("n", Style::default().fg(Color::Red).bold()),
    ])
}

/// Node-pick help bar — shows instructions for picking a target
fn render_node_pick_help(prompt: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {} ", prompt),
            Style::default().fg(Color::Yellow).bold(),
        ),
        Span::styled("Enter", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": confirm  "),
        Span::styled("Esc", Style::default().fg(Color::DarkGray)),
        Span::raw(": cancel"),
    ])
}
