use std::net::IpAddr;

// CONSTANTS
pub const DEFAULT_REMOTE_DIR: &str = "/cfg";
pub const DEFAULT_DATA_DIR: &str = "/data";
pub const LOCALHOST: IpAddr = "127.0.0.1".parse().unwrap();
/// The port substrate listens for p2p connections on
pub const P2P_PORT: u16 = 30333;
