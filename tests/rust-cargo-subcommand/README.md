# Cargo Subcommand Precedence

Cargo normally [prioritizes external subcommands from
`$CARGO_HOME/bin`][cargo-custom-subcommands]. The devenv Rust module appends
that directory to `$PATH`, causing Cargo to respect normal `$PATH` ordering
while keeping user-installed subcommands available.

[cargo-custom-subcommands]: https://doc.rust-lang.org/cargo/reference/external-tools.html#custom-subcommands
