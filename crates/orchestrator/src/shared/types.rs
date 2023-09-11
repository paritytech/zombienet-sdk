use std::{collections::HashMap, net::TcpListener, sync::Arc};

pub type Accounts = HashMap<String, NodeAccount>;
use configuration::shared::{
    resources::Resources,
    types::{Arg, AssetLocation, Command, Image, Port},
};

#[derive(Debug, Clone, PartialEq)]
pub struct NodeAccount {
    address: String,
    public_key: String,
}

impl NodeAccount {
    pub fn new(addr: impl Into<String>, pk: impl Into<String>) -> Self {
        Self {
            address: addr.into(),
            public_key: pk.into(),
        }
    }

    pub fn address(&self) -> &str {
        self.address.as_str()
    }

    pub fn public_key(&self) -> &str {
        self.public_key.as_str()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeAccounts {
    pub seed: String,
    pub accounts: Accounts,
}

#[derive(Clone, Debug)]
pub struct ParkedPort(Port, Arc<TcpListener>);
impl ParkedPort {
    pub(crate) fn new(port: u16, listener: Arc<TcpListener>) -> ParkedPort {
        ParkedPort(port, listener)
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
