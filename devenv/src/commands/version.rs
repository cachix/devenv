//! `devenv version`: print the devenv version, build revision, and target system.

use clap::crate_version;

/// VERGEN_GIT_SHA is set by `build.rs`:
/// - From vergen when building from a git checkout
/// - Parsed from DEVENV_GIT_REV for flake builds
/// - VERGEN_IDEMPOTENT_OUTPUT for tarball builds (nixpkgs)
fn build_rev() -> Option<String> {
    let sha = env!("VERGEN_GIT_SHA");
    if sha.is_empty() || sha == "VERGEN_IDEMPOTENT_OUTPUT" {
        return None;
    }
    if env!("VERGEN_GIT_DIRTY") == "true" {
        Some(format!("{sha}-dirty"))
    } else {
        Some(sha.to_string())
    }
}

pub fn run(system: Option<&str>) {
    let version = crate_version!();
    let system = system
        .map(str::to_owned)
        .unwrap_or_else(devenv_core::settings::default_system);
    match build_rev() {
        Some(rev) => println!("devenv {version}+{rev} ({system})"),
        None => println!("devenv {version} ({system})"),
    }
}
