# devenv-reload

A Rust library for shell session management with hot-reload capability. When watched files change, it spawns a new shell in the background and seamlessly swaps to it, preserving terminal state.

## Usage

```rust
use devenv_reload::{ShellManager, ShellBuilder, Config, BuildContext, BuildError, CommandBuilder, ManagerMessage, WatcherHandle};
use tokio::sync::mpsc;

struct MyBuilder;

impl ShellBuilder for MyBuilder {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        // Add new watch paths at runtime
        ctx.watcher.watch(&ctx.cwd.join("extra.nix")).ok();

        let mut cmd = CommandBuilder::new("bash");
        cmd.cwd(&ctx.cwd);
        Ok(cmd)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::new(vec!["devenv.nix".into(), "devenv.lock".into()]);
    let (msg_tx, mut msg_rx) = mpsc::channel::<ManagerMessage>(10);

    // Handle messages in background
    tokio::spawn(async move {
        while let Some(msg) = msg_rx.recv().await {
            match msg {
                ManagerMessage::Reloaded { files } => println!("Reloaded: {:?}", files),
                ManagerMessage::ReloadFailed { files, error } => println!("Reload failed ({:?}): {}", files, error),
                ManagerMessage::BuildFailed { files, error } => println!("Build failed ({:?}): {}", files, error),
            }
        }
    });

    ShellManager::run(config, MyBuilder, msg_tx).await?;
    Ok(())
}
```

## API

- `Config::new(watch_files)` — configuration with files to watch
- `ShellBuilder` trait — implement `build()` to return a `CommandBuilder`
- `BuildContext.watcher` — `WatcherHandle` to add new watch paths at runtime via `watcher.watch(&path)`
- `ShellManager::run(config, builder, messages)` — runs the shell session, blocks until exit
- `ManagerMessage` — enum with `Reloaded`, `ReloadFailed`, `BuildFailed` variants

## How it works

1. Calls `builder.build()` to spawn initial shell in a PTY
2. Watches configured files for changes
3. On file change, calls `builder.build()` again in background
4. If successful, captures terminal state via AVT, swaps PTYs, replays state, sends `Reloaded` message
5. If failed, sends `ReloadFailed` or `BuildFailed` message and keeps current shell

## Development

```bash
devenv shell
cargo build
cargo test
```
