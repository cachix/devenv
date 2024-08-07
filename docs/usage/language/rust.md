

[comment]: # (Please add your documentation on top of this line)

## languages\.rust\.enable



Whether to enable tools for Rust development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.rust\.channel

The rustup toolchain to install\.



*Type:*
one of “nixpkgs”, “stable”, “beta”, “nightly”



*Default:*
` "nixpkgs" `



## languages\.rust\.components



List of [Rustup components](https://rust-lang\.github\.io/rustup/concepts/components\.html)
to install\. Defaults to those available in ` nixpkgs `\.



*Type:*
list of string



*Default:*
` [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ] `



## languages\.rust\.mold\.enable



Enable mold as the linker\.

Enabled by default on x86_64 Linux machines when no cross-compilation targets are specified\.



*Type:*
boolean



*Default:*
` pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64 && languages.rust.targets == [ ] `



## languages\.rust\.rustflags



Extra flags to pass to the Rust compiler\.



*Type:*
string



*Default:*
` "" `



## languages\.rust\.targets



List of extra [targets](https://github\.com/nix-community/fenix\#supported-platforms-and-targets)
to install\. Defaults to only the native target\.



*Type:*
list of string



*Default:*
` [ ] `



## languages\.rust\.toolchain



Rust component packages\. May optionally define additional components, for example ` miri `\.



*Type:*
attribute set of package



*Default:*
` nixpkgs `



## languages\.rust\.toolchain\.cargo



cargo package



*Type:*
null or package



*Default:*
` pkgs.cargo `



## languages\.rust\.toolchain\.clippy



clippy package



*Type:*
null or package



*Default:*
` pkgs.clippy `



## languages\.rust\.toolchain\.rust-analyzer



rust-analyzer package



*Type:*
null or package



*Default:*
` pkgs.rust-analyzer `



## languages\.rust\.toolchain\.rustc



rustc package



*Type:*
null or package



*Default:*
` pkgs.rustc `



## languages\.rust\.toolchain\.rustfmt



rustfmt package



*Type:*
null or package



*Default:*
` pkgs.rustfmt `
