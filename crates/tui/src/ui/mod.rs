mod help;
mod layout;
mod widgets;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode, PendingAction, View};

/// Main render function for the TUI.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header.
            Constraint::Min(10),   // Main content.
            Constraint::Length(3), // Status bar.
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_main_content(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);

    // Render overlays.
    match app.input_mode() {
        InputMode::Help => {
            help::render_help_overlay(frame);
        },
        InputMode::Confirm => {
            render_confirm_dialog(frame, app);
        },
        InputMode::Normal => {},
    }
}

/// Render the header bar.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let network_name = app.network_name().unwrap_or("Not connected");
    let base_dir = app.network_base_dir().unwrap_or("-");
    let node_count = app.nodes().len();

    let header_text = format!(
        " Zombienet TUI | Network: {} | Nodes: {} | Dir: {}",
        network_name, node_count, base_dir
    );

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .block(Block::default().borders(Borders::NONE));

    frame.render_widget(header, area);
}

/// Render the main content area with sidebar and panels.
fn render_main_content(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Sidebar.
            Constraint::Percentage(75), // Main panel.
        ])
        .split(area);

    render_sidebar(frame, app, chunks[0]);
    render_main_panel(frame, app, chunks[1]);
}

/// Render the sidebar with node list.
fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.current_view() == View::Nodes;

    let items: Vec<ListItem> = app
        .nodes()
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let prefix = if node.para_id.is_some() {
                "  " // Indent collators.
            } else {
                ""
            };

            let type_indicator = format!("[{}]", node.node_type.icon());

            let para_suffix = node
                .para_id
                .map(|id| format!(" (para:{})", id))
                .unwrap_or_default();

            let content = format!("{}{} {}{}", prefix, type_indicator, node.name, para_suffix);

            let style = if i == app.selected_node_index() {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(content).style(style)
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Nodes [1] ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_widget(list, area);
}

/// Render the main panel (details or logs).
fn render_main_panel(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // Details.
            Constraint::Percentage(60), // Logs.
        ])
        .split(area);

    render_details_panel(frame, app, chunks[0]);
    render_logs_panel(frame, app, chunks[1]);
}

/// Render the node details panel.
fn render_details_panel(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.current_view() == View::Details;

    let content = if let Some(node) = app.selected_node() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&node.name, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
                Span::styled(node.node_type.as_str(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("WS URI: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&node.ws_uri, Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("Multiaddr: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&node.multiaddr, Style::default().fg(Color::Green)),
            ]),
        ];

        if let Some(para_id) = node.para_id {
            lines.insert(
                2,
                Line::from(vec![
                    Span::styled("Para ID: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(para_id.to_string(), Style::default().fg(Color::Magenta)),
                ]),
            );
        }

        lines
    } else {
        vec![Line::from("No node selected")]
    };

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let details = Paragraph::new(content)
        .block(
            Block::default()
                .title(" Details [2] ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(details, area);
}

/// Render the logs panel.
fn render_logs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.current_view() == View::Logs;

    let log_lines = app.log_lines();
    let scroll = app.log_scroll();
    let follow_indicator = if app.log_follow() { " [FOLLOW]" } else { "" };

    let title = format!(
        " Logs [3]{} ({}/{}) ",
        follow_indicator,
        scroll + 1,
        log_lines.len().max(1)
    );

    // Calculate visible lines.
    let visible_height = area.height.saturating_sub(2) as usize;
    let start = scroll;
    let end = (start + visible_height).min(log_lines.len());

    let visible_lines: Vec<Line> = log_lines
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|line| {
            // Color log lines based on content.
            let style = if line.contains("ERROR") || line.contains("error") {
                Style::default().fg(Color::Red)
            } else if line.contains("WARN") || line.contains("warn") {
                Style::default().fg(Color::Yellow)
            } else if line.contains("INFO") || line.contains("info") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            Line::styled(line.clone(), style)
        })
        .collect();

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let logs = Paragraph::new(visible_lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style),
    );

    frame.render_widget(logs, area);
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let status_text = app.status_message().unwrap_or("");

    let keybindings = " q:Quit | Tab:Switch | j/k:Navigate | p:Pause | u:Resume | r:Restart | Q:Shutdown | ?:Help ";

    let status_line = if status_text.is_empty() {
        keybindings.to_string()
    } else {
        format!("{} | {}", status_text, keybindings)
    };

    let status =
        Paragraph::new(status_line).style(Style::default().fg(Color::White).bg(Color::DarkGray));

    frame.render_widget(status, area);
}

/// Render the confirmation dialog overlay.
fn render_confirm_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(50, 30, frame.area());

    // Clear the background.
    frame.render_widget(Clear, area);

    let message = match app.pending_action() {
        Some(PendingAction::RestartNode(name)) => {
            format!("Are you sure you want to restart node '{}'?", name)
        },
        Some(PendingAction::ShutdownNetwork) => {
            "Are you sure you want to shutdown the entire network?".to_string()
        },
        None => "Confirm action?".to_string(),
    };

    let text = vec![
        Line::from(""),
        Line::from(message),
        Line::from(""),
        Line::from(vec![
            Span::styled("[y]", Style::default().fg(Color::Green)),
            Span::raw(" Yes  "),
            Span::styled("[n]", Style::default().fg(Color::Red)),
            Span::raw(" No"),
        ]),
    ];

    let dialog = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().bg(Color::Black))
        .wrap(Wrap { trim: true });

    frame.render_widget(dialog, area);
}

/// Create a centered rectangle within the given area.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
