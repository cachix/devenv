use std::sync::Arc;

use miette::{Result, bail, miette};

use super::tasks::{self, Tasks};
use super::{Devenv, ShellCommand, run_tasks_with_ui};

fn sanitize_container_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
}

impl Devenv {
    pub async fn container_build(&self, name: &str) -> Result<String> {
        let _ = self.container_name.set(name.to_string());
        self.assemble().await?;

        let sanitized_name = sanitize_container_name(name);
        let gc_root = self
            .devenv_dot_gc
            .join(format!("container-{sanitized_name}-derivation"));
        let host_arch = env!("TARGET_ARCH");
        let host_os = env!("TARGET_OS");
        let target_system = if host_os == "macos" {
            match host_arch {
                "aarch64" => "aarch64-linux",
                "x86_64" => "x86_64-linux",
                _ => bail!("Unsupported container architecture for macOS: {host_arch}"),
            }
        } else {
            &self.nix_settings.system
        };
        let paths = self
            .nix
            .build(
                &[&format!(
                    "devenv.perSystem.{target_system}.config.containers.{name}.derivation"
                )],
                None,
                Some(&gc_root),
            )
            .await?;
        let container_store_path = paths[0].to_string_lossy().into_owned();
        Ok(container_store_path)
    }

    pub async fn container_copy(
        &self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<()> {
        let spec = self.container_build(name).await?;

        let sanitized_name = sanitize_container_name(name);
        let gc_root = self
            .devenv_dot_gc
            .join(format!("container-{sanitized_name}-copy"));
        let paths = self
            .nix
            .build(
                &[&format!("devenv.config.containers.{name}.copyScript")],
                None,
                Some(&gc_root),
            )
            .await?;
        let copy_script = paths[0].to_string_lossy().into_owned();

        let envs = self.capture_shell_environment().await?;

        let task_name = "devenv:container:copy";
        let mut task_configs = self.load_tasks().await?;

        let task = task_configs
            .iter_mut()
            .find(|t| t.name == task_name)
            .ok_or_else(|| miette!("Task {task_name} not found"))?;

        task.input = Some(serde_json::json!({
            "copy_script": copy_script,
            "spec": spec,
            "registry": registry.unwrap_or("false"),
            "copy_args": copy_args,
        }));

        let config = self.make_task_config(
            vec![task_name.to_string()],
            task_configs,
            devenv_tasks::RunMode::Single,
            envs,
            String::new(),
        )?;

        let tasks = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown))
            .with_refresh_task_cache(self.cache_settings.refresh_task_cache)
            .build()
            .await?;

        let (status, _outputs) = run_tasks_with_ui(tasks, verbosity, tui).await?;

        if status.has_failures() {
            bail!("Failed to copy container");
        }

        Ok(())
    }

    pub async fn container_run(
        &self,
        name: &str,
        copy_args: &[String],
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<ShellCommand> {
        self.container_copy(name, copy_args, Some("docker-daemon:"), verbosity, tui)
            .await?;

        let sanitized_name = sanitize_container_name(name);
        let gc_root = self
            .devenv_dot_gc
            .join(format!("container-{sanitized_name}-run"));
        let paths = self
            .nix
            .build(
                &[&format!("devenv.config.containers.{name}.dockerRun")],
                None,
                Some(&gc_root),
            )
            .await?;

        Ok(ShellCommand {
            command: std::process::Command::new(&paths[0]),
        })
    }
}
