use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode, PendingAction, View};

/// Poll for terminal events with a timeout.
pub fn poll_event(timeout: Duration) -> Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Handle a key event based on the current application state.
pub async fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.input_mode() {
        InputMode::Normal => handle_normal_mode(app, key).await,
        InputMode::Confirm => handle_confirm_mode(app, key).await,
        InputMode::Help => handle_help_mode(app, key),
        InputMode::Search => handle_search_mode(app, key),
    }
}

/// Handle key events in normal navigation mode.
async fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Quit.
        KeyCode::Char('q') => {
            app.quit();
        },
        // Quit with Ctrl+C.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.quit();
        },

        // Navigation - up.
        KeyCode::Up | KeyCode::Char('k') => {
            if app.current_view() == View::Logs {
                app.scroll_logs_up(1);
            } else {
                app.select_previous();
            }
        },
        // Navigation - down.
        KeyCode::Down | KeyCode::Char('j') => {
            if app.current_view() == View::Logs {
                app.scroll_logs_down(1);
            } else {
                app.select_next();
            }
        },

        // Page up in logs.
        KeyCode::PageUp => {
            if app.current_view() == View::Logs {
                app.scroll_logs_up(20);
            }
        },
        // Page down in logs.
        KeyCode::PageDown => {
            if app.current_view() == View::Logs {
                app.scroll_logs_down(20);
            }
        },

        // Home - scroll to top of logs.
        KeyCode::Home => {
            if app.current_view() == View::Logs {
                app.scroll_logs_to_top();
            }
        },

        // End - scroll to bottom of logs.
        KeyCode::End => {
            if app.current_view == View::Logs {
                app.scroll_logs_to_bottom();
            }
        },

        // Switch views with Tab.
        KeyCode::Tab => {
            app.next_view();
        },
        // Switch views with Shift+Tab.
        KeyCode::BackTab => {
            app.previous_view();
        },

        // View-specific shortcuts.
        KeyCode::Char('1') => {
            app.current_view = View::Nodes;
        },
        KeyCode::Char('2') => {
            app.current_view = View::Details;
        },
        KeyCode::Char('3') => {
            app.current_view = View::Logs;
        },

        // Toggle help.
        KeyCode::Char('?') | KeyCode::F(1) => {
            app.toggle_help();
        },

        // Toggle log follow mode.
        KeyCode::Char('f') => {
            if app.current_view() == View::Logs {
                app.toggle_log_follow();
            }
        },

        // Start log search.
        KeyCode::Char('/') => {
            if app.current_view() == View::Logs {
                app.start_search();
            }
        },

        // Next search match.
        KeyCode::Char('n') => {
            if app.current_view() == View::Logs {
                app.next_search_match();
            }
        },

        // Previous search match.
        KeyCode::Char('N') => {
            if app.current_view() == View::Logs {
                app.prev_search_match();
            }
        },

        // Node actions.
        KeyCode::Char('p') => {
            // Pause selected node.
            if let Err(e) = app.pause_selected_node().await {
                app.set_status(format!("Error pausing node: {}", e));
            }
        },
        KeyCode::Char('u') => {
            // Resume (unpause) selected node.
            if let Err(e) = app.resume_selected_node().await {
                app.set_status(format!("Error resuming node: {}", e));
            }
        },
        KeyCode::Char('r') => {
            // Restart selected node (with confirmation).
            if let Some(node) = app.selected_node() {
                let name = node.name.clone();
                app.request_confirmation(PendingAction::RestartNode(name));
            }
        },

        // Network-wide shutdown (with confirmation).
        KeyCode::Char('Q') => {
            app.request_confirmation(PendingAction::ShutdownNetwork);
        },

        // Refresh node list.
        KeyCode::Char('R') => {
            app.refresh_nodes();
            app.set_status("Node list refreshed");
        },

        // Load/refresh logs.
        KeyCode::Enter => {
            if app.current_view() == View::Nodes || app.current_view() == View::Details {
                app.current_view = View::Logs;
            }
            if let Err(e) = app.load_selected_node_logs().await {
                app.set_status(format!("Error loading logs: {}", e));
            }
        },

        _ => {},
    }

    Ok(())
}

/// Handle key events in confirmation mode.
async fn handle_confirm_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Confirm with 'y' or Enter.
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Err(e) = app.confirm_action().await {
                app.set_status(format!("Error executing action: {}", e));
            }
        },
        // Cancel with 'n', Escape, or 'q'.
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => {
            app.cancel_action();
        },
        _ => {},
    }

    Ok(())
}

/// Handle key events in help mode.
fn handle_help_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Close help with any common key.
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::F(1) | KeyCode::Enter => {
            app.toggle_help();
        },
        _ => {},
    }

    Ok(())
}

fn handle_search_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Cancel search with Escape.
        KeyCode::Esc => {
            app.cancel_search();
        },
        // Confirm search with Enter.
        KeyCode::Enter => {
            app.confirm_search();
        },
        // Delete last character with Backspace.
        KeyCode::Backspace => {
            app.remove_search_char();
        },
        // Next match with Ctrl+n or Down.
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.next_search_match();
        },
        KeyCode::Down => {
            app.next_search_match();
        },
        // Previous match with Ctrl+p or Up.
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.prev_search_match();
        },
        KeyCode::Up => {
            app.prev_search_match();
        },
        // Add typed characters to search.
        KeyCode::Char(c) => {
            app.add_search_char(c);
        },
        _ => {},
    }

    Ok(())
}
