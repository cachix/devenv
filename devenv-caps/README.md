# devenv-caps

Linux capability granting for devenv processes.

Allows devenv to run processes with specific Linux capabilities (e.g. binding to
port 80) without running the entire environment as root.

## The problem

A devenv project might declare that a process needs a privileged operation:

```nix
processes.webserver = {
  exec = "node server.js";
  capabilities = [ "net_bind_service" ];
};

processes.postgres = {
  exec = "postgres -D $PGDATA";
  capabilities = [ "ipc_lock" ];
};
```

On Linux, these operations require specific capabilities. Normally you'd run as
root or use `setcap` on the binary — neither is great for a development
environment where the config comes from an untrusted project repo.

## Design

### Session-scoped approval

Capabilities are approved **per session** in the TUI. There is no persistent
approval store — every `devenv up` shows the user what capabilities are being
requested, and a config reload presents a fresh list. This means:

- You always see what you're granting
- A dependency update that adds new capability requests is immediately visible
- No stale approvals for capabilities a project removed
- No files to manage or permissions to worry about

### Architecture

```
devenv up
  │
  │  TUI shows capability summary, user confirms
  │
  │  [sudo] password for user:
  │
  ├─ socketpair() ──────────────────────────┐
  │                                         │
  │  sudo devenv-cap-server --fd N          │
  │       --uid 1000 --gid 1000             │
  │       --groups 1000,27,100              │
  │       │                                 │
  │       │  (runs as root)                 │
  │       │  (only reachable via inherited fd)
  │       │                                 │
  ├─ "launch webserver with net_bind_service" ──►│
  │       │                                 │
  │       │  fork()                         │
  │       │   ├─ tighten bounding set       │
  │       │   ├─ securebits (keep caps)     │
  │       │   ├─ set cap state              │
  │       │   ├─ setgid/setuid (drop root)  │
  │       │   ├─ raise ambient caps         │
  │       │   ├─ clear securebits           │
  │       │   └─ exec node server.js        │
  │       │                                 │
  │       ◄── pid 4821 ────────────────────┘
  │
  ├─ "launch postgres with ipc_lock" ──► (same flow)
  │
  ├─ start worker (no caps needed, spawn directly)
  │
  └─ on exit: "shutdown" ──► cap-server kills children, exits

reload (config changed)
  │
  ├─ re-read process config (fresh capability list)
  ├─ diff against running processes
  ├─ new caps → prompt in TUI, launch via cap-server
  ├─ removed caps → restart process without them (direct spawn)
  └─ unchanged → leave running
```

### Key design decisions

**Socketpair, not a named socket.** The cap-server communicates over a file
descriptor inherited from devenv via `socketpair()`. There is no filesystem
socket to discover — no other process (even one running as the same user) can
connect to the cap-server.

**One sudo per session.** `sudo` is invoked once when `devenv up` starts (if
any process needs capabilities). sudo's credential caching means the user
typically only types their password once per terminal session.

**Hard-coded allowlist.** The cap-server refuses to grant capabilities outside
a curated list. `CAP_SYS_ADMIN`, `CAP_SYS_PTRACE`, etc. are never grantable,
regardless of what a project config requests. See `src/lib/caps.rs` for the
list.

**Tight bounding set.** Each child process has its bounding set restricted to
only the capabilities it was granted. Even if the process is compromised, it
cannot acquire additional capabilities.

**Processes without capabilities skip the server entirely.** Only processes
that actually need capabilities are launched through the cap-server. Everything
else is spawned directly by devenv with no privilege escalation.

## Components

### `devenv-cap-server` (binary)

A minimal root-privileged process spawned via `sudo`. It:

- Reads launch requests from the inherited fd
- Validates requested capabilities against the allowlist
- Forks a child for each request
- In the child: sets ambient caps → drops to target uid/gid → execs the command
- Tracks launched PIDs and only allows signals to its own children
- Shuts down when devenv closes the connection or sends a Shutdown request

### `devenv_caps::client::CapServer` (library)

Used by devenv's process manager to interact with the cap-server:

```rust
use devenv_caps::client::{CapServer, CapServerConfig};

let config = CapServerConfig::current_user(cap_server_binary_path);
let mut server = CapServer::start(&config)?;

let pid = server.launch(
    "webserver",
    &["net_bind_service".into()],
    "/usr/bin/node",
    &["server.js".into()],
    &env_map,
    &working_dir,
)?;

server.signal(pid, libc::SIGTERM)?;
server.shutdown()?;
```

### `devenv_caps::caps` (library)

Capability allowlist with human-readable descriptions for the TUI:

```rust
use devenv_caps::caps;

let parsed = caps::parse_and_validate(&["net_bind_service".into()])?;

if let Some(info) = caps::info_for("net_bind_service") {
    println!("{}: {}", info.name, info.description);
    // "net_bind_service: Bind to TCP/UDP ports below 1024"
}
```

## Allowed capabilities

| Capability | Description |
|---|---|
| `net_bind_service` | Bind to TCP/UDP ports below 1024 |
| `net_raw` | Use raw and packet sockets (ping, tcpdump) |
| `net_admin` | Configure network interfaces and routing |
| `ipc_lock` | Lock memory (PostgreSQL huge pages, mlock) |
| `sys_nice` | Set real-time scheduling priority |
| `sys_resource` | Override resource limits (open file descriptors) |

Capabilities outside this list (notably `sys_admin`, `sys_ptrace`,
`dac_override`) are never granted.

## Security model

| Threat | Mitigation |
|---|---|
| Malicious project config requests dangerous caps | Hard-coded allowlist rejects them |
| Another process connects to cap-server | Impossible — socketpair fd, no filesystem socket |
| Compromised child escalates privileges | Bounding set tightened to only granted caps |
| Cap-server binary is replaced | sudo verifies the path; Nix store paths are read-only |
| devenv crashes without shutdown | Cap-server detects closed socketpair, kills children, exits |
| Config reload sneaks in new caps | TUI shows fresh capability list on every reload |

## UX flow

### Starting

```
$ devenv up

  ✓ worker      started (pid 4825)

  ⚠ 2 processes require Linux capabilities:

    webserver   NET_BIND_SERVICE   Bind to TCP/UDP ports below 1024
    postgres    IPC_LOCK           Lock memory (PostgreSQL huge pages)

  Approve? [Y/n]

  [sudo] password for domen:

  ✓ webserver   started (pid 4821)  [NET_BIND_SERVICE]
  ✓ postgres    started (pid 4823)  [IPC_LOCK]
```

### Reload with new capability

```
  config reloaded

  ⚠ 1 new capability request:

    webserver   NET_RAW   Use raw and packet sockets

  Approve? [Y/n]

  ✓ webserver   restarted (pid 5201)  [NET_BIND_SERVICE, NET_RAW]
```

### Reload with removed capability

```
  config reloaded

  ✓ webserver   restarted (pid 5301)  (no capabilities)
```

No prompt — fewer privileges is always fine.

## Integration with devenv

```rust
fn start_processes(config: &DevenvConfig) {
    let cap_requests: Vec<(&str, &[String])> = config.processes.iter()
        .filter(|(_, p)| !p.capabilities.is_empty())
        .map(|(name, p)| (name.as_str(), p.capabilities.as_slice()))
        .collect();

    let mut cap_server = if !cap_requests.is_empty() {
        if !tui::confirm_capabilities(&cap_requests) {
            None // user denied
        } else {
            let config = CapServerConfig::current_user(cap_server_binary());
            Some(CapServer::start(&config).expect("failed to start cap-server"))
        }
    } else {
        None
    };

    for (name, process) in &config.processes {
        if process.capabilities.is_empty() || cap_server.is_none() {
            spawn_directly(process);
        } else {
            let pid = cap_server.as_mut().unwrap().launch(
                name,
                &process.capabilities,
                &process.command,
                &process.args,
                &process.env,
                &process.working_dir,
            ).unwrap();
            register_process(name, pid);
        }
    }
}

fn reload(config: &DevenvConfig, cap_server: &mut Option<CapServer>) {
    let new_caps = /* from new config */;
    let old_caps = /* from running state */;

    for (name, process) in &config.processes {
        let had_caps = old_caps.contains_key(name);
        let needs_caps = !process.capabilities.is_empty();

        match (had_caps, needs_caps) {
            (_, true) if caps_changed(name) => {
                // New or changed caps — prompt and relaunch via cap-server.
                tui::confirm_new_capabilities(name, &process.capabilities);
                kill_process(name);
                let server = cap_server.get_or_insert_with(|| {
                    let c = CapServerConfig::current_user(cap_server_binary());
                    CapServer::start(&c).unwrap()
                });
                let pid = server.launch(/* ... */).unwrap();
                register_process(name, pid);
            }
            (true, false) => {
                // Caps removed — restart directly, no prompt.
                kill_process(name);
                spawn_directly(process);
            }
            _ => {} // unchanged
        }
    }
}
```

## Platform support

Linux-only. On macOS, devenv should skip the capability system and suggest
workarounds (e.g. `sysctl net.ipv4.ip_unprivileged_port_start` or running
the specific process as root).

## Building

```bash
cargo build --release
```

Produces `target/release/devenv-cap-server` which must be accessible via
`sudo`. In a Nix-based setup, the binary lives in the Nix store and is
referenced by its full path.
