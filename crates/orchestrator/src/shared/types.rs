use std::{collections::HashMap, net::TcpListener, sync::Arc};

use configuration::shared::types::Port;
pub type Accounts = HashMap<String, NodeAccount>;

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
