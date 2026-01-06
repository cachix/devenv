use clap::crate_version;
use devenv::{
    Devenv, RunMode,
    cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand},
    tracing as devenv_tracing,
};
use devenv_activity::ActivityLevel;
use devenv_core::config::{self, Config};
use miette::{IntoDiagnostic, Result, WrapErr};
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
    let cli = Cli::parse_and_resolve_options();

    // Handle commands that don't need a runtime
    match &cli.command {
        None | Some(Commands::Version) => {
            println!(
                "devenv {} ({})",
                crate_version!(),
                cli.global_options.system
            );
            return Ok(());
        }
        Some(Commands::Direnvrc) => {
            print!("{}", *devenv::DIRENVRC);
            return Ok(());
        }
        _ => {}
    }

    // Determine which mode to run in:
    // - Tracing mode: when trace-output is stdout/stderr (conflicts with TUI/CLI output)
    // - TUI mode: interactive terminal UI (default)
    // - Legacy CLI mode: spinners and progress indicators (--no-tui or --log-format cli)
    if cli.global_options.use_tracing_mode() {
        run_with_tracing(cli)
    } else if cli.global_options.use_legacy_cli() {
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
    let level = get_log_level(&cli);
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

    // Shutdown coordination (TUI handles Ctrl+C, no install_signals needed)
    let shutdown = Shutdown::new();

    // Devenv on background thread (own runtime with GC-registered workers)
    let shutdown_clone = shutdown.clone();
    let devenv_thread = std::thread::spawn(move || {
        build_gc_runtime().block_on(async {
            let result = tokio::select! {
                result = run_devenv(cli, shutdown_clone.clone()) => result,
                _ = shutdown_clone.wait_for_shutdown() => Ok(CommandResult::Done),
            };
            devenv_activity::signal_done();
            result
        })
    });

    // TUI on main thread (owns terminal)
    let _ = devenv_tui::TuiApp::new(activity_rx, shutdown)
        .filter_level(filter_level)
        .run()
        .await;

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    // Wait for devenv thread to finish and get the result
    let thread_result = devenv_thread
        .join()
        .map_err(|_| miette::miette!("Devenv thread panicked"))?;

    // Check if secrets need prompting (special case: TUI stopped for password entry)
    let result = match thread_result {
        Ok(cmd_result) => cmd_result,
        Err(err) => {
            // Check if error is SecretsNeedPrompting
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

        let level = get_log_level(&cli);
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

        let level = get_log_level(&cli);
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

fn get_log_level(cli: &Cli) -> devenv_tracing::Level {
    if cli.global_options.verbose {
        devenv_tracing::Level::Debug
    } else if cli.global_options.quiet {
        devenv_tracing::Level::Silent
    } else {
        devenv_tracing::Level::default()
    }
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
        Commands::Generate { .. } => match which::which("devenv-generate") {
            Ok(devenv_generate) => {
                let mut cmd = Command::new(devenv_generate);
                cmd.args(std::env::args().skip(1).filter(|arg| arg != "generate"));
                CommandResult::Exec(cmd)
            }
            Err(_) => {
                miette::bail!(indoc::formatdoc! {"
                    devenv-generate was not found in PATH

                    It was moved to a separate binary due to https://github.com/cachix/devenv/issues/1733

                    For now, use the web version at https://devenv.new
                "})
            }
        },
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
            let paths = devenv.build(&attributes).await?;
            let output = paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            CommandResult::Print(format!("{output}\n"))
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
            } => {
                let output = devenv.tasks_run(tasks, mode, show_output).await?;
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
        Commands::Mcp {} => {
            let config = devenv.config.read().await.clone();
            devenv::mcp::run_mcp_server(config).await?;
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
