use clap::crate_version;
use devenv::{
    CommandResult, Devenv,
    cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand},
    log,
};
use devenv_activity::{ActivityLevel, message};
use devenv_core::config::{self, Config};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{process::Command, sync::Arc};
use tempfile::TempDir;
use tokio_shutdown::Shutdown;
use tracing::info;

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

    // Branch based on log format
    if cli.global_options.log_format == log::LogFormat::Tui {
        run_with_tui(cli)
    } else {
        run_without_tui(cli)
    }
}

fn run_with_tui(cli: Cli) -> Result<()> {
    // Initialize activity channel and register it
    let (activity_rx, activity_handle) = devenv_activity::init();
    activity_handle.install();

    // Initialize tracing
    let level = get_log_level(&cli);
    log::init_tracing(
        level,
        cli.global_options.log_format,
        cli.global_options.trace_export_file.as_deref(),
    );

    // Shutdown coordination (TUI handles Ctrl+C, no install_signals needed)
    let shutdown = Shutdown::new();

    // Devenv on background thread (own runtime)
    let shutdown_clone = shutdown.clone();
    let devenv_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create devenv runtime");

        let result = rt.block_on(async {
            tokio::select! {
                result = run_devenv(cli, shutdown_clone.clone()) => result,
                _ = shutdown_clone.wait_for_shutdown() => Ok(CommandResult::Done(())),
            }
        });

        // Signal TUI to shut down now that devenv is done
        shutdown_clone.shutdown();

        result
    });

    // TUI on main thread (owns terminal)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;

    let error_messages = rt
        .block_on(devenv_tui::TuiApp::new(activity_rx, shutdown).run())
        .unwrap_or_default();

    // Restore terminal to normal state (disable raw mode, show cursor)
    devenv_tui::app::restore_terminal();

    // Wait for devenv thread to finish and get the result
    let result = devenv_thread
        .join()
        .map_err(|_| miette::miette!("Devenv thread panicked"))??;

    // Print queued error messages at the very end
    for msg in &error_messages {
        eprintln!("{}", msg.text);
        if let Some(details) = &msg.details {
            eprintln!("{}", details);
        }
    }

    // Execute any pending command (e.g., shell exec) now that TUI is cleaned up
    result.exec()
}

#[tokio::main]
async fn run_without_tui(cli: Cli) -> Result<()> {
    let shutdown = Shutdown::new();
    shutdown.install_signals().await;

    // Initialize tracing
    let level = get_log_level(&cli);
    log::init_tracing(
        level,
        cli.global_options.log_format,
        cli.global_options.trace_export_file.as_deref(),
    );

    let result = tokio::select! {
        result = run_devenv(cli, shutdown.clone()) => result,
        _ = shutdown.wait_for_shutdown() => Ok(CommandResult::Done(())),
    }?;

    // Execute any pending command immediately (no TUI to clean up)
    result.exec()
}

fn get_log_level(cli: &Cli) -> log::Level {
    if cli.global_options.verbose {
        log::Level::Debug
    } else if cli.global_options.quiet {
        log::Level::Silent
    } else {
        log::Level::default()
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
        Commands::Shell { cmd, ref args } => match cmd {
            Some(cmd) => devenv.prepare_exec(Some(cmd), args).await?,
            None => devenv.shell().await?,
        },
        Commands::Test { .. } => devenv.test().await?,
        Commands::Container {
            registry,
            copy,
            docker_run,
            copy_args,
            name,
            command,
        } => {
            // Backwards compatibility for the legacy container flags:
            //   `devenv container <name> --copy` is now `devenv container copy <name>`
            //   `devenv container <name> --docker-run` is now `devenv container run <name>`
            //   `devenv container <name>` is now `devenv container build <name>`
            let command = if let Some(name) = name {
                if copy {
                    message(
                        ActivityLevel::Warn,
                        "The --copy flag is deprecated. Use `devenv container copy` instead.",
                    );
                    ContainerCommand::Copy { name }
                } else if docker_run {
                    message(
                        ActivityLevel::Warn,
                        "The --docker-run flag is deprecated. Use `devenv container run` instead.",
                    );
                    ContainerCommand::Run { name }
                } else {
                    message(
                        ActivityLevel::Warn,
                        "Calling `devenv container` without a subcommand is deprecated. Use `devenv container build {name}` instead.",
                    );
                    ContainerCommand::Build { name }
                }
            } else {
                // Error out if we don't have a subcommand at this point.
                if let Some(cmd) = command {
                    cmd
                } else {
                    // Impossible. This handled by clap, but if we have no subcommand at this point, error out.
                    bail!(
                        "No container subcommand provided. Use `devenv container build` or specify a command."
                    )
                }
            };

            match command {
                ContainerCommand::Build { name } => {
                    let path = devenv.container_build(&name).await?.unwrap();
                    println!("{path}");
                    CommandResult::done()
                }
                ContainerCommand::Copy { name } => {
                    devenv
                        .container_copy(&name, &copy_args, registry.as_deref())
                        .await?
                }
                ContainerCommand::Run { name } => {
                    devenv
                        .container_run(&name, &copy_args, registry.as_deref())
                        .await?
                }
            }
        }
        Commands::Init { target } => devenv.init(&target)?,
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
        Commands::Search { name } => devenv.search(&name).await?,
        Commands::Gc {} => devenv.gc().await?,
        Commands::Info {} => devenv.info().await?,
        Commands::Repl {} => devenv.repl().await?,
        Commands::Build { attributes } => {
            let paths = devenv.build(&attributes).await?.unwrap();
            for path in paths {
                println!("{}", path.display());
            }
            CommandResult::done()
        }
        Commands::Update { name } => devenv.update(&name).await?,
        Commands::Up { processes, detach }
        | Commands::Processes {
            command: ProcessesCommand::Up { processes, detach },
        } => {
            let options = devenv::ProcessOptions {
                detach,
                log_to_file: detach,
                ..Default::default()
            };
            devenv.up(processes, &options).await?
        }
        Commands::Processes {
            command: ProcessesCommand::Down {},
        } => devenv.down().await?,
        Commands::Tasks { command } => match command {
            TasksCommand::Run {
                tasks,
                mode,
                show_output,
            } => devenv.tasks_run(tasks, mode, show_output).await?,
            TasksCommand::List {} => devenv.tasks_list().await?,
        },
        Commands::Inputs { command } => match command {
            InputsCommand::Add { name, url, follows } => {
                devenv.inputs_add(&name, &url, &follows).await?
            }
        },
        Commands::Changelogs {} => devenv.changelogs().await?,

        // hidden
        Commands::Assemble => devenv.assemble(false).await?,
        Commands::PrintDevEnv { json } => devenv.print_dev_env(json).await?,
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
            CommandResult::Done(())
        }
        Commands::Mcp {} => {
            let config = devenv.config.read().await.clone();
            devenv::mcp::run_mcp_server(config).await?;
            CommandResult::Done(())
        }
        Commands::Direnvrc => unreachable!(),
        Commands::Version => unreachable!(),
    };

    Ok(result)
}
