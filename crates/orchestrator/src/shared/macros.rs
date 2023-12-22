macro_rules! create_add_options {
    ($struct:ident {$( $field:ident:$type:ty ),*}) =>{
        #[derive(Default, Debug, Clone)]
        pub struct $struct {
            /// Image to run the node
            pub image: Option<Image>,
            /// Command to run the node
            pub command: Option<Command>,
            /// Subcommand for the node
            pub subcommand: Option<Command>,
            /// Arguments to pass to the node
            pub args: Vec<Arg>,
            /// Env vars to set
            pub env: Vec<EnvVar>,
            /// Make the node a validator
            ///
            /// This implies `--validator` or `--collator`
            pub is_validator: bool,
            /// RPC port to use, if None a random one will be set
            pub rpc_port: Option<Port>,
            /// Prometheus port to use, if None a random one will be set
            pub prometheus_port: Option<Port>,
            /// P2P port to use, if None a random one will be set
            pub p2p_port: Option<Port>,
            $(
                pub $field: $type,
            )*
        }
    };
}

pub(crate) use create_add_options;
