mod container;
mod gc;
mod search;

use super::{processes, tasks, util};
use clap::crate_version;
use devenv_activity::ActivityInstrument;
use devenv_activity::{Activity, ActivityLevel, activity, message};
use devenv_cache_core::compute_string_hash;
use devenv_core::{
    cachix::{CachixManager, CachixPaths},
    config::{Input, NixBackendType, NixpkgsConfig},
    nix_args::{CliOptionsConfig, NixArgs, SecretspecData, parse_cli_options},
    nix_backend::{DevenvPaths, NixBackend},
    ports::PortAllocator,
    settings::{CacheSettings, InputOverrides, NixSettings, SecretSettings, ShellSettings},
};
use devenv_shell::dialect::{BashDialect, ShellDialect};
use include_dir::{Dir, include_dir};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use nix::sys::signal;
use nix::unistd::Pid;
use once_cell::sync::{Lazy, OnceCell as SyncOnceCell};
use processes::ProcessManager as _;
use secrecy::ExposeSecret;
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
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::process;
use tokio::sync::{OnceCell, Semaphore};
use tracing::{Instrument, debug, info, instrument};

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
    include_str!("../../direnvrc").replace(
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

#[derive(Clone, Debug)]
pub struct DevenvOptions {
    pub inputs: BTreeMap<String, Input>,
    pub imports: Vec<String>,
    pub git_root: Option<PathBuf>,
    pub nixpkgs_config: NixpkgsConfig,
    pub nix_settings: NixSettings,
    pub shell_settings: ShellSettings,
    pub cache_settings: CacheSettings,
    pub secret_settings: SecretSettings,
    pub input_overrides: InputOverrides,
    pub from_external: bool,
    pub devenv_root: Option<PathBuf>,
    pub devenv_dotfile: Option<PathBuf>,
    pub devenv_state: Option<PathBuf>,
    pub shutdown: Arc<tokio_shutdown::Shutdown>,
    pub is_testing: bool,
}

impl DevenvOptions {
    pub fn new(shutdown: Arc<tokio_shutdown::Shutdown>) -> Self {
        Self {
            inputs: BTreeMap::new(),
            imports: Vec::new(),
            git_root: None,
            nixpkgs_config: NixpkgsConfig::default(),
            nix_settings: NixSettings::default(),
            shell_settings: ShellSettings::default(),
            cache_settings: CacheSettings::default(),
            secret_settings: SecretSettings::default(),
            input_overrides: InputOverrides::default(),
            from_external: false,
            devenv_root: None,
            devenv_dotfile: None,
            devenv_state: None,
            shutdown,
            is_testing: false,
        }
    }
}

impl Default for DevenvOptions {
    fn default() -> Self {
        Self::new(tokio_shutdown::Shutdown::new())
    }
}

#[derive(Default, Debug)]
pub struct ProcessOptions {
    /// Whether the process should be detached from the current process.
    pub detach: bool,
    /// Whether the process should be logged to a file.
    pub log_to_file: bool,
    /// When true, fail if a port is in use instead of auto-allocating the next available.
    pub strict_ports: bool,
    /// Command receiver for process control (restart, etc.)
    pub command_rx: Option<tokio::sync::mpsc::Receiver<processes::ProcessCommand>>,
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
#[diagnostic(
    code(devenv::secrets_need_prompting),
    help("Run `devenv shell` to set the missing secrets.")
)]
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
    pub inputs: BTreeMap<String, Input>,
    pub imports: Vec<String>,
    pub git_root: Option<PathBuf>,
    pub nixpkgs_config: NixpkgsConfig,
    pub nix_settings: NixSettings,
    pub shell_settings: ShellSettings,
    pub cache_settings: CacheSettings,
    pub secret_settings: SecretSettings,
    pub input_overrides: InputOverrides,
    pub from_external: bool,
    is_testing: bool,

    pub nix: Arc<Box<dyn NixBackend>>,

    // All kinds of paths
    devenv_root: PathBuf,
    devenv_dotfile: PathBuf,
    devenv_state: Option<PathBuf>,
    devenv_dot_gc: PathBuf,
    devenv_home_gc: PathBuf,
    devenv_tmp: PathBuf,
    devenv_runtime: PathBuf,
    process_runtime_dir: SyncOnceCell<PathBuf>,

    // Container name for container builds, set before assemble
    container_name: std::sync::OnceLock<String>,

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

    // Native process manager started in-process (for detach mode used by test())
    native_process_manager: Arc<OnceCell<Arc<processes::NativeProcessManager>>>,

    // Shutdown handle for coordinated shutdown
    shutdown: Arc<tokio_shutdown::Shutdown>,

    // Task-exported env vars (e.g., PATH with venv/bin, VIRTUAL_ENV) set by
    // run_enter_shell_tasks(). Injected into the bash script by prepare_shell()
    // so they take effect AFTER the Nix shell env is applied.
    task_exports: std::sync::Mutex<BTreeMap<String, String>>,
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

        let devenv_root = {
            let root = options.devenv_root.unwrap_or_else(|| {
                std::env::current_dir().expect("Failed to get current directory")
            });
            // Canonicalize to resolve symlinks (e.g. /var -> /private/var on macOS),
            // ensuring consistent runtime directory hashes across processes.
            std::fs::canonicalize(&root).unwrap_or(root)
        };

        let nix_settings = options.nix_settings;
        let shell_settings = options.shell_settings;
        let cache_settings = options.cache_settings;
        let secret_settings = options.secret_settings;

        // Compute profile-aware dotfile path for state isolation.
        // Canonicalize the parent to resolve symlinks (e.g. /var -> /private/var on macOS),
        // ensuring consistent runtime directory hashes across processes.
        // The default path (devenv_root.join(".devenv")) is already canonical since
        // devenv_root was canonicalized above; this handles custom --dotfile paths.
        let base_devenv_dotfile = options
            .devenv_dotfile
            .map(|p| {
                p.parent()
                    .and_then(|parent| std::fs::canonicalize(parent).ok())
                    .map(|parent| parent.join(p.file_name().expect("dotfile must have a filename")))
                    .unwrap_or(p)
            })
            .unwrap_or_else(|| devenv_root.join(".devenv"));
        let devenv_dotfile =
            if let Some(suffix) = compute_profile_dir_suffix(&shell_settings.profiles) {
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
            hex::encode(hasher.finalize())
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

        let nix: Box<dyn NixBackend> = match nix_settings.backend {
            NixBackendType::Nix => Box::new(
                devenv_nix_backend::nix_backend::NixRustBackend::new(
                    paths,
                    options.nixpkgs_config.clone(),
                    nix_settings.clone(),
                    cache_settings.clone(),
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
                    nix_settings.clone(),
                    cache_settings.clone(),
                    paths,
                    cachix_manager,
                    Some(eval_cache_pool.clone()),
                )
                .await
                .expect("Failed to initialize Snix backend"),
            ),
        };

        Self {
            inputs: options.inputs,
            imports: options.imports,
            git_root: options.git_root,
            nixpkgs_config: options.nixpkgs_config,
            nix_settings,
            shell_settings,
            cache_settings,
            secret_settings,
            input_overrides: options.input_overrides,
            from_external: options.from_external,
            is_testing: options.is_testing,
            devenv_root,
            devenv_dotfile,
            devenv_state: options.devenv_state,
            devenv_dot_gc,
            devenv_home_gc,
            devenv_tmp,
            devenv_runtime,
            process_runtime_dir: SyncOnceCell::new(),
            nix: Arc::new(nix),
            container_name: std::sync::OnceLock::new(),
            assembled: Arc::new(AtomicBool::new(false)),
            assemble_lock: Arc::new(Semaphore::new(1)),
            has_processes: Arc::new(OnceCell::new()),
            dev_env_cache: Arc::new(OnceCell::new()),
            eval_cache_pool,
            secretspec_resolved,
            nix_args_string: Arc::new(OnceCell::new()),
            port_allocator,
            native_process_manager: Arc::new(OnceCell::new()),
            shutdown: options.shutdown,
            task_exports: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    pub fn processes_log(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.log")
    }

    pub fn processes_pid(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.pid")
    }

    async fn processes_running(&self) -> bool {
        if self.processes_pid().exists()
            && let Ok(pid_str) = fs::read_to_string(self.processes_pid()).await
            && let Ok(pid) = pid_str.trim().parse::<i32>()
        {
            match signal::kill(Pid::from_raw(pid), None) {
                Ok(_) => return true,
                Err(nix::errno::Errno::EPERM) => return true,
                Err(nix::errno::Errno::ESRCH) => {}
                Err(_) => {}
            }
        }

        let socket_path = self.devenv_runtime.join("pc.sock");
        let Ok(meta) = fs::metadata(&socket_path).await else {
            return false;
        };
        if !meta.file_type().is_socket() {
            return false;
        }

        matches!(
            tokio::time::timeout(
                std::time::Duration::from_millis(200),
                UnixStream::connect(&socket_path),
            )
            .await,
            Ok(Ok(_))
        )
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

    /// Get the process runtime directory, creating it on first access.
    fn process_runtime_dir(&self) -> Result<&PathBuf> {
        self.process_runtime_dir
            .get_or_try_init(|| processes::get_process_runtime_dir(&self.devenv_runtime))
    }

    /// Build a `tasks::Config` with common fields filled in.
    fn make_task_config(
        &self,
        roots: Vec<String>,
        tasks: Vec<tasks::TaskConfig>,
        run_mode: devenv_tasks::RunMode,
        env: HashMap<String, String>,
        bash: String,
    ) -> Result<tasks::Config> {
        let runtime_dir = self.process_runtime_dir()?.clone();
        Ok(tasks::Config {
            roots,
            tasks,
            run_mode,
            runtime_dir,
            cache_dir: self.devenv_dotfile.clone(),
            sudo_context: None,
            env,
            bash,
            ignore_process_deps: false,
        })
    }

    pub fn native_manager_pid_file(&self) -> PathBuf {
        self.process_runtime_dir()
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
            info!(devenv.is_user_message = true, "Creating {}", target_name);

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

    pub async fn changelogs(&self) -> Result<Option<String>> {
        let changelog = crate::changelog::Changelog::new(&**self.nix, &self.paths());
        changelog.show_all().await
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
        let output = String::from_utf8(env.output.clone()).expect("Failed to convert env to utf-8");
        // Cache so that later callers (e.g. capture_shell_environment via
        // prepare_shell) reuse the result instead of re-evaluating.
        let _ = self.dev_env_cache.set(env);
        Ok(output)
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

        let bash = self.get_bash_path().await?;

        let mut shell_cmd = process::Command::new(&bash);

        // The Nix output ends with "exec bash" which would start a new shell without
        // the devenv environment. Strip it for ALL modes - we handle shell execution ourselves.
        let output_str = String::from_utf8_lossy(output);
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
            script.push_str(&Self::format_task_exports_bash(&exports));
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

        crate::shell_env::apply_shell_env(&mut shell_cmd, &bash, &self.shell_settings.clean);

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

    pub async fn update(&self, input_name: &Option<String>) -> Result<Option<String>> {
        let msg = match input_name {
            Some(input_name) => format!("Updating devenv.lock with input {input_name}"),
            None => "Updating devenv.lock".to_string(),
        };

        let activity = Activity::operation(&msg).start();
        self.nix
            .update(
                input_name,
                &self.inputs,
                &self.input_overrides.override_inputs,
            )
            .in_activity(&activity)
            .await?;

        // Assemble is required for changelog.show_new() which builds changelog.json
        // Allow assemble to fail gracefully - changelogs are informational only
        match self.assemble().await {
            Ok(_) => {
                // Show new changelogs (if any)
                let changelog = crate::changelog::Changelog::new(&**self.nix, &self.paths());
                match changelog.show_new().await {
                    Ok(output) => Ok(output),
                    Err(e) => {
                        // Don't fail the update if changelogs fail to load
                        tracing::warn!("Failed to show changelogs: {}", e);
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to assemble environment, skipping changelog: {}", e);
                Ok(None)
            }
        }
    }

    pub async fn repl(&self) -> Result<()> {
        self.assemble().await?;
        self.nix.repl().await?;
        Ok(())
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
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<String> {
        self.assemble().await?;
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

        let config = self.make_task_config(roots, tasks, run_mode, envs, String::new())?;

        if let Ok(config_value) = devenv_activity::SerdeValue::from_serialize(&config) {
            use valuable::Valuable;
            debug!(event = config_value.as_value(), "Loaded task config");
        }

        let tasks = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown))
            .with_refresh_task_cache(self.cache_settings.refresh_task_cache)
            .build()
            .await?;

        let (status, outputs) = run_tasks_with_ui(tasks, verbosity, tui).await?;

        if status.has_failures() {
            miette::bail!("Some tasks failed");
        }

        Ok(serde_json::to_string(&outputs).expect("parsing of outputs failed"))
    }

    pub async fn tasks_list(&self) -> Result<String> {
        self.assemble().await?;

        let tasks = self.load_tasks().await?;

        if tasks.is_empty() {
            return Ok("No tasks defined.".to_string());
        }

        Ok(format_tasks_tree(&tasks))
    }

    /// Run enterShell tasks and return env vars exported by tasks (e.g., PATH with venv/bin).
    /// This runs tasks via Rust (not bash hook) to enable TUI progress reporting.
    /// Task failures are logged as warnings but don't prevent shell entry.
    ///
    /// If `pre_captured_envs` is provided (e.g. from test() which already captured envs),
    /// those are used directly; otherwise a fresh capture is performed.
    pub async fn run_enter_shell_tasks(
        &self,
        pre_captured_envs: Option<HashMap<String, String>>,
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<BTreeMap<String, String>> {
        self.assemble().await?;

        let envs = match pre_captured_envs {
            Some(e) => e,
            None => self.capture_shell_environment().await?,
        };

        let task_configs = self.load_tasks().await?;
        // Shell entry proceeds even if some tasks fail (matches interactive reload behavior).
        // Task failures are shown in the TUI/UI output, no need to bail here.
        let (_status, exports) = self
            .run_tasks_with_roots(
                vec!["devenv:enterShell".to_string()],
                task_configs,
                envs,
                verbosity,
                tui,
            )
            .await?;
        Ok(exports)
    }

    /// Run tasks with the given roots, storing exports on self for prepare_shell().
    async fn run_tasks_with_roots(
        &self,
        roots: Vec<String>,
        task_configs: Vec<tasks::TaskConfig>,
        envs: HashMap<String, String>,
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<(tasks::TasksStatus, BTreeMap<String, String>)> {
        let config = tasks::Config {
            roots,
            tasks: task_configs,
            run_mode: devenv_tasks::RunMode::All,
            runtime_dir: self.devenv_runtime.clone(),
            cache_dir: self.devenv_dotfile.clone(),
            sudo_context: None,
            env: envs,
            bash: String::new(),
            ignore_process_deps: false,
        };

        let tasks = Tasks::builder(config, verbosity, Arc::clone(&self.shutdown))
            .build()
            .await?;

        let (status, outputs) = run_tasks_with_ui(tasks, verbosity, tui).await?;

        let exports = outputs.collect_env_exports();
        // Store on self so prepare_shell() can inject them into the bash script
        *self.task_exports.lock().unwrap() = exports.clone();
        Ok((status, exports))
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

    /// Check whether a string is a valid POSIX environment variable name
    /// (`[a-zA-Z_][a-zA-Z0-9_]*`). Used to filter bash internal entries like
    /// `BASH_FUNC_my_func%%` that would produce invalid `export` statements.
    fn is_valid_env_name(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    fn parse_env_null_separated(content: &[u8]) -> Vec<(String, String)> {
        let mut envs = Vec::new();
        for entry in content.split(|&b| b == 0) {
            if entry.is_empty() {
                continue;
            }
            let entry_str = String::from_utf8_lossy(entry);
            let mut parts = entry_str.splitn(2, '=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                envs.push((key.to_string(), value.to_string()));
            }
        }
        envs
    }

    async fn capture_shell_environment(&self) -> Result<HashMap<String, String>> {
        let temp_dir = tempfile::TempDir::with_prefix("devenv-env")
            .into_diagnostic()
            .wrap_err("Failed to create temporary directory for environment capture")?;

        let script_path = temp_dir.path().join("script");
        let env_path = temp_dir.path().join("env");

        let script = format!("env -0 > {}", env_path.to_string_lossy());
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

        // Run script and capture the Nix shell environment variables.
        // Skip the legacy shellHook task runner (devenv-tasks run devenv:enterShell)
        // because the 2.0+ Rust code runs enterShell tasks separately via
        // run_enter_shell_tasks(). Running them inside this subprocess would
        // be redundant and, worse, a @completed task failure there would cause the
        // subprocess to exit non-zero, aborting the environment capture.
        let mut cmd = self
            .prepare_shell(&Some(script_path.to_string_lossy().into()), &[])
            .await?;
        cmd.env("DEVENV_SKIP_TASKS", "1");
        let output = cmd
            .output()
            .await
            .into_diagnostic()
            .wrap_err("Failed to execute environment capture script")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            miette::bail!("Shell environment capture failed: {}", stderr);
        }

        // Parse the null-separated environment variables (env -0 output).
        // Using null separators correctly handles multiline values such as
        // BASH_FUNC_* entries, which would be truncated by line-based parsing.
        let content = fs::read(&env_path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to read environment file at {}", env_path.display())
            })?;
        let shell_envs = Self::parse_env_null_separated(&content);

        let mut envs: HashMap<String, String> = self.shell_settings.clean.kept_env_vars();

        for (key, value) in shell_envs {
            if Self::is_valid_env_name(&key) {
                envs.insert(key, value);
            }
        }

        Ok(envs)
    }

    /// Format a map of task exports as bash `export KEY=VALUE` lines.
    ///
    /// Keys are already sorted (BTreeMap), giving deterministic output (important for direnv diffing).
    pub fn format_task_exports_bash(exports: &BTreeMap<String, String>) -> String {
        use std::borrow::Cow;
        let mut result = String::with_capacity(exports.len() * 50);
        for (key, value) in exports {
            result.push_str("export ");
            result.push_str(&shell_escape::escape(Cow::Borrowed(key)));
            result.push('=');
            result.push_str(&shell_escape::escape(Cow::Borrowed(value)));
            result.push('\n');
        }
        result
    }

    /// Assemble, build dev environment, cache it, and capture shell env vars.
    async fn configure_shell(&self) -> Result<HashMap<String, String>> {
        let phase1 = Activity::operation("Configuring shell")
            .parent(None)
            .start();
        async {
            self.assemble().await?;
            let dev_env = self.get_dev_environment_inner(false).await?;
            let _ = self.dev_env_cache.set(dev_env);
            self.capture_shell_environment().await
        }
        .in_activity(&phase1)
        .await
    }

    pub async fn test(&self, verbosity: tasks::VerbosityLevel, tui: bool) -> Result<()> {
        // Enable port allocation before assemble so that ports resolved
        // during Nix evaluation (e.g. in enterTest) are properly allocated.
        self.port_allocator.set_enabled(true);

        // ── Phase 1: Configuring shell ──────────────────────────────
        let envs = self.configure_shell().await?;
        let has_processes = self.has_processes().await?;

        // ── Phase 2: Running enterTest tasks ─────────────────────────
        // Run all tasks rooted at devenv:enterTest, which includes enterShell
        // tasks (e.g., devenv:python:virtualenv) as dependencies. This runs
        // git-hooks:run and other enterTest tasks in a single pass.
        // Exports are stored on self so prepare_shell() injects them.
        let mut envs = envs;
        {
            let task_configs = self.load_tasks().await?;
            let (status, exports) = self
                .run_tasks_with_roots(
                    vec!["devenv:enterTest".to_string()],
                    task_configs,
                    envs.clone(),
                    verbosity,
                    tui,
                )
                .await?;
            if status.has_failures() {
                bail!("enterTest tasks failed");
            }
            envs.extend(exports);
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
                Ok::<String, miette::Report>(test_script[0].to_string_lossy().into_owned())
            }
            .in_activity(&phase3)
            .await?
        };

        // ── Phase 4: Starting processes (if needed) ─────────────────
        if has_processes {
            let options = ProcessOptions {
                detach: true,
                ..Default::default()
            };
            self.start_processes(vec![], envs, options).await?;
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
            info!(devenv.is_user_message = true, "Tests passed :)");
            Ok(())
        }
    }

    pub async fn info(&self) -> Result<String> {
        self.assemble().await?;
        self.nix.metadata().await
    }

    pub async fn build(&self, attributes: &[String]) -> Result<Vec<(String, PathBuf)>> {
        let activity = Activity::operation("Building").start();
        async move {
            self.assemble().await?;

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
            self.assemble().await?;

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

    pub async fn up(
        &self,
        processes: Vec<String>,
        options: ProcessOptions,
        verbosity: tasks::VerbosityLevel,
        tui: bool,
    ) -> Result<RunMode> {
        // Set strict port mode before assemble (which triggers port allocation)
        self.port_allocator.set_strict(options.strict_ports);
        self.port_allocator.set_enabled(true);

        // ── Phase 1: Configuring shell ──────────────────────────────
        let mut envs = self.configure_shell().await?;

        if !self.has_processes().await? {
            message(
                ActivityLevel::Error,
                "No 'processes' option defined: https://devenv.sh/processes/",
            );
            bail!("No processes defined");
        }

        // ── Phase 2: Loading and running enterShell tasks ─────────────
        let task_configs = self.load_tasks().await?;
        let (_status, exports) = self
            .run_tasks_with_roots(
                vec!["devenv:enterShell".to_string()],
                task_configs,
                envs.clone(),
                verbosity,
                tui,
            )
            .await?;
        envs.extend(exports);

        // ── Phase 3: Running processes ──────────────────────────────
        self.start_processes(processes, envs, options).await
    }

    /// Start processes after shell environment and tasks are already configured.
    async fn start_processes(
        &self,
        processes: Vec<String>,
        envs: HashMap<String, String>,
        mut options: ProcessOptions,
    ) -> Result<RunMode> {
        // Release port reservations so processes can bind their allocated ports.
        // The port allocator holds TcpListeners during Nix evaluation to prevent
        // race conditions; dropping them here makes the ports available.
        drop(self.port_allocator.take_reservations());

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

            let task_configs = self.load_tasks().await?;
            let roots: Vec<String> = if processes.is_empty() {
                task_configs
                    .iter()
                    .filter(|t| t.name.starts_with(devenv_tasks::PROCESS_TASK_PREFIX))
                    .map(|t| t.name.clone())
                    .collect()
            } else {
                processes
                    .iter()
                    .map(|p| format!("{}{}", devenv_tasks::PROCESS_TASK_PREFIX, p))
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

            let bash = self.get_bash_path().await?;
            let config =
                self.make_task_config(roots, task_configs, tasks::RunMode::All, envs, bash)?;

            let tasks_runner =
                tasks::Tasks::builder(config, tasks::VerbosityLevel::Normal, self.shutdown.clone())
                    .build()
                    .await
                    .map_err(|e| miette!("Failed to build task runner: {}", e))?;

            // Start command processing before task execution so that
            // Ctrl-R works even while tasks are still running (e.g. when
            // a process task is waiting on an auto start off dependency).
            if let Some(rx) = options.command_rx.take() {
                tasks_runner.process_manager().start_command_listener(rx);
            }

            // Run process tasks under the Phase 4 activity.
            // Auto start off processes (start.enable = false) are handled by the
            // process manager: they appear in the TUI as stopped.
            debug!("devenv.up: running process tasks (run_with_parent_activity)");
            let _outputs = tasks_runner
                .run_with_parent_activity(Arc::new(phase4))
                .await;
            debug!("devenv.up: process tasks completed");

            // API server is started inside run_internal() so it's available
            // while processes are still starting up.

            let pid_file = tasks_runner.process_manager().manager_pid_file();
            processes::write_pid(&pid_file, std::process::id())
                .await
                .map_err(|e| miette!("Failed to write manager PID: {}", e))?;

            if !options.detach {
                debug!(
                    "devenv.up: calling run_foreground (native manager, detach=false), global_token_cancelled={}",
                    self.shutdown.is_cancelled()
                );
                let result = tasks_runner
                    .process_manager()
                    .run_foreground(self.shutdown.cancellation_token(), None)
                    .await
                    .map_err(|e| miette!("Process manager error: {}", e));
                debug!("devenv.up: run_foreground returned");

                let _ = tokio::fs::remove_file(&pid_file).await;
                result?;
            } else {
                // Store manager for later stop via down()
                let _ = self
                    .native_process_manager
                    .set(Arc::clone(tasks_runner.process_manager()));
            }

            return Ok(RunMode::Detached);
        }

        // Non-native manager (process-compose)
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

        let manager =
            processes::ProcessComposeManager::new(procfile_script, self.devenv_dotfile.clone());

        if options.detach {
            let start_options = processes::StartOptions {
                process_configs: HashMap::new(),
                processes,
                detach: true,
                log_to_file: options.log_to_file,
                env: envs,
                cancellation_token: Some(self.shutdown.cancellation_token()),
            };
            manager.start(start_options).await?;
            Ok(RunMode::Detached)
        } else {
            let command = manager
                .prepare_foreground_command(&processes, &envs)
                .await?;
            Ok(RunMode::Foreground(ShellCommand { command }))
        }
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
            let runtime_dir = self.process_runtime_dir()?.clone();
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
            let socket_path = self.process_runtime_dir()?.join("native.sock");
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
    pub async fn assemble(&self) -> Result<String> {
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
        if self.input_overrides.nix_module_options.is_empty()
            && !self.from_external
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

        // TODO: superceded by eval caching.
        // Remove once direnvrc migration is implemented.
        util::write_file_with_lock(
            self.devenv_dotfile.join("imports.txt"),
            self.imports.join("\n"),
        )?;

        fs::create_dir_all(&self.devenv_runtime)
            .await
            .map_err(|e| {
                miette::miette!("Failed to create {}: {}", self.devenv_runtime.display(), e)
            })?;

        // Initialize eval-cache database (framework layer concern, used by backends)
        if self.cache_settings.eval_cache {
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
        let secretspec_config_exists = self.secret_settings.secretspec.is_some();
        let secretspec_enabled = self
            .secret_settings
            .secretspec
            .as_ref()
            .map(|c| c.enable)
            .unwrap_or(false);

        if secretspec_path.exists() {
            // Log warning when secretspec.toml exists but is not configured
            if !secretspec_enabled && !secretspec_config_exists {
                info!(
                    devenv.is_user_message = true,
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
                // Get profile and provider from resolved secret settings
                let (profile, provider) =
                    if let Some(ref secretspec_config) = self.secret_settings.secretspec {
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

                // Validate secrets — return SecretsNeedPrompting on missing secrets.
                // The caller (main.rs) decides whether to prompt interactively or fail.
                let validated_secrets = match secrets.validate()? {
                    Ok(validated) => validated,
                    Err(e) => {
                        return Err(SecretsNeedPrompting {
                            provider: provider.clone(),
                            profile: profile.clone(),
                            missing: e.missing_required,
                        }
                        .into());
                    }
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

        let username = whoami::username().ok();

        // TODO: remove devenv_dotfile_path and derive the relative path inside NixArgs instead
        let dotfile_relative_path = PathBuf::from(format!(
            "./{}",
            self.devenv_dotfile
                .file_name()
                // This should never fail
                .expect("Failed to extract the directory name from devenv_dotfile")
                .to_string_lossy()
        ));

        // Convert secretspec::Resolved to SecretspecData if available
        let secretspec_data: Option<SecretspecData> =
            self.secretspec_resolved
                .get()
                .map(|resolved| SecretspecData {
                    profile: resolved.profile.clone(),
                    provider: resolved.provider.clone(),
                    secrets: resolved.secrets.clone().into_iter().collect(),
                });

        let active_profiles = &self.shell_settings.profiles;

        // Parse CLI options into structured format with typed values
        let cli_options =
            CliOptionsConfig(parse_cli_options(&self.input_overrides.nix_module_options)?);

        // Compute lock fingerprint for eval-cache invalidation
        // This includes narHashes from local path inputs that aren't stored in devenv.lock
        let lock_fingerprint = self.nix.lock_fingerprint().await?;

        // Create the Nix arguments struct
        let container_name = self.container_name.get();
        let args = NixArgs {
            version: crate_version!(),
            is_development_version: crate::is_development_version(),
            system: &self.nix_settings.system,
            devenv_root: &self.devenv_root,
            skip_local_src: self.from_external
                || (!self.input_overrides.nix_module_options.is_empty()
                    && !self.devenv_root.join("devenv.nix").exists()),
            devenv_dotfile: &self.devenv_dotfile,
            devenv_dotfile_path: &dotfile_relative_path,
            devenv_tmpdir: &self.devenv_tmp,
            devenv_runtime: &self.devenv_runtime,
            devenv_istesting: self.is_testing,
            devenv_direnvrc_latest_version: *DIRENVRC_VERSION,
            container_name: container_name.map(String::as_str),
            active_profiles,
            cli_options,
            hostname: hostname.as_deref(),
            username: username.as_deref(),
            git_root: self.git_root.as_deref(),
            secretspec: secretspec_data.as_ref(),
            devenv_inputs: &self.inputs,
            devenv_imports: &self.imports,
            impure: self.nix_settings.impure,
            nixpkgs_config: self.nixpkgs_config.clone(),
            lock_fingerprint: &lock_fingerprint,
            devenv_state: self.devenv_state.as_deref(),
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
        self.assemble().await?;

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
                .map(|path| path.to_string_lossy().into_owned())
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

/// Run tasks, dispatching to TUI mode (direct run) or shell mode (with TasksUi).
///
/// In TUI mode the activity channel is already set up in main.rs, so tasks run
/// directly and status is read afterwards. In shell mode we initialise a local
/// activity channel and drive TasksUi for interactive output.
async fn run_tasks_with_ui(
    tasks: Tasks,
    verbosity: tasks::VerbosityLevel,
    tui: bool,
) -> Result<(tasks::TasksStatus, tasks::Outputs)> {
    if tui {
        let outputs = tasks.run(false).await;
        let status = tasks.get_completion_status().await;
        Ok((status, outputs))
    } else {
        let (activity_rx, activity_handle) = devenv_activity::init();
        let _activity_guard = activity_handle.install();

        let tasks = Arc::new(tasks);
        let tasks_clone = Arc::clone(&tasks);

        let run_handle = tokio::spawn(async move { tasks_clone.run(false).await });

        let ui = TasksUi::new(Arc::clone(&tasks), activity_rx, verbosity);
        Ok(ui.run(run_handle).await?)
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

fn format_tasks_tree(tasks: &[tasks::TaskConfig]) -> String {
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

    #[test]
    fn test_parse_env_null_separated_basic() {
        let input = b"HOME=/home/user\0LANG=en_US.UTF-8\0";
        let result = Devenv::parse_env_null_separated(input);
        assert_eq!(
            result,
            vec![
                ("HOME".to_string(), "/home/user".to_string()),
                ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_null_separated_multiline_value() {
        let input =
            b"SIMPLE=value\0BASH_FUNC_my_func%%=() { echo hello\n  echo world\n}\0OTHER=val\0";
        let result = Devenv::parse_env_null_separated(input);
        assert_eq!(
            result,
            vec![
                ("SIMPLE".to_string(), "value".to_string()),
                (
                    "BASH_FUNC_my_func%%".to_string(),
                    "() { echo hello\n  echo world\n}".to_string()
                ),
                ("OTHER".to_string(), "val".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_env_null_separated_empty() {
        let result = Devenv::parse_env_null_separated(b"");
        assert!(result.is_empty());
    }

    #[test]
    fn test_is_valid_env_name() {
        assert!(Devenv::is_valid_env_name("HOME"));
        assert!(Devenv::is_valid_env_name("_PRIVATE"));
        assert!(Devenv::is_valid_env_name("var123"));
        assert!(!Devenv::is_valid_env_name("BASH_FUNC_my_func%%"));
        assert!(!Devenv::is_valid_env_name("123BAD"));
        assert!(!Devenv::is_valid_env_name("has-dashes"));
        assert!(!Devenv::is_valid_env_name(""));
    }

    #[test]
    fn test_parse_env_null_separated_value_with_equals() {
        let input = b"CONFIG=key=value=extra\0";
        let result = Devenv::parse_env_null_separated(input);
        assert_eq!(
            result,
            vec![("CONFIG".to_string(), "key=value=extra".to_string())]
        );
    }
}
