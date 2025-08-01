use crate::{config, Devenv, DevenvOptions};
use devenv_shell_hook::{EnvironmentStatus, ShellHookManager};
use miette::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{debug, info};

/// Manages shell hook integration with background building
pub struct ShellHookIntegration {
    manager: Arc<ShellHookManager>,
    /// Channel to notify when a build completes
    build_notifier: Arc<Mutex<Option<oneshot::Sender<BuildResult>>>>,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub project_root: std::path::PathBuf,
    pub env_hash: String,
    pub bash_script: String,
    pub success: bool,
    pub error: Option<String>,
}

impl ShellHookIntegration {
    pub async fn new() -> Result<Self> {
        let manager = Arc::new(ShellHookManager::new().await?);
        Ok(Self {
            manager,
            build_notifier: Arc::new(Mutex::new(None)),
        })
    }

    /// Check if environment needs updating and handle it
    pub async fn handle_directory_change(
        &self,
        pwd: &Path,
        options: Vec<String>,
    ) -> Result<String> {
        // Simplified approach: check if we can find a devenv project in the current directory or parents
        let mut current_dir = pwd;
        loop {
            let devenv_nix = current_dir.join("devenv.nix");
            let devenv_yaml = current_dir.join("devenv.yaml");

            if devenv_nix.exists() || devenv_yaml.exists() {
                // Found a devenv project, create a Devenv instance for it
                let config = config::Config::load()?;
                let devenv_options = DevenvOptions {
                    config,
                    devenv_root: Some(current_dir.to_path_buf()),
                    ..Default::default()
                };

                let devenv = Devenv::new(devenv_options).await;
                return devenv.handle_shell_hook(pwd, options).await;
            }

            // Move up to parent directory
            if let Some(parent) = current_dir.parent() {
                current_dir = parent;
            } else {
                break;
            }
        }

        // No devenv project found / we've left a devenv project
        debug!("No devenv project found in {:?}", pwd);

        // Check if we had a previous project (for deactivation)
        let status = self.manager.check_environment(pwd, &options).await?;
        match status {
            EnvironmentStatus::NeedsDeactivation => {
                info!("Deactivating environment");
                self.manager.clear_state().await?;
                Ok("echo 'Left devenv project' >&2".to_string())
            }
            _ => Ok(String::new()),
        }
    }

    /// Set up a notifier for build completion
    pub async fn set_build_notifier(&self, sender: oneshot::Sender<BuildResult>) {
        let mut notifier = self.build_notifier.lock().await;
        *notifier = Some(sender);
    }

    /// Wait for any ongoing build to complete
    pub async fn wait_for_build(&self, timeout: Duration) -> Result<Option<BuildResult>> {
        let (tx, rx) = oneshot::channel();
        self.set_build_notifier(tx).await;

        tokio::select! {
            result = rx => {
                Ok(result.ok())
            }
            _ = sleep(timeout) => {
                Ok(None)
            }
        }
    }
}
