use miette::{bail, IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tokio::fs;

use crate::ShellHookConfig;
use devenv_eval_cache::CachedCommand;

/// Minimal state tracking for the shell integration
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ShellState {
    /// Currently active project root
    pub active_project: Option<PathBuf>,
    /// Hash of the last activated environment (from eval cache)
    pub last_env_hash: Option<String>,
}

impl ShellState {
    pub async fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path).await.into_diagnostic()?;
            Ok(serde_json::from_str(&content).into_diagnostic()?)
        } else {
            Ok(Self::default())
        }
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.into_diagnostic()?;
        }

        // Write atomically
        let temp_path = path.with_extension("tmp");
        let content = serde_json::to_string_pretty(self).into_diagnostic()?;
        fs::write(&temp_path, content).await.into_diagnostic()?;
        fs::rename(&temp_path, path).await.into_diagnostic()?;

        Ok(())
    }
}

/// Result of environment activation
pub struct ActivationResult {
    /// Shell commands to execute
    pub commands: Vec<String>,
    /// Whether this was a cache hit
    pub cache_hit: bool,
}

/// Get a hash of the current environment to detect changes
pub async fn get_env_hash(
    project_root: &Path,
    options: &[String],
    db_pool: &sqlx::SqlitePool,
) -> Result<String> {
    // Build a minimal command that just evaluates a constant
    // This will give us the same cache key as the real environment
    let mut cmd = std::process::Command::new("devenv");
    cmd.args(&["eval", "\"cache-check\""])
        .current_dir(project_root);

    // Add user-provided options
    for opt in options {
        cmd.arg(opt);
    }

    // Use devenv-eval-cache to get the cache metadata
    let cached_cmd = CachedCommand::new(db_pool);
    let output = cached_cmd.output(&mut cmd).await.into_diagnostic()?;

    // Create a hash from all the inputs that affect the environment
    use devenv_cache_core::compute_string_hash;
    let mut hash_input = String::new();

    // Include all file paths and their modification times
    use devenv_eval_cache::command::{FileInputDesc, Input};
    for input in &output.inputs {
        match input {
            Input::File(FileInputDesc {
                path, modified_at, ..
            }) => {
                hash_input.push_str(&path.to_string_lossy());
                hash_input.push(':');
                // Convert SystemTime to a string representation
                if let Ok(duration) = modified_at.duration_since(UNIX_EPOCH) {
                    hash_input.push_str(&duration.as_secs().to_string());
                } else {
                    hash_input.push_str("0");
                }
                hash_input.push('\n');
            }
            Input::Env(env) => {
                hash_input.push_str("ENV:");
                hash_input.push_str(&env.name);
                hash_input.push('=');
                if let Some(ref hash) = env.content_hash {
                    hash_input.push_str(hash);
                }
                hash_input.push('\n');
            }
        }
    }

    // Include options in the hash
    for opt in options {
        hash_input.push_str("OPT:");
        hash_input.push_str(opt);
        hash_input.push('\n');
    }

    Ok(compute_string_hash(&hash_input))
}

pub async fn activate_project(
    project_root: &Path,
    config: &ShellHookConfig,
    options: &[String],
    db_pool: &sqlx::SqlitePool,
) -> Result<ActivationResult> {
    let mut commands = vec![];
    let mut state = ShellState::load(&config.state_file).await?;

    // Get current environment hash
    let current_hash = get_env_hash(project_root, options, db_pool).await?;

    // Check if we're already in the right environment
    if state.active_project.as_ref().map(|p| p.as_path()) == Some(project_root)
        && state.last_env_hash.as_ref() == Some(&current_hash)
    {
        // Environment is already active and unchanged
        return Ok(ActivationResult {
            commands: vec![],
            cache_hit: true,
        });
    }

    // Check if we need to deactivate a different project
    if let Some(ref old_project) = state.active_project {
        if old_project != project_root {
            commands.push(format!("# Leaving {}", old_project.display()));
        }
    }

    // Build the devenv command with proper options
    let mut cmd = std::process::Command::new("devenv");
    cmd.args(&["print-dev-env"]).current_dir(project_root);

    // Add user-provided options
    for opt in options {
        cmd.arg(opt);
    }

    // Use devenv-eval-cache for caching
    let cached_cmd = CachedCommand::new(db_pool);
    let output = cached_cmd.output(&mut cmd).await.into_diagnostic()?;

    if !output.status.success() {
        bail!(
            "Failed to build environment: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // The output is a bash script that we need to source
    // We'll output it directly for the shell to eval
    let bash_script = String::from_utf8(output.stdout).into_diagnostic()?;
    commands.push(bash_script);

    // Update state
    state.active_project = Some(project_root.to_path_buf());
    state.last_env_hash = Some(current_hash);
    state.save(&config.state_file).await?;

    // Add status message
    if output.cache_hit {
        commands.push(format!(
            "echo 'Devenv activated for {} (cached)' >&2",
            project_root.display()
        ));
    } else {
        commands.push(format!(
            "echo 'Devenv activated for {}' >&2",
            project_root.display()
        ));
    }

    Ok(ActivationResult {
        commands,
        cache_hit: output.cache_hit,
    })
}

pub async fn deactivate_project(config: &ShellHookConfig) -> Result<Vec<String>> {
    let mut commands = vec![];

    // Clear the state
    let mut state = ShellState::load(&config.state_file).await?;
    if let Some(project) = &state.active_project {
        commands.push(format!("echo 'Leaving devenv: {}' >&2", project.display()));
    }

    state.active_project = None;
    state.last_env_hash = None;
    state.save(&config.state_file).await?;

    Ok(commands)
}
