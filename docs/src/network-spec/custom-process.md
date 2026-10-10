# Custom Processes

Allow to set custom processes to spawn after the network is up: services that run next to the nodes, such as an RPC adapter, an IPFS daemon or a database another process needs.

A custom process is a member of the network like a node: it is tracked on the `Network`, written to `zombie.json`, attached to again from it and destroyed with the namespace. Look it up with `network.get_custom_process("name")` or list them with `network.custom_processes()`. A process may not share its name with a node, since they share one registry: in TOML the process is rejected; in the builder a node declared after a process with its name is renamed, as it would be after another node, so declare processes last.

Unlike nodes, a custom process runs its `command` directly, not through zombienet's wrapper script: most of them are off-the-shelf images, which may not have `bash`. On docker and kubernetes the pause/resume/restart controls go through that wrapper, so on a custom process they return an error; on native they signal the process and work as for nodes.

**A process must listen on all interfaces** (`0.0.0.0`) on docker and kubernetes, where it is reached through a published port or a `Service`; one bound to `127.0.0.1` is reachable from nowhere. Most daemons need a flag for that (`--rpc-external` for eth-rpc below, `--host 0.0.0.0` and the like elsewhere).

A process gets the same writable `/data` directory a node gets, on every provider; that is where kubo above keeps its repo (`IPFS_PATH`) and where anything that must survive a restart of the process belongs.

**`command` replaces the image's entrypoint**, on every provider. An image whose entrypoint does setup work (kubo's runs `ipfs init` and binds the API and gateway to all interfaces; the postgres image's runs `initdb`) needs that entrypoint named as the command, with the image's usual arguments as `args`.

### TOML

```toml
[[custom_processes]]
name = "eth-rpc"
command = "eth-rpc"
args = [ "--node-rpc-url", "{{ZOMBIE:alice:internal_ws_uri}}", "--rpc-port", "{{ZOMBIE:eth-rpc:port_http}}", "--rpc-external" ]
env = [
    { name = "RUST_LOG", value = "info" }
]
ports = [ { name = "http", port = 0 } ]

[[custom_processes]]
name = "ipfs"
image = "docker.io/ipfs/kubo:v0.39.0"
command = "/usr/local/bin/start_ipfs"
args = [ "daemon" ]
ports = [ { name = "api", port = 5001 }, { name = "gateway", port = 8080 } ]

[custom_processes.resources]
limits = { cpu = "500m", memory = "512Mi" }
```

### Builder

```rust
let config = NetworkConfigBuilder::new()
    .with_relaychain(|r| { /* ... */ })
    .with_custom_process(|c| c.with_name("eth-rpc").with_command("eth-rpc"))
```

```rust
let cp = CustomProcessBuilder::new()
    .with_name("ipfs")
    .with_command("/usr/local/bin/start_ipfs")
    .with_image("docker.io/ipfs/kubo:v0.39.0")
    .with_args(vec!["daemon".into()])
    .with_env(vec![("IPFS_PATH", "/data/ipfs")])
    .with_named_port("api", 5001)
    .with_named_port("gateway", 8080)
    .with_resources(|r| r.with_limit_cpu("500m").with_limit_memory("512Mi"))
    .with_ready_check(ReadyCheck::tcp("api"))
    .with_timeout(60)
    .with_dependency("ipfs-init")
    .build()
    .unwrap();
```

### Placeholders

The `args` and `env` values of a process may refer to the nodes, which are all running by then, with the same `{{ZOMBIE:<node-name>:<field>}}` placeholders node args use: `ws_uri`, `internal_ws_uri` (the address other members of the network use, which is what a process wants), `multiaddr`, `prometheus_uri` for a substrate node, `rpc_uri` and `peer_addr` for a JAM node. On docker a node's addresses are host addresses, so a process in a container reaches a node only through the host (`host.docker.internal` on Docker Desktop). They may also refer to any custom process's ports, their own or another's, as `{{ZOMBIE:<process-name>:port_<port-name>}}`, since every port is picked before anything starts (see Lifecycle). A placeholder that resolves to nothing (a node or port that does not exist) fails the spawn of that process, which is logged and skipped.

### Ports

Each declared port is TCP and exposed by name. A port declared as `0` is picked free by zombienet when the process is spawned. Whatever the number, the process learns it through the placeholder above; `eth-rpc` is told `--rpc-port <picked>` that way.

On kubernetes the declared ports are the pod's `Service` ports, so other pods reach the process as `<process-name>:<port>`, and a port-forward is opened from the host running zombienet for each (unless running in CI, where the pod's own address is used). The pod's readiness probe is the process's check (see Lifecycle), so the `Service` routes to the pod once that passes on the pod address, which a process bound to `127.0.0.1` never does (see above). On docker each port is published on a free host port picked at spawn, so two networks can share a host; on native the process listens on them as declared, and a fixed port that is busy fails the spawn.

The running process reports where every port is reachable:

```rust
let ipfs = network.get_custom_process("ipfs")?;
println!("{}", ipfs.get_uri_for_name("api").unwrap()); // from here, e.g. 127.0.0.1:53421 on k8s
let api = ipfs.port("api").unwrap();
println!("{}", api.external); // the same address
println!("{}", api.internal); // from inside the network, e.g. ipfs:5001 on k8s
```

On native `external` and `internal` are the same address. On docker `external` is the published host port and `internal` the container's own address. On kubernetes `external` is a port-forward that lives as long as the zombienet process; re-attaching opens new ones. A port-forward, like docker's published port, accepts connections whether or not the process is listening, so `is_responsive()` on kubernetes and docker reports the forward, not the process; only on native and on kubernetes in CI does it reach the process itself. Re-attaching takes the processes as recorded in `zombie.json`, with the same caveat.

### Lifecycle

A process is **ready** when its `ready_check` passes: `tcp = "<port name>"`, the port accepts a connection, or `http = { port = "<port name>", path = "/health" }`, a `GET` answers 2xx, every `interval` seconds (1 by default). The process's `timeout`, in seconds, bounds the wait (the global `node_spawn_timeout` by default); a one-shot is waited on to exit within it, and marked failed, not stopped, when it does not. Without a check, a process with ports is ready when its first port accepts a TCP connection, and one without ports is ready once started. On kubernetes the check is the pod's readiness probe, so the kubelet does the checking from inside the cluster and the `Service` routes only once it passes; on docker and native zombienet probes from where it runs. On docker a published port answers for docker's proxy whether or not the process listens, so a tcp check there, including the default on the first port, passes at once and gives no real ordering; zombienet warns about it at spawn, and `http` is the check to use there.

A **one-shot** process (`one_shot = true`) runs to completion within its `timeout`: exit 0 is success, anything else a failure. It has no readiness, so no `ready_check`. On kubernetes a custom process's pod is never restarted by the kubelet, one-shot or not: a process that exits is done, by success or by failure, and its pod says so.

`depends_on` lists the processes this one starts after, with a condition as docker-compose names them: `service_started`, `service_healthy` (passed its check), `service_completed_successfully` (a one-shot that exited 0). A bare name means the target's natural condition: completed for a one-shot, healthy when it has a check or a port, started otherwise. Every process starts as soon as its dependencies are met, in parallel with whatever else can start; one with no dependencies starts once the nodes are up. A dependency must name a custom process, its condition must fit the target, and there may be no cycle; all three are checked when the config loads.

```toml
[[custom_processes]]
name = "migrate"
image = "docker.io/library/postgres:16"
command = "psql"
args = [ "-h", "db", "-f", "/data/schema.sql" ]
one_shot = true
depends_on = ["db"]

[[custom_processes]]
name = "api"
image = "example/api:1.0"
command = "api"
ports = [ { name = "http", port = 8080 } ]
ready_check = { http = { port = "http", path = "/health" } }
timeout = 120
depends_on = [ "migrate", { name = "cache", condition = "service_started" } ]
```

What happened is on the running process: `process.state()` is `Starting`, `Ready`, `Completed` or `Failed { reason }`, and is written to `zombie.json`. A process that fails its check or exits with an error is **kept, not torn down**, so its logs can be read, and `spawn` still returns the `Network`; the processes whose condition on it can no longer be met are not started and are listed with the reason in `network.skipped_custom_processes()` (a `service_started` dependent still starts). `wait_ready()` on a process waits for its condition again; the network-wide `wait_until_is_up()` leaves processes out, for the same reason the spawn does not fail on them.

A process may name another's port the way it names its own, `{{ZOMBIE:<process>:port_<name>}}`, since every port is picked before anything starts. The host part of the address is the other process's name on kubernetes and the host itself on native; on docker the containers have no names for each other, so the placeholder is of little use there.

### Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | String | — | **Required.** Name of the process; also its pod/container name and the key it is looked up by. Lowercase letters, digits and `-`, starting with a letter, at most 63 characters, unique among nodes and processes |
| `command` | String | — | **Required.** Command to execute; replaces the image's entrypoint |
| `image` | String | — | Container image |
| `args` | Array | — | CLI arguments; may use the placeholders above |
| `env` | Array | — | Environment variables as `{name, value}` pairs; values may use the same placeholders |
| `ports` | Array | — | TCP ports the process listens on, as `{name, port}` pairs. Names are 1 to 15 lowercase letters, digits or `-`, with a letter, unique per process; `port: 0` is picked at spawn |
| `resources` | Table | — | CPU/memory `requests` and `limits` (kubernetes only; a warning is logged where ignored), as for nodes |
| `ready_check` | Table | first port over TCP | `{ tcp = "<port>" }` or `{ http = { port = "<port>", path = "..." } }`, plus `interval` in seconds |
| `depends_on` | Array | — | Names, or `{ name, condition }` with `service_started`, `service_healthy` or `service_completed_successfully` |
| `one_shot` | Bool | `false` | Runs to completion; exit 0 is success. No `ready_check` |
| `timeout` | Integer | `node_spawn_timeout` | Seconds to wait for the ready check to pass, or the one-shot to exit |
