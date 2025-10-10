# devenv - Development Environment Project Guide

## Build & Development Commands
- Build project: `cargo build`
- Run CLI: `cargo run -- [args]`
- Build with Nix: `nix build`
- Format code: `cargo fmt`
- Lint code: `cargo clippy`
- Run all tests: `cargo test` or `devenv-run-tests tests`
- Run single test: `devenv-run-tests --only <test_name> tests`

## Code Style Guidelines
- **Imports**: Group by category (std lib first, then external crates, then internal)
- **Naming**: Use `snake_case` for functions/variables, `CamelCase` for types/traits
- **Error Handling**: Use `thiserror` crate with custom error types, `bail!` (instead of `panic~`) and `?` operator
- **Types**: Prefer strong typing with descriptive names and appropriate generics
- **Formatting**: Follow standard rustfmt rules, use pre-commit hooks
- **Documentation**: Document public APIs with rustdoc comments
- **No unsafe**: Don't use `unsafe` code

## Project Structure
- Uses workspace with multiple crates (`devenv`, `devenv-eval-cache`, etc.)
- Nix modules in `/src/modules/` define supported languages and services
- Examples in `/examples/` show various configurations
- Tests in `/tests/` validate functionality

## Adding New CLI Subcommands
To add a new subcommand (like `machines`, `processes`, `tasks`), follow these steps:

### 1. Create Implementation Module
Create a new directory under `devenv/src/` (e.g., `devenv/src/myfeature/`)
```rust
// devenv/src/myfeature/mod.rs
use miette::Result;

pub mod implementation;  // Additional modules as needed

// Main entry point for the subcommand
pub async fn subcommand(devenv: &crate::Devenv, args: Args) -> Result<()> {
    // Implementation here
    Ok(())
}
```

### 2. Define CLI Structure
Add enum to `devenv/src/cli.rs`:
```rust
#[derive(Subcommand, Clone)]
#[clap(about = "Description here. https://devenv.sh/myfeature/")]
pub enum MyFeatureCommand {
    #[command(about = "Subcommand description.")]
    Action1 {
        #[arg(help = "Argument help text")]
        arg1: Option<String>,

        #[arg(long, help = "Flag help text")]
        flag1: bool,
    },
    // Add more subcommands as needed
}
```

Add variant to main `Commands` enum:
```rust
pub enum Commands {
    // ... existing commands ...

    #[command(about = "My feature. https://devenv.sh/myfeature/")]
    MyFeature {
        #[command(subcommand)]
        command: MyFeatureCommand,
    },
}
```

### 3. Wire Up in Main
Add match arm in `devenv/src/main.rs`:
```rust
Commands::MyFeature { command } => match command {
    MyFeatureCommand::Action1 { arg1, flag1 } => {
        myfeature::subcommand(&devenv, arg1, flag1).await
    }
},
```

Never edit these files: docs/reference/options.md
