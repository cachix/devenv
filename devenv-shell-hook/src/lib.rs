pub mod detector;
pub mod env;
pub mod shell;

pub use env::{ActivationResult, ShellState};
pub use shell::{ShellHook, ShellType};

use miette::{IntoDiagnostic, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ShellHookConfig {
    pub cache_dir: std::path::PathBuf,
    pub state_file: std::path::PathBuf,
    pub db_path: std::path::PathBuf,
}

impl Default for ShellHookConfig {
    fn default() -> Self {
        let base_dirs = xdg::BaseDirectories::with_prefix("devenv")
            .expect("Failed to determine XDG directories");

        let cache_dir = base_dirs.get_cache_home();
        let state_file = base_dirs
            .place_data_file("shell-state.json")
            .expect("Failed to create state file path");
        let db_path = base_dirs
            .place_cache_file("eval-cache.db")
            .expect("Failed to create db path");

        Self {
            cache_dir,
            state_file,
            db_path,
        }
    }
}

/// Shell hook manager that can be embedded in devenv
pub struct ShellHookManager {
    config: ShellHookConfig,
    db_pool: sqlx::SqlitePool,
    state: Arc<RwLock<ShellState>>,
}

impl ShellHookManager {
    pub async fn new() -> Result<Self> {
        let config = ShellHookConfig::default();
        let db_pool = initialize_db(&config.db_path).await?;
        let state = Arc::new(RwLock::new(ShellState::load(&config.state_file).await?));

        Ok(Self {
            config,
            db_pool,
            state,
        })
    }

    /// Check if environment needs updating for the given directory
    pub async fn check_environment(
        &self,
        pwd: &Path,
        options: &[String],
    ) -> Result<EnvironmentStatus> {
        let project = detector::find_devenv_root(pwd)?;
        let state = self.state.read().await;

        if let Some(project_root) = project {
            // Check if we need to activate or update
            let current_hash = env::get_env_hash(&project_root, options, &self.db_pool).await?;

            if state.active_project.as_ref() == Some(&project_root)
                && state.last_env_hash.as_ref() == Some(&current_hash)
            {
                Ok(EnvironmentStatus::Current)
            } else {
                Ok(EnvironmentStatus::NeedsUpdate {
                    project_root,
                    current_hash,
                })
            }
        } else if state.active_project.is_some() {
            Ok(EnvironmentStatus::NeedsDeactivation)
        } else {
            Ok(EnvironmentStatus::NoProject)
        }
    }

    /// Update shell state after successful environment build
    pub async fn update_state(&self, project_root: PathBuf, env_hash: String) -> Result<()> {
        let mut state = self.state.write().await;
        state.active_project = Some(project_root);
        state.last_env_hash = Some(env_hash);
        state.save(&self.config.state_file).await
    }

    /// Clear shell state when leaving a project
    pub async fn clear_state(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.active_project = None;
        state.last_env_hash = None;
        state.save(&self.config.state_file).await
    }

    /// Get the activation script for the current shell
    pub fn get_activation_script(
        &self,
        bash_script: String,
        project_root: &Path,
        cached: bool,
    ) -> String {
        let mut commands = vec![bash_script];

        if cached {
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

        commands.join("\n")
    }

    /// Get the deactivation script
    pub fn get_deactivation_script(&self, project_root: &Path) -> String {
        format!("echo 'Leaving devenv: {}' >&2", project_root.display())
    }
}

#[derive(Debug, Clone)]
pub enum EnvironmentStatus {
    /// Environment is current, no action needed
    Current,
    /// Environment needs update
    NeedsUpdate {
        project_root: PathBuf,
        current_hash: String,
    },
    /// Need to deactivate current environment
    NeedsDeactivation,
    /// No project found
    NoProject,
}

pub fn detect_shell() -> ShellType {
    if std::env::var("FISH_VERSION").is_ok() {
        ShellType::Fish
    } else if std::env::var("ZSH_VERSION").is_ok() {
        ShellType::Zsh
    } else if std::env::var("BASH_VERSION").is_ok() {
        ShellType::Bash
    } else {
        ShellType::Bash
    }
}

pub fn init_shell(shell: Option<ShellType>) -> Result<String> {
    let shell = shell.unwrap_or_else(detect_shell);
    let hook = ShellHook::new(shell);
    Ok(hook.init_script())
}

/// Entry point for the shell hook evaluation
/// This is called by the shell and communicates with the devenv daemon
pub async fn eval_hook(pwd: &Path, options: Vec<String>) -> Result<String> {
    // Wrap errors to avoid displaying them in the shell prompt
    match eval_hook_inner(pwd, options).await {
        Ok(output) => Ok(output),
        Err(e) => {
            // Log error to stderr but don't break the shell
            eprintln!("devenv shell-hook error: {}", e);
            Ok(String::new())
        }
    }
}

async fn eval_hook_inner(pwd: &Path, options: Vec<String>) -> Result<String> {
    // TODO: Instead of doing the work here, communicate with devenv daemon
    // For now, keep the existing implementation
    let config = ShellHookConfig::default();
    let db_pool = initialize_db(&config.db_path).await?;
    let project = detector::find_devenv_root(pwd)?;
    let state = env::ShellState::load(&config.state_file).await?;

    let result = if let Some(project_root) = project {
        env::activate_project(&project_root, &config, &options, &db_pool).await?
    } else if state.active_project.is_some() {
        env::ActivationResult {
            commands: env::deactivate_project(&config).await?,
            cache_hit: false,
        }
    } else {
        return Ok("".to_string());
    };

    Ok(result.commands.join("\n"))
}

async fn initialize_db(db_path: &Path) -> Result<sqlx::SqlitePool> {
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await.into_diagnostic()?;
    }

    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::SqlitePool::connect(&db_url).await.into_diagnostic()?;

    sqlx::migrate!("../devenv-eval-cache/migrations")
        .run(&pool)
        .await
        .into_diagnostic()?;

    Ok(pool)
}

/// Parse shell-hook options from command line arguments
pub fn parse_options(args: &[String]) -> (Option<&Path>, Vec<String>) {
    let mut pwd = None;
    let mut options = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--pwd" => {
                if i + 1 < args.len() {
                    pwd = Some(Path::new(&args[i + 1]));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            arg => {
                options.push(arg.to_string());
                i += 1;
            }
        }
    }

    (pwd, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_detection() {
        let shell = detect_shell();
        assert!(matches!(
            shell,
            ShellType::Bash | ShellType::Zsh | ShellType::Fish
        ));
    }

    #[test]
    fn test_init_script_generation() {
        let bash_script = init_shell(Some(ShellType::Bash)).unwrap();
        assert!(bash_script.contains("__devenv_hook"));
        assert!(bash_script.contains("PROMPT_COMMAND"));

        let zsh_script = init_shell(Some(ShellType::Zsh)).unwrap();
        assert!(zsh_script.contains("precmd_functions"));

        let fish_script = init_shell(Some(ShellType::Fish)).unwrap();
        assert!(fish_script.contains("--on-variable PWD"));
    }

    #[test]
    fn test_parse_options() {
        let args = vec![
            "--pwd".to_string(),
            "/home/user/project".to_string(),
            "--impure".to_string(),
            "--system".to_string(),
            "x86_64-linux".to_string(),
        ];

        let (pwd, options) = parse_options(&args);
        assert_eq!(pwd, Some(Path::new("/home/user/project")));
        assert_eq!(options, vec!["--impure", "--system", "x86_64-linux"]);
    }
}
