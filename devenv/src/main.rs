use clap::{CommandFactory, crate_version};
use clap_complete::CompleteEnv;
use devenv::{
    Devenv, RunMode,
    cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand},
    tracing as devenv_tracing,
};
use devenv_activity::ActivityLevel;
use devenv_core::config::{self, Config};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{process::Command, sync::Arc};
use tempfile::TempDir;
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
    /// Prompt for missing secrets after TUI cleanup
    PromptSecrets {
        provider: Option<String>,
        profile: Option<String>,
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
            let system = &cli.global_options.system;
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
    );

    if cli.global_options.use_tracing_mode() {
        run_with_tracing(cli)
    } else if force_legacy_cli || cli.global_options.use_legacy_cli() {
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
        cli.global_options.trace_format,
        cli.global_options.trace_output.as_ref(),
    );

    // Determine TUI filter level based on verbose flag
    let filter_level = if cli.global_options.verbose {
        ActivityLevel::Debug
    } else {
        ActivityLevel::Info
    };

    // Shutdown coordination
    // Signal handlers catch external signals (SIGINT from `kill`, SIGTERM, etc.)
    // TUI also handles Ctrl+C as keyboard event and sets last_signal manually
    let shutdown = Shutdown::new();
    shutdown.install_signals().await;

    // Channel to signal TUI when backend is fully done (including cleanup)
    let (backend_done_tx, backend_done_rx) = tokio::sync::oneshot::channel();

    // Devenv on background thread (own runtime with GC-registered workers)
    let shutdown_clone = shutdown.clone();
    let devenv_thread = std::thread::spawn(move || {
        build_gc_runtime().block_on(async {
            let result = tokio::select! {
                result = run_devenv(cli, shutdown_clone.clone()) => result,
                _ = shutdown_clone.wait_for_shutdown() => Ok(CommandResult::Done),
            };

            // Wait for cleanup to complete (e.g., Nix interrupt, cachix finalization)
            shutdown_clone.wait_for_shutdown_complete().await;

            // Signal TUI that backend is fully done
            let _ = backend_done_tx.send(());

            result
        })
    });

    // TUI on main thread (owns terminal)
    // Runs until backend signals completion, then drains remaining events
    let _ = devenv_tui::TuiApp::new(activity_rx, shutdown)
        .filter_level(filter_level)
        .run(backend_done_rx)
        .await;

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    let Ok(devenv_result) = devenv_thread.join() else {
        bail!("devenv thread panicked");
    };

    let result = match devenv_result {
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
    build_gc_runtime().block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let level = cli.get_log_level();
        devenv_tracing::init_cli_tracing(level, cli.global_options.trace_output.as_ref());

        let result = tokio::select! {
            result = run_devenv(cli, shutdown.clone()) => result,
            _ = shutdown.wait_for_shutdown() => Ok(CommandResult::Done),
        }?;

        result.exec()
    })
}

fn run_with_tracing(cli: Cli) -> Result<()> {
    build_gc_runtime().block_on(async {
        let shutdown = Shutdown::new();
        shutdown.install_signals().await;

        let level = cli.get_log_level();
        devenv_tracing::init_tracing(
            level,
            cli.global_options.trace_format,
            cli.global_options.trace_output.as_ref(),
        );

        let result = tokio::select! {
            result = run_devenv(cli, shutdown.clone()) => result,
            _ = shutdown.wait_for_shutdown() => Ok(CommandResult::Done),
        }?;

        result.exec()
    })
}

async fn run_devenv(cli: Cli, shutdown: Arc<Shutdown>) -> Result<CommandResult> {
    // Command is guaranteed to exist (Version/Direnvrc handled in main)
    let command = cli.command.expect("Command should exist");

    let mut config = Config::load()?;
    for input in cli.global_options.override_input.chunks_exact(2) {
        config
            .override_input_url(&input[0].clone(), &input[1].clone())
            .wrap_err_with(|| {
                format!(
                    "Failed to override input {} with URL {}",
                    &input[0], &input[1]
                )
            })?;
    }

    // If --from is provided, create a new input and add it to imports
    if let Some(ref from) = cli.global_options.from {
        // Convert to absolute path if it's a local filesystem path
        let url = if std::path::Path::new(from).exists() {
            // It's a local path - prefix with "path:" and make it absolute
            let abs_path =
                std::fs::canonicalize(from).unwrap_or_else(|_| std::path::PathBuf::from(from));
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

    let mut options = devenv::DevenvOptions {
        config,
        global_options: Some(cli.global_options),
        devenv_root: None,
        devenv_dotfile: None,
        shutdown: shutdown.clone(),
    };

    // we let Drop delete the dir after all commands have ran
    let _tmpdir = if let Commands::Test {
        dont_override_dotfile,
    } = command
    {
        let pwd = std::env::current_dir()
            .into_diagnostic()
            .wrap_err("Failed to get current directory")?;
        let tmpdir = TempDir::with_prefix_in(".devenv.", pwd)
            .into_diagnostic()
            .wrap_err("Failed to create temporary directory")?;
        if !dont_override_dotfile {
            let file_name = tmpdir
                .path()
                .file_name()
                .ok_or_else(|| miette::miette!("Temporary directory path is invalid"))?
                .to_str()
                .ok_or_else(|| {
                    miette::miette!("Temporary directory name contains invalid Unicode")
                })?;
            info!("Overriding .devenv to {}", file_name);
            options.devenv_dotfile = Some(tmpdir.path().to_path_buf());
        }
        Some(tmpdir)
    } else {
        None
    };

    let mut devenv = Devenv::new(options).await;

    let result = match command {
        Commands::Shell { cmd, ref args } => {
            let shell_config = match cmd {
                Some(cmd) => devenv.prepare_exec(Some(cmd), args).await?,
                None => devenv.shell().await?,
            };
            CommandResult::Exec(shell_config.command)
        }
        Commands::Test { .. } => {
            devenv.test().await?;
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
        Commands::Up { processes, detach }
        | Commands::Processes {
            command: ProcessesCommand::Up { processes, detach },
        } => {
            let options = devenv::ProcessOptions {
                detach,
                log_to_file: detach,
                ..Default::default()
            };
            match devenv.up(processes, &options).await? {
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
        Commands::Tasks { command } => match command {
            TasksCommand::Run {
                tasks,
                mode,
                show_output,
                input,
                input_json,
            } => {
                let output = devenv
                    .tasks_run(tasks, mode, show_output, input, input_json)
                    .await?;
                CommandResult::Print(format!("{output}\n"))
            }
            TasksCommand::List {} => {
                let output = devenv.tasks_list().await?;
                CommandResult::Print(format!("{output}\n"))
            }
        },
        Commands::Inputs { command } => match command {
            InputsCommand::Add { name, url, follows } => {
                devenv.inputs_add(&name, &url, &follows).await?;
                CommandResult::Done
            }
        },
        Commands::Changelogs {} => {
            devenv.changelogs().await?;
            CommandResult::Done
        }

        // hidden
        Commands::Assemble => {
            devenv.assemble(false).await?;
            CommandResult::Done
        }
        Commands::PrintDevEnv { json } => {
            let output = devenv.print_dev_env(json).await?;
            CommandResult::Print(output)
        }
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
            CommandResult::Done
        }
        Commands::Mcp { http } => {
            let config = devenv.config.read().await.clone();
            devenv::mcp::run_mcp_server(config, http.map(|p| p.unwrap_or(8080))).await?;
            CommandResult::Done
        }
        Commands::Lsp { print_config } => {
            devenv::lsp::run(&devenv, print_config).await?;
            CommandResult::Done
        }
        Commands::Direnvrc => unreachable!(),
        Commands::Version => unreachable!(),
    };

    Ok(result)
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
