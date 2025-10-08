# Zombienet SDK Examples

This directory contains a collection of examples demonstrating the features of the Zombienet SDK.

## How to Run

- **Rust examples:**  
  Run with  
  ```sh
  cargo run --example <EXAMPLE_NAME>
  ```

- **Config files:**  
  Spawn a network with  
  ```sh
  zombie-cli spawn -p <provider> <config-file>
  ```

## Example Index

### Basic Network Setup

*   **`simple_network_example`**
    *   **Description**: Launches a minimal relay chain with two validator nodes using `simple.toml`. This is the best starting point for new users.
    *   **Run**: `cargo run --example simple_network_example`

*   **`small_network_with_default`**
    *   **Description**: Demonstrates how to override the default node command and image for all nodes in the network.
    *   **Run**: `cargo run --example small_network_with_default`

*   **`small_network_with_base_dir`**
    *   **Description**: Shows how to specify a custom base directory for node data, which is useful for debugging and persisting state.
    *   **Run**: `cargo run --example small_network_with_base_dir`

### Advanced Configuration

*   **`resource_limits`**
    *   **Description**: Demonstrates how to apply CPU and memory resource limits to nodes, using `resource_limits.toml`. This is essential for containerized providers like Docker and Kubernetes.
    *   **Run**: `cargo run --example resource_limits`

*   **`wasm-override`**
    *   **Description**: Shows how to launch a network with a custom WASM runtime that overrides the one built into the node binary.
    *   **Run**: `cargo run --example wasm-override`

*   **`arg_removal`**
    *   **Description**: Demonstrates how to remove default command-line arguments from a node's startup command.
    *   **Run**: `cargo run --example arg_removal`

*   **`db_snapshot`**
    *   **Description**: Illustrates how to start a network, create a database snapshot, and then launch a new network from that snapshot to speed up initialization.
    *   **Run**: `cargo run --example db_snapshot`

*   **`chain_spec_generation`**
    *   **Description**: Shows how to generate a chain spec dynamically when launching a network by providing a `chain_spec_command`.
    *   **Run**: `cargo run --example chain_spec_generation`

*   **`raw_spec_override`**
    *   **Description**: Demonstrates how to override fields in the relay chain's raw chain spec (such as the chain name or bootNodes) using the SDK's `.with_raw_spec_override()` method.
    *   **Run**: `cargo run --example raw_spec_override`

*   **`genesis_override`**
    *   **Description**: Shows how to override the genesis configuration (such as balances) for a parachain using `.with_genesis_overrides()`, and verifies the change by querying the balance with Polkadot.js.
    *   **Run**: `cargo run --example genesis_override`

### Parachain Lifecycle

*   **`small_network_with_para`**
    *   **Description**: Launches a relay chain and a single parachain, demonstrating a basic parachain topology and and node interaction (e.g., pausing nodes).
    *   **Run**: `cargo run --example small_network_with_para`

*   **`register_para`**
    *   **Description**: A full example of spawning a relay chain and a collator, then submitting the extrinsic to register the parachain on the relay chain.
    *   **Run**: `cargo run --example register_para`

*   **`add_para`**
    *   **Description**: Demonstrates how to deploy a relay chain and parachain, then dynamically add a new parachain to the running network using the SDK’s API.
    *   **Run**: `cargo run --example add_para`

*   **`para_upgrade`**
    *   **Description**: Shows how to perform a runtime upgrade on a running parachain by submitting an `authorize_upgrade` and `enact_authorized_upgrade` extrinsic.
    *   **Run**: `cargo run --example para_upgrade`

*   **Group Nodes Examples**
    *   **Description**: Demonstrates how to configure and interact with group nodes (multiple validators/collators) in a network.
        - `big_network_with_group_nodes`: Programmatically builds a network with grouped nodes.
        - `network_example_with_group_nodes`: Loads a TOML config with group nodes and prints out all relay and collator nodes.
    *   **Run**:  
        - `cargo run --example big_network_with_group_nodes`  
        - `cargo run --example network_example_with_group_nodes`

*   **`two_paras_same_id`**
    *   **Description**: Demonstrates what happens when two parachains with the same ID are added to the network, useful for testing duplicate parachain ID handling.
    *   **Run**: `cargo run --example two_paras_same_id`

### Scripting and Testing

*   **`pjs`**
    *   **Description**: Demonstrates how to execute a JavaScript test file (`pjs_transfer.js`) against a running network using the integrated Polkadot.js runner.
    *   **Run**: `cargo run --example pjs`