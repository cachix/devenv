use clap::{CommandFactory, crate_version};
use clap_complete::CompleteEnv;
use devenv::{
    Devenv, RunMode,
    cli::{
        Cli, CliOptions, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand,
        TraceOutputSpec,
    },
    commands,
    processes::ProcessCommand,
    reload::DevenvShellBuilder,
    tracing as devenv_tracing,
};
use devenv_activity::{ActivityGuard, ActivityLevel};
use devenv_core::{
    CacheSettings, InputOverrides, NixSettings, SecretSettings, ShellSettings, VerbosityLevel,
    config::{self, Config},
};
use devenv_reload::{Config as ReloadConfig, ShellCoordinator};
use devenv_tui::{SessionIo, ShellSession, TuiHandoff};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal},
    os, panic,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tempfile::TempDir;
use tokio_shutdown::Shutdown;
use tracing::{info, instrument};

fn main() {
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
    // Retry loop: if the backend discovers secrets need interactive prompting,
    // we prompt the user and re-run the entire command with secrets now available.
    loop {
        let cli = Cli::parse_preprocessed();

        // Handle commands that don't need config or runtime
        match &cli.command {
            Commands::Version => {
                commands::version::run();
                return Ok(());
            }
            Commands::Direnvrc => {
                commands::direnvrc::run();
                return Ok(());
            }
            Commands::Hook { shell } => {
                commands::hook::print(shell);
                return Ok(());
            }
            Commands::Allow => {
                return commands::hook::allow();
            }
            Commands::Revoke => {
                return commands::hook::revoke();
            }
            Commands::HookShouldActivate => {
                return commands::hook::should_activate();
            }
            Commands::DaemonProcesses { config_file } => {
                return commands::daemon_processes::run(config_file);
            }
            Commands::Init { target } => {
                let verbosity = resolve_verbosity(&cli.cli_options);
                return commands::init::run(target.as_deref(), verbosity);
            }
            Commands::Inputs {
                command: InputsCommand::Add { name, url, follows },
            } => {
                return commands::inputs::add(name, url, follows);
            }
            _ => {}
        }

        let shutdown = Shutdown::new();
        let (ui, backend) = resolve(cli, shutdown.clone())?;

        match run(ui, backend, shutdown) {
            Err(err) => match err.downcast::<devenv::SecretsNeedPrompting>() {
                Ok(secrets_err) => {
                    // Only prompt interactively when stdin is a terminal;
                    // in non-interactive contexts (e.g. direnv), fail with the error.
                    if !io::stdin().is_terminal() {
                        return Err(secrets_err.into());
                    }
                    prompt_secrets(secrets_err.provider, secrets_err.profile)?;
                    continue;
                }
                Err(err) => return Err(err),
            },
            ok => return ok,
        }
    }
}

/// Options for the UI/renderer thread.
struct UiOptions {
    tui: bool,
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
    use_pty: bool,
    nix_debugger: bool,
    strict_ports: bool,
    /// Kept alive for the duration of the backend run; `Drop` removes the dirs.
    test_dirs: TestDirs,
}

/// Temporary dotfile / state directories for `devenv test`.
///
/// The `TempDir` guards must outlive the command; dropping `TestDirs` removes
/// the directories.
struct TestDirs {
    dotfile: Option<PathBuf>,
    state: Option<PathBuf>,
    _guards: (Option<TempDir>, Option<TempDir>),
}

impl TestDirs {
    /// Resolve test directories for the given command.
    ///
    /// Non-`test` commands get no overrides (`Devenv` uses the default
    /// `.devenv` layout). `devenv test` either runs fully isolated in temp
    /// directories (`--override-dotfile`) or reuses the real `.devenv` with an
    /// isolated `.devenv/test-state` so the eval cache survives across runs.
    fn setup(command: &Commands) -> Result<Self> {
        let Commands::Test {
            override_dotfile, ..
        } = command
        else {
            return Ok(Self {
                dotfile: None,
                state: None,
                _guards: (None, None),
            });
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

            Ok(Self {
                dotfile: Some(dotfile_tmp.path().to_path_buf()),
                state: Some(state_tmp.path().to_path_buf()),
                _guards: (Some(dotfile_tmp), Some(state_tmp)),
            })
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
            Ok(Self {
                dotfile: None,
                state: Some(test_state),
                _guards: (None, None),
            })
        }
    }
}

/// Detect whether we are running inside an AI coding agent.
///
/// LLM tools typically allocate a PTY so is_terminal() returns true,
/// but their verbose TUI output wastes tokens. We check for well-known
/// environment variables set by popular AI agents and the emerging
/// AI_AGENT standard (https://github.com/anthropics/claude-code/blob/main/AI_AGENT.md).
///
/// Set `DEVENV_NO_AI_AGENT=1` to opt out of detection (forces normal output/TUI
/// even when running under a detected agent).
fn is_ai_agent() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        if env::var_os("DEVENV_NO_AI_AGENT").is_some() {
            return false;
        }
        env::var_os("CLAUDECODE").is_some()
            || env::var_os("OPENCODE_CLIENT").is_some()
            || env::var_os("AI_AGENT").is_some()
    })
}

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

/// Resolve CLI + config files + environment into UI and backend options.
fn resolve(cli: Cli, shutdown: Arc<Shutdown>) -> Result<(UiOptions, BackendOptions)> {
    let command = cli.command;

    // UI options: verbosity, log level, tracing, TUI. Pure CLI + env, no config.
    let verbosity = resolve_verbosity(&cli.cli_options);
    let quiet = matches!(verbosity, VerbosityLevel::Quiet);
    let log_level = match verbosity {
        VerbosityLevel::Verbose => devenv_tracing::Level::Debug,
        VerbosityLevel::Quiet => devenv_tracing::Level::Warn,
        VerbosityLevel::Normal => devenv_tracing::Level::default(),
    };
    // `resolve()` folds legacy `--trace-output` into `tracing_specs`,
    // so a single walk covers env, --trace-to, and --trace-output.
    let tracing_specs = cli.tracing_args.resolve().into_diagnostic()?;
    let tracing_owns_terminal = tracing_specs.iter().any(|s| s.targets_terminal());

    // Explicit --tui/--no-tui wins, otherwise default to TUI when running
    // interactively outside CI and outside AI agents.
    let tui_requested = devenv_core::settings::flag(cli.cli_options.tui, cli.cli_options.no_tui)
        .unwrap_or_else(|| {
            let is_ci = env::var_os("CI").is_some();
            let is_tty = io::stdin().is_terminal() && io::stderr().is_terminal();
            is_tty && !is_ci && !is_ai_agent()
        });
    // Some commands don't support the TUI regardless of user options.
    let tui_unsupported = matches!(
        &command,
        Commands::Mcp { http: None } // stdio mode needs stderr for output
                | Commands::Lsp { .. } // LSP needs direct stdout for protocol/config output
                | Commands::PrintPaths // print output directly, no TUI needed
    );
    let tui = tui_requested && !tui_unsupported && !tracing_owns_terminal && !quiet;

    let ui = UiOptions {
        tui,
        tracing_owns_terminal,
        log_level,
        tracing_specs,
        verbosity,
    };

    // Backend options. Read before the `From` conversions consume `cli.nix_args`.
    let nix_debugger = cli.nix_args.nix_debugger;

    let mut config = Config::load()?;
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

    // If --from is provided, create a new input and add it to imports.
    let from_external = cli.from.is_some();
    if let Some(from) = &cli.from {
        let url = if let Some(path_str) = from.strip_prefix("path:") {
            let path = Path::new(path_str);
            let full_path = if path.is_relative() {
                env::current_dir().unwrap_or_default().join(path)
            } else {
                path.to_path_buf()
            };
            let abs_path = fs::canonicalize(&full_path).unwrap_or(full_path);
            format!("path:{}", abs_path.display())
        } else {
            from.clone()
        };

        let from_input = devenv_core::config::Input {
            url: Some(url),
            flake: true,
            follows: None,
            inputs: BTreeMap::new(),
            overlays: Vec::new(),
        };
        config.inputs.insert("from".to_string(), from_input);
        config.imports.push("from".to_string());
    }

    // Resolve settings from CLI + Config (pure functions, no mutation).
    let mut nix_settings =
        NixSettings::resolve(devenv_core::NixOptions::from(cli.nix_args), &config);
    if matches!(command, Commands::Update { .. }) {
        nix_settings.refresh_fetchers = true;
    }
    let shell_settings =
        ShellSettings::resolve(devenv_core::ShellOptions::from(cli.shell_args), &config);
    let cache_settings = CacheSettings::resolve(devenv_core::CacheOptions::from(cli.cache_args));
    let secret_settings =
        SecretSettings::resolve(devenv_core::SecretOptions::from(cli.secret_args), &config);
    let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);

    // Per-command backend flags.
    let is_testing = matches!(&command, Commands::Test { .. });
    let test_dirs = TestDirs::setup(&command)?;
    let use_pty = shell_settings.reload
        && matches!(&command, Commands::Shell { cmd: None, .. })
        && io::stdin().is_terminal()
        && io::stdout().is_terminal();

    // Read off `config` before its fields are moved into `DevenvOptions`.
    let strict_ports = config.strict_ports.unwrap_or(false);
    let require_version_match = config.requires_version_match();

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
        from_external,
        require_version_match,
        devenv_root: None,
        devenv_dotfile: test_dirs.dotfile.clone(),
        devenv_state: test_dirs.state.clone(),
        shutdown,
        is_testing,
    };

    let backend = BackendOptions {
        devenv: devenv_options,
        command,
        verbosity,
        use_pty,
        nix_debugger,
        strict_ports,
        test_dirs,
    };

    Ok((ui, backend))
}

/// The main-thread activity sink for a run.
///
/// Three mutually exclusive variants. `None` means tracing owns the terminal —
/// `send_activity_event` then falls through to `tracing::trace!` only, which is
/// exactly what `--trace-to <terminal>` wants.
enum Renderer {
    Tui(tokio::sync::mpsc::UnboundedReceiver<devenv_activity::ActivityEvent>),
    Console(tokio::sync::mpsc::UnboundedReceiver<devenv_activity::ActivityEvent>),
    None,
}

impl Renderer {
    /// Pick the renderer from the resolved UI options and install its activity
    /// sink. The returned guard clears the sink on drop and must outlive the
    /// backend (it produces the events).
    fn init(ui: &UiOptions) -> (Self, Option<ActivityGuard>) {
        if ui.tui {
            let (rx, handle) = devenv_activity::init();
            (Renderer::Tui(rx), Some(handle.install()))
        } else if !ui.tracing_owns_terminal {
            let (rx, handle) = devenv_activity::init();
            (Renderer::Console(rx), Some(handle.install()))
        } else {
            (Renderer::None, None)
        }
    }

    /// Drive the renderer to completion on the main thread.
    ///
    /// Owns its own current-thread runtime. Returns the TUI render height (0
    /// when there is no TUI) so the backend can position its cursor. Only the
    /// TUI consumes `command_tx` (process commands from the UI); the other
    /// sinks drop it so the backend's receiver closes.
    fn drive(
        self,
        shutdown: &Arc<Shutdown>,
        backend_done_rx: tokio::sync::oneshot::Receiver<()>,
        command_tx: tokio::sync::mpsc::Sender<ProcessCommand>,
        verbosity: VerbosityLevel,
    ) -> Result<u16> {
        match self {
            Renderer::Tui(activity_rx) => {
                let filter_level = if matches!(verbosity, VerbosityLevel::Verbose) {
                    ActivityLevel::Debug
                } else {
                    ActivityLevel::Info
                };
                current_thread_runtime("TUI")?.block_on(async {
                    Ok(devenv_tui::TuiApp::new(activity_rx, shutdown.clone())
                        .with_command_sender(command_tx)
                        .filter_level(filter_level)
                        .run(backend_done_rx)
                        .await
                        .unwrap_or(0))
                })
            }
            Renderer::Console(activity_rx) => {
                drop(command_tx);
                current_thread_runtime("console")?.block_on(async {
                    devenv::console::ConsoleOutput::new(activity_rx, verbosity)
                        .run(backend_done_rx)
                        .await;
                });
                Ok(0)
            }
            Renderer::None => {
                drop(command_tx);
                drop(backend_done_rx);
                Ok(0)
            }
        }
    }
}

/// Coordination channels between the main-thread renderer and the backend
/// thread, split into the half each side owns. Pairing tx/rx through one
/// constructor makes mismatched wiring unrepresentable.
///
/// - `backend_done`: backend signals the renderer to stop. Sending — or
///   dropping the sender — is the signal; a closed channel is a delivered
///   "stop", so the panic/early-return path is safe with no guard.
/// - `terminal_ready`: renderer tells the backend the terminal is free, with
///   the TUI render height for cursor positioning.
/// - `command`: process commands (restart, etc.) from the TUI to the backend.
struct RenderSide {
    backend_done_rx: tokio::sync::oneshot::Receiver<()>,
    terminal_ready_tx: tokio::sync::oneshot::Sender<u16>,
    command_tx: tokio::sync::mpsc::Sender<ProcessCommand>,
}

struct BackendSide {
    backend_done_tx: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: tokio::sync::oneshot::Receiver<u16>,
    command_rx: tokio::sync::mpsc::Receiver<ProcessCommand>,
}

fn handoff() -> (RenderSide, BackendSide) {
    let (backend_done_tx, backend_done_rx) = tokio::sync::oneshot::channel();
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(16);
    let (terminal_ready_tx, terminal_ready_rx) = tokio::sync::oneshot::channel();
    (
        RenderSide {
            backend_done_rx,
            terminal_ready_tx,
            command_tx,
        },
        BackendSide {
            backend_done_tx,
            terminal_ready_rx,
            command_rx,
        },
    )
}

/// Single entry point for all command execution.
///
/// Both TUI and direct modes share the same structure:
/// 1. Common setup (activity, tracing, shutdown, channels)
/// 2. Backend runs on a dedicated GC-registered thread
/// 3. TUI runs on the main thread (if enabled), otherwise we just wait
fn run(ui: UiOptions, backend: BackendOptions, shutdown: Arc<Shutdown>) -> Result<()> {
    let (renderer, _activity_guard) = Renderer::init(&ui);

    let _tracing_guard = devenv_tracing::init_tracing(ui.log_level, &ui.tracing_specs);

    let tui = ui.tui;
    let verbosity = ui.verbosity;

    // TUI terminal setup: save state before raw mode, install restore hooks
    // for panics and force-exit (second Ctrl+C)
    if tui {
        devenv_tui::app::save_terminal_state();

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            devenv_tui::app::restore_terminal();
            prev_hook(info);
        }));

        shutdown.set_pre_exit_hook(devenv_tui::app::restore_terminal);
    }

    let (render_side, backend_side) = handoff();
    let RenderSide {
        backend_done_rx,
        terminal_ready_tx,
        command_tx,
    } = render_side;

    // Backend on dedicated thread (own runtime with GC-registered workers)
    let shutdown_clone = shutdown.clone();
    let devenv_thread = std::thread::Builder::new()
        .name("devenv".into())
        .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .spawn(move || {
            build_gc_runtime().block_on(async {
                shutdown_clone.install_signals().await;

                let output = run_backend(backend, shutdown_clone.clone(), backend_side).await;

                // Fallback for paths that didn't run cleanup themselves
                // (PTY shell, REPL). No-op when run_backend already did it.
                shutdown_clone.shutdown_and_wait().await;

                output
            })
        })
        .into_diagnostic()
        .wrap_err("Failed to spawn devenv thread")?;

    let tui_render_height = renderer.drive(&shutdown, backend_done_rx, command_tx, verbosity)?;

    // Signal backend that terminal is available (with TUI render height for cursor positioning)
    let _ = terminal_ready_tx.send(tui_render_height);

    // Wait for backend thread
    let backend_result = devenv_thread
        .join()
        .map_err(|e| miette::miette!("devenv thread panicked: {}", panic_message(e)))?;

    // Flush tracing before exec() — CommandResult::Exec replaces the
    // process via exec(), so destructors after that point never run.
    drop(_tracing_guard);

    match backend_result {
        Ok(CommandResult::Debugger(devenv, err)) => launch_debugger(*devenv, err),
        Ok(cmd_result) => cmd_result.exec(),
        Err(err) => Err(err),
    }
}

/// Print the error and launch the Nix debugger REPL on a fresh GC-registered thread.
fn launch_debugger(devenv: devenv::Devenv, err: miette::Report) -> Result<()> {
    eprintln!("{err:?}");
    let handle = std::thread::Builder::new()
        .name("repl".into())
        .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .spawn(move || {
            // Skip prepare_repl() — the debugger already has eval context from
            // the failed command, and re-evaluating would likely fail again,
            // preventing debugger_is_pending() from being checked in launch_repl().
            build_gc_runtime().block_on(async { devenv.launch_repl().await })
        })
        .into_diagnostic()
        .wrap_err("Failed to spawn REPL thread")?;
    handle
        .join()
        .map_err(|e| miette::miette!("REPL thread panicked: {}", panic_message(e)))?
}

/// Run the backend: construct Devenv and dispatch the command.
#[instrument(name = "devenv", skip_all)]
async fn run_backend(
    backend: BackendOptions,
    shutdown: Arc<Shutdown>,
    side: BackendSide,
) -> Result<CommandResult> {
    let BackendSide {
        backend_done_tx,
        terminal_ready_rx,
        command_rx,
    } = side;
    let command_rx = Some(command_rx);

    let BackendOptions {
        devenv: devenv_options,
        command,
        verbosity,
        use_pty,
        nix_debugger,
        strict_ports: config_strict_ports,
        // Held until the backend run completes; `Drop` removes the temp dirs.
        test_dirs: _test_dirs,
    } = backend;

    // `backend_done_tx` is the renderer's stop signal: send at the right
    // point, or — on early return / panic — its drop closes the channel,
    // which the renderer also treats as "stop". No guard needed.
    let devenv = Devenv::new(devenv_options).await?;

    // PTY shell hands Devenv off to an owner task; we reclaim it after the session.
    if use_pty && let Commands::Shell { cmd, args } = command {
        // Pre-compute shell environment while we still own Devenv directly.
        // This must happen while TUI is active since get_dev_environment has #[activity].
        let dotfile = devenv.dotfile().to_path_buf();
        let initial_env_script = devenv.print_dev_env(false).await?;
        let bash_path = devenv.get_bash_path().await?;
        let clean = devenv.shell_settings.clean.clone();
        let shell = devenv.shell_settings.shell.clone();
        let (task_exports, task_messages) = devenv.run_enter_shell_tasks(None, verbosity).await?;

        let (client, owner_handle) = devenv::reload::spawn_owner(devenv);
        let result = run_reload_shell(ReloadShellArgs {
            devenv: client,
            cmd,
            args,
            backend_done: backend_done_tx,
            terminal_ready_rx,
            initial_env_script,
            bash_path,
            clean,
            shell,
            dotfile,
            task_exports,
            task_messages,
        })
        .await
        .map(|exit_code| match exit_code {
            Some(code) => CommandResult::ExitCode(code as i32),
            None => CommandResult::Done,
        });
        let devenv = tokio::task::block_in_place(|| owner_handle.join())
            .map_err(|e| miette::miette!("Devenv owner thread panicked: {}", panic_message(e)))?;
        return debugger_or_err(result, nix_debugger, devenv);
    }

    // REPL: run assembly with TUI active, then hand off terminal for interactive REPL
    if let Commands::Repl {} = command {
        let result = run_repl(&devenv, backend_done_tx, terminal_ready_rx).await;
        return debugger_or_err(result, nix_debugger, devenv);
    }

    // All other commands
    let result =
        dispatch_command(&devenv, command, verbosity, command_rx, config_strict_ports).await;

    // Drain cleanup (e.g. cachix push finalization) while the TUI is
    // still rendering, so its activity stays visible to the user.
    shutdown.shutdown_and_wait().await;

    // Signal the renderer to stop, after the drain, so its activity stayed
    // visible. Done before the debugger check so the TUI releases the
    // terminal before the debugger takes it.
    let _ = backend_done_tx.send(());

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
            CommandResult::Debugger(..) => {
                unreachable!("Debugger is handled in run() before exec()")
            }
        }
    }
}

/// Start processes and map the run mode to a command result.
async fn run_up(
    devenv: &Devenv,
    processes: Vec<String>,
    mode: devenv::tasks::RunMode,
    options: devenv::ProcessOptions,
    verbosity: VerbosityLevel,
) -> Result<CommandResult> {
    match devenv.up(processes, mode, options, verbosity).await? {
        RunMode::Detached => Ok(CommandResult::Done),
        RunMode::Foreground(shell_command) => Ok(CommandResult::Exec(shell_command.command)),
    }
}

/// Resolve `UpArgs` into `ProcessOptions` and start processes.
async fn run_up_args(
    devenv: &Devenv,
    up_args: devenv::cli::UpArgs,
    config_strict_ports: bool,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
    verbosity: VerbosityLevel,
) -> Result<CommandResult> {
    let strict_ports = devenv_core::settings::flag(up_args.strict_ports, up_args.no_strict_ports)
        .unwrap_or(config_strict_ports);
    let options = devenv::ProcessOptions {
        detach: up_args.detach,
        log_to_file: up_args.detach,
        strict_ports,
        command_rx,
        daemon: up_args.detach,
    };
    run_up(devenv, up_args.processes, up_args.mode, options, verbosity).await
}

/// Dispatch a CLI command to the appropriate Devenv method.
#[instrument(skip_all)]
async fn dispatch_command(
    devenv: &Devenv,
    command: Commands,
    verbosity: VerbosityLevel,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
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
            let output = devenv.info().await?;
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
            run_up_args(devenv, up_args, config_strict_ports, command_rx, verbosity).await
        }
        Commands::Processes { command } => match command {
            ProcessesCommand::Up { up_args } => {
                run_up_args(devenv, up_args, config_strict_ports, command_rx, verbosity).await
            }
            ProcessesCommand::Start { name: None, detach } => {
                let options = devenv::ProcessOptions {
                    detach,
                    log_to_file: detach,
                    strict_ports: config_strict_ports,
                    command_rx,
                    daemon: detach,
                };
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
                devenv.processes_start(&name).await?;
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
            TasksCommand::List {} => {
                let output = devenv.tasks_list().await?;
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
            let mut output = devenv.print_dev_env(false).await?;
            // Discard messages: direnv captures stdout as env var definitions,
            // so echo statements would corrupt the output.
            let task_exports = match devenv.run_enter_shell_tasks(None, verbosity).await {
                Ok((exports, _messages)) => exports,
                Err(e) => {
                    tracing::warn!("enterShell tasks failed, skipping exports: {e}");
                    BTreeMap::new()
                }
            };
            output.push_str(&devenv::format_shell_exports(&task_exports));
            Ok(CommandResult::Print(output))
        }
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
            Ok(CommandResult::Done)
        }
        Commands::GenerateYamlOptionsDoc => {
            config::write_yaml_options_doc()
                .await
                .wrap_err("Failed to generate yaml-options doc")?;
            Ok(CommandResult::Done)
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
            let mcp_options = devenv::DevenvOptions {
                inputs: devenv.inputs.clone(),
                imports: devenv.imports.clone(),
                git_root: devenv.git_root.clone(),
                nixpkgs_config: devenv.nixpkgs_config.clone(),
                ..Default::default()
            };
            devenv::mcp::run_mcp_server(mcp_options, http.map(|p| p.unwrap_or(8080))).await?;
            Ok(CommandResult::Done)
        }
        Commands::Lsp { print_config } => {
            devenv::lsp::run(devenv, print_config).await?;
            Ok(CommandResult::Done)
        }
        Commands::Direnvrc
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
    backend_done: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: tokio::sync::oneshot::Receiver<u16>,
    initial_env_script: String,
    bash_path: String,
    clean: devenv_core::config::Clean,
    shell: String,
    dotfile: std::path::PathBuf,
    task_exports: BTreeMap<String, String>,
    task_messages: Vec<String>,
}

/// Run shell with hot-reload.
///
/// `ShellCoordinator` handles file watching and build coordination;
/// `ShellSession` owns the PTY and terminal I/O. enterShell tasks have
/// already been executed by the caller (so they can run in parallel via the
/// DAG task system before the PTY starts).
///
/// Terminal handoff:
/// - `backend_done`: signals the renderer to stop (sent by `ShellSession`
///   after the initial build, or its drop on error — both mean stop).
/// - `terminal_ready_rx`: waits for the renderer to release the terminal
///   before `ShellSession` takes it.
async fn run_reload_shell(args: ReloadShellArgs) -> Result<Option<u32>> {
    let ReloadShellArgs {
        devenv,
        cmd,
        args,
        backend_done,
        terminal_ready_rx,
        initial_env_script,
        bash_path,
        clean,
        shell,
        dotfile,
        task_exports,
        task_messages,
    } = args;

    // Watch files come from the eval cache during the first build.
    let reload_config = ReloadConfig::new(vec![]);

    // Status line emits escape codes; disable when output is captured.
    let is_interactive = cmd.is_none();

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
    };

    let (command_tx, command_rx) = tokio::sync::mpsc::channel(16);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);

    let coordinator_handle = tokio::spawn(async move {
        ShellCoordinator::run(reload_config, builder, command_tx, event_rx).await
    });

    let handoff = Some(TuiHandoff {
        backend_done,
        terminal_ready_rx,
    });

    let shell_session = ShellSession::with_defaults().with_status_line(is_interactive);
    let exit_code = shell_session
        .run(command_rx, event_tx, handoff, SessionIo::default())
        .await
        .into_diagnostic()
        .wrap_err("Shell session error")?;

    let _ = coordinator_handle.await;

    Ok(exit_code)
}

/// Run the REPL with TUI handoff.
///
/// Performs assembly and Nix evaluation while TUI is active (showing progress),
/// then signals the TUI to release the terminal before launching the interactive REPL.
async fn run_repl(
    devenv: &Devenv,
    backend_done_tx: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: tokio::sync::oneshot::Receiver<u16>,
) -> Result<CommandResult> {
    // Phase 1: Assemble and evaluate with TUI active (shows progress)
    devenv.prepare_repl().await?;

    // Phase 2: signal the renderer to stop, then wait for terminal release
    let _ = backend_done_tx.send(());
    let _ = terminal_ready_rx.await;

    // Phase 3: Terminal is ours — launch the interactive REPL
    devenv.launch_repl().await?;

    Ok(CommandResult::Done)
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
fn prompt_secrets(provider: Option<String>, profile: Option<String>) -> Result<()> {
    let mut secrets = secretspec::Secrets::load()
        .into_diagnostic()
        .wrap_err("Failed to load secretspec")?;

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
