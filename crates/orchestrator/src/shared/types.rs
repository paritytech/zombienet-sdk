use std::{
    collections::HashMap,
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use configuration::shared::{
    resources::Resources,
    types::{Arg, AssetLocation, Command, Image, Port},
};

pub type Accounts = HashMap<String, NodeAccount>;

#[derive(Debug, Clone, PartialEq)]
pub struct NodeAccount {
    pub address: String,
    pub public_key: String,
}

impl NodeAccount {
    pub fn new(addr: impl Into<String>, pk: impl Into<String>) -> Self {
        Self {
            address: addr.into(),
            public_key: pk.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeAccounts {
    pub(crate) seed: String,
    pub(crate) accounts: Accounts,
}

#[derive(Clone, Debug)]
pub struct ParkedPort(pub(crate) Port, pub(crate) Arc<RwLock<Option<TcpListener>>>);

impl ParkedPort {
    pub(crate) fn new(port: u16, listener: TcpListener) -> ParkedPort {
        let listener = Arc::new(RwLock::new(Some(listener)));
        ParkedPort(port, listener)
    }

    pub(crate) fn drop_listener(&self) {
        // drop the listener will allow the running node to start listenen connections
        let mut l = self.1.write().unwrap();
        *l = None;
    }
}

#[derive(Debug, Clone)]
pub struct ChainDefaultContext<'a> {
    pub default_command: Option<&'a Command>,
    pub default_image: Option<&'a Image>,
    pub default_resources: Option<&'a Resources>,
    pub default_db_snapshot: Option<&'a AssetLocation>,
    pub default_args: Vec<&'a Arg>,
}

#[derive(Debug, Clone)]
pub struct RegisterParachainOptions {
    pub id: u32,
    pub wasm_path: PathBuf,
    pub state_path: PathBuf,
    pub node_ws_url: String,
    pub onboard_as_para: bool,
    pub seed: Option<[u8; 32]>,
    pub finalization: bool,
}

#[derive(Debug, Clone)]
pub struct ParachainGenesisArgs {
    pub genesis_head: String,
    pub validation_code: String,
    pub parachain: bool,
}
