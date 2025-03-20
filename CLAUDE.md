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
- **Error Handling**: Use `thiserror` crate with custom error types and `?` operator
- **Types**: Prefer strong typing with descriptive names and appropriate generics
- **Formatting**: Follow standard rustfmt rules, use pre-commit hooks
- **Documentation**: Document public APIs with rustdoc comments
- **No unsafe**: Don't use `unsafe` code

## Project Structure
- Uses workspace with multiple crates (`devenv`, `devenv-eval-cache`, etc.)
- Nix modules in `/src/modules/` define supported languages and services
- Examples in `/examples/` show various configurations
- Tests in `/tests/` validate functionality
