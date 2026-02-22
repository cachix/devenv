use clap::{CommandFactory, crate_version};
use clap_complete::CompleteEnv;
use devenv::{
    Devenv, RunMode,
    cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand},
    processes::ProcessCommand,
    reload::DevenvShellBuilder,
    tracing as devenv_tracing,
};
use devenv_activity::ActivityLevel;
use devenv_core::config::{self, Config};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{process::Command, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio_shutdown::Shutdown;
use tracing::info;

/// Create a tokio runtime with worker threads registered with Boehm GC.
///
/// Nix uses Boehm GC with parallel marking. During stop-the-world collection,
/// only registered threads are paused. This ensures all tokio worker threads
/// are properly registered to avoid race conditions.
fn build_gc_runtime() -> tokio::runtime::Runtime {
    devenv_nix_backend::nix_init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .on_thread_start(|| {
            let _ = devenv_nix_backend::gc_register_current_thread();
        })
        .build()
        .expect("Failed to create tokio runtime")
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
    /// Prompt for missing secrets after TUI cleanup
    PromptSecrets {
        provider: Option<String>,
        profile: Option<String>,
    },
}

/// Internal result from command dispatch.
/// Separates commands that complete within `run_devenv_inner` from those
/// that need ownership of `Devenv` (handled by `run_devenv`).
enum InnerResult {
    /// Command completed normally
    Done(CommandResult),
    /// Shell with reload needs owned Devenv — caller handles this
    ReloadShell {
        cmd: Option<String>,
        args: Vec<String>,
        backend_done_tx: tokio::sync::oneshot::Sender<()>,
        terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    },
}

impl CommandResult {
    /// Execute the pending action.
    /// - Done: returns Ok(())
    /// - Print: prints to stdout and returns Ok(())
    /// - Exec: replaces the current process (never returns on success)
    /// - PromptSecrets: prompts for missing secrets interactively
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
            CommandResult::PromptSecrets { provider, profile } => {
                // Load secretspec and prompt for missing secrets
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

                eprintln!("\nSecrets have been set. Please re-run your command.");
                Ok(())
            }
        }
    }
}

fn main() -> Result<()> {
    // Handle shell completion requests (COMPLETE=bash devenv)
    // Use "devenv" as completer so scripts work after installation (not absolute path)
    CompleteEnv::with_factory(Cli::command)
        .completer("devenv")
        .complete();

    let cli = Cli::parse_and_resolve_options();

    // Handle commands that don't need a runtime
    match &cli.command {
        None | Some(Commands::Version) => {
            let version = crate_version!();
            let system = &cli.nix_cli.system;
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
        _ => {}
    }

    // Determine which mode to run in:
    // - TUI mode: interactive terminal UI (default)
    // - Legacy CLI mode: spinners and progress indicators (--no-tui or --log-format cli)
    // - Tracing mode: when --trace-output is stdout/stderr (conflicts with TUI/CLI output)
    //
    // Some commands require specific modes regardless of user options:
    // - MCP stdio mode uses legacy CLI (stdout is JSON-RPC, progress goes to stderr)
    // - MCP HTTP mode can use TUI
    let force_legacy_cli = matches!(
        &cli.command,
        Some(Commands::Mcp { http: None }) // stdio mode needs legacy CLI (stderr output)
            | Some(Commands::Lsp { .. }) // LSP needs direct stdout for protocol/config output
            | Some(Commands::PrintPaths) // print output directly, no TUI needed
    );

    if cli.cli_options.use_tracing_mode() {
        run_with_tracing(cli)
    } else if force_legacy_cli || cli.cli_options.use_legacy_cli() {
        run_with_legacy_cli(cli)
    } else {
        run_with_tui(cli)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run_with_tui(cli: Cli) -> Result<()> {
    // Initialize activity channel and register it
    let (activity_rx, activity_handle) = devenv_activity::init();
    activity_handle.install();

    // Initialize tracing
    let level = cli.get_log_level();
    devenv_tracing::init_tracing(
        level,
        cli.cli_options.trace_format,
        cli.cli_options.trace_output.as_ref(),
    );

    // Determine TUI filter level based on verbose flag
    let filter_level = if cli.cli_options.verbose {
        ActivityLevel::Debug
    } else {
        ActivityLevel::Info
    };

    // Save terminal state before TUI enters raw mode, so we can restore it reliably
    devenv_tui::app::save_terminal_state();

    // Install panic hook to restore terminal state on panic.
    // Without this, a panic during TUI rendering leaves the terminal in raw mode.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        devenv_tui::app::restore_terminal();
        prev_hook(info);
    }));

    // In reload shell mode, backend_done is just a handoff signal; don't trigger global shutdown.
    let shutdown_on_backend_done =
        !matches!(&cli.command, Some(Commands::Shell { .. })) || !cli.shell_cli.reload;

    // Shutdown coordination
    // Signal handlers catch external signals (SIGINT from `kill`, SIGTERM, etc.)
    // TUI also handles Ctrl+C as keyboard event and sets last_signal manually
    let shutdown = Shutdown::new();

    // Restore terminal before force-exit (second Ctrl+C) to prevent
    // leaving the terminal in raw mode with echo disabled.
    shutdown.set_pre_exit_hook(devenv_tui::app::restore_terminal);

    shutdown.install_signals().await;

    // Channel to signal TUI when backend is fully done (including cleanup)
    let (backend_done_tx, backend_done_rx) = tokio::sync::oneshot::channel();

    // Channel for process commands (restart, etc.) from TUI to process manager
    let (command_tx, command_rx) = tokio::sync::mpsc::channel::<ProcessCommand>(16);

    // Channel for terminal handoff: signals ShellSession when TUI has released the terminal
    // Passes the TUI's final render height for cursor positioning
    let (terminal_ready_tx, terminal_ready_rx) = tokio::sync::oneshot::channel::<u16>();

    // Devenv on background thread (own runtime with GC-registered workers)
    let shutdown_clone = shutdown.clone();
    let devenv_thread = std::thread::spawn(move || {
        build_gc_runtime().block_on(async {
            // Don't race with shutdown - let run_devenv handle shutdown via cancellation token
            // This ensures process cleanup happens before the future is dropped
            let output = run_devenv(
                cli,
                shutdown_clone.clone(),
                backend_done_tx,
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
    });

    // TUI on main thread (owns terminal)
    // Runs until backend signals completion, then drains remaining events
    let tui_render_height = devenv_tui::TuiApp::new(activity_rx, shutdown.clone())
        .with_command_sender(command_tx)
        .filter_level(filter_level)
        .shutdown_on_backend_done(shutdown_on_backend_done)
        .run(backend_done_rx)
        .await
        .unwrap_or(0);

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    // Signal backend that terminal is now available for shell, passing render height
    let _ = terminal_ready_tx.send(tui_render_height);

    // Poll instead of blocking join() — a blocking join would stall the
    // single-threaded tokio event loop, preventing signal handlers from running.
    // With polling, a second Ctrl+C (real SIGINT) can be processed and force-exit.
    let devenv_output = loop {
        if devenv_thread.is_finished() {
            break devenv_thread.join();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    let Ok(devenv_output) = devenv_output else {
        bail!("devenv thread panicked");
    };

    let result = match devenv_output.try_launch_debugger() {
        DebuggerResult::Launched(result) => return result,
        DebuggerResult::NotLaunched(result) => result,
    };

    let result = match result {
        Ok(cmd_result) => cmd_result,
        Err(err) => {
            // Check if secrets need prompting (special case: TUI stopped for password entry)
            if let Some(secrets_err) = err.downcast_ref::<devenv::SecretsNeedPrompting>() {
                CommandResult::PromptSecrets {
                    provider: secrets_err.provider.clone(),
                    profile: secrets_err.profile.clone(),
                }
            } else {
                return Err(err);
            }
        }
    };

    // Execute any pending command (e.g., shell exec) now that TUI is cleaned up
    result.exec()
}

fn run_with_legacy_cli(cli: Cli) -> Result<()> {
    let devenv_output = build_gc_runtime().block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let level = cli.get_log_level();
        devenv_tracing::init_cli_tracing(level, cli.cli_options.trace_output.as_ref());

        // No TUI in legacy mode - create dummy channel (drop receiver immediately)
        let (backend_done_tx, _) = tokio::sync::oneshot::channel();

        // Don't race with shutdown - let run_devenv handle shutdown via cancellation token
        run_devenv(cli, shutdown.clone(), backend_done_tx, None, None).await
    });

    match devenv_output.try_launch_debugger() {
        DebuggerResult::Launched(result) => result,
        DebuggerResult::NotLaunched(result) => handle_secrets_or_exec(result),
    }
}

fn run_with_tracing(cli: Cli) -> Result<()> {
    let devenv_output = build_gc_runtime().block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let level = cli.get_log_level();
        devenv_tracing::init_tracing(
            level,
            cli.cli_options.trace_format,
            cli.cli_options.trace_output.as_ref(),
        );

        // No TUI in tracing mode - create dummy channel (drop receiver immediately)
        let (backend_done_tx, _) = tokio::sync::oneshot::channel();

        // Don't race with shutdown - let run_devenv handle shutdown via cancellation token
        run_devenv(cli, shutdown.clone(), backend_done_tx, None, None).await
    });

    match devenv_output.try_launch_debugger() {
        DebuggerResult::Launched(result) => result,
        DebuggerResult::NotLaunched(result) => handle_secrets_or_exec(result),
    }
}

/// Output from run_devenv containing the command result.
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
            let repl_result = std::thread::spawn(move || {
                build_gc_runtime().block_on(async { devenv.repl().await })
            })
            .join()
            .map_err(|_| miette::miette!("REPL thread panicked"))
            .and_then(|r| r);
            DebuggerResult::Launched(repl_result)
        } else {
            DebuggerResult::NotLaunched(self.result)
        }
    }
}

/// Handle SecretsNeedPrompting errors by prompting interactively, otherwise exec.
fn handle_secrets_or_exec(result: Result<CommandResult>) -> Result<()> {
    match result {
        Err(err) => {
            if let Some(secrets_err) = err.downcast_ref::<devenv::SecretsNeedPrompting>() {
                CommandResult::PromptSecrets {
                    provider: secrets_err.provider.clone(),
                    profile: secrets_err.profile.clone(),
                }
                .exec()
            } else {
                Err(err)
            }
        }
        Ok(cmd_result) => cmd_result.exec(),
    }
}

/// Setup devenv and run the command.
async fn run_devenv(
    cli: Cli,
    shutdown: Arc<Shutdown>,
    backend_done_tx: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
) -> DevenvOutput {
    // Command is guaranteed to exist (Version/Direnvrc handled in main)
    let command = cli.command.clone().expect("Command should exist");
    let nix_debugger = cli.nix_cli.nix_debugger;

    // Helper to create output without debugger context
    let output = |result| DevenvOutput {
        result,
        devenv_for_debugger: None,
    };

    let mut config = match Config::load() {
        Ok(c) => c,
        Err(e) => return output(Err(e)),
    };

    for input in cli.input_overrides.override_input.chunks_exact(2) {
        if let Err(e) = config
            .override_input_url(&input[0].clone(), &input[1].clone())
            .wrap_err_with(|| {
                format!(
                    "Failed to override input {} with URL {}",
                    &input[0], &input[1]
                )
            })
        {
            return output(Err(e));
        }
    }

    // Early-dispatch commands that only need Config (no Devenv construction required)
    if let Some(Commands::Inputs { command }) = &cli.command {
        match command {
            InputsCommand::Add { name, url, follows } => {
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

    // If --from is provided, create a new input and add it to imports
    let from_external = cli.from.is_some();
    if let Some(ref from) = cli.from {
        let url = if let Some(path_str) = from.strip_prefix("path:") {
            // Resolve relative paths to absolute and canonicalize
            let path = std::path::Path::new(path_str);
            let full_path = if path.is_relative() {
                std::env::current_dir().unwrap_or_default().join(path)
            } else {
                path.to_path_buf()
            };
            let abs_path = std::fs::canonicalize(&full_path).unwrap_or(full_path);
            format!("path:{}", abs_path.display())
        } else {
            // It's a flake input reference (e.g., "nixpkgs", "github:org/repo")
            from.clone()
        };

        let from_input = devenv_core::config::Input {
            url: Some(url),
            flake: true,
            follows: None,
            inputs: std::collections::BTreeMap::new(),
            overlays: Vec::new(),
        };
        config.inputs.insert("from".to_string(), from_input);
        config.imports.push("from".to_string());
    }

    // Resolve settings from CLI + Config (pure functions, no mutation).
    let nix_settings = devenv_core::NixSettings::resolve(cli.nix_cli, &config);
    let shell_settings = devenv_core::ShellSettings::resolve(cli.shell_cli, &config);
    let cache_settings = devenv_core::CacheSettings::resolve(cli.cache_cli);
    let secret_settings = devenv_core::SecretSettings::resolve(cli.secret_cli, &config);

    // Construct UI parameters from CLI options (kept out of the library)
    let verbosity = if cli.cli_options.quiet {
        devenv::tasks::VerbosityLevel::Quiet
    } else if cli.cli_options.verbose {
        devenv::tasks::VerbosityLevel::Verbose
    } else {
        devenv::tasks::VerbosityLevel::Normal
    };
    let tui = cli.cli_options.tui;

    let is_testing = matches!(&command, Commands::Test { .. });
    let mut options = devenv::DevenvOptions {
        config,
        nix_settings: Some(nix_settings),
        shell_settings: Some(shell_settings),
        cache_settings: Some(cache_settings),
        secret_settings: Some(secret_settings),
        input_overrides: cli.input_overrides,
        from_external,
        devenv_root: None,
        devenv_dotfile: None,
        shutdown: shutdown.clone(),
        is_testing,
    };

    // we let Drop delete the dir after all commands have ran
    let _tmpdir = match &command {
        Commands::Test {
            dont_override_dotfile,
        } => {
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
            if !dont_override_dotfile {
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
            }
            Some(tmpdir)
        }
        _ => None,
    };

    let devenv = Devenv::new(options).await;

    // Run the command
    let inner = run_devenv_inner(
        &devenv,
        command,
        backend_done_tx,
        terminal_ready_rx,
        command_rx,
        verbosity,
        tui,
    )
    .await;

    match inner {
        Ok(InnerResult::ReloadShell {
            cmd,
            args,
            backend_done_tx,
            terminal_ready_rx,
        }) => {
            // Reload shell consumes devenv by value — no second instance needed
            let result = run_reload_shell(
                devenv,
                cmd,
                args,
                backend_done_tx,
                terminal_ready_rx,
                verbosity,
                tui,
            )
            .await
            .map(|exit_code| match exit_code {
                Some(code) => CommandResult::ExitCode(code as i32),
                None => CommandResult::Done,
            });
            output(result)
        }
        Ok(InnerResult::Done(cmd_result)) => output(Ok(cmd_result)),
        Err(e) => {
            if nix_debugger {
                DevenvOutput {
                    result: Err(e),
                    devenv_for_debugger: Some(devenv),
                }
            } else {
                output(Err(e))
            }
        }
    }
}

/// Run the devenv command.
async fn run_devenv_inner(
    devenv: &Devenv,
    command: Commands,
    backend_done_tx: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    command_rx: Option<tokio::sync::mpsc::Receiver<ProcessCommand>>,
    verbosity: devenv::tasks::VerbosityLevel,
    tui: bool,
) -> Result<InnerResult> {
    // Wrap in Option so shell commands can consume it, others send at end
    let mut backend_done_tx = Some(backend_done_tx);

    let result = match command {
        Commands::Shell { cmd, ref args } => {
            if !devenv.shell_settings.reload {
                // Run enterShell tasks first (TUI shows progress).
                // Exports are stored on self so prepare_shell() injects them
                // into the bash script after the Nix shell env is applied.
                devenv.run_enter_shell_tasks(verbosity, tui).await?;

                // Signal TUI can exit now (tasks completed)
                if let Some(tx) = backend_done_tx.take() {
                    let _ = tx.send(());
                }

                // Prepare shell (tasks already ran via Rust, Nix checks cliVersion >= 2.0)
                let shell_config = match cmd {
                    Some(cmd) => devenv.prepare_exec(Some(cmd), args).await?,
                    None => devenv.shell().await?,
                };

                CommandResult::Exec(shell_config.command)
            } else {
                // Reload shell needs owned Devenv — return to caller
                return Ok(InnerResult::ReloadShell {
                    cmd,
                    args: args.clone(),
                    backend_done_tx: backend_done_tx
                        .take()
                        .expect("backend_done_tx should exist"),
                    terminal_ready_rx,
                });
            }
        }
        Commands::Test { .. } => {
            devenv.test(verbosity, tui).await?;
            CommandResult::Done
        }
        Commands::Container { command } => match command {
            ContainerCommand::Build { name } => {
                let path = devenv.container_build(&name).await?;
                CommandResult::Print(format!("{path}\n"))
            }
            ContainerCommand::Copy {
                name,
                copy_args,
                registry,
            } => {
                devenv
                    .container_copy(&name, &copy_args, registry.as_deref())
                    .await?;
                CommandResult::Done
            }
            ContainerCommand::Run { name, copy_args } => {
                let shell_config = devenv.container_run(&name, &copy_args).await?;
                CommandResult::Exec(shell_config.command)
            }
        },
        Commands::Init { target } => {
            devenv.init(&target)?;
            CommandResult::Done
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
            devenv.search(&name).await?;
            CommandResult::Done
        }
        Commands::Gc {} => {
            let (paths_deleted, bytes_freed) = devenv.gc().await?;
            let mb_freed = bytes_freed / (1024 * 1024);
            CommandResult::Print(format!(
                "Done. Deleted {} store paths, freed {} MB.\n",
                paths_deleted, mb_freed
            ))
        }
        Commands::Info {} => {
            let output = devenv.info().await?;
            CommandResult::Print(format!("{output}\n"))
        }
        Commands::Repl {} => {
            devenv.repl().await?;
            CommandResult::Done
        }
        Commands::Build { attributes } => {
            let results = devenv.build(&attributes).await?;
            let json_map: serde_json::Map<String, serde_json::Value> = results
                .into_iter()
                .map(|(attr, path)| (attr, serde_json::Value::String(path.display().to_string())))
                .collect();
            let json = serde_json::to_string_pretty(&json_map)
                .map_err(|e| miette::miette!("Failed to serialize JSON: {}", e))?;
            CommandResult::Print(format!("{json}\n"))
        }
        Commands::Eval { attributes } => {
            let json = devenv.eval(&attributes).await?;
            CommandResult::Print(format!("{json}\n"))
        }
        Commands::Update { name } => {
            devenv.update(&name).await?;
            CommandResult::Done
        }
        Commands::Up {
            processes,
            detach,
            strict_ports,
        }
        | Commands::Processes {
            command:
                ProcessesCommand::Up {
                    processes,
                    detach,
                    strict_ports,
                },
        } => {
            let options = devenv::ProcessOptions {
                envs: None,
                detach,
                log_to_file: detach,
                strict_ports,
                command_rx,
            };
            match devenv.up(processes, options, verbosity, tui).await? {
                RunMode::Detached => CommandResult::Done,
                RunMode::Foreground(shell_command) => CommandResult::Exec(shell_command.command),
            }
        }
        Commands::Processes {
            command: ProcessesCommand::Down {},
        } => {
            devenv.down().await?;
            CommandResult::Done
        }
        Commands::Processes {
            command: ProcessesCommand::Wait { timeout },
        } => {
            devenv.wait_for_ready(Duration::from_secs(timeout)).await?;
            CommandResult::Done
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
                CommandResult::Print(format!("{output}\n"))
            }
            TasksCommand::List {} => {
                let output = devenv.tasks_list().await?;
                CommandResult::Print(format!("{output}\n"))
            }
        },
        // inputs add is early-dispatched in run_devenv before Devenv construction
        Commands::Inputs { .. } => unreachable!(),
        Commands::Changelogs {} => {
            devenv.changelogs().await?;
            CommandResult::Done
        }
        // hidden
        Commands::Assemble => {
            devenv.assemble().await?;
            CommandResult::Done
        }
        Commands::PrintDevEnv { json } => {
            let output = devenv.print_dev_env(json).await?;
            CommandResult::Print(output)
        }
        Commands::DirenvExport => {
            let output = devenv.print_dev_env(false).await?;
            CommandResult::Print(output)
        }
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
            CommandResult::Done
        }
        Commands::PrintPaths => {
            let paths = devenv.paths();
            let output = format!(
                "DEVENV_DOTFILE=\"{}\"\nDEVENV_ROOT=\"{}\"\nDEVENV_GC=\"{}\"",
                paths.dotfile.display(),
                paths.root.display(),
                paths.dot_gc.display()
            );
            CommandResult::Print(output)
        }
        Commands::Mcp { http } => {
            devenv::mcp::run_mcp_server(devenv.config.clone(), http.map(|p| p.unwrap_or(8080)))
                .await?;
            CommandResult::Done
        }
        Commands::Lsp { print_config } => {
            devenv::lsp::run(devenv, print_config).await?;
            CommandResult::Done
        }
        Commands::Direnvrc => unreachable!(),
        Commands::Version => unreachable!(),
    };

    // Signal TUI that backend is done (if not already consumed by shell commands)
    if let Some(tx) = backend_done_tx {
        let _ = tx.send(());
    }

    Ok(InnerResult::Done(result))
}

/// Returns the git revision suffix for the version string.
///
/// Prefers the vergen-injected SHA (available when building from a git checkout),
/// falls back to DEVENV_GIT_REV (set by Nix builds where .git is unavailable).
fn build_rev() -> Option<String> {
    let sha = env!("VERGEN_GIT_SHA");
    // vergen emits "VERGEN_IDEMPOTENT_OUTPUT" when git is unavailable
    if !sha.is_empty() && sha != "VERGEN_IDEMPOTENT_OUTPUT" {
        let dirty = env!("VERGEN_GIT_DIRTY");
        if dirty == "true" {
            return Some(format!("{sha}-dirty"));
        }
        return Some(sha.to_string());
    }

    // Nix builds pass the flake's git rev via this env var
    option_env!("DEVENV_GIT_REV")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Run shell with hot-reload capability.
///
/// This function manages a shell session that automatically reloads
/// when configuration files change. Uses the inverted architecture where:
/// - ShellCoordinator handles file watching and build coordination
/// - ShellSession owns the PTY and handles terminal I/O
///
/// Tasks are executed inside the PTY via PtyExecutor, allowing them to
/// run in the same shell environment as the interactive session.
///
/// Terminal handoff:
/// - `backend_done_tx`: Signals TUI to exit (sent after initial build completes)
/// - `terminal_ready_rx`: Waits for TUI cleanup before ShellSession takes terminal (receives render height)
async fn run_reload_shell(
    devenv: Devenv,
    cmd: Option<String>,
    args: Vec<String>,
    backend_done_tx: tokio::sync::oneshot::Sender<()>,
    terminal_ready_rx: Option<tokio::sync::oneshot::Receiver<u16>>,
    verbosity: devenv::tasks::VerbosityLevel,
    tui: bool,
) -> Result<Option<u32>> {
    use devenv_reload::{Config as ReloadConfig, ShellCoordinator};
    use devenv_tasks::PtyExecutor;
    use devenv_tui::{PtyTaskRequest, SessionIo, ShellSession, TuiHandoff};
    use tokio::sync::mpsc;

    let dotfile = devenv.dotfile().to_path_buf();

    // Pre-compute shell environment BEFORE starting coordinator.
    // This must happen while TUI is active since get_dev_environment has #[activity].
    let initial_env_script = devenv.print_dev_env(false).await?;
    let bash_path = devenv.get_bash_path().await?;
    let clean = devenv.shell_settings.clean.clone();

    // Get eval cache info (after print_dev_env set it up)
    let eval_cache_pool = devenv.eval_cache_pool().cloned();
    let shell_cache_key = devenv.shell_cache_key();
    tracing::debug!(
        "Reload setup: eval_cache_pool={}, shell_cache_key={}",
        eval_cache_pool.is_some(),
        shell_cache_key.is_some()
    );

    // For command mode, run tasks with subprocess executor BEFORE spawning PTY.
    // The PTY will immediately exec the command and exit, so we can't use PTY tasks.
    // For interactive mode, tasks run inside the PTY via PtyExecutor.
    let use_pty_tasks = cmd.is_none();
    if !use_pty_tasks {
        // Run enterShell tasks with subprocess executor (like --no-reload mode)
        // Task exports are stored in devenv.task_exports and injected into the
        // shell script by prepare_shell().
        let _task_exports = devenv.run_enter_shell_tasks(verbosity, tui).await?;
    }

    // Create reload config - watch files will be populated from eval cache
    // during the first build by DevenvShellBuilder
    let reload_config = ReloadConfig::new(vec![]);

    // Wrap owned devenv for shared access by builder and task runner
    let devenv_arc = Arc::new(Mutex::new(devenv));

    // Clone devenv for task runner (needs its own reference)
    let devenv_for_tasks = devenv_arc.clone();

    // Disable status line for non-interactive commands to avoid escape codes in output
    let is_interactive = cmd.is_none();

    // Create the shell builder with pre-computed environment
    let handle = tokio::runtime::Handle::current();
    let builder = DevenvShellBuilder::new(
        handle,
        devenv_arc,
        cmd,
        args,
        initial_env_script,
        bash_path,
        clean,
        dotfile,
        eval_cache_pool,
        shell_cache_key,
    );

    // Set up communication channels between coordinator and shell runner
    let (command_tx, command_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(16);

    // Spawn coordinator in background task
    let coordinator_handle = tokio::spawn(async move {
        ShellCoordinator::run(reload_config, builder, command_tx, event_rx).await
    });

    // For interactive mode, run tasks inside the PTY via PtyExecutor
    // For command mode, tasks were already run above with subprocess executor
    let (task_rx, pty_ready_tx, task_handle) = if use_pty_tasks {
        // Create task channel for PTY-based task execution
        let (task_tx, task_rx) = mpsc::channel::<PtyTaskRequest>(16);

        // Create PTY ready signal - task runner waits for this before sending tasks
        let (pty_ready_tx, pty_ready_rx) = tokio::sync::oneshot::channel();

        // Spawn task runner on a separate thread with its own runtime
        // This is needed because devenv's async code has non-Send futures (due to Nix bindings)
        let task_handle = std::thread::spawn(move || {
            // Register with Boehm GC - required because the task runner calls
            // Nix FFI operations (assemble, capture_shell_environment, load_tasks)
            let _ = devenv_nix_backend::gc_register_current_thread();

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create task runner runtime");

            rt.block_on(async move {
                // Wait for PTY to be ready before sending tasks
                if pty_ready_rx.await.is_err() {
                    return Err(miette::miette!("PTY ready signal failed"));
                }

                let executor = Arc::new(PtyExecutor::new(task_tx));
                let devenv = devenv_for_tasks.lock().await;
                let result = devenv
                    .run_enter_shell_tasks_with_executor(Some(executor), None, verbosity, tui)
                    .await;
                drop(devenv);
                result
            })
        });

        (Some(task_rx), Some(pty_ready_tx), Some(task_handle))
    } else {
        (None, None, None)
    };

    // Create TUI handoff configuration
    // If no terminal_ready_rx (no TUI), create a dummy channel that immediately completes
    let handoff = if let Some(terminal_ready_rx) = terminal_ready_rx {
        Some(TuiHandoff {
            backend_done_tx,
            terminal_ready_rx,
            task_rx,
            pty_ready_tx,
        })
    } else {
        // No TUI - create dummy channel that completes immediately with 0 height
        let (dummy_tx, dummy_rx) = tokio::sync::oneshot::channel::<u16>();
        let _ = dummy_tx.send(0); // Immediately signal ready with no render height
        Some(TuiHandoff {
            backend_done_tx,
            terminal_ready_rx: dummy_rx,
            task_rx,
            pty_ready_tx,
        })
    };

    // Run shell session on current thread (owns terminal)
    let shell_session = ShellSession::with_defaults().with_status_line(is_interactive);
    let exit_code = shell_session
        .run(command_rx, event_tx, handoff, SessionIo::default())
        .await
        .map_err(|e| miette::miette!("Shell session error: {}", e))?;

    // Wait for task runner (if any) and coordinator to finish
    if let Some(handle) = task_handle
        && let Ok(Err(e)) = handle.join()
    {
        tracing::warn!("enterShell tasks failed: {}", e);
    }
    let _ = coordinator_handle.await;

    Ok(exit_code)
}
