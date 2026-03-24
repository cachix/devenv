//! Native shell hook support for auto-activation on directory change.
//!
//! Provides:
//! - Trust database management (allow/revoke)
//! - Project discovery and activation check (should_activate)
//! - Shell hook script output (bash/zsh/fish/nushell)

use crate::cli::HookShell;
use devenv_cache_core::compute_file_hash;
use miette::{IntoDiagnostic, Result};
use std::path::{Path, PathBuf};

// ---- Helpers ----

fn canonical_str(path: &Path) -> Result<String> {
    let abs_path = std::fs::canonicalize(path).into_diagnostic()?;
    abs_path
        .to_str()
        .ok_or_else(|| miette::miette!("Path is not valid UTF-8: {}", abs_path.display()))
        .map(String::from)
}

/// BLAKE3 hex hashes are always 64 characters, so trust entries have the format
/// `<64-char-hash>:<path>`. Parse by fixed offset to handle paths containing colons.
fn parse_trust_entry(entry: &str) -> Option<(&str, &str)> {
    if entry.len() > 65 && entry.as_bytes()[64] == b':' {
        Some((&entry[..64], &entry[65..]))
    } else {
        None
    }
}

fn remove_path_entries(entries: &mut Vec<String>, abs_str: &str) {
    entries.retain(|e| parse_trust_entry(e).is_none_or(|(_, path)| path != abs_str));
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

fn compute_project_hash(project_dir: &Path) -> Result<String> {
    let devenv_yaml = project_dir.join("devenv.yaml");
    compute_file_hash(&devenv_yaml).map_err(|e| miette::miette!("{e}"))
}

fn read_trust_entries(db_path: &Path) -> Result<Vec<String>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(db_path).into_diagnostic()?;
    Ok(content
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

fn is_trusted(abs_str: &str) -> Result<bool> {
    let db_path = trust_db_path()?;
    if !db_path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&db_path).into_diagnostic()?;

    let stored_hash = content
        .lines()
        .filter(|l| !l.is_empty())
        .find_map(|e| parse_trust_entry(e).filter(|(_, path)| *path == abs_str))
        .map(|(hash, _)| hash);

    let Some(stored_hash) = stored_hash else {
        return Ok(false);
    };

    let hash = compute_project_hash(Path::new(abs_str))?;
    Ok(hash == stored_hash)
}

pub fn allow(project_dir: &Path) -> Result<()> {
    let hash = compute_project_hash(project_dir)?;
    let db_path = trust_db_path()?;

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }

    let abs_str = canonical_str(project_dir)?;

    let mut entries = read_trust_entries(&db_path)?;
    remove_path_entries(&mut entries, &abs_str);
    entries.push(format!("{hash}:{abs_str}"));

    let content = entries.join("\n") + "\n";
    std::fs::write(&db_path, content).into_diagnostic()?;

    eprintln!("devenv: allowed {abs_str}");
    Ok(())
}

pub fn revoke(project_dir: &Path) -> Result<()> {
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

/// Check if the hook should activate devenv in the current directory.
///
/// Returns the canonical project directory if activation should proceed,
/// or `None` if no activation is needed.
pub fn should_activate(last_project: Option<&str>) -> Result<Option<String>> {
    let cwd = std::env::current_dir().into_diagnostic()?;

    let project_dir = match find_project(&cwd) {
        Some(dir) => dir,
        None => return Ok(None),
    };

    let abs_str = canonical_str(&project_dir)?;

    if let Some(last) = last_project {
        if last == abs_str {
            return Ok(None);
        }
    }

    if !is_trusted(&abs_str)? {
        eprintln!(
            "devenv: {abs_str} is not allowed or devenv.yaml has changed. Run 'devenv allow' to trust this directory."
        );
        return Ok(None);
    }

    Ok(Some(abs_str))
}

// ---- Hook script output ----

const HOOK_POSIX: &str = include_str!("../hook-posix.sh");

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

pub fn print_hook(shell: &HookShell) {
    match shell {
        HookShell::Bash => print!("{HOOK_POSIX}\n{HOOK_BASH_REGISTER}"),
        HookShell::Zsh => print!("{HOOK_POSIX}\n{HOOK_ZSH_REGISTER}"),
        HookShell::Fish => print!("{}", include_str!("../hook-fish.fish")),
        HookShell::Nu => print!("{}", include_str!("../hook-nu.nu")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_compute_project_hash_deterministic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("devenv.yaml"), "inputs:\n  nixpkgs:\n").unwrap();

        let hash = compute_project_hash(dir.path()).unwrap();
        assert_eq!(hash.len(), 64); // blake3 hex = 64 chars

        let hash2 = compute_project_hash(dir.path()).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_compute_project_hash_changes_on_edit() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("devenv.yaml"), "inputs:\n  nixpkgs:\n").unwrap();
        let hash1 = compute_project_hash(dir.path()).unwrap();

        fs::write(
            dir.path().join("devenv.yaml"),
            "inputs:\n  nixpkgs:\n  foo:\n",
        )
        .unwrap();
        let hash2 = compute_project_hash(dir.path()).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_project_hash_requires_yaml() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("devenv.nix"), "{}").unwrap();
        assert!(compute_project_hash(dir.path()).is_err());
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

        allow(&project).unwrap();

        let db_path = devenv_home_dir.join("allowed");
        let content = fs::read_to_string(&db_path).unwrap();
        let canonical = canonical_str(&project).unwrap();
        assert!(content.contains(&canonical));

        revoke(&project).unwrap();

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
        allow(&project).unwrap();
        assert!(is_trusted(&abs_str).unwrap());

        // Change devenv.yaml -> trust should fail
        fs::write(project.join("devenv.yaml"), "inputs:\n  nixpkgs:\n  new:\n").unwrap();
        assert!(!is_trusted(&abs_str).unwrap());

        unsafe { std::env::remove_var("DEVENV_HOME") };
    }
}
