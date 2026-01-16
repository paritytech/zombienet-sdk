//! Zombienet TUI - Terminal User Interface for monitoring and managing zombienet networks.
//!
//! This crate provides a TUI for:
//! - Viewing all nodes in a running network
//! - Real-time log tailing
//! - Storage monitoring
//! - Node lifecycle control (pause, resume, restart, destroy)
//! - Network-wide operations

pub mod app;
pub mod event;
pub mod network;
pub mod ui;

pub use app::App;
