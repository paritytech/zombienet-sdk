use provider::DynNode;

use crate::network_spec::node::NodeSpec;

#[derive(Clone)]
pub struct NetworkNode {
    pub(crate) inner: DynNode,
    // TODO: do we need the full spec here?
    // Maybe a reduce set of values.
    pub(crate) spec: NodeSpec,
    pub(crate) name: String,
    pub(crate) ws_uri: String,
    pub(crate) prometheus_uri: String,
}

impl NetworkNode {
    fn new(inner: DynNode, spec: NodeSpec, _ip: String) -> Self {
        let name = spec.name.clone();
        let ws_uri = "".into();
        let prometheus_uri = "".into();

        Self {
            inner,
            spec,
            name,
            ws_uri,
            prometheus_uri,
        }
    }
}

impl std::fmt::Debug for NetworkNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkNode")
            .field("inner", &"inner_skipped")
            .field("spec", &self.spec)
            .field("name", &self.name)
            .field("ws_uri", &self.ws_uri)
            .field("prometheus_uri", &self.prometheus_uri)
            .finish()
    }
}
