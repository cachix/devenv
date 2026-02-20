use super::{processes, tasks, util};
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
    nix_args::{CliOptionsConfig, NixArgs, SecretspecData, parse_cli_options},
    nix_backend::{DevenvPaths, NixBackend, Options},
    ports::PortAllocator,
};
use devenv_shell::dialect::{BashDialect, ShellDialect};
use include_dir::{Dir, include_dir};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use nix::sys::signal;
use nix::unistd::Pid;
use once_cell::sync::Lazy;
use secrecy::ExposeSecret;
use serde::Deserialize;
use sha2::Digest;
use similar::{ChangeTag, TextDiff};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tasks::{Tasks, TasksUi};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::process;
use tokio::sync::{OnceCell, RwLock, Semaphore};
use tracing::{Instrument, debug, info, instrument, trace, warn};

// templates
// Note: gitignore is stored without the dot to work around include_dir not including dotfiles
const REQUIRED_FILES: [(&str, &str); 3] = [
    ("devenv.nix", "devenv.nix"),
    ("devenv.yaml", "devenv.yaml"),
    ("gitignore", ".gitignore"), // source name -> target name
];
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
    /// When true, fail if a port is in use instead of auto-allocating the next available.
    pub strict_ports: bool,
    /// Command receiver for process control (restart, etc.)
    pub command_rx: Option<tokio::sync::mpsc::Receiver<processes::ProcessCommand>>,
    /// Sender for requesting the TUI to temporarily pause so the terminal
    /// can be used for interactive I/O (e.g., sudo password prompt).
    pub terminal_pause_tx: Option<tokio::sync::mpsc::Sender<devenv_tui::TerminalPauseRequest>>,
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

    // Cached DevEnv result from get_dev_environment_inner, used by up() to avoid
    // redundant activity wrapping when prepare_shell is called later.
    dev_env_cache: Arc<OnceCell<DevEnv>>,

    // Eval-cache pool (framework layer concern, used by backends)
    eval_cache_pool: Arc<OnceCell<SqlitePool>>,

    // Secretspec resolved data to pass to Nix
    secretspec_resolved: Arc<OnceCell<secretspec::Resolved<HashMap<String, String>>>>,

    // Cached serialized NixArgs from assemble
    nix_args_string: Arc<OnceCell<String>>,

    // Port allocator shared with NixBackend for holding port reservations
    port_allocator: Arc<PortAllocator>,

    // TODO: make private.
    // Pass as an arg or have a setter.
    pub container_name: Option<String>,

    // Native process manager started in-process (for detach mode used by test())
    native_process_manager: Arc<OnceCell<Arc<processes::NativeProcessManager>>>,

    // Shutdown handle for coordinated shutdown
    shutdown: Arc<tokio_shutdown::Shutdown>,

    // Task-exported env vars (e.g., PATH with venv/bin, VIRTUAL_ENV) set by
    // run_enter_shell_tasks(). Injected into the bash script by prepare_shell()
    // so they take effect AFTER the Nix shell env is applied.
    task_exports: std::sync::Mutex<HashMap<String, String>>,
}

/// Sanitize profile name to be filesystem-safe
fn sanitize_profile_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ' ' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

/// Compute the profile directory suffix for state isolation
fn compute_profile_dir_suffix(profiles: &[String]) -> Option<String> {
    if profiles.is_empty() {
        None
    } else {
        let mut sorted: Vec<String> = profiles.iter().map(|p| sanitize_profile_name(p)).collect();
        sorted.sort();
        Some(format!("profiles/{}", sorted.join("-")))
    }
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

        // Get global_options early to access profiles for state directory isolation
        let global_options = options.global_options.unwrap_or_default();

        // Compute profile-aware dotfile path for state isolation
        let base_devenv_dotfile = options
            .devenv_dotfile
            .map(|p| p.to_path_buf())
            .unwrap_or(devenv_root.join(".devenv"));
        let devenv_dotfile =
            if let Some(suffix) = compute_profile_dir_suffix(&global_options.profile) {
                base_devenv_dotfile.join(suffix)
            } else {
                base_devenv_dotfile
            };
        let devenv_dot_gc = devenv_dotfile.join("gc");

        // TMPDIR for build artifacts - should NOT use XDG_RUNTIME_DIR as that's
        // a small tmpfs meant for runtime files (sockets), not build artifacts
        let devenv_tmp =
            PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()));

        // first 7 chars of sha256 hash of devenv_state
        let devenv_state_hash = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
            let result = hasher.finalize();
            hex::encode(result)
        };

        // Runtime directory for sockets - XDG_RUNTIME_DIR is the correct location
        // per the XDG Base Directory Specification
        let devenv_runtime_base =
            PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
                std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string())
            }));
        let devenv_runtime =
            devenv_runtime_base.join(format!("devenv-{}", &devenv_state_hash[..7]));

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

        // Create port allocator shared with backend for holding port reservations
        let port_allocator = Arc::new(PortAllocator::new());

        let nix: Box<dyn NixBackend> = match backend_type {
            NixBackendType::Nix => Box::new(
                devenv_nix_backend::nix_backend::NixRustBackend::new(
                    paths,
                    options.config.clone(),
                    global_options.clone(),
                    cachix_manager.clone(),
                    options.shutdown.clone(),
                    Some(eval_cache_pool.clone()),
                    None,
                    port_allocator.clone(),
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
            dev_env_cache: Arc::new(OnceCell::new()),
            eval_cache_pool,
            secretspec_resolved,
            nix_args_string: Arc::new(OnceCell::new()),
            port_allocator,
            container_name: None,
            native_process_manager: Arc::new(OnceCell::new()),
            shutdown: options.shutdown,
            task_exports: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn processes_log(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.log")
    }

    pub fn processes_pid(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.pid")
    }

    async fn processes_running(&self) -> bool {
        if self.processes_pid().exists() {
            if let Ok(pid_str) = fs::read_to_string(self.processes_pid()).await {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    match signal::kill(Pid::from_raw(pid), None) {
                        Ok(_) => return true,
                        Err(nix::errno::Errno::EPERM) => return true,
                        Err(nix::errno::Errno::ESRCH) => {}
                        Err(_) => {}
                    }
                }
            }
        }

        let socket_path = self.devenv_runtime.join("pc.sock");
        let Ok(meta) = fs::metadata(&socket_path).await else {
            return false;
        };
        if !meta.file_type().is_socket() {
            return false;
        }

        match tokio::time::timeout(
            std::time::Duration::from_millis(200),
            UnixStream::connect(&socket_path),
        )
        .await
        {
            Ok(Ok(_)) => true,
            _ => false,
        }
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

    pub fn native_manager_pid_file(&self) -> PathBuf {
        processes::get_process_runtime_dir(&self.devenv_runtime)
            .map(|dir| dir.join("native-manager.pid"))
            .unwrap_or_else(|_| self.devenv_dotfile.join("native-manager.pid"))
    }

    /// Get the path to the .devenv/state directory
    pub fn devenv_state_dir(&self) -> PathBuf {
        self.devenv_dotfile.join("state")
    }

    /// Get the eval cache database pool, if initialized.
    ///
    /// The pool is initialized lazily during `assemble()` when eval caching is enabled.
    pub fn eval_cache_pool(&self) -> Option<&SqlitePool> {
        self.eval_cache_pool.get()
    }

    /// Get the NixArgs string used for cache key computation.
    ///
    /// This is set during `assemble()` and can be used to compute cache keys
    /// for specific evaluations.
    pub fn nix_args_string(&self) -> Option<&str> {
        self.nix_args_string.get().map(|s| s.as_str())
    }

    /// Get the cache key for shell evaluation.
    ///
    /// This returns the same key that was used to cache the shell evaluation,
    /// which can be used to look up the file inputs that the shell depends on.
    ///
    /// The cache key must match the backend's format which includes port allocation info:
    /// `{nix_args}:port_allocation={enabled}:strict_ports={strict}:shell`
    pub fn shell_cache_key(&self) -> Option<devenv_eval_cache::EvalCacheKey> {
        let nix_args_str = self.nix_args_string.get()?;
        // The backend uses cache_key_args = format!("{}:port_allocation={}:strict_ports={}", args_nix, is_enabled, is_strict)
        // We must match this format for the cache key lookup to work
        let cache_key_args = format!(
            "{}:port_allocation={}:strict_ports={}",
            nix_args_str,
            self.port_allocator.is_enabled(),
            self.port_allocator.is_strict()
        );
        Some(devenv_eval_cache::EvalCacheKey::from_nix_args_str(
            &cache_key_args,
            "shell",
        ))
    }

    pub fn init(&self, target: &Option<PathBuf>) -> Result<()> {
        let target = target.clone().unwrap_or_else(|| {
            std::fs::canonicalize(".").expect("Failed to get current directory")
        });

        // create directory target if not exists
        if !target.exists() {
            std::fs::create_dir_all(&target).expect("Failed to create target directory");
        }

        for (source_name, target_name) in REQUIRED_FILES {
            info!("Creating {}", target_name);

            let path = PROJECT_DIR
                .get_file(source_name)
                .ok_or_else(|| miette::miette!("missing {} in the executable", source_name))?;

            // write path.contents to target/target_name
            let target_path = target.join(target_name);

            // add a check for files like .gitignore to append buffer instead of bailing out
            if target_path.exists() && EXISTING_REQUIRED_FILES.contains(&target_name) {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)
                    .and_then(|mut file| {
                        file.write_all(b"\n")?;
                        file.write_all(path.contents())
                    })
                    .expect("Failed to append to existing file");
            } else if target_path.exists() && !EXISTING_REQUIRED_FILES.contains(&target_name) {
                if let Some(utf8_contents) = path.contents_utf8() {
                    confirm_overwrite(&target_path, utf8_contents.to_string())?;
                } else {
                    bail!("Failed to read file contents as UTF-8");
                }
            } else {
                std::fs::write(&target_path, path.contents()).expect("Failed to write file");
            }
        }

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

    /// Invalidate cached state for hot-reload.
    ///
    /// This clears evaluation caches to force re-evaluation when files change.
    /// Must be called before `print_dev_env()` during hot-reload to pick up changes.
    pub fn invalidate_for_reload(&self) {
        self.nix.invalidate();
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
        // Use cached DevEnv if available (set by up() Phase 1), otherwise
        // call get_dev_environment which wraps with "Configuring shell" activity.
        let owned_dev_env;
        let output = if let Some(cached) = self.dev_env_cache.get() {
            &cached.output
        } else {
            owned_dev_env = self.get_dev_environment(false).await?;
            &owned_dev_env.output
        };

        let bash = match self.nix.get_bash(false).await {
            Err(e) => {
                trace!("Failed to get bash: {}. Rebuilding.", e);
                self.nix.get_bash(true).await?
            }
            Ok(bash) => bash,
        };

        let mut shell_cmd = process::Command::new(&bash);

        // The Nix output ends with "exec bash" which would start a new shell without
        // the devenv environment. Strip it for ALL modes - we handle shell execution ourselves.
        let output_str = String::from_utf8_lossy(&output);
        let shell_env = output_str
            .trim_end()
            .trim_end_matches("exec bash")
            .trim_end_matches("exec $SHELL")
            .to_string();

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
            shell_env
        };

        // Inject task-exported env vars (e.g., PATH with venv/bin, VIRTUAL_ENV)
        // after the Nix shell env is applied so they aren't overridden.
        {
            let exports = self.task_exports.lock().unwrap();
            for (key, value) in exports.iter() {
                script.push_str(&format!(
                    "export {}={}\n",
                    shell_escape::escape(std::borrow::Cow::Borrowed(key)),
                    shell_escape::escape(std::borrow::Cow::Borrowed(value))
                ));
            }
        }

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
                let dialect = BashDialect;
                let interactive_args = dialect.interactive_args();
                shell_cmd.args(&interactive_args.prefix);
                shell_cmd.arg(&script_path);
                shell_cmd.args(&interactive_args.suffix);
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

    /// Run a command and return the output, streaming stdout/stderr to the TUI.
    ///
    /// This method accepts `String` (not `Option<String>`) because it's specifically
    /// designed for running commands and capturing their output. Unlike `exec_in_shell`,
    /// this method always requires a command and spawns the process to stream output
    /// line by line to the TUI activity.
    pub async fn run_in_shell(
        &self,
        cmd: String,
        args: &[String],
        activity_name: Option<&str>,
    ) -> Result<Output> {
        let mut shell_cmd = self.prepare_shell(&Some(cmd), args).await?;
        shell_cmd.stdout(Stdio::piped());
        shell_cmd.stderr(Stdio::piped());

        let activity = Activity::operation(activity_name.unwrap_or("Running in shell")).start();

        let mut child = shell_cmd.spawn().into_diagnostic()?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut stdout_bytes: Vec<u8> = Vec::new();
        let mut stderr_bytes: Vec<u8> = Vec::new();
        let mut stdout_closed = false;
        let mut stderr_closed = false;

        loop {
            if stdout_closed && stderr_closed {
                break;
            }

            tokio::select! {
                result = stdout_reader.next_line(), if !stdout_closed => {
                    match result {
                        Ok(Some(line)) => {
                            activity.log(&line);
                            stdout_bytes.extend(line.as_bytes());
                            stdout_bytes.push(b'\n');
                        }
                        Ok(None) => stdout_closed = true,
                        Err(e) => {
                            activity.error(format!("Error reading stdout: {e}"));
                            stdout_closed = true;
                        }
                    }
                }
                result = stderr_reader.next_line(), if !stderr_closed => {
                    match result {
                        Ok(Some(line)) => {
                            activity.error(&line);
                            stderr_bytes.extend(line.as_bytes());
                            stderr_bytes.push(b'\n');
                        }
                        Ok(None) => stderr_closed = true,
                        Err(e) => {
                            activity.error(format!("Error reading stderr: {e}"));
                            stderr_closed = true;
                        }
                    }
                }
            }
        }

        let status = child.wait().await.into_diagnostic()?;

        if !status.success() {
            activity.fail();
        }

        Ok(Output {
            status,
            stdout: stdout_bytes,
            stderr: stderr_bytes,
        })
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

        eprintln!(
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
        let search_results = self.nix.search(name, Some(search_options)).await?;
        let results = search_results
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

        Ok(results)
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
            .map_err(|e| miette::miette!("Failed to read task config file: {}", e))?;
        let tasks: Vec<tasks::TaskConfig> = serde_json::from_str(&tasks_json)
            .map_err(|e| miette::miette!("Failed to parse task config: {}", e))?;

        // Cache task names for shell completions
        let task_names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        let cache_path = self.devenv_dotfile.join("task-names.txt");
        if let Err(e) = fs::write(&cache_path, task_names.join("\n")).await {
            debug!("Failed to write task name cache for completions: {}", e);
        }

        Ok(tasks)
    }

    /// Run tasks and return their outputs as JSON string.
    pub async fn tasks_run(
        &self,
        roots: Vec<String>,
        run_mode: devenv_tasks::RunMode,
        show_output: bool,
        cli_inputs: Vec<String>,
        input_json: Option<String>,
    ) -> Result<String> {
        self.assemble(false).await?;
        if roots.is_empty() {
            bail!("No tasks specified.");
        }

        // Capture the shell environment to ensure tasks run with proper devenv setup
        let envs = self.capture_shell_environment().await?;

        let mut tasks = self.load_tasks().await?;

        // If --show-output flag is present, enable output for all tasks
        if show_output {
            for task in &mut tasks {
                task.show_output = true;
            }
        }

        // Parse and merge CLI inputs into root task configs
        let cli_input = parse_cli_task_inputs(&cli_inputs, input_json.as_deref())?;
        if !cli_input.is_empty() {
            for task in &mut tasks {
                if roots
                    .iter()
                    .any(|root| task.name == *root || task.name.starts_with(&format!("{root}:")))
                {
                    merge_task_input(task, &cli_input)?;
                }
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

        let runtime_dir = processes::get_process_runtime_dir(&self.devenv_runtime)?;
        let config = tasks::Config {
            roots,
            tasks,
            run_mode,
            runtime_dir,
            cache_dir: self.devenv_dotfile.clone(),
            sudo_context: None,
            env: envs,
        };

        if let Ok(config_value) = devenv_activity::SerdeValue::from_serialize(&config) {
            use valuable::Valuable;
            debug!(event = config_value.as_value(), "Loaded task config");
        }

        let tasks = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown))
            .with_refresh_task_cache(self.global_options.refresh_task_cache)
            .build()
            .await?;

        // In TUI mode, skip TasksUi to avoid corrupting the TUI display
        // TUI captures activity events directly via the channel initialized in main.rs
        let (status, outputs) = if self.global_options.tui {
            let outputs = tasks.run(false).await;
            let status = tasks.get_completion_status().await;
            (status, outputs)
        } else {
            // Shell mode - initialize activity channel for TasksUi
            let (activity_rx, activity_handle) = devenv_activity::init();
            activity_handle.install();

            let tasks = Arc::new(tasks);
            let tasks_clone = Arc::clone(&tasks);

            // Spawn task runner - UI will detect completion via JoinHandle
            let run_handle = tokio::spawn(async move { tasks_clone.run(false).await });

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

    /// Run enterShell tasks and return env vars exported by tasks (e.g., PATH with venv/bin).
    /// This runs tasks via Rust (not bash hook) to enable TUI progress reporting.
    /// Task failures are logged as warnings but don't prevent shell entry.
    pub async fn run_enter_shell_tasks(&self) -> Result<HashMap<String, String>> {
        self.run_enter_shell_tasks_with_executor(None, None).await
    }

    /// Run enterShell tasks with a custom executor and optional pre-captured shell env.
    /// Used for running tasks inside a PTY for hot-reload mode, or from test() which
    /// already captured envs in Phase 1.
    /// Task failures are logged as warnings but don't prevent shell entry.
    pub async fn run_enter_shell_tasks_with_executor(
        &self,
        executor: Option<Arc<dyn tasks::TaskExecutor>>,
        pre_captured_envs: Option<HashMap<String, String>>,
    ) -> Result<HashMap<String, String>> {
        self.assemble(false).await?;

        // Use pre-captured envs if provided (e.g. from test() Phase 1), otherwise capture fresh.
        let envs = match pre_captured_envs {
            Some(e) => e,
            None => self.capture_shell_environment().await?,
        };

        let task_configs = self.load_tasks().await?;
        self.run_enter_shell_tasks_inner(task_configs, executor, envs)
            .await
    }

    /// Core logic for running enterShell tasks with a pre-loaded task config.
    /// Accepts an optional custom executor (e.g., PTY executor for hot-reload mode).
    /// Stores task-exported env vars on self so prepare_shell() can inject them.
    async fn run_enter_shell_tasks_inner(
        &self,
        task_configs: Vec<tasks::TaskConfig>,
        executor: Option<Arc<dyn tasks::TaskExecutor>>,
        envs: HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let verbosity = if self.global_options.quiet {
            tasks::VerbosityLevel::Quiet
        } else if self.global_options.verbose {
            tasks::VerbosityLevel::Verbose
        } else {
            tasks::VerbosityLevel::Normal
        };

        let config = tasks::Config {
            roots: vec!["devenv:enterShell".to_string()],
            tasks: task_configs,
            run_mode: devenv_tasks::RunMode::All,
            runtime_dir: self.devenv_runtime.clone(),
            cache_dir: self.devenv_dotfile.clone(),
            sudo_context: None,
            env: envs,
        };

        let has_custom_executor = executor.is_some();
        let mut builder = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown));
        if let Some(exec) = executor {
            builder = builder.with_executor(exec);
        }
        let tasks = builder.build().await?;

        // Custom executor implies PTY/TUI mode; otherwise respect the tui flag.
        // In TUI mode, skip TasksUi to avoid corrupting the TUI display —
        // the TUI captures activity events directly via the channel in main.rs.
        let outputs = if has_custom_executor || self.global_options.tui {
            tasks.run(false).await
        } else {
            // Shell mode - initialize activity channel for TasksUi
            let (activity_rx, activity_handle) = devenv_activity::init();
            activity_handle.install();

            let tasks = Arc::new(tasks);
            let tasks_clone = Arc::clone(&tasks);

            // Spawn task runner - UI will detect completion via JoinHandle
            let run_handle = tokio::spawn(async move { tasks_clone.run(false).await });

            // Run UI - processes events and waits for run_handle
            let ui = TasksUi::new(Arc::clone(&tasks), activity_rx, verbosity);
            let (_status, outputs) = ui.run(run_handle).await?;
            outputs
        };

        // Note: Task failures are shown in the TUI/UI output, no need to bail here.
        // Shell entry proceeds even if some tasks fail (matches interactive reload behavior).

        let exports = Self::collect_task_exports(&outputs);
        // Store on self so prepare_shell() can inject them into the bash script
        *self.task_exports.lock().unwrap() = exports.clone();
        Ok(exports)
    }

    /// Get the shell environment as a map of environment variables.
    /// This captures the environment after running the devenv shell script.
    pub async fn get_shell_environment(&self) -> Result<HashMap<String, String>> {
        self.capture_shell_environment().await
    }

    /// Get the path to bash.
    pub async fn get_bash_path(&self) -> Result<String> {
        match self.nix.get_bash(false).await {
            Err(e) => {
                tracing::trace!("Failed to get bash: {}. Rebuilding.", e);
                Ok(self.nix.get_bash(true).await?)
            }
            Ok(bash) => Ok(bash),
        }
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
        // We need to let enterShell tasks run to ensure the complete environment is captured
        // (e.g., Python virtualenv setup adds .devenv/state/venv/bin to PATH)
        let output = self
            .prepare_shell(&Some(script_path.to_string_lossy().into()), &[])
            .await?
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

    /// Extract env vars exported by tasks (e.g., PATH from Python venv)
    /// from task outputs into a HashMap.
    fn collect_task_exports(outputs: &tasks::Outputs) -> HashMap<String, String> {
        let mut envs = HashMap::new();
        for value in outputs.values() {
            if let Some(env_obj) = value
                .get("devenv")
                .and_then(|d| d.get("env"))
                .and_then(|e| e.as_object())
            {
                for (env_key, env_value) in env_obj {
                    if let Some(env_str) = env_value.as_str() {
                        envs.insert(env_key.clone(), env_str.to_string());
                    }
                }
            }
        }
        envs
    }

    pub async fn test(&self) -> Result<()> {
        // Enable port allocation before assemble so that ports resolved
        // during Nix evaluation (e.g. in enterTest) are properly allocated.
        self.port_allocator.set_enabled(true);

        // ── Phase 1: Configuring shell ──────────────────────────────
        // Assemble with testing enabled, build dev environment, cache it,
        // check for processes, and capture shell env vars.
        let (has_processes, envs) = {
            let phase1 = Activity::operation("Configuring shell")
                .parent(None)
                .start();
            async {
                self.assemble(true).await?;
                let dev_env = self.get_dev_environment_inner(false).await?;
                let _ = self.dev_env_cache.set(dev_env);
                let has_processes = self.has_processes().await?;
                let envs = self.capture_shell_environment().await?;
                Ok::<(bool, HashMap<String, String>), miette::Report>((has_processes, envs))
            }
            .in_activity(&phase1)
            .await?
        };

        // ── Phase 2: Running enterShell tasks ─────────────────────
        // Run tasks like devenv:python:virtualenv that set up the environment
        // (e.g., create venv, pip install, export PATH/VIRTUAL_ENV).
        // In devenv 2.0+, these don't run via the bash enterShell hook.
        // Exports are stored on self so prepare_shell() injects them into the
        // bash script AFTER the Nix shell env is applied.
        // When processes are present, up() (Phase 4) calls run_enter_shell_tasks_inner()
        // as part of its startup sequence, storing exports in self.task_exports.
        if !has_processes {
            self.run_enter_shell_tasks_with_executor(None, Some(envs.clone()))
                .await?;
        }

        // ── Phase 3: Building tests ─────────────────────────────────
        let test_script = {
            let phase3 = Activity::operation("Building tests").parent(None).start();
            async {
                let gc_root = self.devenv_dot_gc.join("test");
                let test_script = self
                    .nix
                    .build(&["devenv.config.test"], None, Some(&gc_root))
                    .await?;
                Ok::<String, miette::Report>(test_script[0].to_string_lossy().to_string())
            }
            .in_activity(&phase3)
            .await?
        };

        // ── Phase 4: Starting processes (if needed) ─────────────────
        if has_processes {
            let options = ProcessOptions {
                envs: Some(&envs),
                detach: true,
                log_to_file: false,
                strict_ports: false,
                command_rx: None,
                terminal_pause_tx: None,
            };
            self.up(vec![], options).await?;
        }

        // ── Phase 5: Running tests ──────────────────────────────────
        // prepare_shell will use cached dev_env, avoiding redundant activity wrapping.
        let result = self
            .run_in_shell(test_script, &[], Some("Running tests"))
            .await?;

        // ── Phase 6: Stopping processes ─────────────────────────────
        if has_processes {
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
        mut options: ProcessOptions<'a>,
    ) -> Result<RunMode> {
        // Set strict port mode before assemble (which triggers port allocation)
        self.port_allocator.set_strict(options.strict_ports);
        self.port_allocator.set_enabled(true);

        // ── Phase 1: Configuring shell ──────────────────────────────
        // Assemble, check processes, build dev environment, capture shell env vars.
        // When called from test(), dev_env_cache is already populated and envs are
        // passed in, so we skip the activity to avoid a duplicate "Configuring shell".
        let mut envs = if self.dev_env_cache.get().is_some() && options.envs.is_some() {
            options.envs.unwrap().clone()
        } else {
            let phase1 = Activity::operation("Configuring shell")
                .parent(None)
                .start();
            async {
                self.assemble(false).await?;
                if !self.has_processes().await? {
                    message(
                        ActivityLevel::Error,
                        "No 'processes' option defined: https://devenv.sh/processes/",
                    );
                    bail!("No processes defined");
                }

                let dev_env = self.get_dev_environment_inner(false).await?;
                let _ = self.dev_env_cache.set(dev_env);

                // Capture shell environment (uses cached dev_env via prepare_shell)
                let envs = if let Some(envs) = options.envs {
                    envs.clone()
                } else {
                    self.capture_shell_environment().await?
                };

                Ok::<HashMap<String, String>, miette::Report>(envs)
            }
            .in_activity(&phase1)
            .await?
        };

        // ── Phase 2: Loading tasks ──────────────────────────────────
        // load_tasks() already has #[activity("Loading tasks")]
        let task_configs = self.load_tasks().await?;

        // ── Phase 3: Running enterShell tasks ───────────────────────
        // Merge task-exported env vars (e.g., PATH with venv/bin) on top of
        // the nix shell env. Task exports take precedence.
        // Exports are also stored on self so prepare_shell() can inject them
        // into the bash script (e.g. when called from test()).
        // Reuse task_configs from Phase 2 to avoid a redundant load_tasks() call.
        {
            let exports = self
                .run_enter_shell_tasks_inner(task_configs.clone(), None, envs.clone())
                .await?;
            envs.extend(exports);
        }

        // ── Phase 4: Running processes ──────────────────────────────
        // Release port reservations so processes can bind their allocated ports.
        // The port allocator holds TcpListeners during Nix evaluation to prevent
        // race conditions; dropping them here makes the ports available.
        drop(self.port_allocator.take_reservations());

        // Check which process manager to use
        let implementation = {
            let phase4 = Activity::operation("Running processes")
                .parent(None)
                .start();
            let impl_result = async {
                self.nix
                    .eval(&["devenv.config.process.manager.implementation"])
                    .await
            }
            .in_activity(&phase4)
            .await?
            .trim()
            .trim_matches('"')
            .to_string();

            // Create appropriate manager based on implementation
            if impl_result == "native" {
                info!("Using native process manager with task-based dependency ordering");

                // Reuse task_configs from Phase 2 for process tasks
                let roots: Vec<String> = if processes.is_empty() {
                    task_configs
                        .iter()
                        .filter(|t| t.name.starts_with("devenv:processes:"))
                        .map(|t| t.name.clone())
                        .collect()
                } else {
                    processes
                        .iter()
                        .map(|p| format!("devenv:processes:{}", p))
                        .collect()
                };

                if roots.is_empty() {
                    bail!("No process tasks found to run");
                }

                debug!(
                    "Running {} process tasks with dependency ordering: {:?}",
                    roots.len(),
                    roots
                );

                let runtime_dir = processes::get_process_runtime_dir(&self.devenv_runtime)?;

                // On Linux: check whether any process task needs capabilities, and if
                // so authenticate sudo and locate the cap-server binary *before*
                // task_configs is consumed.
                //
                // When NOPASSWD is configured (or credentials are cached), the
                // non-interactive check succeeds without any terminal I/O, so
                // the TUI is undisturbed.  Otherwise we ask the TUI to pause so
                // the user can type their password.
                #[cfg(target_os = "linux")]
                let cap_server_binary: Option<std::path::PathBuf> = {
                    let needs_caps = task_configs
                        .iter()
                        .filter_map(|t| t.process.as_ref())
                        .any(|p| !p.linux.capabilities.is_empty());

                    if needs_caps {
                        match devenv_caps::client::find_cap_server_binary() {
                            Some(binary) => {
                                if devenv_caps::client::can_sudo_noninteractive(&binary) {
                                    // NOPASSWD or cached session — no prompt needed.
                                    Some(binary)
                                } else if let Some(ref pause_tx) = options.terminal_pause_tx {
                                    // Need a password — ask the TUI to step aside.
                                    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                                    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                                    let req =
                                        devenv_tui::TerminalPauseRequest { ready_tx, done_rx };
                                    if pause_tx.send(req).await.is_ok() {
                                        // Wait for the TUI to clear and restore the terminal.
                                        let _ = ready_rx.await;
                                        let auth_result =
                                            devenv_caps::client::preflight_sudo_auth(&binary);
                                        // Signal the TUI to resume (drop or send).
                                        let _ = done_tx.send(());
                                        auth_result?;
                                        Some(binary)
                                    } else {
                                        // TUI is gone — fall through to direct prompt.
                                        devenv_caps::client::preflight_sudo_auth(&binary)?;
                                        Some(binary)
                                    }
                                } else {
                                    // No TUI (legacy/tracing mode) — prompt directly.
                                    devenv_caps::client::preflight_sudo_auth(&binary)?;
                                    Some(binary)
                                }
                            }
                            None => {
                                warn!(
                                    "devenv-cap-server not found; processes requiring \
                                     capabilities will fall back to pre_exec"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    }
                };

                let config = tasks::Config {
                    tasks: task_configs,
                    roots,
                    run_mode: tasks::RunMode::All,
                    runtime_dir,
                    cache_dir: self.devenv_dotfile.clone(),
                    sudo_context: None,
                    env: envs,
                };

                let tasks_runner = tasks::Tasks::builder(
                    config,
                    tasks::VerbosityLevel::Normal,
                    self.shutdown.clone(),
                )
                .build()
                .await
                .map_err(|e| miette!("Failed to build task runner: {}", e))?;

                // Pass the cap-server binary path to the process manager so that
                // configure_spawn_via_cap_server() can use it when processes start.
                #[cfg(target_os = "linux")]
                if let Some(binary) = cap_server_binary {
                    tasks_runner.process_manager.set_cap_server_binary(binary);
                }

                let command_rx = options.command_rx.take();

                // Run process tasks under the Phase 4 activity
                let _outputs = tasks_runner
                    .run_with_parent_activity(Arc::new(phase4))
                    .await;

                // Start the API server so process IPC (status, restart, etc.) works
                tasks_runner.process_manager.start_api_server()?;

                let pid_file = tasks_runner.process_manager.manager_pid_file();
                processes::write_pid(&pid_file, std::process::id())
                    .await
                    .map_err(|e| miette!("Failed to write manager PID: {}", e))?;

                if !options.detach {
                    let result = tasks_runner
                        .process_manager
                        .run_foreground(self.shutdown.cancellation_token(), command_rx)
                        .await
                        .map_err(|e| miette!("Process manager error: {}", e));

                    let _ = tokio::fs::remove_file(&pid_file).await;
                    result?;
                } else {
                    // Store manager for later stop via down()
                    let _ = self
                        .native_process_manager
                        .set(Arc::clone(&tasks_runner.process_manager));
                }

                return Ok(RunMode::Detached);
            }

            // Non-native manager (process-compose)
            let manager: Box<dyn processes::ProcessManager> = {
                let procfile_script = async {
                    let gc_root = self.devenv_dot_gc.join("procfilescript");
                    let paths = self
                        .nix
                        .build(&["devenv.config.procfileScript"], None, Some(&gc_root))
                        .await?;
                    Ok::<PathBuf, miette::Report>(paths[0].clone())
                }
                .in_activity(&phase4)
                .await?;

                Box::new(processes::ProcessComposeManager::new(
                    procfile_script,
                    self.devenv_dotfile.clone(),
                ))
            };

            let start_options = processes::StartOptions {
                process_configs: HashMap::new(),
                processes,
                detach: options.detach,
                log_to_file: options.log_to_file,
                env: envs,
                cancellation_token: Some(self.shutdown.cancellation_token()),
                cap_server_binary: None,
            };

            manager.start(start_options).await?;

            // ProcessComposeManager foreground mode uses exec() and never returns here.
            // In detached mode, we reach here.
            Ok::<RunMode, miette::Report>(RunMode::Detached)
        };

        implementation
    }

    pub async fn down(&self) -> Result<()> {
        // In-process native manager (started by test() or up(detach=true))
        if let Some(manager) = self.native_process_manager.get() {
            manager.stop_all().await?;
            return Ok(());
        }

        // Determine which manager is running and create appropriate instance
        let manager: Box<dyn processes::ProcessManager> = if self.native_manager_pid_file().exists()
        {
            // Native process manager is running
            let runtime_dir = processes::get_process_runtime_dir(&self.devenv_runtime)?;
            Box::new(processes::NativeProcessManager::new(runtime_dir)?)
        } else if self.processes_pid().exists() {
            // Process-compose is running
            // We don't need the procfile_script for stopping, just use a dummy path
            Box::new(processes::ProcessComposeManager::new(
                PathBuf::new(),
                self.devenv_dotfile.clone(),
            ))
        } else {
            bail!("No processes running");
        };

        manager.stop().await
    }

    pub async fn wait_for_ready(&self, timeout: std::time::Duration) -> Result<()> {
        if self.native_manager_pid_file().exists() {
            let runtime_dir = processes::get_process_runtime_dir(&self.devenv_runtime)?;
            let socket_path = runtime_dir.join("native.sock");
            tokio::time::timeout(
                timeout,
                processes::NativeProcessManager::wait_for_ready(&socket_path),
            )
            .await
            .map_err(|_| miette!("Timed out waiting for processes to be ready"))?
        } else if self.processes_pid().exists() {
            bail!("'devenv processes wait' is not yet supported for the process-compose backend")
        } else {
            bail!("No processes running")
        }
    }

    /// Assemble the devenv environment and return the serialized NixArgs string.
    /// The returned string can be used with `import bootstrap/default.nix <args>`.
    pub async fn assemble(&self, is_testing: bool) -> Result<String> {
        let processes_running = self.processes_running().await;
        self.port_allocator.set_allow_in_use(processes_running);

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

        // TODO: remove devenv_dotfile_path and derive the relative path inside NixArgs instead
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

        // Parse CLI options into structured format with typed values
        let cli_options = CliOptionsConfig(parse_cli_options(&self.global_options.option)?);

        // Compute lock fingerprint for eval-cache invalidation
        // This includes narHashes from local path inputs that aren't stored in devenv.lock
        let lock_fingerprint = self.nix.lock_fingerprint().await?;

        // Create the Nix arguments struct
        let nixpkgs_config = config.nixpkgs_config(&self.global_options.system);
        let args = NixArgs {
            version: crate_version!(),
            is_development_version: crate::is_development_version(),
            system: &self.global_options.system,
            devenv_root: &self.devenv_root,
            skip_local_src: self.global_options.from.is_some()
                || (!self.global_options.option.is_empty()
                    && !self.devenv_root.join("devenv.nix").exists()),
            devenv_dotfile: &self.devenv_dotfile,
            devenv_dotfile_path: &dotfile_relative_path,
            devenv_tmpdir: &self.devenv_tmp,
            devenv_runtime: &self.devenv_runtime,
            devenv_istesting: is_testing,
            devenv_direnvrc_latest_version: *DIRENVRC_VERSION,
            container_name: self.container_name.as_deref(),
            active_profiles: &active_profiles,
            cli_options,
            hostname: hostname.as_deref(),
            username: username.as_deref(),
            git_root: git_root.as_deref(),
            secretspec: secretspec_data.as_ref(),
            devenv_config: &config,
            nixpkgs_config,
            lock_fingerprint: &lock_fingerprint,
        };

        // Serialize NixArgs for caching and return
        let nix_args_str = ser_nix::to_string(&args).into_diagnostic()?;

        // Initialise the backend (generates flake and other backend-specific files)
        self.nix.assemble(&args).await?;

        // Cache the serialized args
        self.nix_args_string
            .set(nix_args_str.clone())
            .expect("nix_args_string should only be set once");

        self.assembled.store(true, Ordering::Release);
        Ok(nix_args_str)
    }

    /// Inner implementation without activity wrapper.
    /// Called directly by `up()` (which creates its own "Configuring shell" activity)
    /// and by `get_dev_environment()` (which wraps with `#[activity]`).
    async fn get_dev_environment_inner(&self, json: bool) -> Result<DevEnv> {
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

        util::write_file_with_lock(
            self.devenv_dotfile.join("input-paths.txt"),
            env.inputs
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )?;

        Ok(DevEnv {
            output: env.bash_env,
        })
    }

    /// Get dev environment with "Configuring shell" activity wrapper.
    /// Used by non-up callers (shell, print-dev-env).
    #[activity("Configuring shell")]
    pub async fn get_dev_environment(&self, json: bool) -> Result<DevEnv> {
        self.get_dev_environment_inner(json).await
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

/// Parse CLI `--input key=value` and `--input-json '{...}'` into a JSON object map.
///
/// The `--input-json` value (if any) is used as the base, then each `--input key=value`
/// is layered on top. Values are parsed as JSON if valid, otherwise treated as strings.
fn parse_cli_task_inputs(
    inputs: &[String],
    input_json: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut map: serde_json::Map<String, serde_json::Value> = if let Some(json_str) = input_json {
        let value: serde_json::Value = serde_json::from_str(json_str)
            .into_diagnostic()
            .wrap_err("--input-json must be valid JSON")?;
        match value {
            serde_json::Value::Object(m) => m,
            _ => bail!("--input-json must be a JSON object"),
        }
    } else {
        serde_json::Map::new()
    };

    for entry in inputs {
        let (key, raw_value) = entry
            .split_once('=')
            .ok_or_else(|| miette!("--input must be KEY=VALUE, got: {entry}"))?;
        if key.is_empty() {
            bail!("--input key must not be empty, got: {entry}");
        }
        let value = match serde_json::from_str::<serde_json::Value>(raw_value) {
            Ok(v) => v,
            Err(_) => serde_json::Value::String(raw_value.to_string()),
        };
        map.insert(key.to_string(), value);
    }

    Ok(map)
}

/// Merge CLI inputs into a task config's `input` field (shallow merge, CLI wins).
fn merge_task_input(
    task: &mut tasks::TaskConfig,
    cli_input: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let existing = task
        .input
        .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    match existing {
        serde_json::Value::Object(obj) => {
            for (k, v) in cli_input {
                obj.insert(k.clone(), v.clone());
            }
            Ok(())
        }
        _ => bail!(
            "Task '{}' has a non-object input; cannot merge CLI inputs",
            task.name
        ),
    }
}

fn format_tasks_tree(tasks: &Vec<tasks::TaskConfig>) -> String {
    use std::fmt::Write;

    let mut output = String::new();

    // Build task config lookup for extra info
    let task_configs: HashMap<&str, &tasks::TaskConfig> =
        tasks.iter().map(|t| (t.name.as_str(), t)).collect();

    // Get hierarchy edges from the shared function
    let edges = tasks::compute_display_hierarchy(tasks);

    // Build parent -> children mapping
    let mut children_map: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
    for (parent, child) in &edges {
        children_map
            .entry(parent.as_deref())
            .or_default()
            .push(child.as_str());
    }

    // Sort children at each level
    for children in children_map.values_mut() {
        children.sort();
    }

    // Track visited tasks to avoid duplicates
    let mut visited = HashSet::new();

    // Recursive function to format a task and its children
    fn format_task(
        output: &mut String,
        task_name: &str,
        children_map: &HashMap<Option<&str>, Vec<&str>>,
        task_configs: &HashMap<&str, &tasks::TaskConfig>,
        visited: &mut HashSet<String>,
        prefix: &str,
        is_last: bool,
    ) {
        if visited.contains(task_name) {
            return;
        }
        visited.insert(task_name.to_string());

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

        // Get children of this task
        let children = children_map
            .get(&Some(task_name))
            .cloned()
            .unwrap_or_default();
        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

        for (i, child) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            format_task(
                output,
                child,
                children_map,
                task_configs,
                visited,
                &new_prefix,
                is_last_child,
            );
        }
    }

    // Format root tasks (those with None as parent)
    let roots = children_map.get(&None).cloned().unwrap_or_default();
    for (i, root) in roots.iter().enumerate() {
        let is_last = i == roots.len() - 1;
        format_task(
            &mut output,
            root,
            &children_map,
            &task_configs,
            &mut visited,
            "",
            is_last,
        );
    }

    // Remove trailing newline for consistency with other commands
    output.truncate(output.trim_end().len());
    output
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
                command: Some("echo typecheck".to_string()),
                ..Default::default()
            },
            TaskConfig {
                name: "devenv:lint".to_string(),
                command: Some("echo lint".to_string()),
                ..Default::default()
            },
            // Level 2 tasks (depend on Level 1)
            TaskConfig {
                name: "devenv:test".to_string(),
                after: vec!["devenv:lint".to_string(), "devenv:typecheck".to_string()],
                command: Some("echo test".to_string()),
                ..Default::default()
            },
            // Different namespace
            TaskConfig {
                name: "myapp:setup".to_string(),
                command: Some("echo setup".to_string()),
                ..Default::default()
            },
            TaskConfig {
                name: "myapp:build".to_string(),
                after: vec!["myapp:setup".to_string()],
                command: Some("echo build".to_string()),
                ..Default::default()
            },
            // Level 3 (deeply nested)
            TaskConfig {
                name: "myapp:package".to_string(),
                after: vec!["myapp:build".to_string()],
                command: Some("echo package".to_string()),
                ..Default::default()
            },
            // Standalone task
            TaskConfig {
                name: "cleanup".to_string(),
                command: Some("echo cleanup".to_string()),
                ..Default::default()
            },
        ];

        // Use the shared function to compute hierarchy
        let edges = tasks::compute_display_hierarchy(&test_tasks);

        // Build parent -> children mapping
        let mut children_map: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
        for (parent, child) in &edges {
            children_map
                .entry(parent.as_deref())
                .or_default()
                .push(child.as_str());
        }

        // Get root tasks (those with None as parent)
        let mut roots: Vec<&str> = children_map.get(&None).cloned().unwrap_or_default();
        roots.sort();

        // Verify roots are sorted - these are entry points (tasks nothing depends on)
        assert_eq!(roots, vec!["cleanup", "devenv:test", "myapp:package"]);

        // Verify we have roots from different namespaces at the same level
        assert!(roots.iter().any(|t| t.starts_with("devenv:")));
        assert!(roots.iter().any(|t| t.starts_with("myapp:")));
        assert!(roots.iter().any(|t| !t.contains(":")));

        // Verify children are dependencies (tasks the parent depends on)
        let mut test_children: Vec<&str> = children_map
            .get(&Some("devenv:test"))
            .cloned()
            .unwrap_or_default();
        test_children.sort();
        assert_eq!(test_children, vec!["devenv:lint", "devenv:typecheck"]);

        let mut package_children: Vec<&str> = children_map
            .get(&Some("myapp:package"))
            .cloned()
            .unwrap_or_default();
        package_children.sort();
        assert_eq!(package_children, vec!["myapp:build"]);

        let mut build_children: Vec<&str> = children_map
            .get(&Some("myapp:build"))
            .cloned()
            .unwrap_or_default();
        build_children.sort();
        assert_eq!(build_children, vec!["myapp:setup"]);
    }

    #[test]
    fn test_parse_cli_task_inputs_empty() {
        let result = parse_cli_task_inputs(&[], None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_cli_task_inputs_key_value_string() {
        let inputs = vec!["name=hello".to_string()];
        let result = parse_cli_task_inputs(&inputs, None).unwrap();
        assert_eq!(
            result.get("name").unwrap(),
            &serde_json::Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_parse_cli_task_inputs_key_value_json() {
        let inputs = vec!["count=3".to_string(), "flag=true".to_string()];
        let result = parse_cli_task_inputs(&inputs, None).unwrap();
        assert_eq!(result.get("count").unwrap(), &serde_json::json!(3));
        assert_eq!(result.get("flag").unwrap(), &serde_json::json!(true));
    }

    #[test]
    fn test_parse_cli_task_inputs_json_base() {
        let result = parse_cli_task_inputs(&[], Some(r#"{"a":1,"b":"two"}"#)).unwrap();
        assert_eq!(result.get("a").unwrap(), &serde_json::json!(1));
        assert_eq!(
            result.get("b").unwrap(),
            &serde_json::Value::String("two".to_string())
        );
    }

    #[test]
    fn test_parse_cli_task_inputs_json_override() {
        let inputs = vec!["a=99".to_string()];
        let result = parse_cli_task_inputs(&inputs, Some(r#"{"a":1,"b":"two"}"#)).unwrap();
        assert_eq!(result.get("a").unwrap(), &serde_json::json!(99));
        assert_eq!(
            result.get("b").unwrap(),
            &serde_json::Value::String("two".to_string())
        );
    }

    #[test]
    fn test_parse_cli_task_inputs_invalid_format() {
        let inputs = vec!["no_equals_sign".to_string()];
        assert!(parse_cli_task_inputs(&inputs, None).is_err());
    }

    #[test]
    fn test_parse_cli_task_inputs_empty_key() {
        let inputs = vec!["=value".to_string()];
        assert!(parse_cli_task_inputs(&inputs, None).is_err());
    }

    #[test]
    fn test_parse_cli_task_inputs_invalid_json_base() {
        assert!(parse_cli_task_inputs(&[], Some("not json")).is_err());
    }

    #[test]
    fn test_parse_cli_task_inputs_json_not_object() {
        assert!(parse_cli_task_inputs(&[], Some("[1,2,3]")).is_err());
    }

    #[test]
    fn test_merge_task_input_into_none() {
        let mut task = tasks::TaskConfig {
            name: "test".to_string(),
            ..Default::default()
        };
        let mut cli = serde_json::Map::new();
        cli.insert("key".to_string(), serde_json::json!("value"));

        merge_task_input(&mut task, &cli).unwrap();

        let obj = task.input.unwrap();
        assert_eq!(obj.get("key").unwrap(), &serde_json::json!("value"));
    }

    #[test]
    fn test_merge_task_input_shallow_merge() {
        let mut task = tasks::TaskConfig {
            name: "test".to_string(),
            input: Some(serde_json::json!({"existing": 1, "override_me": "old"})),
            ..Default::default()
        };
        let mut cli = serde_json::Map::new();
        cli.insert("override_me".to_string(), serde_json::json!("new"));
        cli.insert("added".to_string(), serde_json::json!(42));

        merge_task_input(&mut task, &cli).unwrap();

        let obj = task.input.unwrap();
        assert_eq!(obj.get("existing").unwrap(), &serde_json::json!(1));
        assert_eq!(obj.get("override_me").unwrap(), &serde_json::json!("new"));
        assert_eq!(obj.get("added").unwrap(), &serde_json::json!(42));
    }
}
