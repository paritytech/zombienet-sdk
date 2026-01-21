mod help;
mod widgets;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, InputMode, PendingAction, View},
    network::NodeStatus,
};

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
        InputMode::Search => {
            render_search_input(frame, app);
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

            // Status icon with color.
            let status_color = match node.status {
                NodeStatus::Running => Color::Green,
                NodeStatus::Paused => Color::Yellow,
                NodeStatus::Unknown => Color::DarkGray,
            };

            let type_indicator = format!("[{}]", node.node_type.icon());

            let para_suffix = node
                .para_id
                .map(|id| format!(" ({})", id))
                .unwrap_or_default();

            // Storage indicator.
            let (storage_text, storage_color) = if let Some(storage) = &node.storage {
                let level = storage.level();
                let color = match level {
                    crate::network::StorageLevel::Low => Color::Green,
                    crate::network::StorageLevel::Medium => Color::Yellow,
                    crate::network::StorageLevel::High => Color::Red,
                    crate::network::StorageLevel::Critical => Color::LightRed,
                };
                (format!(" {}", storage.total_formatted()), color)
            } else {
                (String::new(), Color::DarkGray)
            };

            // Build line with colored status indicator.
            let is_selected = i == app.selected_node_index();
            let base_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                Span::raw(prefix),
                Span::styled(node.status.icon(), Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(type_indicator, base_style),
                Span::styled(format!(" {}", node.name), base_style),
                Span::styled(para_suffix, Style::default().fg(Color::DarkGray)),
                Span::styled(storage_text, Style::default().fg(storage_color)),
            ]);

            ListItem::new(line)
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
        // Status color based on state.
        let status_color = match node.status {
            NodeStatus::Running => Color::Green,
            NodeStatus::Paused => Color::Yellow,
            NodeStatus::Unknown => Color::DarkGray,
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&node.name, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:?} {:?}", node.status.icon(), node.status.as_str()),
                    Style::default().fg(status_color),
                ),
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
                3,
                Line::from(vec![
                    Span::styled("Para ID: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(para_id.to_string(), Style::default().fg(Color::Magenta)),
                ]),
            );
        }

        if let Some(base_dir) = app.network_base_dir() {
            let log_path = crate::network::derive_log_path(base_dir, &node.name);
            let data_dir = crate::network::derive_data_dir(base_dir, &node.name);
            lines.push(Line::from(vec![
                Span::styled("Log: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    log_path.to_string_lossy().to_string(),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Data: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    data_dir.to_string_lossy().to_string(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        // Storage information.
        if let Some(storage) = &node.storage {
            let level = storage.level();
            let storage_color = match level {
                crate::network::StorageLevel::Low => Color::Green,
                crate::network::StorageLevel::Medium => Color::Yellow,
                crate::network::StorageLevel::High => Color::Red,
                crate::network::StorageLevel::Critical => Color::LightRed,
            };

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Storage: ", Style::default().fg(Color::DarkGray)),
                Span::styled(level.icon(), Style::default().fg(storage_color)),
                Span::styled(
                    format!(" {} total", storage.total_formatted()),
                    Style::default().fg(storage_color),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Data dir: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    crate::network::format_size(storage.data_bytes),
                    Style::default().fg(Color::White),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Storage: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "(press 's' to calculate)",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
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
    let log_viewer = app.log_viewer();

    let follow_indicator = if log_viewer.follow() { " [FOLLOW]" } else { "" };

    let search_info = if log_viewer.search_query().is_some() {
        format!(
            " [{}/{}]",
            log_viewer.current_search_index(),
            log_viewer.search_match_count()
        )
    } else {
        String::new()
    };

    let title = format!(
        " Logs [3]{}{} ({}/{}) ",
        follow_indicator,
        search_info,
        log_viewer.scroll() + 1,
        log_viewer.len().max(1)
    );

    // Calculate visible lines.
    let visible_height = area.height.saturating_sub(2) as usize;

    let visible_lines: Vec<Line> = log_viewer
        .visible_lines(visible_height)
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_index = log_viewer.scroll() + i;
            let is_match = log_viewer.is_search_match(line_index);

            // Color log lines based on content.
            let base_style = if line.contains("ERROR") || line.contains("error") {
                Style::default().fg(Color::Red)
            } else if line.contains("WARN") || line.contains("warn") {
                Style::default().fg(Color::Yellow)
            } else if line.contains("INFO") || line.contains("info") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            let style = if is_match {
                base_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            Line::styled(line.to_string(), style)
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

    let keybindings = " q:Quit | Tab:Switch | j/k:Navigate | /:Search | f:Follow | s:Storage | p:Pause | u:Resume | ?:Help ";

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
        Some(PendingAction::RestartAllNodes) => {
            "Are you sure you want to restart ALL nodes?".to_string()
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

/// Render the search input bar at the bottom of the screen.
fn render_search_input(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let search_area = Rect {
        x: 0,
        y: area.height.saturating_sub(3),
        width: area.width,
        height: 3,
    };

    frame.render_widget(Clear, search_area);

    let log_viewer = app.log_viewer();
    let match_info = if log_viewer.search_match_count() > 0 {
        format!(
            " ({}/{})",
            log_viewer.current_search_index(),
            log_viewer.search_match_count()
        )
    } else if !app.search_input().is_empty() {
        " (no matches)".to_string()
    } else {
        String::new()
    };

    let search_text = format!("/{}{}", app.search_input(), match_info);

    let search_bar = Paragraph::new(Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::Cyan)),
        Span::styled(search_text, Style::default().fg(Color::White)),
        Span::styled(
            "_",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Search [Enter:confirm | Esc:cancel | n:next | N:prev] "),
    )
    .style(Style::default().bg(Color::Black));

    frame.render_widget(search_bar, search_area);
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
