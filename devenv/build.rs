use std::process::Command;
use vergen_gitcl::{Emitter, GitclBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "cargo:rustc-env=TARGET_ARCH={}",
        std::env::var("CARGO_CFG_TARGET_ARCH").unwrap()
    );
    println!(
        "cargo:rustc-env=TARGET_OS={}",
        std::env::var("CARGO_CFG_TARGET_OS").unwrap()
    );
    // Rerun if init directory changes
    println!("cargo:rerun-if-changed=init");

    // Git revision info via vergen (works when .git is available)
    let gitcl = GitclBuilder::default().sha(true).dirty(true).build()?;

    Emitter::default().add_instructions(&gitcl)?.emit()?;

    // Rerun when git state changes (for tag detection and SHA)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");

    // Detect if HEAD is on a release tag (for cargo builds with .git)
    let on_tag = Command::new("git")
        .args(["describe", "--tags", "--exact-match", "HEAD"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!("cargo:rustc-env=DEVENV_ON_RELEASE_TAG={on_tag}");

    // Forward DEVENV_GIT_REV for Nix builds where .git is unavailable
    println!("cargo:rerun-if-env-changed=DEVENV_GIT_REV");
    if let Ok(rev) = std::env::var("DEVENV_GIT_REV") {
        println!("cargo:rustc-env=DEVENV_GIT_REV={rev}");
    }

    // Forward DEVENV_IS_RELEASE for Nix release builds
    println!("cargo:rerun-if-env-changed=DEVENV_IS_RELEASE");
    if let Ok(val) = std::env::var("DEVENV_IS_RELEASE") {
        println!("cargo:rustc-env=DEVENV_IS_RELEASE={val}");
    }

    Ok(())
}
