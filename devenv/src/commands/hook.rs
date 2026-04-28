//! `devenv hook`/`allow`/`revoke`/`hook-should-activate`: native shell hook
//! commands for auto-activation on directory change.
//!
//! Provides:
//! - Trust database management (allow/revoke)
//! - Project discovery and activation check (should_activate)
//! - Shell hook script output (bash/zsh/fish/nushell)

use crate::cli::HookShell;
use miette::{IntoDiagnostic, Result};
use std::path::{Path, PathBuf};

// ---- CLI entry points ----

pub fn print(shell: &HookShell) {
    match shell {
        HookShell::Bash => print!("{HOOK_POSIX}\n{HOOK_BASH_REGISTER}"),
        HookShell::Zsh => print!("{HOOK_POSIX}\n{HOOK_ZSH_REGISTER}"),
        HookShell::Fish => print!("{}", include_str!("../../hook-fish.fish")),
        HookShell::Nu => print!("{}", include_str!("../../hook-nu.nu")),
    }
}

pub fn allow() -> Result<()> {
    allow_path(&std::env::current_dir().into_diagnostic()?)
}

pub fn revoke() -> Result<()> {
    revoke_path(&std::env::current_dir().into_diagnostic()?)
}

pub fn should_activate() -> Result<()> {
    match check_activation()? {
        ActivationCheck::Activate(dir) => println!("{dir}"),
        ActivationCheck::Skip => {}
        ActivationCheck::Untrusted => std::process::exit(2),
    }
    Ok(())
}

// ---- Helpers ----

fn canonical_str(path: &Path) -> Result<String> {
    let abs_path = std::fs::canonicalize(path).into_diagnostic()?;
    abs_path
        .to_str()
        .ok_or_else(|| miette::miette!("Path is not valid UTF-8: {}", abs_path.display()))
        .map(String::from)
}

/// Extract the project path from a trust entry.
///
/// Current format: `<path>` (one path per line).
/// Legacy format: `<64-char-hash>:<path>` — the hash is stripped for backward
/// compatibility with trust databases written by older devenv versions.
fn entry_path(entry: &str) -> &str {
    if entry.len() > 65 && entry.as_bytes()[64] == b':' {
        &entry[65..]
    } else {
        entry
    }
}

fn remove_path_entries(entries: &mut Vec<String>, abs_str: &str) {
    entries.retain(|e| entry_path(e) != abs_str);
}

// ---- Trust database ----

fn devenv_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("DEVENV_HOME") {
        return Ok(PathBuf::from(home));
    }
    xdg::BaseDirectories::with_prefix("devenv")
        .get_data_home()
        .ok_or_else(|| {
            miette::miette!("Could not determine devenv data directory. Set DEVENV_HOME or HOME.")
        })
}

fn trust_db_path() -> Result<PathBuf> {
    Ok(devenv_home()?.join("allowed"))
}

fn read_trust_entries(db_path: &Path) -> Result<Vec<String>> {
    match std::fs::read_to_string(db_path) {
        Ok(content) => Ok(content
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).into_diagnostic(),
    }
}

fn is_trusted(abs_str: &str) -> Result<bool> {
    let db_path = trust_db_path()?;
    let entries = read_trust_entries(&db_path)?;
    Ok(entries.iter().any(|e| entry_path(e) == abs_str))
}

fn allow_path(project_dir: &Path) -> Result<()> {
    let abs_str = canonical_str(project_dir)?;

    if !project_dir.join("devenv.yaml").exists() {
        miette::bail!("No devenv.yaml found in {abs_str}");
    }

    let db_path = trust_db_path()?;

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }

    let mut entries = read_trust_entries(&db_path)?;
    remove_path_entries(&mut entries, &abs_str);
    entries.push(abs_str.clone());

    let content = entries.join("\n") + "\n";
    std::fs::write(&db_path, content).into_diagnostic()?;

    eprintln!("devenv: allowed {abs_str}");
    Ok(())
}

fn revoke_path(project_dir: &Path) -> Result<()> {
    let db_path = trust_db_path()?;
    let abs_str = canonical_str(project_dir)?;

    let mut entries = read_trust_entries(&db_path)?;
    let before = entries.len();
    remove_path_entries(&mut entries, &abs_str);

    if entries.len() == before {
        eprintln!("devenv: {abs_str} was not in the allow list");
    } else {
        let content = if entries.is_empty() {
            String::new()
        } else {
            entries.join("\n") + "\n"
        };
        std::fs::write(&db_path, content).into_diagnostic()?;
        eprintln!("devenv: revoked {abs_str}");
    }

    Ok(())
}

// ---- Project discovery ----

fn find_project(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("devenv.yaml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Result of checking whether the hook should activate.
enum ActivationCheck {
    /// Activate devenv in this project directory.
    Activate(String),
    /// No project found or already activated; safe to cache and skip future checks.
    Skip,
    /// Project found but not trusted; should retry on next prompt.
    Untrusted,
}

/// Check if the hook should activate devenv in the current directory.
fn check_activation() -> Result<ActivationCheck> {
    let cwd = std::env::current_dir().into_diagnostic()?;

    let project_dir = match find_project(&cwd) {
        Some(dir) => dir,
        None => return Ok(ActivationCheck::Skip),
    };

    let abs_str = canonical_str(&project_dir)?;

    if !is_trusted(&abs_str)? {
        eprintln!("devenv: {abs_str} is not allowed. Run 'devenv allow' to trust this directory.");
        return Ok(ActivationCheck::Untrusted);
    }

    Ok(ActivationCheck::Activate(abs_str))
}

// ---- Hook script output ----

const HOOK_POSIX: &str = include_str!("../../hook-posix.sh");

const HOOK_BASH_REGISTER: &str = r#"# Register hook
if [[ -z "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="_devenv_hook"
else
    PROMPT_COMMAND="_devenv_hook;${PROMPT_COMMAND}"
fi
"#;

const HOOK_ZSH_REGISTER: &str = r#"# Register hook via precmd
typeset -ag precmd_functions
if (( ! ${precmd_functions[(I)_devenv_hook]} )); then
    precmd_functions=(_devenv_hook $precmd_functions)
fi
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_entry_path_current_format() {
        assert_eq!(entry_path("/home/me/project"), "/home/me/project");
    }

    #[test]
    fn test_entry_path_legacy_hash_format() {
        // Legacy format: <64-char hash>:<path>
        let hash = "a".repeat(64);
        let entry = format!("{hash}:/home/me/project");
        assert_eq!(entry_path(&entry), "/home/me/project");
    }

    #[test]
    fn test_find_project() {
        let dir = TempDir::new().unwrap();

        // No devenv.yaml
        assert!(find_project(dir.path()).is_none());

        // Add devenv.yaml
        fs::write(dir.path().join("devenv.yaml"), "inputs:\n").unwrap();
        assert_eq!(find_project(dir.path()), Some(dir.path().to_path_buf()));

        // Subdirectory should find parent's devenv.yaml
        let sub = dir.path().join("sub").join("deep");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(find_project(&sub), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn test_allow_and_revoke() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.yaml"), "inputs:\n  nixpkgs:\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        // SAFETY: test runs single-threaded (cargo nextest runs each test in its own process)
        unsafe { std::env::set_var("DEVENV_HOME", &devenv_home_dir) };

        allow_path(&project).unwrap();

        let db_path = devenv_home_dir.join("allowed");
        let content = fs::read_to_string(&db_path).unwrap();
        let canonical = canonical_str(&project).unwrap();
        assert!(content.contains(&canonical));

        revoke_path(&project).unwrap();

        let content = fs::read_to_string(&db_path).unwrap();
        assert!(!content.contains(&canonical));

        unsafe { std::env::remove_var("DEVENV_HOME") };
    }

    #[test]
    fn test_is_trusted() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.yaml"), "inputs:\n  nixpkgs:\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        unsafe { std::env::set_var("DEVENV_HOME", &devenv_home_dir) };

        let abs_str = canonical_str(&project).unwrap();

        // Not trusted yet
        assert!(!is_trusted(&abs_str).unwrap());

        // Allow and verify
        allow_path(&project).unwrap();
        assert!(is_trusted(&abs_str).unwrap());

        // Changing devenv.yaml should not invalidate trust
        fs::write(project.join("devenv.yaml"), "inputs:\n  nixpkgs:\n  new:\n").unwrap();
        assert!(is_trusted(&abs_str).unwrap());

        unsafe { std::env::remove_var("DEVENV_HOME") };
    }

    #[test]
    fn test_legacy_hash_entries_are_trusted() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.yaml"), "inputs:\n  nixpkgs:\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        unsafe { std::env::set_var("DEVENV_HOME", &devenv_home_dir) };

        let abs_str = canonical_str(&project).unwrap();

        // Seed the trust DB with a legacy `<hash>:<path>` entry.
        fs::create_dir_all(&devenv_home_dir).unwrap();
        let legacy_entry = format!("{}:{}\n", "a".repeat(64), abs_str);
        fs::write(devenv_home_dir.join("allowed"), legacy_entry).unwrap();

        assert!(is_trusted(&abs_str).unwrap());

        unsafe { std::env::remove_var("DEVENV_HOME") };
    }
}
