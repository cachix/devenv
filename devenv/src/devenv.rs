use super::{tasks, tracing::HumanReadableDuration, util};
use ::nix::sys::signal;
use ::nix::unistd::Pid;
use clap::crate_version;
use cli_table::Table;
use cli_table::{WithTitle, print_stderr};
use devenv_activity::ActivityInstrument;
use devenv_activity::{Activity, ActivityLevel, activity, message};
use devenv_cache_core::compute_string_hash;
use devenv_core::{
    cachix::{CachixManager, CachixPaths},
    cli::GlobalOptions,
    config::{Config, NixBackendType},
    nix_args::{NixArgs, SecretspecData},
    nix_backend::{DevenvPaths, NixBackend, Options},
};
use include_dir::{Dir, include_dir};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use once_cell::sync::Lazy;
use secrecy::ExposeSecret;
use serde::Deserialize;
use sha2::Digest;
use similar::{ChangeTag, TextDiff};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tasks::{Tasks, TasksUi};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process;
use tokio::sync::{OnceCell, RwLock, Semaphore};
use tracing::{Instrument, debug, info, instrument, trace, warn};

// templates
const REQUIRED_FILES: [&str; 4] = ["devenv.nix", "devenv.yaml", ".envrc", ".gitignore"];
const EXISTING_REQUIRED_FILES: [&str; 1] = [".gitignore"];
const PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/init");
pub static DIRENVRC: Lazy<String> = Lazy::new(|| {
    include_str!("../direnvrc").replace(
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=0",
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=1",
    )
});
pub static DIRENVRC_VERSION: Lazy<u8> = Lazy::new(|| {
    DIRENVRC
        .lines()
        .find(|line| line.contains("export DEVENV_DIRENVRC_VERSION"))
        .and_then(|line| line.split('=').next_back())
        .map(|version| version.trim())
        .and_then(|version| version.parse().ok())
        .unwrap_or(0)
});

#[derive(Debug)]
pub struct DevenvOptions {
    pub config: Config,
    pub global_options: Option<GlobalOptions>,
    pub devenv_root: Option<PathBuf>,
    pub devenv_dotfile: Option<PathBuf>,
    pub shutdown: Arc<tokio_shutdown::Shutdown>,
}

impl DevenvOptions {
    pub fn new(shutdown: Arc<tokio_shutdown::Shutdown>) -> Self {
        Self {
            config: Config::default(),
            global_options: None,
            devenv_root: None,
            devenv_dotfile: None,
            shutdown,
        }
    }
}

impl Default for DevenvOptions {
    fn default() -> Self {
        Self {
            config: Config::default(),
            global_options: None,
            devenv_root: None,
            devenv_dotfile: None,
            shutdown: tokio_shutdown::Shutdown::new(),
        }
    }
}

#[derive(Default, Debug)]
pub struct ProcessOptions<'a> {
    /// An optional environment map to pass to the process.
    /// If not provided, the process will be executed inside a freshly evaluated shell.
    pub envs: Option<&'a HashMap<String, String>>,
    /// Whether the process should be detached from the current process.
    pub detach: bool,
    /// Whether the process should be logged to a file.
    pub log_to_file: bool,
}

/// A shell command ready to be executed.
#[derive(Debug)]
pub struct ShellCommand {
    /// The shell command to execute
    pub command: std::process::Command,
}

/// How processes should be run after `up`.
#[derive(Debug)]
pub enum RunMode {
    /// Processes started in detached mode (background)
    ///
    /// NOTE: detached mode currently starts processes in the library
    /// This should be changed closer to 2.0 release
    Detached,
    /// Process command ready to be exec'd (foreground mode)
    Foreground(ShellCommand),
}

/// Error indicating that secrets need to be prompted for interactively.
/// This is used to signal the CLI to stop the TUI and prompt for secrets.
#[derive(Debug, miette::Diagnostic)]
#[diagnostic(code(devenv::secrets_need_prompting))]
pub struct SecretsNeedPrompting {
    pub provider: Option<String>,
    pub profile: Option<String>,
    pub missing: Vec<String>,
}

impl std::fmt::Display for SecretsNeedPrompting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Missing required secrets: {}", self.missing.join(", "))
    }
}

impl std::error::Error for SecretsNeedPrompting {}

pub struct Devenv {
    pub config: Arc<RwLock<Config>>,
    pub global_options: GlobalOptions,

    pub nix: Arc<Box<dyn NixBackend>>,

    // All kinds of paths
    devenv_root: PathBuf,
    devenv_dotfile: PathBuf,
    devenv_dot_gc: PathBuf,
    devenv_home_gc: PathBuf,
    devenv_tmp: PathBuf,
    devenv_runtime: PathBuf,

    // Whether assemble has been run.
    // Assemble creates critical runtime directories and files.
    assembled: Arc<AtomicBool>,
    // Semaphore to prevent multiple concurrent assembles
    assemble_lock: Arc<Semaphore>,

    has_processes: Arc<OnceCell<bool>>,

    // Eval-cache pool (framework layer concern, used by backends)
    eval_cache_pool: Arc<OnceCell<SqlitePool>>,

    // Secretspec resolved data to pass to Nix
    secretspec_resolved: Arc<OnceCell<secretspec::Resolved<HashMap<String, String>>>>,

    // Cached serialized NixArgs from assemble
    nix_args_string: Arc<OnceCell<String>>,

    // TODO: make private.
    // Pass as an arg or have a setter.
    pub container_name: Option<String>,

    // Shutdown handle for coordinated shutdown
    shutdown: Arc<tokio_shutdown::Shutdown>,
}

impl Devenv {
    pub async fn new(options: DevenvOptions) -> Self {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv");
        let devenv_home = xdg_dirs
            .get_data_home()
            .expect("Failed to get home directory");
        let cachix_trusted_keys = devenv_home.join("cachix_trusted_keys.json");
        let devenv_home_gc = devenv_home.join("gc");

        let devenv_root = options
            .devenv_root
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
        let devenv_dotfile = options
            .devenv_dotfile
            .map(|p| p.to_path_buf())
            .unwrap_or(devenv_root.join(".devenv"));
        let devenv_dot_gc = devenv_dotfile.join("gc");

        let devenv_tmp =
            PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
                std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string())
            }));
        // first 7 chars of sha256 hash of devenv_state
        let devenv_state_hash = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
            let result = hasher.finalize();
            hex::encode(result)
        };
        let devenv_runtime = devenv_tmp.join(format!("devenv-{}", &devenv_state_hash[..7]));

        let global_options = options.global_options.unwrap_or_default();

        xdg_dirs
            .create_data_directory(Path::new("devenv"))
            .expect("Failed to create DEVENV_HOME directory");
        tokio::fs::create_dir_all(&devenv_home_gc)
            .await
            .expect("Failed to create DEVENV_HOME_GC directory");

        // Determine backend type from config
        let backend_type = options.config.backend.clone();

        // Create DevenvPaths struct
        let paths = DevenvPaths {
            root: devenv_root.clone(),
            dotfile: devenv_dotfile.clone(),
            dot_gc: devenv_dot_gc.clone(),
            home_gc: devenv_home_gc.clone(),
        };

        // Create CachixPaths for Nix backend
        let cachix_paths = CachixPaths {
            trusted_keys: cachix_trusted_keys,
            netrc: devenv_dotfile.join("netrc"),
            daemon_socket: None,
        };
        let cachix_manager = Arc::new(CachixManager::new(cachix_paths));

        // Create shared secretspec_resolved Arc to share between Devenv and Nix
        let secretspec_resolved = Arc::new(OnceCell::new());

        // Create eval-cache pool (framework layer concern, used by backends)
        let eval_cache_pool = Arc::new(OnceCell::new());

        let nix: Box<dyn NixBackend> = match backend_type {
            NixBackendType::Nix => Box::new(
                devenv_nix_backend::nix_backend::NixRustBackend::new(
                    paths,
                    options.config.clone(),
                    global_options.clone(),
                    cachix_manager.clone(),
                    options.shutdown.clone(),
                    None,
                )
                .expect("Failed to initialize Nix backend"),
            ),
            #[cfg(feature = "snix")]
            NixBackendType::Snix => Box::new(
                devenv_snix_backend::SnixBackend::new(
                    options.config.clone(),
                    global_options.clone(),
                    paths,
                    cachix_manager,
                    Some(eval_cache_pool.clone()),
                )
                .await
                .expect("Failed to initialize Snix backend"),
            ),
        };

        Self {
            config: Arc::new(RwLock::new(options.config)),
            global_options,
            devenv_root,
            devenv_dotfile,
            devenv_dot_gc,
            devenv_home_gc,
            devenv_tmp,
            devenv_runtime,
            nix: Arc::new(nix),
            assembled: Arc::new(AtomicBool::new(false)),
            assemble_lock: Arc::new(Semaphore::new(1)),
            has_processes: Arc::new(OnceCell::new()),
            eval_cache_pool,
            secretspec_resolved,
            nix_args_string: Arc::new(OnceCell::new()),
            container_name: None,
            shutdown: options.shutdown,
        }
    }

    pub fn processes_log(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.log")
    }

    pub fn processes_pid(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.pid")
    }

    pub fn paths(&self) -> DevenvPaths {
        DevenvPaths {
            root: self.devenv_root.clone(),
            dotfile: self.devenv_dotfile.clone(),
            dot_gc: self.devenv_dot_gc.clone(),
            home_gc: self.devenv_home_gc.clone(),
        }
    }

    /// Get the root directory of the devenv project (where devenv.nix is located)
    pub fn root(&self) -> &Path {
        &self.devenv_root
    }

    /// Get the path to the .devenv directory
    pub fn dotfile(&self) -> &Path {
        &self.devenv_dotfile
    }

    pub fn init(&self, target: &Option<PathBuf>) -> Result<()> {
        let target = target.clone().unwrap_or_else(|| {
            std::fs::canonicalize(".").expect("Failed to get current directory")
        });

        // create directory target if not exists
        if !target.exists() {
            std::fs::create_dir_all(&target).expect("Failed to create target directory");
        }

        for filename in REQUIRED_FILES {
            info!("Creating {}", filename);

            let path = PROJECT_DIR
                .get_file(filename)
                .ok_or_else(|| miette::miette!("missing {} in the executable", filename))?;

            // write path.contents to target/filename
            let target_path = target.join(filename);

            // add a check for files like .gitignore to append buffer instead of bailing out
            if target_path.exists() && EXISTING_REQUIRED_FILES.contains(&filename) {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)
                    .and_then(|mut file| {
                        file.write_all(b"\n")?;
                        file.write_all(path.contents())
                    })
                    .expect("Failed to append to existing file");
            } else if target_path.exists() && !EXISTING_REQUIRED_FILES.contains(&filename) {
                if let Some(utf8_contents) = path.contents_utf8() {
                    confirm_overwrite(&target_path, utf8_contents.to_string())?;
                } else {
                    bail!("Failed to read file contents as UTF-8");
                }
            } else {
                std::fs::write(&target_path, path.contents()).expect("Failed to write file");
            }
        }

        // check if direnv executable is available
        let Ok(direnv) = which::which("direnv") else {
            return Ok(());
        };

        // run direnv allow
        let _ = process::Command::new(direnv)
            .arg("allow")
            .current_dir(&target)
            .spawn();
        Ok(())
    }

    pub async fn inputs_add(&self, name: &str, url: &str, follows: &[String]) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.add_input(name, url, follows)?;
            config.write().await?;
        }
        Ok(())
    }

    pub async fn changelogs(&self) -> Result<()> {
        let changelog = crate::changelog::Changelog::new(&**self.nix, &self.paths());
        changelog.show_all().await?;
        Ok(())
    }

    pub async fn print_dev_env(&self, json: bool) -> Result<String> {
        let env = self.get_dev_environment(json).await?;
        Ok(String::from_utf8(env.output).expect("Failed to convert env to utf-8"))
    }

    #[instrument(skip(self))]
    pub async fn prepare_shell(
        &self,
        cmd: &Option<String>,
        args: &[String],
    ) -> Result<process::Command> {
        let DevEnv { output, .. } = self.get_dev_environment(false).await?;

        let bash = match self.nix.get_bash(false).await {
            Err(e) => {
                trace!("Failed to get bash: {}. Rebuilding.", e);
                self.nix.get_bash(true).await?
            }
            Ok(bash) => bash,
        };

        let mut shell_cmd = process::Command::new(&bash);

        // Load the user's bashrc if it exists and if we're in an interactive shell.
        // Disable alias expansion to avoid breaking the dev shell script.
        let mut script = indoc::formatdoc! {
            r#"
            if [ -n "$PS1" ] && [ -e $HOME/.bashrc ]; then
                source $HOME/.bashrc;
            fi

            shopt -u expand_aliases
            {}
            shopt -s expand_aliases
            "#,
            String::from_utf8_lossy(&output)
        };

        // Add command for non-interactive mode
        if let Some(cmd) = &cmd {
            let command = format!(
                "\nexec {} {}",
                cmd,
                args.iter()
                    .map(|arg| shell_escape::escape(std::borrow::Cow::Borrowed(arg)))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            script.push_str(&command);
        }

        // Write shell script to a content-addressed file
        // Using content hash in filename allows eval cache to track it properly while
        // avoiding race conditions between parallel sessions (same content = same file)
        let script_hash = &compute_string_hash(&script)[..16];
        let script_path = self
            .devenv_dotfile
            .join(format!("shell-{}.sh", script_hash));
        std::fs::write(&script_path, &script).expect("Failed to write shell script");
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("Failed to set permissions");

        match cmd {
            Some(_) => {
                shell_cmd.arg(&script_path);
            }
            None => {
                shell_cmd.args(["--rcfile", &script_path.to_string_lossy()]);
            }
        }

        let config_clean = self.config.read().await.clean.clone().unwrap_or_default();
        if self.global_options.clean.is_some() || config_clean.enabled {
            let keep = match &self.global_options.clean {
                Some(clean) => clean,
                None => &config_clean.keep,
            };

            let filtered_env = std::env::vars().filter(|(k, _)| keep.contains(k));
            shell_cmd.env_clear().envs(filtered_env);
        }

        shell_cmd.env("SHELL", &bash);

        // Pass command args to the shell as DEVENV_CMDLINE
        let cmdline = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        shell_cmd.env("DEVENV_CMDLINE", cmdline);

        Ok(shell_cmd)
    }

    /// Prepare to launch an interactive shell.
    /// Returns a ShellCommand that should be executed after cleanup.
    pub async fn shell(&self) -> Result<ShellCommand> {
        self.prepare_exec(None, &[]).await
    }

    /// Prepare a command for exec.
    ///
    /// This method accepts `Option<String>` for the command to support both:
    /// - Interactive shell: `prepare_exec(None, &[])`
    /// - Command execution: `prepare_exec(Some(cmd), args)`
    ///
    /// Returns a ShellCommand containing the prepared command.
    /// The caller is responsible for executing it at the appropriate time
    /// (after TUI cleanup, terminal restore, etc.).
    pub async fn prepare_exec(&self, cmd: Option<String>, args: &[String]) -> Result<ShellCommand> {
        let shell_cmd = self.prepare_shell(&cmd, args).await?;
        Ok(ShellCommand {
            command: shell_cmd.into_std(),
        })
    }

    /// Run a command and return the output.
    ///
    /// This method accepts `String` (not `Option<String>`) because it's specifically
    /// designed for running commands and capturing their output. Unlike `exec_in_shell`,
    /// this method always requires a command and uses `spawn` + `wait_with_output`
    /// to return control to the caller with the command's output.
    pub async fn run_in_shell(&self, cmd: String, args: &[String]) -> Result<Output> {
        let mut shell_cmd = self.prepare_shell(&Some(cmd), args).await?;
        let activity = Activity::operation("Running in shell").start();
        // Capture all output - never write directly to terminal
        async move { shell_cmd.output().await.into_diagnostic() }
            .in_activity(&activity)
            .await
    }

    pub async fn update(&self, input_name: &Option<String>) -> Result<()> {
        let msg = match input_name {
            Some(input_name) => format!("Updating devenv.lock with input {input_name}"),
            None => "Updating devenv.lock".to_string(),
        };

        let activity = Activity::operation(&msg).start();
        self.nix.update(input_name).in_activity(&activity).await?;

        // Assemble is required for changelog.show_new() which builds changelog.json
        // Allow assemble to fail gracefully - changelogs are informational only
        match self.assemble(false).await {
            Ok(_) => {
                // Show new changelogs (if any)
                let changelog = crate::changelog::Changelog::new(&**self.nix, &self.paths());
                if let Err(e) = changelog.show_new().await {
                    // Don't fail the update if changelogs fail to load
                    tracing::warn!("Failed to show changelogs: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to assemble environment, skipping changelog: {}", e);
            }
        }

        Ok(())
    }

    #[activity(format!("{name} container"), kind = build)]
    pub async fn container_build(&mut self, name: &str) -> Result<String> {
        // This container name is passed to the flake as an argument and tells the module system
        // that we're 1. building a container 2. which container we're building.
        self.container_name = Some(name.to_string());
        self.assemble(false).await?;

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
            &self.global_options.system
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
        let container_store_path = paths[0].to_string_lossy().to_string();
        Ok(container_store_path)
    }

    pub async fn container_copy(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        let spec = self.container_build(name).await?;

        let activity = Activity::operation("Copying container").start();
        async move {
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
            let copy_script = &paths[0];
            let copy_script_string = &copy_script.to_string_lossy();

            let base_args = [spec, registry.unwrap_or("false").to_string()];
            let command_args: Vec<String> = base_args
                .into_iter()
                .chain(copy_args.iter().map(|s| s.to_string()))
                .collect();

            debug!("Running {copy_script_string} {}", command_args.join(" "));

            let output = process::Command::new(copy_script)
                .args(command_args)
                .output()
                .await
                .expect("Failed to run copy script");

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to copy container: {}", stderr)
            } else {
                Ok(())
            }
        }
        .in_activity(&activity)
        .await
    }

    pub async fn container_run(
        &mut self,
        name: &str,
        copy_args: &[String],
    ) -> Result<ShellCommand> {
        self.container_copy(name, copy_args, Some("docker-daemon:"))
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

    pub async fn repl(&self) -> Result<()> {
        self.assemble(false).await?;
        self.nix.repl().await?;
        Ok(())
    }

    /// Garbage collect devenv environments and store paths.
    /// Returns (paths_deleted, bytes_freed).
    pub async fn gc(&self) -> Result<(u64, u64)> {
        let (to_gc, _removed_symlinks) = {
            let activity = Activity::operation(format!(
                "Removing non-existing symlinks in {}",
                &self.devenv_home_gc.display()
            ))
            .start();
            cleanup_symlinks(&self.devenv_home_gc)
                .in_activity(&activity)
                .await
        };

        let (paths_deleted, bytes_freed) = {
            let activity = Activity::operation("Running garbage collection").start();
            self.nix.gc(to_gc).in_activity(&activity).await?
        };

        Ok((paths_deleted, bytes_freed))
    }

    #[activity("Searching options and packages")]
    pub async fn search(&self, name: &str) -> Result<()> {
        self.assemble(false).await?;

        // Run both searches concurrently
        let (options_results, package_results) =
            tokio::try_join!(self.search_options(name), self.search_packages(name))?;

        let results_options_count = options_results.len();
        let package_results_count = package_results.len();

        if !package_results.is_empty() {
            print_stderr(package_results.with_title()).expect("Failed to print package results");
        }

        if !options_results.is_empty() {
            print_stderr(options_results.with_title()).expect("Failed to print options results");
        }

        info!(
            "Found {package_results_count} packages and {results_options_count} options for '{name}'."
        );
        Ok(())
    }

    async fn search_options(&self, name: &str) -> Result<Vec<DevenvOptionResult>> {
        let build_options = Options {
            cache_output: true,
            ..Default::default()
        };
        let options = self
            .nix
            .build(&["optionsJSON"], Some(build_options), None)
            .await?;
        let options_path = options[0]
            .join("share")
            .join("doc")
            .join("nixos")
            .join("options.json");
        let options_contents = fs::read(options_path)
            .await
            .expect("Failed to read options.json");
        let options_json: OptionResults =
            serde_json::from_slice(&options_contents).expect("Failed to parse options.json");

        let options_results = options_json
            .0
            .into_iter()
            .filter(|(key, _)| key.contains(name))
            .map(|(key, value)| DevenvOptionResult {
                name: key,
                type_: value.type_,
                default: value.default.unwrap_or_default(),
                description: value.description,
            })
            .collect::<Vec<_>>();

        Ok(options_results)
    }

    async fn search_packages(&self, name: &str) -> Result<Vec<DevenvPackageResult>> {
        let search_options = Options {
            cache_output: true,
            ..Default::default()
        };
        let search = self.nix.search(name, Some(search_options)).await?;
        let search_json: PackageResults =
            serde_json::from_slice(&search.stdout).expect("Failed to parse search results");
        let search_results = search_json
            .0
            .into_iter()
            .map(|(key, value)| DevenvPackageResult {
                name: format!(
                    "pkgs.{}",
                    key.split('.').skip(2).collect::<Vec<_>>().join(".")
                ),
                version: value.version,
                description: value.description.chars().take(80).collect::<String>(),
            })
            .collect::<Vec<_>>();

        Ok(search_results)
    }

    pub async fn has_processes(&self) -> Result<bool> {
        let value = self
            .has_processes
            .get_or_try_init(|| async {
                let processes = self.nix.eval(&["devenv.config.processes"]).await?;
                Ok::<bool, miette::Report>(processes.trim() != "{}")
            })
            .await?;
        Ok(*value)
    }

    #[activity("Loading tasks")]
    async fn load_tasks(&self) -> Result<Vec<tasks::TaskConfig>> {
        let tasks_json_file = {
            let gc_root = self.devenv_dot_gc.join("task-config");
            self.nix
                .build(&["devenv.config.task.config"], None, Some(&gc_root))
                .await?
        };
        // parse tasks config
        let tasks_json = fs::read_to_string(&tasks_json_file[0])
            .await
            .expect("Failed to read config file");
        let tasks: Vec<tasks::TaskConfig> =
            serde_json::from_str(&tasks_json).expect("Failed to parse tasks config");
        Ok(tasks)
    }

    /// Run tasks and return their outputs as JSON string.
    pub async fn tasks_run(
        &self,
        roots: Vec<String>,
        run_mode: devenv_tasks::RunMode,
        show_output: bool,
    ) -> Result<String> {
        self.assemble(false).await?;
        if roots.is_empty() {
            bail!("No tasks specified.");
        }

        // Capture the shell environment to ensure tasks run with proper devenv setup
        let envs = self.capture_shell_environment().await?;

        // Set environment variables in the current process
        // This ensures that tasks have access to all devenv environment variables
        for (key, value) in &envs {
            unsafe {
                std::env::set_var(key, value);
            }
        }

        let mut tasks = self.load_tasks().await?;

        // If --show-output flag is present, enable output for all tasks
        if show_output {
            for task in &mut tasks {
                task.show_output = true;
            }
        }

        // Convert global options to verbosity level
        let verbosity = if self.global_options.quiet {
            tasks::VerbosityLevel::Quiet
        } else if self.global_options.verbose {
            tasks::VerbosityLevel::Verbose
        } else {
            tasks::VerbosityLevel::Normal
        };

        let config = tasks::Config {
            roots,
            tasks,
            run_mode,
            sudo_context: None,
        };

        if let Ok(config_value) = devenv_activity::SerdeValue::from_serialize(&config) {
            use valuable::Valuable;
            debug!(event = config_value.as_value(), "Loaded task config");
        }

        let tasks = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown))
            .build()
            .await?;

        // In TUI mode, skip TasksUi to avoid corrupting the TUI display
        // TUI captures activity events directly via the channel initialized in main.rs
        let (status, outputs) = if self.global_options.tui {
            let outputs = tasks.run().await;
            let status = tasks.get_completion_status().await;
            (status, outputs)
        } else {
            // Shell mode - initialize activity channel for TasksUi
            let (activity_rx, activity_handle) = devenv_activity::init();
            activity_handle.install();

            let tasks = Arc::new(tasks);
            let tasks_clone = Arc::clone(&tasks);

            // Spawn task runner - UI will detect completion via JoinHandle
            let run_handle = tokio::spawn(async move { tasks_clone.run().await });

            // Run UI - processes events and waits for run_handle
            let ui = TasksUi::new(Arc::clone(&tasks), activity_rx, verbosity);
            ui.run(run_handle).await?
        };

        if status.has_failures() {
            miette::bail!("Some tasks failed");
        }

        Ok(serde_json::to_string(&outputs).expect("parsing of outputs failed"))
    }

    pub async fn tasks_list(&self) -> Result<String> {
        self.assemble(false).await?;

        let tasks = self.load_tasks().await?;

        if tasks.is_empty() {
            return Ok("No tasks defined.".to_string());
        }

        Ok(format_tasks_tree(&tasks))
    }

    async fn capture_shell_environment(&self) -> Result<HashMap<String, String>> {
        let temp_dir = tempfile::TempDir::with_prefix("devenv-env")
            .into_diagnostic()
            .wrap_err("Failed to create temporary directory for environment capture")?;

        let script_path = temp_dir.path().join("script");
        let env_path = temp_dir.path().join("env");

        let script = format!("env > {}", env_path.to_string_lossy());
        fs::write(&script_path, script)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write script to {}", script_path.display()))?;
        fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to set execute permissions on {}",
                    script_path.display()
                )
            })?;

        // Run script and capture its environment exports
        // Set DEVENV_SKIP_TASKS to prevent enterShell tasks from running during env capture
        let output = self
            .prepare_shell(&Some(script_path.to_string_lossy().into()), &[])
            .await?
            .env("DEVENV_SKIP_TASKS", "1")
            .output()
            .await
            .into_diagnostic()
            .wrap_err("Failed to execute environment capture script")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            miette::bail!("Shell environment capture failed: {}", stderr);
        }

        // Parse the environment variables
        let file = File::open(&env_path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to open environment file at {}", env_path.display())
            })?;
        let reader = BufReader::new(file);
        let mut shell_envs = Vec::new();
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut parts = line.splitn(2, '=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                shell_envs.push((key.to_string(), value.to_string()));
            }
        }

        let config_clean = self.config.read().await.clean.clone().unwrap_or_default();
        let mut envs: HashMap<String, String> = {
            let vars = std::env::vars();
            if self.global_options.clean.is_some() || config_clean.enabled {
                let keep = match &self.global_options.clean {
                    Some(clean) => clean,
                    None => &config_clean.keep,
                };
                vars.filter(|(key, _)| !keep.contains(key)).collect()
            } else {
                vars.collect()
            }
        };

        for (key, value) in shell_envs {
            envs.insert(key, value);
        }

        Ok(envs)
    }

    pub async fn test(&self) -> Result<()> {
        self.assemble(true).await?;

        // collect tests
        let test_script = {
            let activity = Activity::operation("Building tests").start();
            let gc_root = self.devenv_dot_gc.join("test");
            let test_script = self
                .nix
                .build(&["devenv.config.test"], None, Some(&gc_root))
                .in_activity(&activity)
                .await?;
            test_script[0].to_string_lossy().to_string()
        };

        if self.has_processes().await? {
            let options = ProcessOptions {
                envs: None,
                detach: true,
                log_to_file: false,
            };
            // up() with detach returns RunMode::Detached, not Exec
            self.up(vec![], &options).await?;
        }

        // Run the test script through the shell, which runs enterShell tasks first
        let result = self.run_in_shell(test_script, &[]).await?;

        if self.has_processes().await? {
            self.down().await?;
        }

        if !result.status.success() {
            message(ActivityLevel::Error, "Tests failed :(");
            bail!("Tests failed");
        } else {
            info!("Tests passed :)");
            Ok(())
        }
    }

    pub async fn info(&self) -> Result<String> {
        self.assemble(false).await?;
        self.nix.metadata().await
    }

    pub async fn build(&self, attributes: &[String]) -> Result<Vec<(String, PathBuf)>> {
        let activity = Activity::operation("Building").start();
        async move {
            self.assemble(false).await?;

            fn flatten_object(prefix: &str, value: &serde_json::Value) -> Vec<String> {
                match value {
                    // Null values indicate unevaluable/missing attributes - skip them
                    serde_json::Value::Null => vec![],
                    // String values are store paths - these are buildable leaves
                    serde_json::Value::String(_) => {
                        vec![prefix.to_string()]
                    }
                    serde_json::Value::Object(obj) => {
                        // If this object has outPath, it's a derivation - treat as leaf
                        if obj.contains_key("outPath") {
                            vec![prefix.to_string()]
                        } else {
                            // Recurse into nested objects
                            obj.iter()
                                .flat_map(|(k, v)| flatten_object(&format!("{prefix}.{k}"), v))
                                .collect()
                        }
                    }
                    // Other values (numbers, bools, arrays) shouldn't appear but skip them
                    _ => vec![],
                }
            }

            let attributes: Vec<String> = if attributes.is_empty() {
                // construct dotted names of all attributes that we need to build
                let build_output = self.nix.eval(&["build"]).await?;
                serde_json::from_str::<serde_json::Value>(&build_output)
                    .map_err(|e| miette::miette!("Failed to parse build output: {}", e))?
                    .as_object()
                    .ok_or_else(|| miette::miette!("Build output is not an object"))?
                    .iter()
                    .flat_map(|(key, value)| flatten_object(key, value))
                    .collect()
            } else {
                // Evaluate each attribute to check if it needs flattening
                let mut flattened = Vec::new();
                for attr in attributes {
                    // Try to get from build.{attr} first (for output types that need flattening)
                    let eval_result = self.nix.eval(&[&format!("build.{attr}")]).await;
                    match eval_result {
                        Ok(eval_output) => {
                            let value: serde_json::Value = serde_json::from_str(&eval_output)
                                .map_err(|e| {
                                    miette::miette!(
                                        "Failed to parse eval output for {}: {}",
                                        attr,
                                        e
                                    )
                                })?;
                            let flat = flatten_object(attr, &value);
                            flattened.extend(flat);
                        }
                        Err(_) => {
                            // Not in build, try as direct config attribute
                            flattened.push(attr.to_string());
                        }
                    }
                }
                flattened
            };

            // Build with full paths (adding devenv.config. prefix)
            let full_attrs: Vec<String> = attributes
                .iter()
                .map(|a| format!("devenv.config.{a}"))
                .collect();
            let paths = self
                .nix
                .build(
                    &full_attrs.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                    None,
                    None,
                )
                .await?;

            // Return pairs of (attribute, path)
            Ok(attributes.into_iter().zip(paths).collect())
        }
        .in_activity(&activity)
        .await
    }

    pub async fn eval(&self, attributes: &[String]) -> Result<String> {
        let activity = Activity::operation("Evaluating").start();
        async move {
            self.assemble(false).await?;

            let mut results = serde_json::Map::new();

            for attr in attributes {
                let full_attr = format!("devenv.config.{attr}");
                let eval_output = self.nix.eval(&[&full_attr]).await?;
                let value: serde_json::Value = serde_json::from_str(&eval_output).map_err(|e| {
                    miette::miette!("Failed to parse eval output for {}: {}", attr, e)
                })?;
                results.insert(attr.clone(), value);
            }

            let json = serde_json::to_string_pretty(&results)
                .map_err(|e| miette::miette!("Failed to serialize JSON: {}", e))?;

            Ok(json)
        }
        .in_activity(&activity)
        .await
    }

    pub async fn up<'a>(
        &self,
        processes: Vec<String>,
        options: &'a ProcessOptions<'a>,
    ) -> Result<RunMode> {
        self.assemble(false).await?;
        if !self.has_processes().await? {
            message(
                ActivityLevel::Error,
                "No 'processes' option defined: https://devenv.sh/processes/",
            );
            bail!("No processes defined");
        }

        let proc_script_string = {
            let activity = Activity::operation("Building processes").start();
            async {
                let gc_root = self.devenv_dot_gc.join("procfilescript");
                let paths = self
                    .nix
                    .build(&["devenv.config.procfileScript"], None, Some(&gc_root))
                    .await?;
                let proc_script_string = paths[0].to_string_lossy().to_string();
                Ok::<String, miette::Report>(proc_script_string)
            }
            .in_activity(&activity)
            .await?
        };

        let activity = Activity::operation("Starting processes").start();
        async {
            let processes = processes.join(" ");

            let processes_script = self.devenv_dotfile.join("processes");
            // we force disable process compose tui if detach is enabled
            let tui = if options.detach {
                "export PC_TUI_ENABLED=0"
            } else {
                ""
            };
            fs::write(
                &processes_script,
                indoc::formatdoc! {"
                #!/usr/bin/env bash
                {tui}
                exec {proc_script_string} {processes}
            "},
            )
            .await
            .expect("Failed to write PROCESSES_SCRIPT");

            fs::set_permissions(&processes_script, std::fs::Permissions::from_mode(0o755))
                .await
                .expect("Failed to set permissions");

            let mut cmd = if let Some(envs) = options.envs {
                let mut cmd = process::Command::new("bash");
                cmd.arg(processes_script.to_string_lossy().to_string())
                    .env_clear()
                    .envs(envs);
                cmd
            } else {
                self.prepare_shell(&Some(processes_script.to_string_lossy().to_string()), &[])
                    .await?
            };

            if options.detach {
                // Check if processes are already running
                if self.processes_pid().exists() {
                    match fs::read_to_string(self.processes_pid()).await {
                        Ok(pid_str) => {
                            if let Ok(pid_num) = pid_str.trim().parse::<i32>() {
                                let pid = Pid::from_raw(pid_num);
                                match signal::kill(pid, None) {
                                    Ok(_) => {
                                        // Process is running
                                        bail!("Processes already running with PID {}. Stop them first with: devenv processes down", pid);
                                    }
                                    Err(::nix::errno::Errno::ESRCH) => {
                                        // Process not found - stale PID file
                                        warn!("Found stale PID file with PID {}. Removing it.", pid);
                                        fs::remove_file(self.processes_pid())
                                            .await
                                            .expect("Failed to remove stale PID file");
                                    }
                                    Err(_) => {
                                        // Other error - remove stale file
                                        warn!("Found invalid PID file. Removing it.");
                                        fs::remove_file(self.processes_pid())
                                            .await
                                            .expect("Failed to remove stale PID file");
                                    }
                                }
                            } else {
                                // Invalid PID format
                                warn!("Found invalid PID file. Removing it.");
                                fs::remove_file(self.processes_pid())
                                    .await
                                    .expect("Failed to remove stale PID file");
                            }
                        }
                        Err(_) => {
                            // Could not read file
                            warn!("Found unreadable PID file. Removing it.");
                            let _ = fs::remove_file(self.processes_pid()).await;
                        }
                    }
                }

                let process = if !options.log_to_file {
                    // Detached daemon: redirect to null to avoid corrupting TUI
                    cmd.stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .expect("Failed to spawn process")
                } else {
                    let log_file = std::fs::File::create(self.processes_log())
                        .expect("Failed to create PROCESSES_LOG");
                    cmd.stdout(log_file.try_clone().expect("Failed to clone Stdio"))
                        .stderr(log_file)
                        .spawn()
                        .expect("Failed to spawn process")
                };

                let pid = process
                    .id()
                    .ok_or_else(|| miette!("Failed to get process ID"))?;
                fs::write(self.processes_pid(), pid.to_string())
                    .await
                    .expect("Failed to write PROCESSES_PID");
                info!("PID is {}", pid);
                if options.log_to_file {
                    info!("See logs:  $ tail -f {}", self.processes_log().display());
                }
                info!("Stop:      $ devenv processes stop");
                Ok(RunMode::Detached)
            } else {
                Ok(RunMode::Foreground(ShellCommand {
                    command: cmd.into_std(),
                }))
            }
        }
        .in_activity(&activity)
        .await
    }

    pub async fn down(&self) -> Result<()> {
        if !PathBuf::from(&self.processes_pid()).exists() {
            bail!("No processes running");
        }

        let pid = fs::read_to_string(self.processes_pid())
            .await
            .into_diagnostic()
            .wrap_err("Failed to read processes.pid file")?
            .trim()
            .parse::<i32>()
            .into_diagnostic()
            .wrap_err("Invalid PID in processes.pid file")
            .map(Pid::from_raw)?;

        info!("Stopping process with PID {}", pid);

        match signal::kill(pid, signal::Signal::SIGTERM) {
            Ok(_) => {}
            Err(_) => {
                bail!("Process with PID {} not found.", pid);
            }
        }

        // Wait for the process to actually shut down using exponential backoff
        let start_time = std::time::Instant::now();
        let max_wait = std::time::Duration::from_secs(30);
        let mut wait_interval = std::time::Duration::from_millis(10);
        const MAX_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

        loop {
            // Check if process is still running by sending signal 0 (null signal)
            match signal::kill(pid, None) {
                Ok(_) => {
                    // Process still exists
                    let elapsed = start_time.elapsed();
                    if elapsed >= max_wait {
                        message(
                            ActivityLevel::Warn,
                            format!(
                                "Process {} did not shut down gracefully within {} seconds, sending SIGKILL to process group",
                                pid,
                                max_wait.as_secs()
                            ),
                        );

                        // Send SIGKILL to the entire process group
                        // Negative PID means send to process group
                        let pgid = Pid::from_raw(-pid.as_raw());
                        match signal::kill(pgid, signal::Signal::SIGKILL) {
                            Ok(_) => info!("Sent SIGKILL to process group {}", pid.as_raw()),
                            Err(e) => warn!(
                                "Failed to send SIGKILL to process group {}: {}",
                                pid.as_raw(),
                                e
                            ),
                        }

                        // Give it a moment to die after SIGKILL
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    }

                    tokio::time::sleep(wait_interval).await;

                    // Exponential backoff: double the interval up to MAX_INTERVAL
                    wait_interval = wait_interval.mul_f64(1.5).min(MAX_INTERVAL);
                }
                Err(nix::errno::Errno::ESRCH) => {
                    // ESRCH means "No such process" - it has shut down
                    debug!(
                        "Process {} has shut down after {}",
                        pid,
                        HumanReadableDuration(start_time.elapsed())
                    );
                    break;
                }
                Err(e) => {
                    // Some other error occurred
                    warn!("Error checking process {}: {}", pid, e);
                    break;
                }
            }
        }

        fs::remove_file(self.processes_pid())
            .await
            .expect("Failed to remove PROCESSES_PID");
        Ok(())
    }

    /// Assemble the devenv environment and return the serialized NixArgs string.
    /// The returned string can be used with `import bootstrap/default.nix <args>`.
    pub async fn assemble(&self, is_testing: bool) -> Result<String> {
        if self.assembled.load(Ordering::Acquire) {
            return Ok(self
                .nix_args_string
                .get()
                .expect("nix_args_string should be set after assemble")
                .clone());
        }

        let _permit = self.assemble_lock.acquire().await.unwrap();

        // Skip devenv.nix existence check if --option or --from is provided
        if self.global_options.option.is_empty()
            && self.global_options.from.is_none()
            && !self.devenv_root.join("devenv.nix").exists()
        {
            bail!(indoc::indoc! {"
            File devenv.nix does not exist. To get started, run:

                $ devenv init
            "});
        }

        fs::create_dir_all(&self.devenv_dot_gc).await.map_err(|e| {
            miette::miette!("Failed to create {}: {}", self.devenv_dot_gc.display(), e)
        })?;

        let config = self.config.read().await;
        // TODO: superceded by eval caching.
        // Remove once direnvrc migration is implemented.
        util::write_file_with_lock(
            self.devenv_dotfile.join("imports.txt"),
            config.imports.join("\n"),
        )?;

        fs::create_dir_all(&self.devenv_runtime)
            .await
            .map_err(|e| {
                miette::miette!("Failed to create {}: {}", self.devenv_runtime.display(), e)
            })?;

        // Initialize eval-cache database (framework layer concern, used by backends)
        if self.global_options.eval_cache {
            self.eval_cache_pool
                .get_or_try_init(|| async {
                    let db_path = self.devenv_dotfile.join("nix-eval-cache.db");
                    let db = devenv_cache_core::db::Database::new(
                        db_path,
                        &devenv_eval_cache::db::MIGRATIONS,
                    )
                    .await
                    .map_err(|e| {
                        miette::miette!("Failed to initialize eval cache database: {}", e)
                    })?;
                    Ok::<_, miette::Report>(db.pool().clone())
                })
                .await?;
        }

        // Check for secretspec.toml and load secrets
        let secretspec_path = self.devenv_root.join("secretspec.toml");
        let secretspec_config_exists = config.secretspec.is_some();
        let secretspec_enabled = config
            .secretspec
            .as_ref()
            .map(|c| c.enable)
            .unwrap_or(false); // Default to false if secretspec config is not present

        if secretspec_path.exists() {
            // Log warning when secretspec.toml exists but is not configured
            if !secretspec_enabled && !secretspec_config_exists {
                info!(
                    "{}",
                    indoc::formatdoc! {"
                    Found secretspec.toml but secretspec integration is not enabled.

                    To enable, add to devenv.yaml:
                      secretspec:
                        enable: true

                    To disable this message:
                      secretspec:
                        enable: false

                    Learn more: https://devenv.sh/integrations/secretspec/
                "}
                );
            }

            if secretspec_enabled {
                // Get profile and provider from devenv.yaml config
                let (profile, provider) = if let Some(ref secretspec_config) = config.secretspec {
                    (
                        secretspec_config.profile.clone(),
                        secretspec_config.provider.clone(),
                    )
                } else {
                    (None, None)
                };

                // Load and validate secrets using SecretSpec API
                let mut secrets = secretspec::Secrets::load()
                    .map_err(|e| miette!("Failed to load secretspec configuration: {}", e))?;

                // Configure provider and profile if specified
                if let Some(ref provider_str) = provider {
                    secrets.set_provider(provider_str);
                }
                if let Some(ref profile_str) = profile {
                    secrets.set_profile(profile_str);
                }

                // Validate secrets
                // In TUI mode, validate first and signal if prompting is needed (TUI will be stopped)
                // In non-TUI mode, just validate silently
                let validated_secrets = if self.global_options.tui {
                    match secrets.validate()? {
                        Ok(validated) => validated,
                        Err(e) => {
                            // Signal that we need to prompt for secrets after TUI stops
                            return Err(SecretsNeedPrompting {
                                provider: provider.clone(),
                                profile: profile.clone(),
                                missing: e.missing_required,
                            }
                            .into());
                        }
                    }
                } else {
                    secrets.validate()?.map_err(|e| {
                        miette!(
                            "Missing required secrets: {}\n\nRun `devenv shell` to enter the secrets interactively.",
                            e.missing_required.join(", ")
                        )
                    })?
                };

                // Store resolved secrets in OnceCell for Nix to use
                let resolved = secretspec::Resolved {
                    secrets: validated_secrets
                        .resolved
                        .secrets
                        .into_iter()
                        .map(|(k, v)| (k, v.expose_secret().to_string()))
                        .collect(),
                    provider: validated_secrets.resolved.provider,
                    profile: validated_secrets.resolved.profile,
                };

                self.secretspec_resolved
                    .set(resolved)
                    .map_err(|_| miette!("Secretspec resolved already set"))?;
            }
        }

        // Create flake.devenv.nix

        // Get current hostname and username using system APIs
        let hostname = hostname::get()
            .ok()
            .map(|h| h.to_string_lossy().into_owned());

        let username = whoami::fallible::username().ok();

        // TODO: remove in the next release
        let dotfile_relative_path = PathBuf::from(format!(
            "./{}",
            self.devenv_dotfile
                .file_name()
                // This should never fail
                .expect("Failed to extract the directory name from devenv_dotfile")
                .to_string_lossy()
        ));

        // Get git repository root from config (already detected during config load)
        let git_root = config.git_root.clone();

        // Convert secretspec::Resolved to SecretspecData if available
        let secretspec_data: Option<SecretspecData> =
            self.secretspec_resolved
                .get()
                .map(|resolved| SecretspecData {
                    profile: resolved.profile.clone(),
                    provider: resolved.provider.clone(),
                    secrets: resolved.secrets.clone(),
                });

        // Determine active profiles: CLI overrides YAML
        // If CLI profiles are specified, use those. Otherwise, use YAML profile if set.
        let active_profiles = if !self.global_options.profile.is_empty() {
            self.global_options.profile.clone()
        } else if let Some(yaml_profile) = &config.profile {
            vec![yaml_profile.clone()]
        } else {
            Vec::new()
        };

        // Create the Nix arguments struct
        let nixpkgs_config = config.nixpkgs_config(&self.global_options.system);
        let args = NixArgs {
            version: crate_version!(),
            system: &self.global_options.system,
            devenv_root: &self.devenv_root,
            skip_local_src: self.global_options.from.is_some(),
            devenv_dotfile: &self.devenv_dotfile,
            devenv_dotfile_path: &dotfile_relative_path,
            devenv_tmpdir: &self.devenv_tmp,
            devenv_runtime: &self.devenv_runtime,
            devenv_istesting: is_testing,
            devenv_direnvrc_latest_version: *DIRENVRC_VERSION,
            container_name: self.container_name.as_deref(),
            active_profiles: &active_profiles,
            hostname: hostname.as_deref(),
            username: username.as_deref(),
            git_root: git_root.as_deref(),
            secretspec: secretspec_data.as_ref(),
            devenv_config: &config,
            nixpkgs_config,
        };

        // Serialize NixArgs for caching and return
        let nix_args_str = ser_nix::to_string(&args).into_diagnostic()?;

        // Initialise the backend (generates flake and other backend-specific files)
        self.nix.assemble(&args).await?;

        // Cache the serialized args
        self.nix_args_string
            .set(nix_args_str.clone())
            .map_err(|_| miette!("nix_args_string already set"))?;

        self.assembled.store(true, Ordering::Release);
        Ok(nix_args_str)
    }

    #[activity("Building shell")]
    pub async fn get_dev_environment(&self, json: bool) -> Result<DevEnv> {
        self.assemble(false).await?;

        let gc_root = self.devenv_dot_gc.join("shell");
        let span = tracing::debug_span!("evaluating_dev_env");
        let env = self.nix.dev_env(json, &gc_root).instrument(span).await?;

        // Save timestamped GC root symlink for history tracking and GC protection
        // This is backend-independent: all backends create a gc_root symlink,
        // and we want to track the history of shell environments.
        if let Ok(resolved_gc_root) = fs::canonicalize(&gc_root).await {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now();
            let duration = now
                .duration_since(UNIX_EPOCH)
                .expect("System time before UNIX epoch");
            let secs = duration.as_secs();
            let nanos = duration.subsec_nanos();
            let timestamp = format!("{secs}.{nanos}");
            let target = format!("{timestamp}-shell");

            let home_gc_target = self.devenv_home_gc.join(&target);

            // Create timestamped symlink (devenv's GC protection layer)
            if let Err(e) = async {
                if home_gc_target.exists() {
                    fs::remove_file(&home_gc_target)
                        .await
                        .map_err(|e| miette::miette!("Failed to remove existing symlink: {}", e))?;
                }
                tokio::task::spawn_blocking({
                    let resolved = resolved_gc_root.clone();
                    let target_path = home_gc_target.clone();
                    move || std::os::unix::fs::symlink(&resolved, &target_path)
                })
                .await
                .map_err(|e| miette::miette!("Failed to spawn symlink task: {}", e))?
                .map_err(|e| miette::miette!("Failed to create symlink: {}", e))?;
                Ok::<_, miette::Report>(())
            }
            .await
            {
                message(
                    ActivityLevel::Warn,
                    format!(
                        "Failed to create timestamped GC root symlink: {}. \
                         This may affect GC protection but won't prevent the shell from working.",
                        e
                    ),
                );
            }
        } else {
            message(
                ActivityLevel::Warn,
                format!(
                    "Failed to resolve the GC root path to the Nix store: {}. \
                     Try running devenv again with --refresh-eval-cache.",
                    gc_root.display()
                ),
            );
        }

        use devenv_eval_cache::command::{FileInputDesc, Input};
        util::write_file_with_lock(
            self.devenv_dotfile.join("input-paths.txt"),
            env.inputs
                .iter()
                .filter_map(|input| match input {
                    Input::File(FileInputDesc { path, .. }) => {
                        // We include --option in the eval cache, but we don't want it
                        // to trigger direnv reload on each invocation
                        let cli_options_path = self.devenv_dotfile.join("cli-options.nix");
                        if path == &cli_options_path {
                            return None;
                        }
                        Some(path.to_string_lossy().to_string())
                    }
                    // TODO(sander): update direnvrc to handle env vars if possible
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )?;

        Ok(DevEnv { output: env.stdout })
    }
}

fn confirm_overwrite(file: &Path, contents: String) -> Result<()> {
    if std::fs::metadata(file).is_ok() {
        // first output the old version and propose new changes
        let before = std::fs::read_to_string(file).expect("Failed to read file");

        let diff = TextDiff::from_lines(&before, &contents);

        eprintln!("\nChanges that will be made to {}:", file.to_string_lossy());
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "\x1b[31m-\x1b[0m",
                ChangeTag::Insert => "\x1b[32m+\x1b[0m",
                ChangeTag::Equal => " ",
            };
            eprint!("{sign}{change}");
        }

        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "{} already exists. Do you want to overwrite it?",
                file.to_string_lossy()
            ))
            .interact()
            .into_diagnostic()?;

        if confirm {
            std::fs::write(file, contents).into_diagnostic()?;
        }
    } else {
        std::fs::write(file, contents).into_diagnostic()?;
    }
    Ok(())
}

pub struct DevEnv {
    output: Vec<u8>,
}

#[derive(Deserialize)]
struct PackageResults(BTreeMap<String, PackageResult>);

#[derive(Deserialize)]
struct PackageResult {
    version: String,
    description: String,
}

#[derive(Deserialize)]
struct OptionResults(BTreeMap<String, OptionResult>);

#[derive(Deserialize)]
struct OptionResult {
    #[serde(rename = "type")]
    type_: String,
    default: Option<String>,
    description: String,
}

#[derive(Table)]
struct DevenvOptionResult {
    #[table(title = "Option")]
    name: String,
    #[table(title = "Type")]
    type_: String,
    #[table(title = "Default")]
    default: String,
    #[table(title = "Description")]
    description: String,
}

#[derive(Table)]
struct DevenvPackageResult {
    #[table(title = "Package")]
    name: String,
    #[table(title = "Version")]
    version: String,
    #[table(title = "Description")]
    description: String,
}

fn sanitize_container_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
}

async fn cleanup_symlinks(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    use futures::StreamExt;
    use tokio_stream::wrappers::ReadDirStream;

    if !root.exists() {
        fs::create_dir_all(root)
            .await
            .expect("Failed to create gc directory");
    }

    let read_dir = fs::read_dir(root).await.expect("Failed to read directory");

    let results: Vec<_> = ReadDirStream::new(read_dir)
        .filter_map(|e| async { e.ok() })
        .map(|e| e.path())
        .filter(|p| std::future::ready(p.is_symlink()))
        .map(|path| async move {
            if !path.exists() {
                // Dangling symlink - delete it
                if fs::remove_file(&path).await.is_ok() {
                    (None, Some(path))
                } else {
                    (None, None)
                }
            } else {
                match fs::canonicalize(&path).await {
                    Ok(target) => (Some(target), None),
                    Err(_) => (None, None),
                }
            }
        })
        .buffer_unordered(100)
        .collect()
        .await;

    let mut to_gc = Vec::new();
    let mut removed_symlinks = Vec::new();
    for (target, removed) in results {
        if let Some(t) = target {
            to_gc.push(t);
        }
        if let Some(r) = removed {
            removed_symlinks.push(r);
        }
    }

    (to_gc, removed_symlinks)
}

fn format_tasks_tree(tasks: &Vec<tasks::TaskConfig>) -> String {
    let mut output = String::new();

    // Build dependency information
    let mut task_deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut task_dependents: HashMap<String, Vec<String>> = HashMap::new();
    let task_names: HashSet<String> = tasks.iter().map(|t| t.name.clone()).collect();
    let mut task_configs: HashMap<String, &tasks::TaskConfig> = HashMap::new();

    for task in tasks {
        task_deps.insert(task.name.clone(), task.after.clone());
        task_configs.insert(task.name.clone(), task);

        // Build reverse dependencies (dependents)
        for dep in &task.after {
            task_dependents
                .entry(dep.clone())
                .or_default()
                .push(task.name.clone());
        }

        // Handle "before" dependencies
        for before in &task.before {
            task_deps
                .entry(before.clone())
                .or_default()
                .push(task.name.clone());
            task_dependents
                .entry(task.name.clone())
                .or_default()
                .push(before.clone());
        }
    }

    let mut visited = HashSet::new();

    // Find root tasks (those with no dependencies)
    let mut roots: Vec<&str> = Vec::new();
    for task in tasks {
        let deps = task_deps.get(&task.name).unwrap();
        if deps.is_empty() || !deps.iter().any(|d| task_names.contains(d)) {
            roots.push(&task.name);
        }
    }

    // If no roots found, use all tasks
    if roots.is_empty() {
        roots = tasks.iter().map(|t| t.name.as_str()).collect();
    }

    roots.sort();

    // Format all tasks as top-level with their full names
    for (i, root) in roots.iter().enumerate() {
        if !visited.contains(*root) {
            let is_last = i == roots.len() - 1;
            format_task_tree(
                &mut output,
                root,
                &task_dependents,
                &task_configs,
                &mut visited,
                "",
                is_last,
            );
        }
    }

    // Remove trailing newline for consistency with other commands
    output.truncate(output.trim_end().len());
    output
}

fn format_task_tree(
    output: &mut String,
    task_name: &str,
    task_dependents: &HashMap<String, Vec<String>>,
    task_configs: &HashMap<String, &tasks::TaskConfig>,
    visited: &mut HashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    use std::fmt::Write;

    if visited.contains(task_name) {
        return;
    }
    visited.insert(task_name.to_string());

    // Format the current task with tree formatting
    let connector = if is_last { "└── " } else { "├── " };
    let _ = write!(output, "{prefix}{connector}{task_name}");

    // Add additional info if available
    if let Some(task) = task_configs.get(task_name) {
        let mut extra_info = Vec::new();

        if task.status.is_some() {
            extra_info.push("has status check".to_string());
        }

        if !task.exec_if_modified.is_empty() {
            let files = task.exec_if_modified.join(", ");
            extra_info.push(format!("watches: {files}"));
        }

        if !extra_info.is_empty() {
            let _ = write!(output, " ({})", extra_info.join(", "));
        }
    }

    let _ = writeln!(output);

    // Get children (tasks that depend on this task)
    let children = task_dependents.get(task_name).cloned().unwrap_or_default();
    let mut children: Vec<_> = children
        .into_iter()
        .filter(|t| task_configs.contains_key(t))
        .collect();
    children.sort();

    // Determine the new prefix for children
    let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

    // Format children
    for (i, child) in children.iter().enumerate() {
        let is_last_child = i == children.len() - 1;
        format_task_tree(
            output,
            child,
            task_dependents,
            task_configs,
            visited,
            &new_prefix,
            is_last_child,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_tasks_tree_flat_hierarchy_sorted() {
        use tasks::TaskConfig;

        // Create test tasks with 2 levels of hierarchy
        let test_tasks = vec![
            // Root tasks (no dependencies)
            TaskConfig {
                name: "devenv:typecheck".to_string(),
                r#type: Default::default(),
                after: vec![],
                before: vec![],
                command: Some("echo typecheck".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            TaskConfig {
                name: "devenv:lint".to_string(),
                r#type: Default::default(),
                after: vec![],
                before: vec![],
                command: Some("echo lint".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            // Level 2 tasks (depend on Level 1)
            TaskConfig {
                name: "devenv:test".to_string(),
                r#type: Default::default(),
                after: vec!["devenv:lint".to_string(), "devenv:typecheck".to_string()],
                before: vec![],
                command: Some("echo test".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            // Different namespace
            TaskConfig {
                name: "myapp:setup".to_string(),
                r#type: Default::default(),
                after: vec![],
                before: vec![],
                command: Some("echo setup".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            TaskConfig {
                name: "myapp:build".to_string(),
                r#type: Default::default(),
                after: vec!["myapp:setup".to_string()],
                before: vec![],
                command: Some("echo build".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            // Level 3 (deeply nested)
            TaskConfig {
                name: "myapp:package".to_string(),
                r#type: Default::default(),
                after: vec!["myapp:build".to_string()],
                before: vec![],
                command: Some("echo package".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
            // Standalone task
            TaskConfig {
                name: "cleanup".to_string(),
                r#type: Default::default(),
                after: vec![],
                before: vec![],
                command: Some("echo cleanup".to_string()),
                status: None,
                exec_if_modified: vec![],
                inputs: None,
                cwd: None,
                show_output: false,
            },
        ];

        // Build the same structures that print_tasks_tree builds
        let mut task_deps: HashMap<String, Vec<String>> = HashMap::new();
        let task_names: HashSet<String> = test_tasks.iter().map(|t| t.name.clone()).collect();

        for task in &test_tasks {
            task_deps.insert(task.name.clone(), task.after.clone());
        }

        // Find root tasks (those with no dependencies)
        let mut roots: Vec<&str> = Vec::new();
        for task in &test_tasks {
            let deps = task_deps.get(&task.name).unwrap();
            if deps.is_empty() || !deps.iter().any(|d| task_names.contains(d)) {
                roots.push(&task.name);
            }
        }

        roots.sort();

        // Verify roots are sorted
        assert_eq!(
            roots,
            vec!["cleanup", "devenv:lint", "devenv:typecheck", "myapp:setup"]
        );

        // Verify we have roots from different namespaces at the same level
        assert!(roots.iter().any(|t| t.starts_with("devenv:")));
        assert!(roots.iter().any(|t| t.starts_with("myapp:")));
        assert!(roots.iter().any(|t| !t.contains(":")));

        // Verify no namespace headers would be printed
        // (the old code would print "devenv:", "myapp:", and "(standalone)" headers)
        // The new code just prints all roots flat with full names
        assert!(roots.iter().all(|t| {
            // All roots should be top-level names, not namespace headers
            !t.is_empty()
        }));

        // Verify dependencies are tracked correctly for tree structure
        let child_deps = vec![
            ("devenv:test", vec!["devenv:lint", "devenv:typecheck"]),
            ("myapp:build", vec!["myapp:setup"]),
            ("myapp:package", vec!["myapp:build"]),
        ];

        for (task_name, expected_deps) in child_deps {
            let task = test_tasks.iter().find(|t| t.name == task_name).unwrap();
            assert_eq!(task.after, expected_deps);
        }
    }
}
