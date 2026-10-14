use std::net::{IpAddr, Ipv4Addr};

/// Namespace prefix
pub const NAMESPACE_PREFIX: &str = "zombie-";
/// Directory for node configuration
pub const NODE_CONFIG_DIR: &str = "/cfg";
/// Directory for node data dir
pub const NODE_DATA_DIR: &str = "/data";
/// Directory for node relay data dir
pub const NODE_RELAY_DATA_DIR: &str = "/relay-data";
/// Directory for node scripts
pub const NODE_SCRIPTS_DIR: &str = "/scripts";
/// Directory for the helper binaries, on docker
pub const HELPERS_DIR: &str = "/helpers";
/// The directories every node and process gets mounted
pub const NODE_MOUNTED_DIRS: &[&str] = &[
    NODE_CONFIG_DIR,
    NODE_DATA_DIR,
    NODE_RELAY_DATA_DIR,
    NODE_SCRIPTS_DIR,
    HELPERS_DIR,
];
/// Localhost ip
pub const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
/// The port substrate listens for p2p connections on
pub const P2P_PORT: u16 = 30333;
/// The remote port Prometheus can be accessed with
pub const PROMETHEUS_PORT: u16 = 9615;
/// The remote port websocket to access the RPC
pub const RPC_WS_PORT: u16 = 9944;
/// The remote port HTTP to access the RPC
pub const RPC_HTTP_PORT: u16 = 9933;
