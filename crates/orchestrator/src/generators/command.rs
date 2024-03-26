use configuration::{shared::constants::THIS_IS_A_BUG, types::Arg};

use crate::{network_spec::node::NodeSpec, shared::constants::*};

pub struct GenCmdOptions<'a> {
    pub relay_chain_name: &'a str,
    pub cfg_path: &'a str,
    pub data_path: &'a str,
    pub relay_data_path: &'a str,
    pub use_wrapper: bool,
    pub bootnode_addr: Vec<String>,
    pub use_default_ports_in_cmd: bool,
}

impl<'a> Default for GenCmdOptions<'a> {
    fn default() -> Self {
        Self {
            relay_chain_name: "rococo-local",
            cfg_path: "/cfg",
            data_path: "/data",
            relay_data_path: "/relay-data",
            use_wrapper: true,
            bootnode_addr: vec![],
            use_default_ports_in_cmd: false,
        }
    }
}

const FLAGS_ADDED_BY_US: [&str; 5] = [
    "--unsafe-rpc-external",
    "--no-telemetry",
    "--no-mdns",
    "--collator",
    "--",
];
const OPS_ADDED_BY_US: [&str; 6] = [
    "--chain",
    "--name",
    "--rpc-cors",
    "--rpc-methods",
    "--parachain-id",
    "--node-key",
];

// TODO: can we abstract this and use only one fn (or at least split and reuse in small fns)
pub fn generate_for_cumulus_node(
    node: &NodeSpec,
    options: GenCmdOptions,
    para_id: u32,
    full_p2p_port: u16,
) -> (String, Vec<String>) {
    let NodeSpec {
        key,
        args,
        is_validator,
        bootnodes_addresses,
        ..
    } = node;

    let mut tmp_args: Vec<String> = vec!["--node-key".into(), key.clone()];

    if !args.contains(&Arg::Flag("--prometheus-external".into())) {
        tmp_args.push("--prometheus-external".into())
    }

    if *is_validator && !args.contains(&Arg::Flag("--validator".into())) {
        tmp_args.push("--collator".into())
    }

    if !bootnodes_addresses.is_empty() {
        tmp_args.push("--bootnodes".into());
        let bootnodes = bootnodes_addresses
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        tmp_args.push(bootnodes)
    }

    // ports
    let (prometheus_port, rpc_port, p2p_port) =
        resolve_ports(node, options.use_default_ports_in_cmd);

    tmp_args.push("--prometheus-port".into());
    tmp_args.push(prometheus_port.to_string());

    tmp_args.push("--rpc-port".into());
    tmp_args.push(rpc_port.to_string());

    tmp_args.push("--listen-addr".into());
    tmp_args.push(format!("/ip4/0.0.0.0/tcp/{}/ws", p2p_port));

    let mut collator_args: &[Arg] = &[];
    let mut full_node_args: &[Arg] = &[];
    if !args.is_empty() {
        if let Some(index) = args.iter().position(|arg| match arg {
            Arg::Flag(flag) => flag.eq("--"),
            Arg::Option(..) => false,
        }) {
            (collator_args, full_node_args) = args.split_at(index);
        };
    }

    // set our base path
    tmp_args.push("--base-path".into());
    tmp_args.push(options.data_path.into());

    let node_specific_bootnodes: Vec<String> = node
        .bootnodes_addresses
        .iter()
        .map(|b| b.to_string())
        .collect();
    let full_bootnodes = [node_specific_bootnodes, options.bootnode_addr].concat();
    if !full_bootnodes.is_empty() {
        tmp_args.push("--bootnodes".into());
        tmp_args.push(full_bootnodes.join(" "));
    }

    let mut full_node_p2p_needs_to_be_injected = false;
    let mut full_node_args_filtered = full_node_args
        .iter()
        .filter_map(|arg| match arg {
            Arg::Flag(flag) => {
                if FLAGS_ADDED_BY_US.contains(&flag.as_str()) {
                    None
                } else {
                    Some(vec![flag.to_owned()])
                }
            },
            Arg::Option(k, v) => {
                if OPS_ADDED_BY_US.contains(&k.as_str()) {
                    None
                } else if k.eq(&"port") && v.eq(&"30333") {
                    full_node_p2p_needs_to_be_injected = true;
                    None
                } else {
                    Some(vec![k.to_owned(), v.to_owned()])
                }
            },
        })
        .flatten()
        .collect::<Vec<String>>();

    // change p2p port if is the default
    full_node_args_filtered.push("--port".into());
    full_node_args_filtered.push(full_p2p_port.to_string());

    let mut args_filtered = collator_args
        .iter()
        .filter_map(|arg| match arg {
            Arg::Flag(flag) => {
                if FLAGS_ADDED_BY_US.contains(&flag.as_str()) {
                    None
                } else {
                    Some(vec![flag.to_owned()])
                }
            },
            Arg::Option(k, v) => {
                if OPS_ADDED_BY_US.contains(&k.as_str()) {
                    None
                } else {
                    Some(vec![k.to_owned(), v.to_owned()])
                }
            },
        })
        .flatten()
        .collect::<Vec<String>>();

    tmp_args.append(&mut args_filtered);

    let parachain_spec_path = format!("{}/{}.json", options.cfg_path, para_id);
    let mut final_args = vec![
        node.command.as_str().to_string(),
        "--chain".into(),
        parachain_spec_path,
        "--name".into(),
        node.name.clone(),
        "--rpc-cors".into(),
        "all".into(),
        "--unsafe-rpc-external".into(),
        "--rpc-methods".into(),
        "unsafe".into(),
    ];

    final_args.append(&mut tmp_args);

    let relaychain_spec_path = format!("{}/{}.json", options.cfg_path, options.relay_chain_name);
    let mut full_node_injected: Vec<String> = vec![
        "--".into(),
        "--base-path".into(),
        options.relay_data_path.into(),
        "--chain".into(),
        relaychain_spec_path,
        "--execution".into(),
        "wasm".into(),
    ];

    final_args.append(&mut full_node_injected);
    final_args.append(&mut full_node_args_filtered);

    if options.use_wrapper {
        ("/cfg/zombie-wrapper.sh".to_string(), final_args)
    } else {
        (final_args.remove(0), final_args)
    }
}

pub fn generate_for_node(
    node: &NodeSpec,
    options: GenCmdOptions,
    para_id: Option<u32>,
) -> (String, Vec<String>) {
    let NodeSpec {
        key,
        args,
        is_validator,
        bootnodes_addresses,
        ..
    } = node;
    let mut tmp_args: Vec<String> = vec![
        "--node-key".into(),
        key.clone(),
        // TODO:(team) we should allow to set the telemetry url from config
        "--no-telemetry".into(),
    ];

    if !args.contains(&Arg::Flag("--prometheus-external".into())) {
        tmp_args.push("--prometheus-external".into())
    }

    if let Some(para_id) = para_id {
        tmp_args.push("--parachain-id".into());
        tmp_args.push(para_id.to_string());
    }

    if *is_validator && !args.contains(&Arg::Flag("--validator".into())) {
        tmp_args.push("--validator".into());
        if node.supports_arg("--insecure-validator-i-know-what-i-do") {
            tmp_args.push("--insecure-validator-i-know-what-i-do".into());
        }
    }

    if !bootnodes_addresses.is_empty() {
        tmp_args.push("--bootnodes".into());
        let bootnodes = bootnodes_addresses
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        tmp_args.push(bootnodes)
    }

    // ports
    let (prometheus_port, rpc_port, p2p_port) =
        resolve_ports(node, options.use_default_ports_in_cmd);

    // Prometheus
    tmp_args.push("--prometheus-port".into());
    tmp_args.push(prometheus_port.to_string());

    // RPC
    // TODO (team): do we want to support old --ws-port?
    tmp_args.push("--rpc-port".into());
    tmp_args.push(rpc_port.to_string());

    let listen_value = if let Some(listen_val) = args.iter().find_map(|arg| match arg {
        Arg::Flag(_) => None,
        Arg::Option(k, v) => {
            if k.eq("--listen-addr") {
                Some(v)
            } else {
                None
            }
        },
    }) {
        let mut parts = listen_val.split('/').collect::<Vec<&str>>();
        // TODO: move this to error
        let port_part = parts
            .get_mut(4)
            .expect(&format!("should have at least 5 parts {THIS_IS_A_BUG}"));
        let port_to_use = p2p_port.to_string();
        *port_part = port_to_use.as_str();
        parts.join("/")
    } else {
        format!("/ip4/0.0.0.0/tcp/{}/ws", p2p_port)
    };

    tmp_args.push("--listen-addr".into());
    tmp_args.push(listen_value);

    // set our base path
    tmp_args.push("--base-path".into());
    tmp_args.push(options.data_path.into());

    let node_specific_bootnodes: Vec<String> = node
        .bootnodes_addresses
        .iter()
        .map(|b| b.to_string())
        .collect();
    let full_bootnodes = [node_specific_bootnodes, options.bootnode_addr].concat();
    if !full_bootnodes.is_empty() {
        tmp_args.push("--bootnodes".into());
        tmp_args.push(full_bootnodes.join(" "));
    }

    // add the rest of the args
    let mut args_filtered = args
        .iter()
        .filter_map(|arg| match arg {
            Arg::Flag(flag) => {
                if FLAGS_ADDED_BY_US.contains(&flag.as_str()) {
                    None
                } else {
                    Some(vec![flag.to_owned()])
                }
            },
            Arg::Option(k, v) => {
                if OPS_ADDED_BY_US.contains(&k.as_str()) {
                    None
                } else {
                    Some(vec![k.to_owned(), v.to_owned()])
                }
            },
        })
        .flatten()
        .collect::<Vec<String>>();

    tmp_args.append(&mut args_filtered);

    let chain_spec_path = format!("{}/{}.json", options.cfg_path, options.relay_chain_name);
    let mut final_args = vec![
        node.command.as_str().to_string(),
        "--chain".into(),
        chain_spec_path,
        "--name".into(),
        node.name.clone(),
        "--rpc-cors".into(),
        "all".into(),
        "--unsafe-rpc-external".into(),
        "--rpc-methods".into(),
        "unsafe".into(),
    ];

    final_args.append(&mut tmp_args);

    if let Some(ref subcommand) = node.subcommand {
        final_args.insert(1, subcommand.as_str().to_string());
    }

    if options.use_wrapper {
        ("/cfg/zombie-wrapper.sh".to_string(), final_args)
    } else {
        (final_args.remove(0), final_args)
    }
}

/// Returns (prometheus, rpc, p2p) ports to use in the command
fn resolve_ports(node: &NodeSpec, use_default_ports_in_cmd: bool) -> (u16, u16, u16) {
    if use_default_ports_in_cmd {
        (PROMETHEUS_PORT, RPC_PORT, P2P_PORT)
    } else {
        (node.prometheus_port.0, node.rpc_port.0, node.p2p_port.0)
    }
}
