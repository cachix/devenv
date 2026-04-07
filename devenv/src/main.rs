use clap::{CommandFactory, Parser, crate_version};
use clap_complete::CompleteEnv;
use devenv::{
    Devenv, RunMode,
    cli::{
        Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand,
        TraceFormat, TraceOutput,
    },
    processes::ProcessCommand,
    reload::DevenvShellBuilder,
    tracing as devenv_tracing,
};
use devenv_activity::ActivityLevel;
use devenv_core::{
    CacheSettings, InputOverrides, NixSettings, SecretSettings, ShellSettings,
    config::{self, Config, NixpkgsConfig},
};
use devenv_shell::dialect::ShellDialect;
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio_shutdown::Shutdown;
use tracing::info;

/// Stack size for threads that run Nix evaluation.
///
/// Nix evaluation can be deeply recursive (e.g. large nixpkgs traversals),
/// and the default 8MB thread stack is not always enough. Match the 64MB
/// stack that the Nix CLI itself uses.
const NIX_STACK_SIZE: usize = 64 * 1024 * 1024;

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

fn main() -> Result<()> {
    // Handle shell completion requests (COMPLETE=bash devenv)
    // Use "devenv" as completer so scripts work after installation (not absolute path)
    CompleteEnv::with_factory(Cli::command)
        .completer("devenv")
        .complete();

    // Re-run on a thread with a larger stack. Nix evaluation via FFI can be
    // deeply recursive (e.g. large nixpkgs traversals) and the default 8MB
    // main-thread stack is not always enough. The Nix CLI itself raises
    // RLIMIT_STACK to 64MB via nix::setStackSize() before evaluating; we
    // achieve the same by running on a dedicated thread.
    std::thread::Builder::new()
        .name("main".into())
        .stack_size(NIX_STACK_SIZE)
        .spawn(main_inner)
        .expect("Failed to spawn main thread")
        .join()
        .map_err(|e| miette::miette!("main thread panicked: {}", panic_message(e)))?
}

fn main_inner() -> Result<()> {
    // Retry loop: if the backend discovers secrets need interactive prompting,
    // we prompt the user and re-run the entire command with secrets now available.
    loop {
        let cli = Cli::parse();

        // Handle commands that don't need config or runtime
        match &cli.command {
            None | Some(Commands::Version) => {
                let version = crate_version!();
                let system = cli
                    .nix_args
                    .system
                    .clone()
                    .unwrap_or_else(devenv_core::settings::default_system);
                match build_rev() {
                    Some(rev) => println!("devenv {version}+{rev} ({system})"),
                    None => println!("devenv {version} ({system})"),
                }
                return Ok(());
            }
            Some(Commands::Direnvrc) => {
                print!("{}", *devenv::DIRENVRC);
                return Ok(());
            }
            Some(Commands::DaemonProcesses { config_file }) => {
                return run_daemon_processes(config_file.clone());
            }
            _ => {}
        }

        let launch = prepare_launch_config(cli)?;

        let result = run(launch);

        match result {
            Err(err) => match err.downcast::<devenv::SecretsNeedPrompting>() {
                Ok(secrets_err) => {
                    // Only prompt interactively when stdin is a terminal;
                    // in non-interactive contexts (e.g. direnv), fail with the error.
                    if !std::io::stdin().is_terminal() {
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

/// Everything resolved from CLI + config + environment, before any async runtime.
struct LaunchConfig {
    command: Commands,
    config: Config,
    nix_settings: NixSettings,
    shell_settings: ShellSettings,
    cache_settings: CacheSettings,
    secret_settings: SecretSettings,
    nixpkgs_config: NixpkgsConfig,
    input_overrides: InputOverrides,
    from_external: bool,
    verbosity: devenv::tasks::VerbosityLevel,
    tui: bool,
    use_pty: bool,
    nix_debugger: bool,
    is_testing: bool,
    needs_terminal_handoff: bool,
    log_level: devenv_tracing::Level,
    tracing_format: TraceFormat,
    tracing_output: Option<TraceOutput>,
}

/// Resolve all configuration from CLI + config files + environment.
/// This is a sync function that runs before any async runtime.
fn prepare_launch_config(mut cli: Cli) -> Result<LaunchConfig> {
    let command = cli.command.take().expect("Command should exist");

    // Extract values from CLI before consuming fields via From conversions
    let log_level = cli.get_log_level();
    let nix_debugger = cli.nix_args.nix_debugger;
    let verbosity = if cli.cli_options.quiet {
        devenv::tasks::VerbosityLevel::Quiet
    } else if cli.cli_options.verbose {
        devenv::tasks::VerbosityLevel::Verbose
    } else {
        devenv::tasks::VerbosityLevel::Normal
    };
    let use_tracing_mode = cli.tracing_args.use_tracing_mode();
    let tracing_format = cli.tracing_args.trace_format;
    let tracing_output = cli.tracing_args.trace_output;

    let mut config = Config::load()?;

    let input_overrides = InputOverrides::from(cli.input_overrides);

    for input in input_overrides.override_inputs.chunks_exact(2) {
        config
            .override_input_url(&input[0], &input[1])
            .wrap_err_with(|| {
                format!(
                    "Failed to override input {} with URL {}",
                    &input[0], &input[1]
                )
            })?;
    }

    // If --from is provided, create a new input and add it to imports
    let from_external = cli.from.is_some();
    if let Some(ref from) = cli.from {
        let url = if let Some(path_str) = from.strip_prefix("path:") {
            let path = std::path::Path::new(path_str);
            let full_path = if path.is_relative() {
                std::env::current_dir().unwrap_or_default().join(path)
            } else {
                path.to_path_buf()
            };
            let abs_path = std::fs::canonicalize(&full_path).unwrap_or(full_path);
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

    // Resolve TUI flag: explicit --tui/--no-tui wins, otherwise default
    // to TUI when running interactively outside CI.
    let tui_requested = devenv_core::settings::flag(cli.cli_options.tui, cli.cli_options.no_tui)
        .unwrap_or_else(|| {
            let is_ci = std::env::var_os("CI").is_some();
            let is_tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
            is_tty && !is_ci
        });

    // Some commands don't support the TUI regardless of user options
    let tui_unsupported = matches!(
        &command,
        Commands::Mcp { http: None } // stdio mode needs stderr for output
            | Commands::Lsp { .. } // LSP needs direct stdout for protocol/config output
            | Commands::PrintPaths // print output directly, no TUI needed
            | Commands::Init { .. } // interactive prompts (dialoguer) need direct terminal
    );

    let quiet = cli.cli_options.quiet;
    let tui = tui_requested && !tui_unsupported && !use_tracing_mode && !quiet;

    // Determine use_pty from resolved settings (single source of truth)
    let use_pty = shell_settings.reload
        && matches!(&command, Commands::Shell { cmd: None, .. })
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal();

    let is_testing = matches!(&command, Commands::Test { .. });

    // Commands that do eval with TUI active, then take over the terminal
    let needs_terminal_handoff = use_pty || matches!(&command, Commands::Repl {});

    Ok(LaunchConfig {
        command,
        config,
        nix_settings,
        shell_settings,
        cache_settings,
        secret_settings,
        nixpkgs_config,
        input_overrides,
        from_external,
        verbosity,
        tui,
        use_pty,
        nix_debugger,
        is_testing,
        needs_terminal_handoff,
        log_level,
        tracing_format,
        tracing_output,
    })
}

/// Single entry point for all command execution.
///
/// Both TUI and direct modes share the same structure:
/// 1. Common setup (activity, tracing, shutdown, channels)
/// 2. Backend runs on a dedicated GC-registered thread
/// 3. TUI runs on the main thread (if enabled), otherwise we just wait
fn run(launch: LaunchConfig) -> Result<()> {
    // Initialize activity channel (always — powers #[activity] macros)
    let (activity_rx, activity_handle) = devenv_activity::init();
    let _activity_guard = activity_handle.install();

    // CLI output: human-readable stderr when no TUI and not in tracing mode
    let cli_output = !launch.tui
        && !matches!(
            launch.tracing_output,
            Some(TraceOutput::Stdout) | Some(TraceOutput::Stderr)
        );
    devenv_tracing::init_tracing(
        launch.log_level,
        launch.tracing_format,
        launch.tracing_output.as_ref(),
        cli_output,
    );

    let tui = launch.tui;
    let needs_terminal_handoff = launch.needs_terminal_handoff;
    let verbosity = launch.verbosity;
    let is_process_view = matches!(
        launch.command,
        Commands::Up { .. }
            | Commands::Processes {
                command: ProcessesCommand::Up { .. },
            }
    );

    // Shutdown coordination (shared between main thread and backend thread)
    let shutdown = Shutdown::new();

    // TUI terminal setup: save state before raw mode, install restore hooks
    // for panics and force-exit (second Ctrl+C)
    if tui {
        devenv_tui::app::save_terminal_state();

        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            devenv_tui::app::restore_terminal();
            prev_hook(info);
        }));

        shutdown.set_pre_exit_hook(devenv_tui::app::restore_terminal);
    }

    // Channels for backend ↔ TUI coordination:
    // - backend_done: signals TUI when backend is fully done
    // - command: process commands (restart, etc.) from TUI to process manager
    // - terminal_ready: signals ShellSession when TUI has released the terminal
    let backend_done = Arc::new(tokio::sync::Notify::new());
    let (command_tx, command_rx) = tokio::sync::mpsc::channel::<ProcessCommand>(16);
    let (terminal_ready_tx, terminal_ready_rx) = tokio::sync::oneshot::channel::<u16>();

    // Backend on dedicated thread (own runtime with GC-registered workers)
    let shutdown_clone = shutdown.clone();
    let backend_done_clone = backend_done.clone();
    let devenv_thread = std::thread::Builder::new()
        .name("devenv".into())
        .stack_size(NIX_STACK_SIZE)
        .spawn(move || {
            build_gc_runtime().block_on(async {
                shutdown_clone.install_signals().await;

                let output = run_backend(
                    launch,
                    shutdown_clone.clone(),
                    backend_done_clone,
                    Some(terminal_ready_rx),
                    Some(command_rx),
                )
                .await;

                // Trigger shutdown to start cleanup (if not already triggered by signal)
                shutdown_clone.shutdown();

                // Wait for cleanup to complete (e.g., Nix interrupt, cachix finalization)
                shutdown_clone.wait_for_shutdown_complete().await;

                output
            })
        })
        .into_diagnostic()
        .wrap_err("Failed to spawn devenv thread")?;

    // TUI on main thread (if enabled), otherwise drop receiver to avoid buffering events
    let tui_render_height = if tui {
        let filter_level = if matches!(verbosity, devenv::tasks::VerbosityLevel::Verbose) {
            ActivityLevel::Debug
        } else {
            ActivityLevel::Info
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .into_diagnostic()
            .wrap_err("Failed to create TUI runtime")?;

        rt.block_on(async {
            let mut app = devenv_tui::TuiApp::new(activity_rx, shutdown.clone())
                .with_command_sender(command_tx)
                .filter_level(filter_level)
                // When a command needs terminal handoff, don't shut down on backend_done —
                // it's used as a handoff signal (eval done), not a completion signal
                .shutdown_on_backend_done(!needs_terminal_handoff);
            if is_process_view {
                app = app.with_mouse_capture();
            }
            app.run(backend_done.clone()).await.unwrap_or(0)
        })
    } else {
        drop(activity_rx);
        0
    };

    // Signal backend that terminal is available (with TUI render height for cursor positioning)
    let _ = terminal_ready_tx.send(tui_render_height);

    // Wait for backend thread
    let devenv_output = devenv_thread
        .join()
        .map_err(|e| miette::miette!("devenv thread panicked: {}", panic_message(e)))?;

    devenv_output.finish()
}

/// Output from run_backend containing the command result.
struct DevenvOutput {
    result: Result<CommandResult>,
    /// Devenv instance for debugger mode - kept alive when nix_debugger is enabled and error occurs
    devenv_for_debugger: Option<devenv::Devenv>,
}

/// Result of attempting to launch the debugger.
enum DebuggerResult {
    /// Debugger was launched and returned this result
    Launched(Result<()>),
    /// Debugger was not launched, proceed with normal command result
    NotLaunched(Result<CommandResult>),
}

impl DevenvOutput {
    /// If debugger mode is enabled and we have a devenv instance, launch the REPL.
    fn try_launch_debugger(self) -> DebuggerResult {
        if let Some(devenv) = self.devenv_for_debugger {
            // Print the error first so user knows what went wrong
            if let Err(ref err) = self.result {
                eprintln!("{:?}", err);
            }
            // Run the REPL on a new thread with its own GC-registered runtime
            let repl_result = std::thread::Builder::new()
                .name("repl".into())
                .stack_size(NIX_STACK_SIZE)
                .spawn(move || {
                    // Skip prepare_repl() — the debugger already has eval context from
                    // the failed command, and re-evaluating would likely fail again,
                    // preventing debugger_is_pending() from being checked in launch_repl().
                    build_gc_runtime().block_on(async { devenv.launch_repl().await })
                })
                .map_err(|_| miette::miette!("Failed to spawn REPL thread"))
                .and_then(|handle| {
                    handle
                        .join()
                        .map_err(|e| miette::miette!("REPL thread panicked: {}", panic_message(e)))
                        .and_then(|r| r)
                });
            DebuggerResult::Launched(repl_result)
        } else {
            DebuggerResult::NotLaunched(self.result)
        }
    }

    /// Handle debugger launch and execute the command result.
    fn finish(self) -> Result<()> {
        match self.try_launch_debugger() {
            DebuggerResult::Launched(result) => result,
            DebuggerResult::NotLaunched(result) => result?.exec(),
        }
    }
}

/// Guard that ensures `backend_done.notify_one()` is called when the backend
/// exits, even on early returns or panics. Without this, the TUI hangs forever
/// waiting for a notification that never arrives.
struct BackendDoneGuard(Option<Arc<tokio::sync::Notify>>);

impl BackendDoneGuard {
    fn new(notify: Arc<tokio::sync::Notify>) -> Self {
        Self(Some(notify))
    }

    /// Take the inner Notify for passing to subsystems (e.g., PTY shell handoff).
    /// After this, the guard no longer notifies on drop.
    fn take(&mut self) -> Arc<tokio::sync::Notify> {
        self.0.take().expect("backend_done already taken")
    }
}

impl Drop for BackendDoneGuard {
    fn drop(&mut self) {
        if let Some(notify) = &self.0 {
            notify.notify_one();
        }
    }
}

/// Run the backend: construct Devenv and dispatch the command.
/// All config loading and settings resolution has already happened in prepare_launch_config.
async fn run_backend(
    launch: LaunchConfig,
    shutdown: Arc<Shutdown>,
    backend_done: Arc<tokio::sync::Notify>,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
) -> DevenvOutput {
    let LaunchConfig {
        command,
        config,
        nix_settings,
        shell_settings,
        cache_settings,
        secret_settings,
        nixpkgs_config,
        input_overrides,
        from_external,
        verbosity,
        tui,
        use_pty,
        nix_debugger,
        is_testing,
        // Consumed by run() before run_backend is called
        needs_terminal_handoff: _,
        log_level: _,
        tracing_format: _,
        tracing_output: _,
    } = launch;

    // Ensure TUI is notified when backend exits, even on early return or panic.
    let mut backend_done_guard = BackendDoneGuard::new(backend_done);

    // Helper to create output without debugger context
    let output = |result| DevenvOutput {
        result,
        devenv_for_debugger: None,
    };

    // Early-dispatch commands that only need Config (no Devenv construction required)
    if let Commands::Inputs { ref command } = command {
        match command {
            InputsCommand::Add { name, url, follows } => {
                let mut config = config;
                if let Err(e) = config.add_input(name, url, follows) {
                    return output(Err(e));
                }
                if let Err(e) = config.write().await {
                    return output(Err(e));
                }
                return output(Ok(CommandResult::Done));
            }
        }
    }

    let config_strict_ports = config.strict_ports.unwrap_or(false);

    let mut options = devenv::DevenvOptions {
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
        devenv_root: None,
        devenv_dotfile: None,
        devenv_state: None,
        shutdown: shutdown.clone(),
        is_testing,
    };

    // we let Drop delete the dirs after all commands have ran
    let (_tmpdir, _state_tmpdir) = match &command {
        Commands::Test {
            override_dotfile,
            dont_override_dotfile: _,
        } => {
            let dotfile_tmpdir = if *override_dotfile {
                let setup_test_tmpdir = || -> Result<TempDir> {
                    let pwd = std::env::current_dir()
                        .into_diagnostic()
                        .wrap_err("Failed to get current directory")?;
                    let tmpdir = TempDir::with_prefix_in(".devenv.", pwd)
                        .into_diagnostic()
                        .wrap_err("Failed to create temporary directory")?;
                    Ok(tmpdir)
                };
                let tmpdir = match setup_test_tmpdir() {
                    Ok(t) => t,
                    Err(e) => return output(Err(e)),
                };
                let file_name = tmpdir
                    .path()
                    .file_name()
                    .and_then(|f| f.to_str())
                    .ok_or_else(|| miette::miette!("Temporary directory path is invalid"));
                let file_name = match file_name {
                    Ok(f) => f,
                    Err(e) => return output(Err(e)),
                };
                info!("Overriding .devenv to {}", file_name);
                options.devenv_dotfile = Some(tmpdir.path().to_path_buf());
                Some(tmpdir)
            } else {
                None
            };

            // When using a temporary dotfile (--override-dotfile), also use a temporary state
            // directory for full isolation. Otherwise, use a stable test-state path so the
            // eval cache can be reused across test runs.
            let state_tmpdir = if *override_dotfile {
                let state_tmpdir = match TempDir::new()
                    .into_diagnostic()
                    .wrap_err("Failed to create temporary state directory")
                {
                    Ok(t) => t,
                    Err(e) => return output(Err(e)),
                };
                info!(
                    "Using temporary state directory: {}",
                    state_tmpdir.path().display()
                );
                options.devenv_state = Some(state_tmpdir.path().to_path_buf());
                Some(state_tmpdir)
            } else {
                // Stable test state path: isolates test services from shell state while
                // keeping the path consistent across runs so the eval cache is effective.
                let dotfile = options.devenv_dotfile.clone().unwrap_or_else(|| {
                    std::env::current_dir()
                        .expect("Failed to get current directory")
                        .join(".devenv")
                });
                let test_state = dotfile.join("test-state");
                info!("Using test state directory: {}", test_state.display());
                options.devenv_state = Some(test_state);
                None
            };

            (dotfile_tmpdir, state_tmpdir)
        }
        _ => (None, None),
    };

    let devenv = Devenv::new(options).await;

    // PTY shell needs shared ownership for the reload coordinator
    if use_pty && let Commands::Shell { cmd, args } = command {
        let devenv = Arc::new(Mutex::new(devenv));
        let result = run_reload_shell(
            devenv.clone(),
            cmd,
            args,
            backend_done_guard.take(),
            terminal_ready_rx,
            verbosity,
            tui,
        )
        .await
        .map(|exit_code| match exit_code {
            Some(code) => CommandResult::ExitCode(code as i32),
            None => CommandResult::Done,
        });
        return match result {
            Err(e) if nix_debugger => {
                // Recover owned Devenv for debugger REPL
                let devenv = Arc::try_unwrap(devenv)
                    .unwrap_or_else(|_| panic!("all Arc references to Devenv should be dropped"))
                    .into_inner();
                DevenvOutput {
                    result: Err(e),
                    devenv_for_debugger: Some(devenv),
                }
            }
            _ => output(result),
        };
    }

    // REPL: run assembly with TUI active, then hand off terminal for interactive REPL
    if let Commands::Repl {} = command {
        let result = run_repl(&devenv, &mut backend_done_guard, terminal_ready_rx).await;
        return match result {
            Err(e) if nix_debugger => DevenvOutput {
                result: Err(e),
                devenv_for_debugger: Some(devenv),
            },
            _ => output(result),
        };
    }

    // All other commands
    let result = dispatch_command(
        &devenv,
        command,
        verbosity,
        tui,
        command_rx,
        config_strict_ports,
    )
    .await;

    // Notify TUI before debugger check, so TUI shuts down before debugger takes the terminal.
    drop(backend_done_guard);

    // Debugger on error
    match result {
        Err(e) if nix_debugger => DevenvOutput {
            result: Err(e),
            devenv_for_debugger: Some(devenv),
        },
        _ => output(result),
    }
}

/// Result of a CLI command execution.
/// This is a CLI concern - the library returns domain types.
#[derive(Debug)]
enum CommandResult {
    /// Command completed normally
    Done,
    /// Print this string after UI cleanup
    Print(String),
    /// Exec into this command after cleanup (TUI shutdown, terminal restore)
    Exec(Command),
    /// Exit with a specific code (e.g., from shell exit)
    ExitCode(i32),
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
                use std::os::unix::process::CommandExt;
                let err = cmd.exec();
                miette::bail!("Failed to exec: {}", err);
            }
            CommandResult::ExitCode(code) => {
                std::process::exit(code);
            }
        }
    }
}

/// Dispatch a CLI command to the appropriate Devenv method.
async fn dispatch_command(
    devenv: &Devenv,
    command: Commands,
    verbosity: devenv::tasks::VerbosityLevel,
    tui: bool,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
    config_strict_ports: bool,
) -> Result<CommandResult> {
    match command {
        Commands::Shell { cmd, ref args } => {
            // Non-PTY shell path (PTY is handled as early return in run_backend)
            // Messages are injected into the shell script by prepare_shell() via self.task_messages.
            devenv.run_enter_shell_tasks(None, verbosity, tui).await?;

            let shell_config = match cmd {
                Some(cmd) => devenv.prepare_exec(Some(cmd), args).await?,
                None => devenv.shell().await?,
            };

            Ok(CommandResult::Exec(shell_config.command))
        }
        Commands::Test { .. } => {
            devenv.test(verbosity, tui).await?;
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
                    .container_copy(&name, &copy_args, registry.as_deref(), verbosity, tui)
                    .await?;
                Ok(CommandResult::Done)
            }
            ContainerCommand::Run { name, copy_args } => {
                let shell_config = devenv
                    .container_run(&name, &copy_args, verbosity, tui)
                    .await?;
                Ok(CommandResult::Exec(shell_config.command))
            }
        },
        Commands::Init { target } => {
            devenv.init(&target)?;
            Ok(CommandResult::Done)
        }
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
                "Done. Deleted {} store paths, freed {} MB.\n",
                paths_deleted, mb_freed
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
                .map_err(|e| miette::miette!("Failed to serialize JSON: {}", e))?;
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
        Commands::Up { up_args }
        | Commands::Processes {
            command: ProcessesCommand::Up { up_args },
        } => {
            let strict_ports =
                devenv_core::settings::flag(up_args.strict_ports, up_args.no_strict_ports)
                    .unwrap_or(config_strict_ports);
            let options = devenv::ProcessOptions {
                detach: up_args.detach,
                log_to_file: up_args.detach,
                strict_ports,
                command_rx,
                daemon: up_args.detach,
            };
            match devenv
                .up(up_args.processes, options, verbosity, tui)
                .await?
            {
                RunMode::Detached => Ok(CommandResult::Done),
                RunMode::Foreground(shell_command) => {
                    Ok(CommandResult::Exec(shell_command.command))
                }
            }
        }
        Commands::Processes {
            command: ProcessesCommand::Down {},
        } => {
            devenv.down().await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes {
            command: ProcessesCommand::Wait { timeout },
        } => {
            devenv.wait_for_ready(Duration::from_secs(timeout)).await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes {
            command: ProcessesCommand::List {},
        } => {
            let output = devenv.processes_list().await?;
            Ok(CommandResult::Print(output))
        }
        Commands::Processes {
            command: ProcessesCommand::Status { name },
        } => {
            let output = devenv.processes_status(&name).await?;
            Ok(CommandResult::Print(output))
        }
        Commands::Processes {
            command:
                ProcessesCommand::Logs {
                    name,
                    lines,
                    stdout,
                    stderr,
                },
        } => {
            let output = devenv.processes_logs(&name, lines, stdout, stderr).await?;
            Ok(CommandResult::Print(output))
        }
        Commands::Processes {
            command: ProcessesCommand::Restart { name },
        } => {
            devenv.processes_restart(&name).await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes {
            command: ProcessesCommand::Start {
                name: Some(name), ..
            },
        } => {
            devenv.processes_start(&name).await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes {
            command: ProcessesCommand::Start { name: None, detach },
        } => {
            let options = devenv::ProcessOptions {
                detach,
                log_to_file: detach,
                strict_ports: config_strict_ports,
                command_rx,
                daemon: detach,
            };
            match devenv.up(vec![], options, verbosity, tui).await? {
                RunMode::Detached => Ok(CommandResult::Done),
                RunMode::Foreground(shell_command) => {
                    Ok(CommandResult::Exec(shell_command.command))
                }
            }
        }
        Commands::Processes {
            command: ProcessesCommand::Stop { name: Some(name) },
        } => {
            devenv.processes_stop(&name).await?;
            Ok(CommandResult::Done)
        }
        Commands::Processes {
            command: ProcessesCommand::Stop { name: None },
        } => {
            devenv.down().await?;
            Ok(CommandResult::Done)
        }
        Commands::Tasks { command } => match command {
            TasksCommand::Run {
                tasks,
                mode,
                show_output,
                input,
                input_json,
            } => {
                let output = devenv
                    .tasks_run(tasks, mode, show_output, input, input_json, verbosity, tui)
                    .await?;
                Ok(CommandResult::Print(format!("{output}\n")))
            }
            TasksCommand::List {} => {
                let output = devenv.tasks_list().await?;
                Ok(CommandResult::Print(format!("{output}\n")))
            }
        },
        // inputs add is early-dispatched above before Devenv construction
        Commands::Inputs { .. } => unreachable!(),
        Commands::Changelogs {} => Ok(devenv
            .changelogs()
            .await?
            .map_or(CommandResult::Done, CommandResult::Print)),
        // hidden
        Commands::Assemble => {
            devenv.assemble().await?;
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
            let task_exports = match devenv.run_enter_shell_tasks(None, verbosity, tui).await {
                Ok((exports, _messages)) => exports,
                Err(e) => {
                    tracing::warn!("enterShell tasks failed, skipping exports: {e}");
                    BTreeMap::new()
                }
            };
            let dialect = devenv_shell::dialect::BashDialect;
            output.push_str(&dialect.format_task_exports(&task_exports));
            Ok(CommandResult::Print(output))
        }
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
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
        Commands::Direnvrc => unreachable!(),
        Commands::Version => unreachable!(),
        Commands::DaemonProcesses { .. } => unreachable!(),
    }
}

/// Run shell with hot-reload capability.
///
/// This function manages a shell session that automatically reloads
/// when configuration files change. Uses the inverted architecture where:
/// - ShellCoordinator handles file watching and build coordination
/// - ShellSession owns the PTY and handles terminal I/O
///
/// Tasks are executed before the PTY starts as subprocesses,
/// allowing parallel execution through the DAG task system.
///
/// Terminal handoff:
/// - `backend_done`: Signals TUI to exit (notified after initial build completes)
/// - `terminal_ready_rx`: Waits for TUI cleanup before ShellSession takes terminal (receives render height)
async fn run_reload_shell(
    devenv: Arc<Mutex<Devenv>>,
    cmd: Option<String>,
    args: Vec<String>,
    backend_done: Arc<tokio::sync::Notify>,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    verbosity: devenv::tasks::VerbosityLevel,
    tui: bool,
) -> Result<Option<u32>> {
    use devenv_reload::{Config as ReloadConfig, ShellCoordinator};
    use devenv_tui::{SessionIo, ShellSession, TuiHandoff};
    use tokio::sync::mpsc;

    // Guard ensures TUI is notified even if we return early from eval errors.
    let mut backend_done_guard = BackendDoneGuard::new(backend_done);

    let devenv_guard = devenv.lock().await;
    let dotfile = devenv_guard.dotfile().to_path_buf();

    // Pre-compute shell environment BEFORE starting coordinator.
    // This must happen while TUI is active since get_dev_environment has #[activity].
    let initial_env_script = devenv_guard.print_dev_env(false).await?;
    let bash_path = devenv_guard.get_bash_path().await?;
    let clean = devenv_guard.shell_settings.clean.clone();

    // Get eval cache info (after print_dev_env set it up)
    let eval_cache_pool = devenv_guard.eval_cache_pool().cloned();
    let shell_cache_key = devenv_guard.shell_cache_key();
    tracing::debug!(
        "Reload setup: eval_cache_pool={}, shell_cache_key={}",
        eval_cache_pool.is_some(),
        shell_cache_key.is_some()
    );

    // Run enterShell tasks with subprocess executor before spawning PTY.
    // Task exports and messages are stored in devenv.task_exports / task_messages
    // and injected into the bash script by prepare_shell().
    let (task_exports, task_messages) = devenv_guard
        .run_enter_shell_tasks(None, verbosity, tui)
        .await?;

    // Drop the lock before passing devenv to the builder
    drop(devenv_guard);

    // Create reload config - watch files will be populated from eval cache
    // during the first build by DevenvShellBuilder
    let reload_config = ReloadConfig::new(vec![]);

    // Disable status line for non-interactive commands to avoid escape codes in output
    let is_interactive = cmd.is_none();

    // Create the shell builder with pre-computed environment
    let handle = tokio::runtime::Handle::current();
    let builder = DevenvShellBuilder::new(
        handle,
        devenv,
        cmd,
        args,
        initial_env_script,
        bash_path,
        clean,
        dotfile,
        eval_cache_pool,
        shell_cache_key,
        task_exports,
        task_messages,
    );

    // Set up communication channels between coordinator and shell runner
    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(16);

    // Spawn coordinator in background task
    let coordinator_handle = tokio::spawn(async move {
        ShellCoordinator::run(reload_config, builder, command_tx, event_rx).await
    });

    // If no TUI, create a dummy channel that immediately signals ready with 0 height
    let terminal_ready_rx = terminal_ready_rx.unwrap_or_else(|| {
        let (tx, rx) = tokio::sync::oneshot::channel::<u16>();
        let _ = tx.send(0);
        rx
    });
    let handoff = Some(TuiHandoff {
        backend_done: backend_done_guard.take(),
        terminal_ready_rx,
    });

    // Run shell session on current thread (owns terminal)
    let shell_session = ShellSession::with_defaults().with_status_line(is_interactive);
    let exit_code = shell_session
        .run(command_rx, event_tx, handoff, SessionIo::default())
        .await
        .map_err(|e| miette::miette!("Shell session error: {}", e))?;

    // Wait for coordinator to finish
    let _ = coordinator_handle.await;

    Ok(exit_code)
}

/// Run the REPL with TUI handoff.
///
/// Performs assembly and Nix evaluation while TUI is active (showing progress),
/// then signals the TUI to release the terminal before launching the interactive REPL.
async fn run_repl(
    devenv: &Devenv,
    backend_done_guard: &mut BackendDoneGuard,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
) -> Result<CommandResult> {
    // Phase 1: Assemble and evaluate with TUI active (shows progress)
    devenv.prepare_repl().await?;

    // Phase 2: TUI handoff — signal TUI to exit and wait for terminal release
    let backend_done = backend_done_guard.take();
    backend_done.notify_one();

    if let Some(rx) = terminal_ready_rx {
        let _ = rx.await;
    }

    // Phase 3: Terminal is ours — launch the interactive REPL
    devenv.launch_repl().await?;

    Ok(CommandResult::Done)
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
        .thread_stack_size(NIX_STACK_SIZE)
        .on_thread_start(|| {
            let _ = devenv_nix_backend::gc_register_current_thread();
        })
        .build()
        .expect("Failed to create tokio runtime")
}

/// Prompt for missing secretspec secrets interactively.
fn prompt_secrets(provider: Option<String>, profile: Option<String>) -> Result<()> {
    let mut secrets = secretspec::Secrets::load()
        .map_err(|e| miette::miette!("Failed to load secretspec: {}", e))?;

    if let Some(ref p) = provider {
        secrets.set_provider(p);
    }
    if let Some(ref p) = profile {
        secrets.set_profile(p);
    }

    secrets
        .ensure_secrets(provider, profile, true)
        .map_err(|e| miette::miette!("Failed to set secrets: {}", e))?;

    Ok(())
}

/// Run the native process manager as a daemon.
///
/// This is invoked by `devenv up -d` via re-exec to avoid fork-safety issues
/// in multithreaded programs. The parent serializes the task config to a JSON
/// file and spawns this process with `setsid` for full detachment.
fn run_daemon_processes(config_file: std::path::PathBuf) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;

    runtime.block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let config_json = tokio::fs::read_to_string(&config_file)
            .await
            .into_diagnostic()
            .wrap_err("Failed to read daemon config")?;
        let config: devenv::tasks::Config = serde_json::from_str(&config_json).into_diagnostic()?;

        // Remove temp config file
        let _ = tokio::fs::remove_file(&config_file).await;

        let tasks_runner = devenv::tasks::Tasks::builder(
            config,
            devenv::tasks::VerbosityLevel::Normal,
            shutdown.clone(),
        )
        .build()
        .await
        .map_err(|e| miette::miette!("Failed to build task runner: {}", e))?;

        // Run the full task DAG (starts processes, waits for readiness probes)
        let phase = devenv_activity::Activity::operation("Running processes")
            .parent(None)
            .start();
        let _outputs = tasks_runner.run_with_parent_activity(Arc::new(phase)).await;

        // Write PID so `devenv processes down` can find us
        let pid_file = tasks_runner.process_manager().manager_pid_file();
        devenv::processes::write_pid(&pid_file, std::process::id())
            .await
            .map_err(|e| miette::miette!("Failed to write PID: {}", e))?;

        // Keep the daemon alive until SIGTERM/SIGINT
        let result = tasks_runner
            .process_manager()
            .run_foreground(shutdown.cancellation_token(), None)
            .await
            .map_err(|e| miette::miette!("Process manager error: {}", e));

        let _ = tokio::fs::remove_file(&pid_file).await;
        result
    })
}

/// Returns the git revision suffix for the version string.
///
/// VERGEN_GIT_SHA is set by build.rs:
/// - From vergen when building from a git checkout
/// - Parsed from DEVENV_GIT_REV for flake builds
/// - VERGEN_IDEMPOTENT_OUTPUT for tarball builds (nixpkgs)
fn build_rev() -> Option<String> {
    let sha = env!("VERGEN_GIT_SHA");
    if sha.is_empty() || sha == "VERGEN_IDEMPOTENT_OUTPUT" {
        return None;
    }
    if env!("VERGEN_GIT_DIRTY") == "true" {
        Some(format!("{sha}-dirty"))
    } else {
        Some(sha.to_string())
    }
}
