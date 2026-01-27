//! Zombienet TUI - Terminal User Interface for monitoring zombienet networks.
//!
//! Usage:
//!   zombie-tui --attach <path-to-zombie.json>
//!
//! Example:
//!   zombie-tui --attach /tmp/zombie-abc123/zombie.json

use std::{io, path::PathBuf, time::Duration};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::Event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use zombie_tui::{app::App, event, network::StorageThresholds, ui};

/// Zombienet TUI - Monitor and manage running zombienet networks.
#[derive(Parser, Debug)]
#[command(name = "zombie-tui")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the zombie.json file of a running network.
    #[arg(short, long)]
    attach: PathBuf,

    /// Enable verbose logging to stderr.
    #[arg(short, long)]
    verbose: bool,

    /// Storage threshold for "Medium" level in MB (default: 100).
    #[arg(long, default_value = "100")]
    storage_medium_mb: u64,

    /// Storage threshold for "High" level in MB (default: 1024 = 1GB).
    #[arg(long, default_value = "1024")]
    storage_high_mb: u64,

    /// Storage threshold for "Critical" level in MB (default: 10240 = 10GB).
    #[arg(long, default_value = "10240")]
    storage_critical_mb: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::DEBUG)
            .with_writer(io::stderr)
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    }

    let storage_thresholds = StorageThresholds::from_mb(
        args.storage_medium_mb,
        args.storage_high_mb,
        args.storage_critical_mb,
    );

    let mut app = App::new();
    app.set_storage_thresholds(storage_thresholds);
    app.set_zombie_json_path(args.attach);
    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal, &mut app).await;

    // Restore the terminal.
    restore_terminal(&mut terminal)?;

    result
}

/// Set up the terminal for TUI rendering.
fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its original state.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run the main application loop.
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    // Attempt to attach to the network.
    if let Err(e) = app.attach_to_network().await {
        app.set_status(format!("Failed to attach: {}. Press 'q' to quit.", e));
    }

    loop {
        if !app.is_running() {
            break;
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        if let Some(event) = event::poll_event(Duration::from_millis(100))? {
            match event {
                Event::Key(key) => {
                    event::handle_key_event(app, key).await?;
                },
                Event::Resize(..) => {
                    // Terminal resize is handled automatically by ratatui.
                },
                _ => {},
            }
        }

        app.tick();
    }

    Ok(())
}
