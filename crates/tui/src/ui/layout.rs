use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct MainLayout {
    pub header: Rect,
    pub sidebar: Rect,
    pub details: Rect,
    pub logs: Rect,
    pub status: Rect,
}

impl MainLayout {
    pub fn from_area(area: Rect) -> Self {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header.
                Constraint::Min(10),   // Main content.
                Constraint::Length(3), // Status bar.
            ])
            .split(area);

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25), // Sidebar.
                Constraint::Percentage(75), // Main panel.
            ])
            .split(vertical[1]);

        let main_panel = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40), // Details.
                Constraint::Percentage(60), // Logs.
            ])
            .split(horizontal[1]);

        Self {
            header: vertical[0],
            sidebar: horizontal[0],
            details: main_panel[0],
            logs: main_panel[1],
            status: vertical[2],
        }
    }
}

/// Calculate a centered popup rectangle.
pub fn centered_popup(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
