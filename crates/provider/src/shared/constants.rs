use std::net::{IpAddr, Ipv4Addr};

/// Default temporary directory
pub const DEFAULT_TMP_DIR: &str = "/tmp";
/// Directory for node configuration
pub const NODE_CONFIG_DIR: &str = "/cfg";
/// Directory for node configuration
pub const NODE_DATA_DIR: &str = "/data";
/// Localhost ip
pub const _LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
/// The port substrate listens for p2p connections on
pub const _P2P_PORT: u16 = 30333;
/// The remote port prometheus can be accessed with
pub const _PROMETHEUS_PORT: u16 = 9615;
/// The remote port websocket to access the RPC
pub const _RPC_WS_PORT: u16 = 9944;
/// The remote port http to access the RPC
pub const _RPC_HTTP_PORT: u16 = 9933;
