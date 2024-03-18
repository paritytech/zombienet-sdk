//! This crate is used to create type safe configuration for Zombienet SDK using nested builders.
//!
//!
//! The main entry point of this crate is the [`NetworkConfigBuilder`] which is used to build a full network configuration
//! but all inners builders are also exposed to allow more granular control over the configuration.
//!
//! **Note**: Not all options can be checked at compile time and some will be checked at runtime when spawning a
//! network (e.g.: supported args for a specific node version).
//!
//! # Example
//! ```
//! use zombienet_configuration::{NetworkConfigBuilder};
//!
//! let network_config = NetworkConfigBuilder::new()
//!     .with_relaychain(|relaychain| {
//!         relaychain
//!             .with_chain("polkadot")
//!             .with_random_nominators_count(10)
//!             .with_default_resources(|resources| {
//!                 resources
//!                     .with_limit_cpu("1000m")
//!                     .with_request_memory("1Gi")
//!                     .with_request_cpu(100_000)
//!             })
//!             .with_node(|node| {
//!                 node.with_name("node")
//!                     .with_command("command")
//!                     .validator(true)
//!             })
//!     })
//!     .with_parachain(|parachain| {
//!         parachain
//!             .with_id(1000)
//!             .with_chain("myparachain1")
//!             .with_initial_balance(100_000)
//!             .with_default_image("myimage:version")
//!             .with_collator(|collator| {
//!                 collator
//!                     .with_name("collator1")
//!                     .with_command("command1")
//!                     .validator(true)
//!             })
//!     })
//!     .with_parachain(|parachain| {
//!         parachain
//!             .with_id(2000)
//!             .with_chain("myparachain2")
//!             .with_initial_balance(50_0000)
//!             .with_collator(|collator| {
//!                 collator
//!                     .with_name("collator2")
//!                     .with_command("command2")
//!                     .validator(true)
//!             })
//!     })
//!     .with_hrmp_channel(|hrmp_channel1| {
//!         hrmp_channel1
//!             .with_sender(1)
//!             .with_recipient(2)
//!             .with_max_capacity(200)
//!             .with_max_message_size(500)
//!     })
//!     .with_hrmp_channel(|hrmp_channel2| {
//!         hrmp_channel2
//!             .with_sender(2)
//!             .with_recipient(1)
//!             .with_max_capacity(100)
//!             .with_max_message_size(250)
//!     })
//!     .with_global_settings(|global_settings| {
//!         global_settings
//!             .with_network_spawn_timeout(1200)
//!             .with_node_spawn_timeout(240)
//!     })
//!     .build();
//!
//! assert!(network_config.is_ok())
//! ```

#![allow(clippy::expect_fun_call)]
mod global_settings;
mod hrmp_channel;
mod network;
mod parachain;
mod relaychain;
pub mod shared;
mod utils;

pub use global_settings::{GlobalSettings, GlobalSettingsBuilder};
pub use hrmp_channel::{HrmpChannelConfig, HrmpChannelConfigBuilder};
pub use network::{NetworkConfig, NetworkConfigBuilder};
pub use parachain::{
    states as para_states, ParachainConfig, ParachainConfigBuilder, RegistrationStrategy,
};
pub use relaychain::{RelaychainConfig, RelaychainConfigBuilder};
// re-export shared
pub use shared::{node::NodeConfig, types};
