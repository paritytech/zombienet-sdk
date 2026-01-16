use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Render the help overlay.
pub fn render_help_overlay(frame: &mut Frame) {
    let area = centered_rect(70, 80, frame.area());

    // Clear the background.
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "Zombienet TUI - Keyboard Shortcuts",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Navigation",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("j / Down", "Move selection down"),
        help_line("k / Up", "Move selection up"),
        help_line("Tab", "Switch to next panel"),
        help_line("Shift+Tab", "Switch to previous panel"),
        help_line("1 / 2 / 3", "Jump to Nodes / Details / Logs panel"),
        help_line("Enter", "View logs for selected node"),
        Line::from(""),
        Line::from(Span::styled(
            "Log Viewer",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("f", "Toggle follow mode (auto-scroll)"),
        help_line("PgUp / PgDn", "Scroll logs by page"),
        help_line("j / k", "Scroll logs line by line (when focused)"),
        Line::from(""),
        Line::from(Span::styled(
            "Node Actions",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("p", "Pause selected node (SIGSTOP)"),
        help_line("u", "Resume selected node (SIGCONT)"),
        help_line("r", "Restart selected node (with confirmation)"),
        Line::from(""),
        Line::from(Span::styled(
            "Network Actions",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("Q", "Shutdown entire network (with confirmation)"),
        help_line("R", "Refresh node list"),
        Line::from(""),
        Line::from(Span::styled(
            "General",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        help_line("?", "Toggle this help"),
        help_line("q", "Quit"),
        help_line("Ctrl+C", "Force quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close this help",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().bg(Color::Black))
        .wrap(Wrap { trim: true });

    frame.render_widget(help, area);
}

/// Create a help line with key and description.
fn help_line<'a>(key: &'a str, description: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:15}", key),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(description, Style::default().fg(Color::White)),
    ])
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
