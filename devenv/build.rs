use std::env;
use std::process::Command;
use vergen_gitcl::{Emitter, GitclBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "cargo:rustc-env=TARGET_ARCH={}",
        env::var("CARGO_CFG_TARGET_ARCH").unwrap()
    );
    println!(
        "cargo:rustc-env=TARGET_OS={}",
        env::var("CARGO_CFG_TARGET_OS").unwrap()
    );
    // Rerun if init directory changes
    println!("cargo:rerun-if-changed=init");

    // DEVENV_IS_RELEASE can be set explicitly (e.g. in package.nix or CI)
    // to mark a build as a release. In local builds, auto-detect from git tags.
    println!("cargo:rerun-if-env-changed=DEVENV_IS_RELEASE");
    let is_release = env::var("DEVENV_IS_RELEASE").unwrap_or_default();
    if is_release.is_empty() {
        let on_tag = Command::new("git")
            .args(["describe", "--tags", "--exact-match", "HEAD"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        println!("cargo:rustc-env=DEVENV_IS_RELEASE={on_tag}");
    } else {
        println!("cargo:rustc-env=DEVENV_IS_RELEASE={is_release}");
    }

    println!("cargo:rerun-if-env-changed=DEVENV_GIT_REV");
    let git_rev = env::var("DEVENV_GIT_REV").unwrap_or_default();

    if !git_rev.is_empty() {
        // Flake build:
        // DEVENV_GIT_REV is set in package.nix from the flake's self.shortRev
        // or self.dirtyShortRev (which appends "-dirty").
        let dirty = git_rev.ends_with("-dirty");
        let sha = git_rev.trim_end_matches("-dirty");
        println!("cargo:rustc-env=VERGEN_GIT_SHA={sha}");
        println!("cargo:rustc-env=VERGEN_GIT_DIRTY={dirty}");
    } else {
        // Local cargo build or nixpkgs tarball (no DEVENV_GIT_REV):
        // Let vergen query git.
        // Idempotent + quiet mode means vergen falls back to
        // VERGEN_IDEMPOTENT_OUTPUT without warnings when git is unavailable.
        let gitcl = GitclBuilder::default().sha(true).dirty(true).build()?;
        Emitter::default()
            .idempotent()
            .quiet()
            .add_instructions(&gitcl)?
            .emit()?;

        // Rerun when git state changes
        println!("cargo:rerun-if-changed=.git/HEAD");
        println!("cargo:rerun-if-changed=.git/refs/tags");
    }

    Ok(())
}
