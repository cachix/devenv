//! `devenv hook`/`allow`/`revoke`/`hook-should-activate`: native shell hook
//! commands for auto-activation on directory change.
//!
//! Provides:
//! - Trust database management (allow/revoke)
//! - Project discovery and activation check (should_activate)
//! - Shell hook script output (bash/zsh/fish/nushell)

use crate::cli::HookShell;
use miette::{IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

// ---- Hook scripts ----
//
// Generated at build time. See `build.rs`.

const HOOK_BASH: &str = include_str!(concat!(env!("OUT_DIR"), "/hook.sh"));
const HOOK_ZSH: &str = include_str!(concat!(env!("OUT_DIR"), "/hook.zsh"));
const HOOK_FISH: &str = include_str!(concat!(env!("OUT_DIR"), "/hook.fish"));
const HOOK_NU: &str = include_str!(concat!(env!("OUT_DIR"), "/hook.nu"));

// ---- CLI entry points ----

/// The hook script for `shell`.
///
/// The hook runs `devenv hook-should-activate` on every prompt — cheap with the
/// static binary — so there's no per-directory activation cache to invalidate;
/// `devenv allow`/`revoke` take effect on the next prompt automatically.
fn hook_script(shell: &HookShell) -> &'static str {
    match shell {
        HookShell::Bash => HOOK_BASH,
        HookShell::Zsh => HOOK_ZSH,
        HookShell::Fish => HOOK_FISH,
        HookShell::Nu => HOOK_NU,
    }
}

/// Print the shell hook script for `shell` to stdout.
pub fn print(shell: &HookShell) {
    let script = hook_script(shell);
    // `BrokenPipe` (e.g. `devenv hook fish | source` after `source` finishes
    // reading) is a normal lifecycle event, not a panic.
    let mut out = io::stdout().lock();
    if let Err(e) = out.write_all(script.as_bytes()).and_then(|()| out.flush())
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("devenv: failed to write hook script: {e}");
        std::process::exit(1);
    }
}

/// Trust the current working directory for auto-activation.
///
/// `from` persists an out-of-tree source (the `--from` value), so later commands
/// in this directory resolve their devenv.nix from it without a local file.
pub fn allow(from: Option<&str>) -> Result<()> {
    allow_path(&env::current_dir().into_diagnostic()?, from)
}

/// Revoke trust for the current working directory.
pub fn revoke() -> Result<()> {
    revoke_path(&env::current_dir().into_diagnostic()?)
}

/// Check whether the shell hook should activate devenv in the current directory.
///
/// Prints the project directory on stdout when activation is wanted, exits with
/// code 2 when a project is found but not trusted, and is silent otherwise.
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
    let abs_path = fs::canonicalize(path).into_diagnostic()?;
    abs_path
        .to_str()
        .ok_or_else(|| miette::miette!("Path is not valid UTF-8: {}", abs_path.display()))
        .map(String::from)
}

/// A trusted directory and, optionally, the out-of-tree source its devenv.nix
/// comes from (the `--from` value passed to `devenv allow`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TrustEntry {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<String>,
}

/// Extract the project path from a legacy (non-JSON) trust entry.
///
/// Plain format: `<path>` (one path per line).
/// Legacy format: `<64-char-hash>:<path>` — the hash is stripped for backward
/// compatibility with trust databases written by older devenv versions.
fn entry_path(entry: &str) -> &str {
    if entry.len() > 65 && entry.as_bytes()[64] == b':' {
        &entry[65..]
    } else {
        entry
    }
}

/// Parse one line of the trust database. Lines starting with `{` are the
/// current JSONL format; anything else is a legacy plain/hash path line.
fn parse_entry(line: &str) -> Option<TrustEntry> {
    if line.starts_with('{') {
        serde_json::from_str(line).ok()
    } else {
        Some(TrustEntry {
            path: entry_path(line).to_string(),
            from: None,
        })
    }
}

fn remove_path_entries(entries: &mut Vec<TrustEntry>, abs_str: &str) {
    entries.retain(|e| e.path != abs_str);
}

// ---- Trust database ----

fn devenv_home() -> Result<PathBuf> {
    if let Ok(home) = env::var("DEVENV_HOME") {
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

fn read_trust_entries(db_path: &Path) -> Result<Vec<TrustEntry>> {
    match fs::read_to_string(db_path) {
        Ok(content) => Ok(content
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(parse_entry)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).into_diagnostic(),
    }
}

fn write_trust_entries(db_path: &Path, entries: &[TrustEntry]) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    let mut content = String::new();
    for entry in entries {
        content.push_str(&serde_json::to_string(entry).into_diagnostic()?);
        content.push('\n');
    }
    fs::write(db_path, content).into_diagnostic()
}

fn find_trust_entry(abs_str: &str) -> Result<Option<TrustEntry>> {
    let db_path = trust_db_path()?;
    let entries = read_trust_entries(&db_path)?;
    Ok(entries.into_iter().find(|e| e.path == abs_str))
}

fn is_trusted(abs_str: &str) -> Result<bool> {
    Ok(find_trust_entry(abs_str)?.is_some())
}

/// Walk up from `dir` to the nearest ancestor bound to an out-of-tree source via
/// `devenv allow --from`, returning its trust entry (whose `from` is always set).
/// In-tree trust entries (no `from`) are skipped.
fn nearest_from(dir: &Path) -> Result<Option<TrustEntry>> {
    let entries = read_trust_entries(&trust_db_path()?)?;
    if entries.is_empty() {
        return Ok(None);
    }
    // Canonicalize once; ancestors of a canonical path are themselves canonical,
    // so we can match by string without re-canonicalizing at each level.
    let mut current = fs::canonicalize(dir).into_diagnostic()?;
    loop {
        if let Some(abs) = current.to_str()
            && let Some(entry) = entries.iter().find(|e| e.path == abs && e.from.is_some())
        {
            return Ok(Some(entry.clone()));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

/// Look up the out-of-tree `--from` source bound to `dir` (or the nearest bound
/// ancestor) via `devenv allow --from`. Returns `None` when no ancestor is bound.
pub fn trusted_from(dir: &Path) -> Result<Option<String>> {
    Ok(nearest_from(dir)?.and_then(|entry| entry.from))
}

/// Trust `project_dir`, optionally binding it to an out-of-tree `--from` source.
///
/// When `from` is `None` a local `devenv.nix` must exist. When `from` is set the
/// directory needs no local `devenv.nix`; the module comes from the source.
fn allow_path(project_dir: &Path, from: Option<&str>) -> Result<()> {
    let abs_str = canonical_str(project_dir)?;

    if from.is_none() && !project_dir.join("devenv.nix").exists() {
        miette::bail!("No devenv.nix found in {abs_str}");
    }

    let db_path = trust_db_path()?;
    let mut entries = read_trust_entries(&db_path)?;
    remove_path_entries(&mut entries, &abs_str);
    entries.push(TrustEntry {
        path: abs_str.clone(),
        from: from.map(String::from),
    });
    write_trust_entries(&db_path, &entries)?;

    match from {
        Some(from) => eprintln!("devenv: allowed {abs_str} from {from}"),
        None => eprintln!("devenv: allowed {abs_str}"),
    }
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
        write_trust_entries(&db_path, &entries)?;
        eprintln!("devenv: revoked {abs_str}");
    }

    Ok(())
}

// ---- Project discovery ----

/// Result of checking whether the hook should activate.
enum ActivationCheck {
    /// Activate devenv in this project directory.
    Activate(String),
    /// No project here; nothing to activate.
    Skip,
    /// Project found but not trusted; should retry on next prompt.
    Untrusted,
}

fn check_activation() -> Result<ActivationCheck> {
    let cwd = env::current_dir().into_diagnostic()?;

    // A local devenv.nix takes priority; its directory must be trusted.
    if let Some(project_dir) = devenv_core::paths::find_project_root(&cwd) {
        let abs_str = canonical_str(&project_dir)?;
        if !is_trusted(&abs_str)? {
            eprintln!(
                "devenv: {abs_str} is not allowed. Run 'devenv allow' to trust this directory."
            );
            return Ok(ActivationCheck::Untrusted);
        }
        return Ok(ActivationCheck::Activate(abs_str));
    }

    // No local project: a directory bound out-of-tree via `allow --from` is
    // trusted by that binding itself, so activate it directly.
    if let Some(entry) = nearest_from(&cwd)? {
        return Ok(ActivationCheck::Activate(entry.path));
    }

    Ok(ActivationCheck::Skip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Set `DEVENV_HOME` for a test. Safe because cargo nextest runs each test
    /// in its own process, so there is no concurrent env access.
    fn set_devenv_home(dir: &Path) {
        unsafe { env::set_var("DEVENV_HOME", dir) };
    }

    fn unset_devenv_home() {
        unsafe { env::remove_var("DEVENV_HOME") };
    }

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
    fn test_hook_script_runs_should_activate() {
        // The hook calls `hook-should-activate` every prompt; there's no trust-DB
        // path baked into the script anymore (no caching to invalidate).
        for shell in [
            HookShell::Bash,
            HookShell::Zsh,
            HookShell::Fish,
            HookShell::Nu,
        ] {
            let script = hook_script(&shell);
            assert!(
                !script.contains("@DEVENV_TRUST_DB@"),
                "{shell:?} hook left an unsubstituted placeholder"
            );
            assert!(
                script.contains("hook-should-activate"),
                "{shell:?} hook does not call hook-should-activate"
            );
        }
    }

    #[test]
    fn test_allow_and_revoke() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.nix"), "{ }\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        allow_path(&project, None).unwrap();

        let db_path = devenv_home_dir.join("allowed");
        let content = fs::read_to_string(&db_path).unwrap();
        let canonical = canonical_str(&project).unwrap();
        assert!(content.contains(&canonical));

        revoke_path(&project).unwrap();

        let content = fs::read_to_string(&db_path).unwrap();
        assert!(!content.contains(&canonical));

        unset_devenv_home();
    }

    #[test]
    fn test_is_trusted() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.nix"), "{ }\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        let abs_str = canonical_str(&project).unwrap();

        // Not trusted yet
        assert!(!is_trusted(&abs_str).unwrap());

        // Allow and verify
        allow_path(&project, None).unwrap();
        assert!(is_trusted(&abs_str).unwrap());

        // Changing devenv.nix should not invalidate trust
        fs::write(project.join("devenv.nix"), "{ pkgs, ... }: { }\n").unwrap();
        assert!(is_trusted(&abs_str).unwrap());

        unset_devenv_home();
    }

    #[test]
    fn test_legacy_hash_entries_are_trusted() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.nix"), "{ }\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        let abs_str = canonical_str(&project).unwrap();

        // Seed the trust DB with a legacy `<hash>:<path>` entry.
        fs::create_dir_all(&devenv_home_dir).unwrap();
        let legacy_entry = format!("{}:{}\n", "a".repeat(64), abs_str);
        fs::write(devenv_home_dir.join("allowed"), legacy_entry).unwrap();

        assert!(is_trusted(&abs_str).unwrap());

        unset_devenv_home();
    }

    #[test]
    fn test_allow_persists_from_as_jsonl() {
        let dir = TempDir::new().unwrap();
        // No local devenv.nix: an out-of-tree source is bound instead.
        let project = dir.path().join("out-of-tree");
        fs::create_dir_all(&project).unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        allow_path(&project, Some("github:cachix/devenv")).unwrap();

        let abs_str = canonical_str(&project).unwrap();
        let entry = find_trust_entry(&abs_str).unwrap().unwrap();
        assert_eq!(entry.from.as_deref(), Some("github:cachix/devenv"));

        // Each line is a self-contained JSON object.
        let content = fs::read_to_string(devenv_home_dir.join("allowed")).unwrap();
        let line = content.lines().next().unwrap();
        assert_eq!(parse_entry(line).unwrap(), entry);

        unset_devenv_home();
    }

    #[test]
    fn test_bound_out_of_tree_dir_activates() {
        let dir = TempDir::new().unwrap();
        // An out-of-tree work dir: no local devenv.nix.
        let work = dir.path().join("work");
        fs::create_dir_all(&work).unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        // Unbound: nothing to activate.
        assert!(nearest_from(&work).unwrap().is_none());

        // Bind it to an out-of-tree source via `allow --from`.
        allow_path(&work, Some("github:cachix/devenv")).unwrap();

        let abs = canonical_str(&work).unwrap();
        let entry = nearest_from(&work).unwrap().unwrap();
        assert_eq!(entry.path, abs);
        assert_eq!(entry.from.as_deref(), Some("github:cachix/devenv"));

        // A subdirectory of the bound dir resolves to the same binding.
        let sub = work.join("nested");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(nearest_from(&sub).unwrap().unwrap().path, abs);

        unset_devenv_home();
    }

    #[test]
    fn test_in_tree_trust_is_not_a_binding() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.nix"), "{ }\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        // Plain in-tree trust (no --from) is not an out-of-tree binding.
        allow_path(&project, None).unwrap();
        assert!(nearest_from(&project).unwrap().is_none());

        unset_devenv_home();
    }

    #[test]
    fn test_legacy_plain_entries_have_no_from() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("myproject");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("devenv.nix"), "{ }\n").unwrap();

        let devenv_home_dir = dir.path().join("devenv-home");
        set_devenv_home(&devenv_home_dir);

        let abs_str = canonical_str(&project).unwrap();

        // Seed the trust DB with a legacy plain-path line.
        fs::create_dir_all(&devenv_home_dir).unwrap();
        fs::write(devenv_home_dir.join("allowed"), format!("{abs_str}\n")).unwrap();

        let entry = find_trust_entry(&abs_str).unwrap().unwrap();
        assert_eq!(entry.path, abs_str);
        assert_eq!(entry.from, None);

        unset_devenv_home();
    }
}
