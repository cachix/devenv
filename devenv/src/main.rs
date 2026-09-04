#![recursion_limit = "256"]

// [tier2] On the fully-static (musl) build, musl's mallocng returns freed
// memory to the kernel aggressively, turning devenv's alloc-heavy startup into
// hundreds of mmap/munmap syscalls (~8 ms). Route the global allocator to
// mimalloc (linked via nix as libmimalloc.a, MI_OVERRIDE=OFF so only the mi_*
// API is exposed). Gated on `--cfg use_mimalloc`, set only for the static build.
#[cfg(use_mimalloc)]
mod mimalloc_global {
    use std::alloc::{GlobalAlloc, Layout};

    unsafe extern "C" {
        fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut u8;
        fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut u8;
        fn mi_realloc_aligned(p: *mut u8, newsize: usize, alignment: usize) -> *mut u8;
        fn mi_free(p: *mut u8);
    }

    pub struct MiMalloc;

    unsafe impl GlobalAlloc for MiMalloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            unsafe { mi_malloc_aligned(layout.size(), layout.align()) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            unsafe { mi_zalloc_aligned(layout.size(), layout.align()) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
            unsafe { mi_free(ptr) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            unsafe { mi_realloc_aligned(ptr, new_size, layout.align()) }
        }
    }
}

#[cfg(use_mimalloc)]
#[global_allocator]
static GLOBAL: mimalloc_global::MiMalloc = mimalloc_global::MiMalloc;

mod commands;

use clap::{CommandFactory, crate_version};
use clap_complete::CompleteEnv;
use devenv::{
    CacheSettings, ClientRunMode, Config, Devenv, InputOverrides, NixSettings, ProcessStartOutcome,
    SecretSettings, ShellSettings, VerbosityLevel,
    activity::{ActivityGuard, ActivityLevel},
    cli::{
        Cli, CliOptions, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand,
        TraceOutputSpec, UserConfigCommand,
    },
    is_ai_agent,
    reload::{Config as ReloadConfig, DevenvShellBuilder, ShellCoordinator},
    terminal::{self, IsForegroundTerminal as _},
    tracing as devenv_tracing,
    tui::{SessionIo, ShellSession},
};
use devenv_mailbox::{FrontendCommand, FrontendEvent};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal},
    os, panic,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_shutdown::Shutdown;
use tracing::{info, instrument};

const DEVENV_CALLER: &str = "_DEVENV_CALLER";
const DEVENV_SHELL_HINT: &str = "_DEVENV_SHELL_HINT";

fn main() {
    if let Some(code) = devenv_processes::maybe_run_process_guardian() {
        process::exit(code);
    }
    if let Some(code) = devenv_processes::maybe_run_capability_helper() {
        process::exit(code);
    }
    // Handle shell completion requests (COMPLETE=bash devenv)
    // Use "devenv" as completer so scripts work after installation (not absolute path)
    CompleteEnv::with_factory(Cli::command)
        .completer("devenv")
        .complete();

    install_miette_hook();

    if let Err(err) = main_inner() {
        eprintln!("{err:?}");
        process::exit(1);
    }
}

fn main_inner() -> Result<()> {
    let invocation = Invocation::from_env();

    // Retry loop: if the backend discovers secrets need interactive prompting,
    // we prompt the user and re-run the entire command with secrets now available.
    loop {
        let cli = Cli::parse_preprocessed();

        // Handle commands that don't need config or runtime
        match &cli.command {
            Commands::Version => {
                return commands::version::run();
            }
            Commands::UserConfig { command } => {
                return run_user_config_command(command, cli.user_config.as_deref());
            }
            Commands::Direnvrc => {
                return commands::direnvrc::run();
            }
            Commands::Hook { shell, shell_args } => {
                return commands::hook::print(shell, shell_args);
            }
            Commands::Allow => {
                let home = devenv_core::paths::resolve_home()?;
                return commands::hook::allow(&home, cli.from.as_deref(), &cli.shell_args.profiles);
            }
            Commands::Revoke => {
                let home = devenv_core::paths::resolve_home()?;
                return commands::hook::revoke(&home);
            }
            Commands::HookShouldActivate => {
                let home = devenv_core::paths::resolve_home()?;
                return commands::hook::should_activate(&home);
            }
            Commands::DaemonProcesses { config_file } => {
                return commands::daemon_processes::run(config_file);
            }
            Commands::Init {
                target,
                include_envrc,
            } => {
                let verbosity = resolve_verbosity(&cli.cli_options);
                return commands::init::run(target.as_deref(), verbosity, *include_envrc);
            }
            Commands::Inputs {
                command: InputsCommand::Add { name, url, follows },
            } => {
                // `inputs add` is dispatched before `prepare_command()` runs discovery,
                // so do it here too: edit the enclosing project's `devenv.yaml`
                // (the one `devenv shell` would use) rather than silently
                // creating a stray one in the current subdirectory.
                enter_discovered_project_root()?;
                return commands::inputs::add(name, url, follows);
            }
            _ => {}
        }

        let prepared = prepare_command(cli, invocation.shell_hint.as_deref())?;

        match run(prepared, invocation.caller) {
            Err(err) => match err.downcast::<devenv::SecretsNeedPrompting>() {
                Ok(secrets_err) => {
                    if !terminal::can_use_stdin_interactively() {
                        return Err(secrets_err.into());
                    }
                    prompt_secrets(
                        secrets_err.source,
                        secrets_err.provider,
                        secrets_err.profile,
                    )?;
                    continue;
                }
                Err(err) => return Err(err),
            },
            Ok(CommandResult::Debugger(devenv, err)) => return launch_debugger(*devenv, err),
            Ok(CommandResult::Repl(devenv)) => return launch_repl_thread(*devenv),
            Ok(cmd_result) => return cmd_result.exec(),
        }
    }
}

fn run_user_config_command(
    command: &UserConfigCommand,
    override_path: Option<&Path>,
) -> Result<()> {
    if matches!(command, UserConfigCommand::Schema) {
        let schema = schemars::schema_for!(devenv_tui::UserConfig);
        println!(
            "{}",
            serde_json::to_string_pretty(&schema).into_diagnostic()?
        );
        return Ok(());
    }
    let path = override_path.map(|path| {
        devenv_core::paths::resolve_against(path, &env::current_dir().unwrap_or_default())
    });
    let path = devenv::user_config::path(path.as_deref())?;
    match command {
        UserConfigCommand::Path => println!("{}", path.display()),
        UserConfigCommand::Validate => {
            devenv_tui::UserConfig::load(&path)?;
            println!("valid: {}", path.display());
        }
        UserConfigCommand::Show => {
            let config = devenv::user_config::load(override_path.map(|_| path.as_path()))?;
            print!("{}", config.to_yaml()?);
        }
        UserConfigCommand::Schema => unreachable!(),
    }
    Ok(())
}

/// Options for the frontend/renderer thread.
struct FrontendOptions {
    tui_allowed: bool,
    tui_preferences: Arc<devenv_tui::TuiPreferences>,
    tui_context: Arc<devenv_tui::TuiRunContext>,
    shell_keybindings: devenv_shell::keybindings::ShellKeybindings,
    tracing_owns_terminal: bool,
    log_level: devenv_tracing::Level,
    tracing_specs: Vec<TraceOutputSpec>,
    verbosity: VerbosityLevel,
}

/// Options for the backend thread: resolved devenv config plus what to run.
struct BackendOptions {
    devenv: devenv::DevenvOptions,
    command: Commands,
    verbosity: VerbosityLevel,
    nix_debugger: bool,
    strict_ports: bool,
    /// Whether an interactive reloadable shell will use the PTY session.
    /// This is decided before the frontend/backend split so both sides agree.
    use_pty: bool,
    shell_keybindings: devenv_shell::keybindings::ShellKeybindings,
}

/// Everything needed to execute a config-backed command.
///
/// Run-wide context and resource guards live here rather than being assigned
/// to the frontend or backend solely for ownership reasons.
struct PreparedCommand {
    frontend: FrontendOptions,
    backend: BackendOptions,
    discovered_root: Option<PathBuf>,
    test_environment: Option<TestEnvironment>,
    shutdown: Arc<Shutdown>,
}

#[derive(Clone, Copy)]
enum Caller {
    Cli,
    Direnv,
    Hook,
}

struct Invocation {
    caller: Caller,
    shell_hint: Option<String>,
}

impl Invocation {
    fn from_env() -> Self {
        let caller = env::var_os(DEVENV_CALLER);
        let shell_hint = env::var(DEVENV_SHELL_HINT).ok();
        // This runs before devenv starts any threads. Removing the one-shot
        // invocation metadata prevents commands launched by an activated shell
        // from inheriting the original caller or its ambient shell hint.
        unsafe {
            env::remove_var(DEVENV_CALLER);
            env::remove_var(DEVENV_SHELL_HINT);
        }

        Self::from_parts(
            caller.as_deref().and_then(|value| value.to_str()),
            shell_hint,
        )
    }

    fn from_parts(caller: Option<&str>, shell_hint: Option<String>) -> Self {
        let caller = match caller {
            Some("direnv") => Caller::Direnv,
            Some("hook") => Caller::Hook,
            _ => Caller::Cli,
        };
        let shell_hint = match caller {
            Caller::Hook => shell_hint,
            Caller::Cli | Caller::Direnv => None,
        };

        Self { caller, shell_hint }
    }
}

impl Caller {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Direnv => "direnv",
            Self::Hook => "hook",
        }
    }
}

/// Dotfile/state paths and temporary-directory guards for `devenv test`.
///
/// The guards must outlive the command; dropping `TestEnvironment` removes
/// any temporary directories.
struct TestEnvironment {
    dotfile: Option<PathBuf>,
    state: PathBuf,
    _dotfile_guard: Option<TempDir>,
    _state_guard: Option<TempDir>,
}

impl TestEnvironment {
    /// Prepare test directories when the command is `devenv test`.
    ///
    /// Non-`test` commands get no overrides (`Devenv` uses the default
    /// `.devenv` layout). `devenv test` either runs fully isolated in temp
    /// directories (`--override-dotfile`) or reuses the real `.devenv` with an
    /// isolated `.devenv/test-state` so the eval cache survives across runs.
    fn for_command(command: &Commands) -> Result<Option<Self>> {
        let Commands::Test {
            override_dotfile, ..
        } = command
        else {
            return Ok(None);
        };

        if *override_dotfile {
            let pwd = env::current_dir()
                .into_diagnostic()
                .wrap_err("Failed to get current directory")?;
            let dotfile_tmp = TempDir::with_prefix_in(".devenv.", pwd)
                .into_diagnostic()
                .wrap_err("Failed to create temporary directory")?;
            let Some(file_name) = dotfile_tmp.path().file_name().and_then(|f| f.to_str()) else {
                return Err(miette::miette!("Temporary directory path is invalid"));
            };
            info!("Overriding .devenv to {file_name}");

            let state_tmp = TempDir::new()
                .into_diagnostic()
                .wrap_err("Failed to create temporary state directory")?;
            info!(
                "Using temporary state directory: {}",
                state_tmp.path().display()
            );

            Ok(Some(Self {
                dotfile: Some(dotfile_tmp.path().to_path_buf()),
                state: state_tmp.path().to_path_buf(),
                _dotfile_guard: Some(dotfile_tmp),
                _state_guard: Some(state_tmp),
            }))
        } else {
            // Stable test state path: isolates test services from shell state
            // while keeping the path consistent across runs so the eval cache
            // is effective.
            let test_state = env::current_dir()
                .into_diagnostic()
                .wrap_err("Failed to get current directory")?
                .join(".devenv")
                .join("test-state");
            info!("Using test state directory: {}", test_state.display());
            Ok(Some(Self {
                dotfile: None,
                state: test_state,
                _dotfile_guard: None,
                _state_guard: None,
            }))
        }
    }
}

/// chdir into the discovered project `root` and keep `$PWD` in sync with the
/// new working directory.
///
/// Shared by both discovery sites: the early dispatch path (`inputs add`, via
/// [`enter_discovered_project_root`]) and [`prepare_command`]. A chdir failure is
/// fatal — silently staying put would write to the wrong project.
fn enter_root(root: &Path) -> Result<()> {
    env::set_current_dir(root)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "Failed to chdir to discovered project root: {}",
                root.display()
            )
        })?;
    // Safety: both callers run single-threaded during early command dispatch /
    // `prepare_command()`, before any tokio runtime or thread is spawned, so no other
    // thread can be reading the environment concurrently.
    unsafe {
        env::set_var("PWD", root);
    }
    Ok(())
}

/// For commands dispatched before `prepare_command()` (e.g. `inputs add`): if invoked
/// from a subdirectory of a project, chdir up to the directory containing
/// `devenv.nix` so the command operates on the enclosing project, matching
/// where `devenv shell` would run.
fn enter_discovered_project_root() -> Result<()> {
    // Best-effort discovery: if the cwd can't be read or there's no enclosing
    // `devenv.nix` above it, run where we are (matches `prepare_command()`).
    let Ok(cwd) = env::current_dir() else {
        return Ok(());
    };
    let root = if let Some(root) = devenv_core::paths::find_project_root(&cwd) {
        Some(root)
    } else {
        // A dir bound via `devenv allow --from` roots at the bound directory.
        let home = devenv_core::paths::resolve_home()?;
        commands::hook::trusted_from(&home, &cwd)?.map(|binding| PathBuf::from(binding.path))
    };
    let Some(root) = root.filter(|r| r.as_path() != cwd) else {
        return Ok(());
    };
    enter_root(&root)
}

/// Prepare a config-backed CLI command for execution.
fn prepare_command(mut cli: Cli, shell_hint: Option<&str>) -> Result<PreparedCommand> {
    // --- Project discovery and working directory ---

    // Source priority: explicit `--from` / `-O` overrides > a local devenv.nix
    // (here or in an ancestor) > a binding persisted by `devenv allow --from`.
    // Profiles persisted by `allow` apply only when no explicit --profile was
    // supplied.
    // Has to run before Config::load() reads "./devenv.yaml".
    let original_cwd = env::current_dir().ok();
    let user_config_path = cli.user_config.as_deref().map(|path| {
        devenv_core::paths::resolve_against(path, original_cwd.as_deref().unwrap_or(Path::new(".")))
    });
    let command = cli.command;
    let has_overrides = !cli.input_overrides.nix_module_options.is_empty();
    let mut from_source = cli.from.clone();
    let mut project_root = None;
    if from_source.is_none()
        && !has_overrides
        && let Some(cwd) = original_cwd.as_deref()
    {
        project_root = devenv_core::paths::find_project_root(cwd);
        if let Some(root) = project_root.as_deref() {
            if cli.shell_args.profiles.is_empty() {
                let home = devenv_core::paths::resolve_home()?;
                cli.shell_args.profiles = commands::hook::trusted_profiles(&home, root)?;
            }
        } else {
            // A bound directory behaves as if `--from <source>` were passed and is
            // entered as the project root below, matching hook activation, so its
            // devenv.yaml, .devenv state, and processes are shared by all subdirs.
            let home = devenv_core::paths::resolve_home()?;
            if let Some(binding) = commands::hook::trusted_from(&home, cwd)? {
                project_root = Some(PathBuf::from(binding.path));
                from_source = Some(binding.from);
                // Bound profiles apply as if passed via --profile; explicit
                // --profile flags win.
                if cli.shell_args.profiles.is_empty() {
                    cli.shell_args.profiles = binding.profiles;
                }
            }
        }
    }

    let discovered_root = project_root
        .as_ref()
        .filter(|r| Some(r.as_path()) != original_cwd.as_deref())
        .cloned();
    // When discovery moves us into a parent root, remember the directory the
    // user actually invoked from so the interactive shell and `-- cmd` still
    // run there. The devenv environment is root-scoped; the cwd is not.
    let shell_cwd = discovered_root.as_ref().and_then(|_| original_cwd.clone());
    if let Some(root) = &discovered_root {
        enter_root(root)?;
    }

    // --- Frontend options ---

    let verbosity = resolve_verbosity(&cli.cli_options);
    let quiet = matches!(verbosity, VerbosityLevel::Quiet);
    let log_level = match verbosity {
        VerbosityLevel::Verbose => devenv_tracing::Level::Debug,
        VerbosityLevel::Quiet => devenv_tracing::Level::Warn,
        VerbosityLevel::Normal => devenv_tracing::Level::default(),
    };
    // `TracingArgs::resolve()` folds legacy `--trace-output` into `tracing_specs`,
    // so a single walk covers env, --trace-to, and --trace-output.
    let tracing_specs = cli.tracing_args.resolve().into_diagnostic()?;
    let tracing_owns_terminal = tracing_specs.iter().any(|s| s.targets_terminal());

    let tui_allowed = command.supports_tui()
        && !tracing_owns_terminal
        && !quiet
        && cli
            .cli_options
            .tui_preference()
            .resolve(env::var_os("CI").is_some() || is_ai_agent());

    let terminal_interactive = terminal::can_use_stdin_interactively();
    let shell_interactive = matches!(&command, Commands::Shell { cmd: None, .. });
    let (tui_preferences, shell_keybindings) = load_user_preferences(
        tui_allowed,
        shell_interactive,
        terminal_interactive,
        user_config_path.as_deref(),
    )?;
    let mut frontend = FrontendOptions {
        tui_allowed,
        tui_preferences: Arc::new(tui_preferences),
        tui_context: Arc::new(devenv_tui::TuiRunContext::default()),
        shell_keybindings: shell_keybindings.clone(),
        tracing_owns_terminal,
        log_level,
        tracing_specs,
        verbosity,
    };

    // --- Project configuration ---

    // A `path:` source resolves to a live directory whose devenv.yaml graph is
    // merged into the config by Config::load_with_source, which also appends
    // the source's module as an absolute `path:` import. Relative refs resolve
    // against the invocation cwd (persisted bindings are always absolute).
    let from_path = from_source
        .as_deref()
        .and_then(|from| from.strip_prefix("path:"))
        .map(|path_str| {
            let full_path = devenv_core::paths::resolve_against(
                Path::new(path_str),
                &env::current_dir().unwrap_or_default(),
            );
            fs::canonicalize(&full_path).unwrap_or(full_path)
        });

    let mut config = Config::load_with_source(from_path.as_deref())?;
    config.check_version(crate_version!())?;

    let input_overrides = InputOverrides::from(cli.input_overrides);
    for chunk in input_overrides.override_inputs.chunks_exact(2) {
        let [name, url] = chunk else {
            unreachable!("chunks_exact(2)")
        };
        config
            .override_input_url(name, url)
            .wrap_err_with(|| format!("Failed to override input {name} with URL {url}"))?;
    }

    // A non-path source (flake ref, via --from or a persisted binding) is
    // fetched as the `from` input and its devenv.nix imported from the store.
    // Its devenv.yaml is not merged (that needs a fetch before config load);
    // `path:` sources get the full merge via Config::load_with_source above.
    if from_path.is_none()
        && let Some(from) = &from_source
    {
        let from_input = devenv_core::config::Input {
            url: Some(from.clone()),
            flake: false,
            follows: None,
            inputs: BTreeMap::new(),
            overlays: Vec::new(),
        };
        config.inputs.insert("from".to_string(), from_input);
        config.imports.push("from".to_string());
    }

    // --- Resolved settings ---

    // Read before the conversion consumes `cli.nix_args`.
    let nix_debugger = cli.nix_args.nix_debugger;
    let mut nix_settings =
        NixSettings::resolve(devenv_core::NixOptions::from(cli.nix_args), &config);
    if matches!(command, Commands::Update { .. }) {
        nix_settings.refresh_fetchers = true;
    }
    let shell_settings = ShellSettings::resolve_with_shell_hint(
        devenv_core::ShellOptions::from(cli.shell_args),
        &config,
        shell_hint,
    );
    frontend.tui_context = Arc::new(devenv_tui::TuiRunContext {
        profiles: shell_settings.profiles.clone(),
        project_root: project_root.or_else(|| env::current_dir().ok()),
        command: Some(command.as_str().to_string()),
        shell: Some(shell_settings.shell.clone()),
        started_at: Some(Instant::now()),
    });
    let cache_settings = CacheSettings::resolve(devenv_core::CacheOptions::from(cli.cache_args));
    let secret_settings =
        SecretSettings::resolve(devenv_core::SecretOptions::from(cli.secret_args), &config);
    let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);

    // --- Command execution ---

    let is_testing = matches!(&command, Commands::Test { .. });
    // `gc` operates on the global devenv store and doesn't need a project.
    let require_project_file = !matches!(&command, Commands::Gc { .. });
    let test_environment = TestEnvironment::for_command(&command)?;

    // The frontend must prepare the PTY session before `Devenv` is constructed.
    // Capture the terminal state once so the backend follows the same decision.
    let use_pty = shell_settings.reload
        && matches!(&command, Commands::Shell { cmd: None, .. })
        && io::stdin().is_foreground_terminal()
        && io::stdout().is_terminal();

    // Read off `config` before its fields are moved into `DevenvOptions`.
    let strict_ports = config.strict_ports.unwrap_or(false);
    let require_version_match = config.requires_version_match();
    let shutdown = Shutdown::new();

    let devenv_options = devenv::DevenvOptions {
        inputs: config.inputs,
        imports: config.imports,
        git_root: config.git_root,
        nixpkgs_config,
        nix_settings,
        shell_settings,
        cache_settings,
        secret_settings,
        input_overrides,
        from_external: from_source.is_some(),
        require_version_match,
        devenv_root: None,
        devenv_dotfile: test_environment
            .as_ref()
            .and_then(|environment| environment.dotfile.clone()),
        devenv_state: test_environment
            .as_ref()
            .map(|environment| environment.state.clone()),
        shell_cwd,
        is_testing,
        require_project_file,
    };

    Ok(PreparedCommand {
        frontend,
        backend: BackendOptions {
            devenv: devenv_options,
            command,
            verbosity,
            nix_debugger,
            strict_ports,
            use_pty,
            shell_keybindings,
        },
        discovered_root,
        test_environment,
        shutdown,
    })
}

fn load_user_preferences(
    tui_allowed: bool,
    shell_interactive: bool,
    terminal_interactive: bool,
    path: Option<&Path>,
) -> Result<(
    devenv_tui::TuiPreferences,
    devenv_shell::keybindings::ShellKeybindings,
)> {
    if terminal_interactive && (tui_allowed || shell_interactive) {
        let config = devenv::user_config::load(path)?;
        let shell_keybindings = config.shell.resolve()?;
        Ok((config.tui, shell_keybindings))
    } else {
        Ok((
            devenv_tui::TuiPreferences::default(),
            devenv_shell::keybindings::ShellKeybindings::default(),
        ))
    }
}

/// The activity sink for a run.
///
/// Three mutually exclusive variants. `None` means tracing owns the terminal;
/// the first-party activity channel has no renderer, while the independently
/// emitted activity spans and update events remain available to trace exports.
enum Renderer {
    Tui {
        activity_rx: tokio_mpsc::UnboundedReceiver<devenv_activity::ActivityEvent>,
        preferences: Arc<devenv_tui::TuiPreferences>,
        context: Arc<devenv_tui::TuiRunContext>,
    },
    Console(tokio_mpsc::UnboundedReceiver<devenv_activity::ActivityEvent>),
    None,
}

impl Renderer {
    /// Pick the renderer and install its activity sink. The returned guard
    /// clears the sink on drop and must outlive the backend (it produces the
    /// events).
    fn init(frontend: &FrontendOptions) -> (Self, Option<ActivityGuard>) {
        if frontend.tui_allowed && terminal::can_use_stdin_interactively() {
            let (rx, handle) = devenv_activity::init();
            (
                Renderer::Tui {
                    activity_rx: rx,
                    preferences: frontend.tui_preferences.clone(),
                    context: frontend.tui_context.clone(),
                },
                Some(handle.install()),
            )
        } else if !frontend.tracing_owns_terminal {
            let (rx, handle) = devenv_activity::init();
            (Renderer::Console(rx), Some(handle.install()))
        } else {
            (Renderer::None, None)
        }
    }

    /// Drive the renderer to completion and return the backend-to-frontend
    /// mailbox for an optional shell-session handoff.
    fn drive(
        self,
        shutdown: &Arc<Shutdown>,
        frontend_rx: tokio_mpsc::Receiver<FrontendCommand>,
        event_tx: tokio_mpsc::Sender<FrontendEvent>,
        verbosity: VerbosityLevel,
    ) -> Result<tokio_mpsc::Receiver<FrontendCommand>> {
        match self {
            Renderer::Tui {
                activity_rx,
                preferences,
                context,
            } => {
                let filter_level = if matches!(verbosity, VerbosityLevel::Verbose) {
                    ActivityLevel::Debug
                } else {
                    ActivityLevel::Info
                };
                current_thread_runtime("TUI")?
                    .block_on(async {
                        devenv_tui::TuiApp::new(activity_rx, frontend_rx, shutdown.clone())
                            .with_event_sender(event_tx)
                            .with_preferences((*preferences).clone())
                            .with_run_context((*context).clone())
                            .filter_level(filter_level)
                            .run()
                            .await
                    })
                    .into_diagnostic()
                    .wrap_err("TUI error")
            }
            Renderer::Console(activity_rx) => {
                drop(event_tx);
                Ok(current_thread_runtime("console")?.block_on(async {
                    devenv::console::ConsoleOutput::new(activity_rx, frontend_rx, verbosity)
                        .run()
                        .await
                }))
            }
            Renderer::None => {
                drop(event_tx);
                let mut frontend_rx = frontend_rx;
                current_thread_runtime("frontend control")?.block_on(async {
                    loop {
                        match frontend_rx.recv().await {
                            Some(FrontendCommand::ExitRenderer) | None => break,
                            Some(FrontendCommand::SetAttached(_)) => {}
                            Some(FrontendCommand::PauseForInteraction { ready, resume }) => {
                                let _ = ready.send(());
                                let _ = tokio::task::spawn_blocking(move || resume.recv()).await;
                            }
                            Some(FrontendCommand::Shell(_)) => {
                                unreachable!("shell command received before renderer exit")
                            }
                        }
                    }
                });
                Ok(frontend_rx)
            }
        }
    }
}

/// How long the frontend may outlive the backend. It only drains remaining
/// events at that point; longer means it's wedged (e.g. on a dead terminal).
const FRONTEND_DRAIN_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum Event {
    Signal(tokio_shutdown::Signal),
    FrontendExited,
    BackendExited,
}

/// Sends its event on drop, so a thread reports its exit even when panicking.
struct ExitNotice(mpsc::Sender<Event>, Event);

impl Drop for ExitNotice {
    fn drop(&mut self) {
        let _ = self.0.send(self.1);
    }
}

/// Apply signals and wait for both threads: the backend without a deadline,
/// then the frontend with `grace` to drain. Returns whether the frontend
/// exited.
fn event_loop(
    events: &mpsc::Receiver<Event>,
    shutdown: &Shutdown,
    frontend_tx: tokio_mpsc::Sender<FrontendCommand>,
    grace: Duration,
) -> bool {
    let mut frontend_exited = false;

    loop {
        match events.recv() {
            Ok(Event::Signal(signal)) => shutdown.handle_signal(signal),
            Ok(Event::FrontendExited) => frontend_exited = true,
            Ok(Event::BackendExited) | Err(_) => break,
        }
    }

    // A normal backend path has already initiated shutdown, but a panic can
    // bypass backend_thread_main's async cleanup tail. Cancellation is also
    // how a ShellSession that already owns the terminal learns that its
    // coordinator is gone.
    shutdown.shutdown();

    // Backstop for a backend that died without telling the renderer to exit:
    // the frontend would otherwise render until the grace expires.
    let _ = frontend_tx.try_send(FrontendCommand::ExitRenderer);
    // This is the root sender. Once the backend has exited there can be no
    // more legitimate commands, so close the mailbox after the backstop. In
    // particular, a shell handoff that never received Spawn must observe EOF
    // instead of waiting for the frontend abandonment deadline.
    drop(frontend_tx);

    let deadline = Instant::now() + grace;
    while !frontend_exited {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(Event::Signal(signal)) => shutdown.handle_signal(signal),
            Ok(Event::FrontendExited) => frontend_exited = true,
            Ok(Event::BackendExited) => {}
            Err(_) => break,
        }
    }

    frontend_exited
}

/// Keep applying signals after the event loop ends (debugger REPL, exec
/// teardown).
fn drain_signals(events: mpsc::Receiver<Event>, shutdown: Arc<Shutdown>) {
    std::thread::spawn(move || {
        for event in events {
            if let Event::Signal(signal) = event {
                shutdown.handle_signal(signal);
            }
        }
    });
}

fn backend_thread_main(
    options: BackendOptions,
    caller: Caller,
    frontend_tx: tokio_mpsc::Sender<FrontendCommand>,
    event_rx: tokio_mpsc::Receiver<FrontendEvent>,
    shutdown: Arc<Shutdown>,
) -> Result<CommandResult> {
    build_gc_runtime().block_on(async {
        let output = run_backend(options, shutdown.clone(), frontend_tx, event_rx, caller).await;

        // Fallback for paths that didn't run cleanup themselves
        // (PTY shell, REPL). No-op when run_backend already did it.
        shutdown.shutdown_and_wait().await;

        output
    })
}

fn frontend_thread_main(
    renderer: Renderer,
    session_status_line: Option<bool>,
    shell_keybindings: devenv_shell::keybindings::ShellKeybindings,
    frontend_rx: tokio_mpsc::Receiver<FrontendCommand>,
    event_tx: tokio_mpsc::Sender<FrontendEvent>,
    verbosity: VerbosityLevel,
    shutdown: Arc<Shutdown>,
) -> Result<()> {
    let frontend_rx = renderer.drive(&shutdown, frontend_rx, event_tx.clone(), verbosity)?;

    // The renderer has exited and released the terminal. A PTY shell session
    // takes it over on this same thread, so terminal ownership never crosses
    // a thread boundary. Its exit code reaches the backend through the
    // session's event mailbox.
    if let Some(show_status_line) = session_status_line {
        current_thread_runtime("session")?
            .block_on(
                ShellSession::with_defaults()
                    .with_status_line(show_status_line)
                    .with_keybindings(shell_keybindings)
                    .with_shutdown_token(shutdown.cancellation_token())
                    .run(frontend_rx, event_tx, SessionIo::default()),
            )
            .into_diagnostic()
            .wrap_err("Shell session error")?;
    }
    Ok(())
}

fn shell_session_status_line(
    command: &Commands,
    use_pty: bool,
    status_line_enabled: bool,
) -> Option<bool> {
    match command {
        Commands::Shell { cmd, .. } if use_pty => Some(cmd.is_none() && status_line_enabled),
        _ => None,
    }
}

/// Single entry point for all command execution. Process replacement
/// (exec, debugger) is the caller's job, after every guard here has dropped.
fn run(prepared: PreparedCommand, caller: Caller) -> Result<CommandResult> {
    let PreparedCommand {
        frontend,
        backend: backend_options,
        discovered_root,
        // Keep command-scoped temporary directories alive until both threads
        // have finished and the command result has been collected.
        test_environment: _test_environment,
        shutdown,
    } = prepared;

    let (renderer, _activity_guard) = Renderer::init(&frontend);
    let tui = matches!(&renderer, Renderer::Tui { .. });
    let shell_keybindings = frontend.shell_keybindings.clone();

    let _tracing_guard = devenv_tracing::init_tracing(frontend.log_level, &frontend.tracing_specs);

    if let Some(root) = &discovered_root {
        tracing::info!("using project root {}", root.display());
    }

    // TUI terminal setup: save state before raw mode, install a restore hook
    // for panics. Force-exit (second Ctrl+C) is handled below.
    if tui {
        devenv_tui::app::save_terminal_state();

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            devenv_tui::app::restore_terminal();
            prev_hook(info);
        }));
    }

    // Force-exit (second Ctrl+C) re-raises the signal, so neither the process
    // manager's teardown nor any destructor runs. Processes live in their own
    // process scopes and would survive as orphans, so signal them here, then hand the
    // terminal back before the process dies.
    shutdown.set_pre_exit_hook(move || {
        devenv_processes::kill_process_scopes();
        if tui {
            devenv_tui::app::restore_terminal();
        }
    });

    let (events_tx, events_rx) = mpsc::channel();

    tokio_shutdown::spawn_signal_listener({
        let events_tx = events_tx.clone();
        move |signal| {
            let _ = events_tx.send(Event::Signal(signal));
        }
    });

    // One typed mailbox in each direction spans the complete frontend/backend
    // boundary. The frontend retains command-receiver ownership across the
    // renderer -> PTY shell transition.
    let (frontend_tx, frontend_rx) = tokio_mpsc::channel(16);
    let (event_tx, event_rx) = tokio_mpsc::channel(16);
    let session_status_line = shell_session_status_line(
        &backend_options.command,
        backend_options.use_pty,
        frontend.tui_preferences.statusline.enabled,
    );

    // Eval futures are !Send and run on the block_on task, so the thread
    // itself needs the Nix stack size too, not just the GC-registered workers.
    let backend_thread = {
        let events_tx = events_tx.clone();
        let shutdown = shutdown.clone();
        let backend_frontend_tx = frontend_tx.clone();
        std::thread::Builder::new()
            .name("devenv".into())
            .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
            .spawn(move || {
                let _notice = ExitNotice(events_tx, Event::BackendExited);
                backend_thread_main(
                    backend_options,
                    caller,
                    backend_frontend_tx,
                    event_rx,
                    shutdown,
                )
            })
            .into_diagnostic()
            .wrap_err("Failed to spawn devenv thread")?
    };

    let frontend_thread = {
        let events_tx = events_tx.clone();
        let shutdown = shutdown.clone();
        let verbosity = frontend.verbosity;
        std::thread::Builder::new()
            .name("frontend".into())
            .spawn(move || {
                let _notice = ExitNotice(events_tx, Event::FrontendExited);
                frontend_thread_main(
                    renderer,
                    session_status_line,
                    shell_keybindings,
                    frontend_rx,
                    event_tx,
                    verbosity,
                    shutdown,
                )
            })
            .into_diagnostic()
            .wrap_err("Failed to spawn frontend thread")?
    };

    let frontend_exited = event_loop(&events_rx, &shutdown, frontend_tx, FRONTEND_DRAIN_GRACE);
    drain_signals(events_rx, shutdown.clone());

    let frontend_result = if frontend_exited {
        frontend_thread
            .join()
            .unwrap_or_else(|payload| panic::resume_unwind(payload))
    } else {
        tracing::error!("frontend did not stop after backend exit; abandoning it");
        if tui {
            devenv_tui::app::restore_terminal();
        }
        Ok(())
    };

    let backend_result = backend_thread
        .join()
        .map_err(|e| miette::miette!("devenv thread panicked: {}", panic_message(e)))?;

    frontend_result?;
    backend_result
}

/// Print the error and launch the Nix debugger REPL.
fn launch_debugger(devenv: devenv::Devenv, err: miette::Report) -> Result<()> {
    eprintln!("{err:?}");
    // Skip prepare_repl() — the debugger already has eval context from
    // the failed command, and re-evaluating would likely fail again,
    // preventing debugger_is_pending() from being checked in launch_repl().
    launch_repl_thread(devenv)
}

/// Launch the interactive REPL on a fresh GC-registered thread. Callers have
/// already evaluated (or tried to), and `run()` has torn down the renderer,
/// so the terminal is free.
fn launch_repl_thread(devenv: devenv::Devenv) -> Result<()> {
    let handle = std::thread::Builder::new()
        .name("repl".into())
        .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .spawn(move || build_gc_runtime().block_on(async { devenv.launch_repl().await }))
        .into_diagnostic()
        .wrap_err("Failed to spawn REPL thread")?;
    handle
        .join()
        .map_err(|e| miette::miette!("REPL thread panicked: {}", panic_message(e)))?
}

/// Run the backend: construct Devenv and dispatch the command.
#[instrument(
    name = "devenv",
    skip_all,
    fields(
        devenv.caller = caller.as_str(),
        devenv.command = backend.command.as_str(),
    )
)]
async fn run_backend(
    backend: BackendOptions,
    shutdown: Arc<Shutdown>,
    frontend_tx: tokio_mpsc::Sender<FrontendCommand>,
    event_rx: tokio_mpsc::Receiver<FrontendEvent>,
    caller: Caller,
) -> Result<CommandResult> {
    let mut event_rx = Some(event_rx);

    let BackendOptions {
        devenv: devenv_options,
        command,
        verbosity,
        nix_debugger,
        strict_ports: config_strict_ports,
        use_pty,
        shell_keybindings,
    } = backend;

    let devenv = Devenv::new(devenv_options, shutdown.clone()).await?;

    // PTY shell hands Devenv off to an owner task; we reclaim it after the session.
    if use_pty && let Commands::Shell { cmd: None, args } = command {
        // Pre-compute shell environment while we still own Devenv directly.
        // This must happen while TUI is active since get_dev_environment has #[activity].
        let dotfile = devenv.dotfile().to_path_buf();
        let bash_path = devenv.get_bash_path().await?;
        let clean = devenv.options().shell_settings.clean.clone();
        let shell = devenv.options().shell_settings.shell.clone();
        let shell_path = devenv.options().shell_settings.shell_path.clone();
        let shell_cwd = devenv.shell_cwd().map(Path::to_path_buf);
        let (task_exports, task_messages) = devenv.run_enter_shell_tasks(None, verbosity).await?;
        // Load dotenv after enterShell tasks so a task that creates or updates the
        // file affects this shell entry immediately.
        let initial_env_script = devenv.print_dev_env(false).await?;

        let (client, owner_handle) = devenv::reload::spawn_owner(devenv, verbosity);
        let result = run_reload_shell(ReloadShellArgs {
            devenv: client,
            cmd: None,
            args,
            initial_env_script,
            bash_path,
            clean,
            shell,
            shell_path,
            dotfile,
            task_exports,
            task_messages,
            shell_cwd,
            shell_keybindings,
            frontend_tx,
            event_rx: event_rx
                .take()
                .expect("frontend event receiver is available for PTY shell"),
        })
        .await
        .map(|exit_code| {
            // On signalled shutdown the session reports no exit code; recover
            // `128 + sig` so callers see e.g. SIGHUP as 129.
            let resolved =
                exit_code.or_else(|| shutdown.last_signal().map(|sig| (128 + sig as i32) as u32));
            match resolved {
                Some(code) => CommandResult::ExitCode(code as i32),
                None => CommandResult::Done,
            }
        });
        let devenv = tokio::task::block_in_place(|| owner_handle.join())
            .map_err(|e| miette::miette!("Devenv owner thread panicked: {}", panic_message(e)))?;
        return debugger_or_err(result, nix_debugger, devenv);
    }

    // REPL: assemble and evaluate under the renderer, then carry `Devenv` out
    // and launch the interactive REPL after `run()` has torn everything down
    // (same pattern as the debugger).
    if let Commands::Repl {} = command {
        let prepared = devenv.prepare_repl().await;
        shutdown.shutdown_and_wait().await;
        let _ = frontend_tx.send(FrontendCommand::ExitRenderer).await;
        return match prepared {
            Ok(()) => Ok(CommandResult::Repl(Box::new(devenv))),
            Err(err) => debugger_or_err(Err(err), nix_debugger, devenv),
        };
    }

    // All other commands
    let result = dispatch_command(
        &devenv,
        command,
        verbosity,
        event_rx,
        frontend_tx.clone(),
        config_strict_ports,
    )
    .await;

    // Drain cleanup (e.g. cachix push finalization) while the TUI is
    // still rendering, so its activity stays visible to the user.
    shutdown.shutdown_and_wait().await;

    // Signal the renderer to stop, after the drain, so its activity stayed
    // visible. Done before the debugger check so the TUI releases the
    // terminal before the debugger takes it.
    let _ = frontend_tx.send(FrontendCommand::ExitRenderer).await;

    debugger_or_err(result, nix_debugger, devenv)
}

/// On error with `--nix-debugger`, defer to the debugger REPL by carrying the
/// owned `Devenv` (and error) back out as a `CommandResult`. The caller, after
/// joining the backend thread and tearing down the TUI, launches the REPL.
/// Without the flag, errors propagate normally.
fn debugger_or_err(
    result: Result<CommandResult>,
    nix_debugger: bool,
    devenv: devenv::Devenv,
) -> Result<CommandResult> {
    match result {
        Err(err) if nix_debugger => Ok(CommandResult::Debugger(Box::new(devenv), err)),
        other => other,
    }
}

/// Result of a CLI command execution.
/// This is a CLI concern - the library returns domain types.
enum CommandResult {
    /// Command completed normally
    Done,
    /// Print this string after UI cleanup
    Print(String),
    /// Exec into this command after cleanup (TUI shutdown, terminal restore)
    Exec(Command),
    /// Exit with a specific code (e.g., from shell exit)
    ExitCode(i32),
    /// Eval failed under `--nix-debugger`: launch the Nix debugger REPL with
    /// the owned `Devenv`. Handled by the caller after TUI teardown, never
    /// reaches `exec()`.
    Debugger(Box<devenv::Devenv>, miette::Report),
    /// `devenv repl`, already prepared: launch the interactive REPL with the
    /// owned `Devenv`. Handled by the caller after TUI teardown, never
    /// reaches `exec()`.
    Repl(Box<devenv::Devenv>),
}

impl CommandResult {
    /// Execute the pending action.
    /// - Done: returns Ok(())
    /// - Print: prints to stdout and returns Ok(())
    /// - Exec: replaces the current process (never returns on success)
    fn exec(self) -> Result<()> {
        match self {
            CommandResult::Done => Ok(()),
            CommandResult::Print(output) => {
                print!("{output}");
                Ok(())
            }
            CommandResult::Exec(mut cmd) => {
                use os::unix::process::CommandExt;
                let err = cmd.exec();
                miette::bail!("Failed to exec: {}", err);
            }
            CommandResult::ExitCode(code) => {
                process::exit(code);
            }
            CommandResult::Debugger(..) | CommandResult::Repl(..) => {
                unreachable!("REPL launch is handled in main_inner() before exec()")
            }
        }
    }
}

/// Start processes and map the outcome to CLI control flow.
async fn run_up(
    devenv: &Devenv,
    processes: Vec<String>,
    mode: devenv::tasks::RunMode,
    options: devenv::ProcessOptions,
    verbosity: VerbosityLevel,
) -> Result<CommandResult> {
    match devenv.up(processes, mode, options, verbosity).await? {
        ProcessStartOutcome::Completed => Ok(CommandResult::Done),
        ProcessStartOutcome::Exec(shell_command) => Ok(CommandResult::Exec(shell_command.command)),
    }
}

fn process_options(
    mode: ClientRunMode,
    strict_ports: bool,
    frontend_event_rx: Option<tokio_mpsc::Receiver<FrontendEvent>>,
    frontend_command_tx: tokio_mpsc::Sender<FrontendCommand>,
) -> devenv::ProcessOptions {
    let detached = mode == ClientRunMode::ReturnAfterStart;
    devenv::ProcessOptions {
        mode,
        log_to_file: detached,
        strict_ports,
        frontend_event_rx,
        frontend_command_tx: Some(frontend_command_tx),
        daemon: detached,
    }
}

/// Resolve `UpArgs` into `ProcessOptions` and start processes.
async fn run_up_args(
    devenv: &Devenv,
    up_args: devenv::cli::UpArgs,
    config_strict_ports: bool,
    frontend_event_rx: Option<tokio_mpsc::Receiver<FrontendEvent>>,
    frontend_command_tx: tokio_mpsc::Sender<FrontendCommand>,
    verbosity: VerbosityLevel,
) -> Result<CommandResult> {
    let strict_ports = devenv_core::settings::flag(up_args.strict_ports, up_args.no_strict_ports)
        .unwrap_or(config_strict_ports);
    let mode = if up_args.detach {
        ClientRunMode::ReturnAfterStart
    } else {
        ClientRunMode::Follow
    };
    let options = process_options(mode, strict_ports, frontend_event_rx, frontend_command_tx);
    run_up(devenv, up_args.processes, up_args.mode, options, verbosity).await
}

/// Dispatch a CLI command to the appropriate Devenv method.
#[instrument(skip_all)]
async fn dispatch_command(
    devenv: &Devenv,
    command: Commands,
    verbosity: VerbosityLevel,
    frontend_event_rx: Option<tokio_mpsc::Receiver<FrontendEvent>>,
    frontend_command_tx: tokio_mpsc::Sender<FrontendCommand>,
    config_strict_ports: bool,
) -> Result<CommandResult> {
    match command {
        Commands::Shell { cmd, ref args } => {
            // Non-PTY shell path (PTY is handled as early return in run_backend)
            // Messages are injected into the shell script by prepare_shell() via self.task_messages.
            devenv.run_enter_shell_tasks(None, verbosity).await?;

            let shell_config = match cmd {
                Some(cmd) => devenv.prepare_exec(Some(cmd), args).await?,
                None => devenv.shell().await?,
            };

            Ok(CommandResult::Exec(shell_config.command))
        }
        Commands::Test { .. } => {
            devenv.test(verbosity).await?;
            Ok(CommandResult::Done)
        }
        Commands::Container { command } => match command {
            ContainerCommand::Build { name } => {
                let path = devenv.container_build(&name).await?;
                Ok(CommandResult::Print(format!("{path}\n")))
            }
            ContainerCommand::Copy {
                name,
                copy_args,
                registry,
            } => {
                devenv
                    .container_copy(&name, &copy_args, registry.as_deref(), verbosity)
                    .await?;
                Ok(CommandResult::Done)
            }
            ContainerCommand::Run { name, copy_args } => {
                let shell_config = devenv.container_run(&name, &copy_args, verbosity).await?;
                Ok(CommandResult::Exec(shell_config.command))
            }
        },
        Commands::Generate => {
            miette::bail!(indoc::indoc! {"
                The generate command has been removed.

                To generate devenv.yaml and devenv.nix using AI, you can:

                1. Use the web version at https://devenv.new

                2. Use `devenv mcp` with an AI agent (Claude Code, Cursor, etc.)
            "})
        }
        Commands::Search { name } => {
            let output = devenv.search(&name).await?;
            Ok(CommandResult::Print(output))
        }
        Commands::Gc {} => {
            let (paths_deleted, bytes_freed) = devenv.gc().await?;
            let mb_freed = bytes_freed / (1024 * 1024);
            Ok(CommandResult::Print(format!(
                "Done. Deleted {paths_deleted} store paths, freed {mb_freed} MB.\n"
            )))
        }
        Commands::Info {} => {
            let output = commands::info::run(devenv).await?;
            Ok(CommandResult::Print(format!("{output}\n")))
        }
        Commands::Repl {} => {
            unreachable!("Repl is handled in run_backend before dispatch_command is called")
        }
        Commands::Build { attributes } => {
            let results = devenv.build(&attributes).await?;
            let json_map: serde_json::Map<String, serde_json::Value> = results
                .into_iter()
                .map(|(attr, path)| (attr, serde_json::Value::String(path.display().to_string())))
                .collect();
            let json = serde_json::to_string_pretty(&json_map)
                .into_diagnostic()
                .wrap_err("Failed to serialize JSON")?;
            Ok(CommandResult::Print(format!("{json}\n")))
        }
        Commands::Eval { attributes } => {
            let json = devenv.eval(&attributes).await?;
            Ok(CommandResult::Print(format!("{json}\n")))
        }
        Commands::Update { name } => Ok(devenv
            .update(&name)
            .await?
            .map_or(CommandResult::Done, CommandResult::Print)),
        Commands::Up { up_args } => {
            run_up_args(
                devenv,
                up_args,
                config_strict_ports,
                frontend_event_rx,
                frontend_command_tx,
                verbosity,
            )
            .await
        }
        Commands::Down {} => {
            devenv.down().await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes { command } => match command {
            ProcessesCommand::Up { up_args } => {
                run_up_args(
                    devenv,
                    up_args,
                    config_strict_ports,
                    frontend_event_rx,
                    frontend_command_tx,
                    verbosity,
                )
                .await
            }
            ProcessesCommand::Start { name: None, detach } => {
                let mode = if detach {
                    ClientRunMode::ReturnAfterStart
                } else {
                    ClientRunMode::Follow
                };
                let options = process_options(
                    mode,
                    config_strict_ports,
                    frontend_event_rx,
                    frontend_command_tx,
                );
                run_up(
                    devenv,
                    vec![],
                    devenv::tasks::RunMode::All,
                    options,
                    verbosity,
                )
                .await
            }
            ProcessesCommand::Start {
                name: Some(name), ..
            } => {
                if devenv.native_manager_running().await
                    || devenv.external_process_manager_state_exists()
                {
                    // Detached external-manager state must also take the control-command
                    // path: `processes_start` then reports that named starts
                    // require the native manager. Treating it like "no manager"
                    // would attempt a second cold `up -d` and disturb the
                    // already-running external-manager instance.
                    devenv.processes_start(&name).await?;
                    Ok(CommandResult::Done)
                } else {
                    let options = process_options(
                        ClientRunMode::ReturnAfterStart,
                        config_strict_ports,
                        frontend_event_rx,
                        frontend_command_tx,
                    );
                    run_up(
                        devenv,
                        vec![name],
                        devenv::tasks::RunMode::Before,
                        options,
                        verbosity,
                    )
                    .await
                }
            }
            ProcessesCommand::Attach {} => {
                devenv
                    .attach(frontend_event_rx, Some(frontend_command_tx))
                    .await?;
                Ok(CommandResult::Done)
            }
            ProcessesCommand::Down {} | ProcessesCommand::Stop { name: None } => {
                devenv.down().await?;
                Ok(CommandResult::Done)
            }
            ProcessesCommand::Stop { name: Some(name) } => {
                devenv.processes_stop(&name).await?;
                Ok(CommandResult::Done)
            }
            ProcessesCommand::Wait { timeout } => {
                devenv.wait_for_ready(Duration::from_secs(timeout)).await?;
                Ok(CommandResult::Done)
            }
            ProcessesCommand::List {} => Ok(CommandResult::Print(devenv.processes_list().await?)),
            ProcessesCommand::Status { name } => {
                Ok(CommandResult::Print(devenv.processes_status(&name).await?))
            }
            ProcessesCommand::Logs {
                name,
                lines,
                stdout,
                stderr,
            } => {
                let output = devenv.processes_logs(&name, lines, stdout, stderr).await?;
                Ok(CommandResult::Print(output))
            }
            ProcessesCommand::Restart { name } => {
                devenv.processes_restart(&name).await?;
                Ok(CommandResult::Done)
            }
        },
        Commands::Tasks { command } => match command {
            TasksCommand::Run {
                tasks,
                mode,
                show_output,
                input,
                input_json,
            } => {
                let output = devenv
                    .tasks_run(tasks, mode, show_output, input, input_json, verbosity)
                    .await?;
                Ok(CommandResult::Print(format!("{output}\n")))
            }
            TasksCommand::List { json } => {
                let output = devenv.tasks_list(json).await?;
                Ok(CommandResult::Print(format!("{output}\n")))
            }
        },
        Commands::Changelogs {} => Ok(devenv
            .changelogs()
            .await?
            .map_or(CommandResult::Done, CommandResult::Print)),
        // hidden
        Commands::Assemble => {
            let _ = devenv.backend();
            Ok(CommandResult::Done)
        }
        Commands::PrintDevEnv { json } => {
            let output = devenv.print_dev_env(json).await?;
            Ok(CommandResult::Print(output))
        }
        Commands::DirenvExport => {
            // Discard messages: direnv captures stdout as env var definitions,
            // so echo statements would corrupt the output.
            let task_exports = match devenv.run_enter_shell_tasks(None, verbosity).await {
                Ok((exports, _messages)) => exports,
                Err(e) => {
                    tracing::warn!("enterShell tasks failed, skipping exports: {e}");
                    BTreeMap::new()
                }
            };
            // Re-read dotenv after tasks so generated files are exported on the
            // same direnv activation.
            let mut output = devenv.print_dev_env(false).await?;
            output.push_str(&devenv::format_shell_exports(&task_exports));
            Ok(CommandResult::Print(output))
        }
        Commands::PrintPaths => {
            let paths = devenv.paths();
            let output = format!(
                "DEVENV_DOTFILE=\"{}\"\nDEVENV_ROOT=\"{}\"\nDEVENV_GC=\"{}\"",
                paths.dotfile.display(),
                paths.root.display(),
                paths.dot_gc.display()
            );
            Ok(CommandResult::Print(output))
        }
        Commands::Mcp { http } => {
            devenv::mcp::run_mcp_server(
                devenv.options().clone(),
                devenv.shutdown(),
                http.map(|p| p.unwrap_or(8080)),
            )
            .await?;
            Ok(CommandResult::Done)
        }
        Commands::Lsp { print_config } => {
            devenv::lsp::run(devenv, print_config).await?;
            Ok(CommandResult::Done)
        }
        Commands::UserConfig { .. }
        | Commands::Direnvrc
        | Commands::Version
        | Commands::Hook { .. }
        | Commands::Allow
        | Commands::Revoke
        | Commands::HookShouldActivate
        | Commands::DaemonProcesses { .. }
        | Commands::Init { .. }
        | Commands::Inputs { .. } => {
            unreachable!("dispatched in main_inner before Devenv construction")
        }
    }
}

struct ReloadShellArgs {
    devenv: devenv::reload::DevenvClient,
    cmd: Option<String>,
    args: Vec<String>,
    initial_env_script: String,
    bash_path: String,
    clean: devenv_core::config::Clean,
    shell: String,
    shell_path: Option<std::path::PathBuf>,
    dotfile: std::path::PathBuf,
    task_exports: BTreeMap<String, String>,
    task_messages: Vec<String>,
    shell_cwd: Option<PathBuf>,
    shell_keybindings: devenv_shell::keybindings::ShellKeybindings,
    frontend_tx: tokio_mpsc::Sender<FrontendCommand>,
    event_rx: tokio_mpsc::Receiver<FrontendEvent>,
}

/// Run the reload coordinator for a PTY shell session.
///
/// `ShellCoordinator` builds the initial shell command, tells the renderer to
/// exit once the build phase is over, and rebuilds on file changes.
/// `ShellSession` — driven by the frontend thread, which owns the terminal —
/// receives the commands and reports events back; its exit ends the
/// coordinator. enterShell tasks have already been executed by the caller (so
/// they can run in parallel via the DAG task system before the PTY starts).
///
/// Returns the shell's exit code as reported over the event mailbox.
async fn run_reload_shell(args: ReloadShellArgs) -> Result<Option<u32>> {
    let ReloadShellArgs {
        devenv,
        cmd,
        args,
        initial_env_script,
        bash_path,
        clean,
        shell,
        shell_path,
        dotfile,
        task_exports,
        task_messages,
        shell_cwd,
        shell_keybindings,
        frontend_tx,
        event_rx,
    } = args;

    // Watch files come from the eval cache during the first build.
    let reload_config = ReloadConfig::new(vec![]);

    let builder = DevenvShellBuilder {
        devenv,
        cmd,
        args,
        initial_env_script,
        bash_path,
        clean,
        dotfile,
        task_exports,
        task_messages,
        shell,
        shell_path,
        shell_cwd,
        shell_keybindings,
    };

    ShellCoordinator::run(reload_config, builder, frontend_tx, event_rx)
        .await
        .into_diagnostic()
        .wrap_err("Shell coordinator error")
}

/// Build a single-threaded tokio runtime for a UI renderer.
fn current_thread_runtime(ctx: &str) -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create {ctx} runtime"))
}

/// Create a tokio runtime with worker threads registered with Boehm GC.
///
/// Nix uses Boehm GC with parallel marking. During stop-the-world collection,
/// only registered threads are paused. This ensures all tokio worker threads
/// are properly registered to avoid race conditions.
///
/// The blocking pool inherits `on_thread_start` and `thread_stack_size`,
/// so `spawn_blocking` threads are registered and sized the same way.
fn build_gc_runtime() -> tokio::runtime::Runtime {
    devenv_nix_backend::nix_init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("devenv-worker")
        .thread_stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .on_thread_start(|| {
            let _ = devenv_nix_backend::gc_register_current_thread();
        })
        .build()
        .expect("Failed to create tokio runtime")
}

/// Prompt for missing secretspec secrets interactively.
fn prompt_secrets(
    source: devenv::SecretsPromptSource,
    provider: Option<String>,
    profile: Option<String>,
) -> Result<()> {
    let mut secrets = match source {
        devenv::SecretsPromptSource::Project => secretspec::Secrets::load()
            .into_diagnostic()
            .wrap_err("Failed to load secretspec")?,
        devenv::SecretsPromptSource::Cachix {
            devenv_root,
            secret_name,
        } => devenv::load_cachix_secretspec(&devenv_root, &secret_name)?,
    };

    if let Some(p) = &provider {
        secrets.set_provider(p);
    }
    if let Some(p) = &profile {
        secrets.set_profile(p);
    }

    secrets
        .ensure_secrets(provider, profile, true)
        .into_diagnostic()
        .wrap_err("Failed to set secrets")?;

    Ok(())
}

// Logging helpers

/// Resolve `--quiet`/`--verbose` (with AI-agent auto-quiet) into a `VerbosityLevel`.
fn resolve_verbosity(cli_options: &CliOptions) -> VerbosityLevel {
    if cli_options.verbose {
        VerbosityLevel::Verbose
    } else if cli_options.quiet || is_ai_agent() {
        VerbosityLevel::Quiet
    } else {
        VerbosityLevel::Normal
    }
}

// Error formatting helpers

/// Install a miette report hook with a custom theme.
///
/// The default theme draws a continuous vertical bar down the left edge of
/// every diagnostic, which makes copying error text awkward.
fn install_miette_hook() {
    miette::set_hook(Box::new(|_| {
        let mut theme = miette::GraphicalTheme::unicode();
        theme.characters.vbar = ' ';
        theme.characters.vbar_break = ' ';
        theme.characters.lbot = ' ';
        theme.characters.ltop = ' ';
        theme.characters.rbot = ' ';
        theme.characters.rtop = ' ';
        theme.characters.lcross = ' ';
        theme.characters.rcross = ' ';
        Box::new(
            miette::MietteHandlerOpts::new()
                .graphical_theme(theme)
                .context_lines(2)
                .wrap_lines(false)
                .build(),
        )
    }))
    .expect("miette hook already installed");
}

/// Extract a human readable message from a thread panic payload.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        format!("{payload:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;

    static PROCESS_STATE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn only_interactive_tui_and_shell_commands_load_user_configuration() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("invalid.yaml");
        fs::write(&path, "not: valid: yaml").unwrap();
        assert!(load_user_preferences(false, false, true, Some(&path)).is_ok());
        assert!(load_user_preferences(true, false, false, Some(&path)).is_ok());
        assert!(load_user_preferences(false, true, false, Some(&path)).is_ok());
        assert!(load_user_preferences(true, false, true, Some(&path)).is_err());
        assert!(load_user_preferences(false, true, true, Some(&path)).is_err());
    }

    #[test]
    fn tui_statusline_setting_controls_interactive_shell_statusline() {
        let shell = Commands::Shell {
            cmd: None,
            args: Vec::new(),
        };
        assert_eq!(shell_session_status_line(&shell, true, true), Some(true));
        assert_eq!(shell_session_status_line(&shell, true, false), Some(false));
        assert_eq!(shell_session_status_line(&shell, false, false), None);
    }

    #[test]
    fn shell_hint_is_only_accepted_from_hook_invocations() {
        let hook = Invocation::from_parts(Some("hook"), Some("zsh".to_string()));
        assert!(matches!(hook.caller, Caller::Hook));
        assert_eq!(hook.shell_hint.as_deref(), Some("zsh"));

        let cli = Invocation::from_parts(None, Some("zsh".to_string()));
        assert!(matches!(cli.caller, Caller::Cli));
        assert!(cli.shell_hint.is_none());

        let direnv = Invocation::from_parts(Some("direnv"), Some("zsh".to_string()));
        assert!(matches!(direnv.caller, Caller::Direnv));
        assert!(direnv.shell_hint.is_none());
    }

    struct ProcessStateGuard {
        cwd: PathBuf,
        devenv_home: Option<OsString>,
    }

    impl ProcessStateGuard {
        fn set(cwd: &Path, devenv_home: &Path) -> Self {
            let guard = Self {
                cwd: env::current_dir().unwrap(),
                devenv_home: env::var_os("DEVENV_HOME"),
            };
            env::set_current_dir(cwd).unwrap();
            // SAFETY: PROCESS_STATE_LOCK serializes this test's process-wide
            // cwd and environment changes, and both are restored by Drop.
            unsafe { env::set_var("DEVENV_HOME", devenv_home) };
            guard
        }
    }

    impl Drop for ProcessStateGuard {
        fn drop(&mut self) {
            env::set_current_dir(&self.cwd).unwrap();
            // SAFETY: See ProcessStateGuard::set; restoration happens before
            // PROCESS_STATE_LOCK is released, including during unwinding.
            unsafe {
                match &self.devenv_home {
                    Some(home) => env::set_var("DEVENV_HOME", home),
                    None => env::remove_var("DEVENV_HOME"),
                }
            }
        }
    }

    #[test]
    fn in_tree_allowed_profiles_reach_resolved_shell_settings() {
        let _lock = PROCESS_STATE_LOCK.lock().unwrap();
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("devenv.nix"), "{ }\n").unwrap();
        let devenv_home = project.path().join("devenv-home");
        let _state = ProcessStateGuard::set(project.path(), &devenv_home);

        commands::hook::allow(&devenv_home, None, &["base".to_string()]).unwrap();

        let cli = <Cli as clap::Parser>::parse_from(["devenv", "shell"]);
        let prepared = prepare_command(cli, None).unwrap();
        assert_eq!(prepared.backend.devenv.shell_settings.profiles, ["base"]);

        let cli = <Cli as clap::Parser>::parse_from(["devenv", "--profile=explicit", "shell"]);
        let prepared = prepare_command(cli, None).unwrap();
        assert_eq!(
            prepared.backend.devenv.shell_settings.profiles,
            ["explicit"]
        );

        commands::hook::allow(&devenv_home, None, &[]).unwrap();
        let cli = <Cli as clap::Parser>::parse_from(["devenv", "shell"]);
        let prepared = prepare_command(cli, None).unwrap();
        assert!(prepared.backend.devenv.shell_settings.profiles.is_empty());
    }

    #[test]
    fn renderer_none_hands_shell_command_off_after_exit() {
        let shutdown = Shutdown::new();
        let (frontend_tx, frontend_rx) = tokio_mpsc::channel(2);
        let (event_tx, _event_rx) = tokio_mpsc::channel(1);
        frontend_tx
            .blocking_send(FrontendCommand::ExitRenderer)
            .unwrap();
        frontend_tx
            .blocking_send(FrontendCommand::Shell(
                devenv_mailbox::ShellCommand::Shutdown,
            ))
            .unwrap();

        let mut frontend_rx = Renderer::None
            .drive(&shutdown, frontend_rx, event_tx, VerbosityLevel::Normal)
            .unwrap();

        assert!(matches!(
            frontend_rx.blocking_recv(),
            Some(FrontendCommand::Shell(
                devenv_mailbox::ShellCommand::Shutdown
            ))
        ));
    }

    #[test]
    fn event_loop_completes_when_both_exit() {
        let (tx, rx) = mpsc::channel();
        let (frontend_tx, mut frontend_rx) = tokio_mpsc::channel(1);
        let shutdown = Shutdown::new();
        drop(ExitNotice(tx.clone(), Event::FrontendExited));
        drop(ExitNotice(tx.clone(), Event::BackendExited));

        assert!(event_loop(
            &rx,
            &shutdown,
            frontend_tx,
            Duration::from_secs(5)
        ));
        assert!(shutdown.is_cancelled());
        assert!(matches!(
            frontend_rx.blocking_recv(),
            Some(FrontendCommand::ExitRenderer)
        ));
        assert!(frontend_rx.blocking_recv().is_none());
    }

    #[test]
    fn event_loop_abandons_wedged_frontend() {
        let (tx, rx) = mpsc::channel();
        let (frontend_tx, _frontend_rx) = tokio_mpsc::channel(1);
        let shutdown = Shutdown::new();
        let _wedged = ExitNotice(tx.clone(), Event::FrontendExited);
        drop(ExitNotice(tx.clone(), Event::BackendExited));

        assert!(!event_loop(
            &rx,
            &shutdown,
            frontend_tx,
            Duration::from_millis(50)
        ));
        assert!(shutdown.is_cancelled());
    }

    #[test]
    fn event_loop_sees_exit_of_panicking_thread() {
        let (tx, rx) = mpsc::channel();
        let (frontend_tx, _frontend_rx) = tokio_mpsc::channel(1);
        let shutdown = Shutdown::new();
        let notice = ExitNotice(tx.clone(), Event::FrontendExited);
        let thread = std::thread::spawn(move || {
            let _notice = notice;
            panic!("boom");
        });
        drop(ExitNotice(tx.clone(), Event::BackendExited));

        assert!(event_loop(
            &rx,
            &shutdown,
            frontend_tx,
            Duration::from_secs(5)
        ));
        assert!(shutdown.is_cancelled());
        assert!(thread.join().is_err());
    }
}
