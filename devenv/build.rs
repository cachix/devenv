use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    println!("cargo:rustc-env=TARGET_ARCH={arch}");
    println!("cargo:rustc-env=TARGET_OS={os}");

    // Rerun if init directory changes
    println!("cargo:rerun-if-changed=init");

    assemble_shell_hooks()?;

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

    let (sha, dirty) = if !git_rev.is_empty() {
        // Flake build:
        // DEVENV_GIT_REV is set in package.nix from the flake's self.shortRev
        // or self.dirtyShortRev (which appends "-dirty").
        let dirty = git_rev.ends_with("-dirty");
        let sha = git_rev.trim_end_matches("-dirty").to_string();
        (sha, dirty)
    } else {
        // Local cargo build or nixpkgs tarball: query git directly.
        // Falls back to empty sha when git is unavailable (tarball builds).
        println!("cargo:rerun-if-changed=.git/HEAD");
        println!("cargo:rerun-if-changed=.git/refs/tags");

        let sha = Command::new("git")
            .args(["rev-parse", "--short=8", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let dirty = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        (sha, dirty)
    };

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let system = match os.as_str() {
        "macos" => format!("{arch}-darwin"),
        other => format!("{arch}-{other}"),
    };
    let version_string = if sha.is_empty() {
        format!("{version} ({system})")
    } else if dirty {
        format!("{version}+{sha}-dirty ({system})")
    } else {
        format!("{version}+{sha} ({system})")
    };
    println!("cargo:rustc-env=DEVENV_VERSION_STRING={version_string}");

    Ok(())
}

// ---- Shell hook assembly ----
//
// Produces a standalone hook script per shell in OUT_DIR:
//   - hook-bash.sh / hook-zsh.sh: `hooks/posix.sh` + the per-shell register snippet
//   - hook-fish.fish / hook-nu.nu: copied verbatim from `hooks/`
// src/commands/hook.rs `include_str!`s the OUT_DIR artifacts uniformly.
fn assemble_shell_hooks() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let posix = fs::read_to_string("hooks/hook.posix.sh")?;
    println!("cargo:rerun-if-changed=hooks/hook.posix.sh");

    for (register_path, out_name) in [
        ("hooks/hook.bash-register.sh", "hook.sh"),
        ("hooks/hook.zsh-register.zsh", "hook.zsh"),
    ] {
        let register = fs::read_to_string(register_path)?;
        fs::write(out_dir.join(out_name), format!("{posix}\n{register}"))?;
        println!("cargo:rerun-if-changed={register_path}");
    }

    for name in ["hook.fish", "hook.nu"] {
        let src = format!("hooks/{name}");
        fs::copy(&src, out_dir.join(name))?;
        println!("cargo:rerun-if-changed={src}");
    }

    Ok(())
}
