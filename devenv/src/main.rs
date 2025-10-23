use clap::crate_version;
use devenv::{
    Devenv,
    cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand, TasksCommand},
    config, log,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{env, os::unix::process::CommandExt, process::Command, sync::Arc};
use tempfile::TempDir;
use tokio_shutdown::Shutdown;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let shutdown = Shutdown::new();
    shutdown.install_signals().await;

    tokio::select! {
        result = run_devenv(shutdown.clone()) => result,
        _ = shutdown.wait_for_shutdown() => Ok(()),
    }
}

async fn run_devenv(shutdown: Arc<Shutdown>) -> Result<()> {
    let cli = Cli::parse_and_resolve_options();

    let print_version = || {
        println!(
            "devenv {} ({})",
            crate_version!(),
            cli.global_options.system
        );
        Ok(())
    };

    let command = match cli.command {
        None | Some(Commands::Version) => return print_version(),
        Some(Commands::Direnvrc) => {
            print!("{}", *devenv::DIRENVRC);
            return Ok(());
        }
        Some(cmd) => cmd,
    };

    let level = if cli.global_options.verbose {
        log::Level::Debug
    } else if cli.global_options.quiet {
        log::Level::Silent
    } else {
        log::Level::default()
    };

    log::init_tracing(
        level,
        cli.global_options.log_format,
        cli.global_options.trace_export_file.as_deref(),
        shutdown.clone(),
    );

    let mut config = config::Config::load()?;
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

    match command {
        Commands::Shell { cmd, ref args } => match cmd {
            Some(cmd) => devenv.exec_in_shell(Some(cmd), args).await,
            None => devenv.shell().await,
        },
        Commands::Test { .. } => devenv.test().await,
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
                    warn!(
                        devenv.is_user_message = true,
                        "The --copy flag is deprecated. Use `devenv container copy` instead."
                    );
                    ContainerCommand::Copy { name }
                } else if docker_run {
                    warn!(
                        devenv.is_user_message = true,
                        "The --docker-run flag is deprecated. Use `devenv container run` instead."
                    );
                    ContainerCommand::Run { name }
                } else {
                    warn!(
                        devenv.is_user_message = true,
                        "Calling `devenv container` without a subcommand is deprecated. Use `devenv container build {name}` instead."
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
                    let path = devenv.container_build(&name).await?;
                    // Print the path to the built container to stdout
                    println!("{path}");
                }
                ContainerCommand::Copy { name } => {
                    devenv
                        .container_copy(&name, &copy_args, registry.as_deref())
                        .await?;
                }
                ContainerCommand::Run { name } => {
                    devenv
                        .container_run(&name, &copy_args, registry.as_deref())
                        .await?;
                }
            }

            Ok(())
        }
        Commands::Init { target } => devenv.init(&target),
        Commands::Generate { .. } => match which::which("devenv-generate") {
            Ok(devenv_generate) => {
                let error = Command::new(devenv_generate)
                    .args(std::env::args().skip(1).filter(|arg| arg != "generate"))
                    .exec();
                miette::bail!("failed to execute devenv-generate {error}");
            }
            Err(_) => {
                miette::bail!(indoc::formatdoc! {"
                    devenv-generate was not found in PATH

                    It was moved to a separate binary due to https://github.com/cachix/devenv/issues/1733

                    For now, use the web version at https://devenv.new
                "})
            }
        },
        Commands::Search { name } => devenv.search(&name).await,
        Commands::Gc {} => devenv.gc().await,
        Commands::Info {} => devenv.info().await,
        Commands::Repl {} => devenv.repl().await,
        Commands::Build { attributes } => devenv.build(&attributes).await,
        Commands::Update { name } => devenv.update(&name).await,
        Commands::Up { processes, detach }
        | Commands::Processes {
            command: ProcessesCommand::Up { processes, detach },
        } => {
            let options = devenv::ProcessOptions {
                detach,
                log_to_file: detach,
                ..Default::default()
            };
            devenv.up(processes, &options).await
        }
        Commands::Processes {
            command: ProcessesCommand::Down {},
        } => devenv.down().await,
        Commands::Tasks { command } => match command {
            TasksCommand::Run { tasks, mode } => devenv.tasks_run(tasks, mode).await,
            TasksCommand::List {} => devenv.tasks_list().await,
        },
        Commands::Inputs { command } => match command {
            InputsCommand::Add { name, url, follows } => {
                devenv.inputs_add(&name, &url, &follows).await
            }
        },

        // hidden
        Commands::Assemble => devenv.assemble(false).await,
        Commands::PrintDevEnv { json } => devenv.print_dev_env(json).await,
        Commands::GenerateJSONSchema => {
            config::write_json_schema()
                .await
                .wrap_err("Failed to generate JSON schema")?;
            Ok(())
        }
        Commands::Mcp {} => {
            let config = devenv.config.read().await.clone();
            devenv::mcp::run_mcp_server(config).await
        }
        Commands::Direnvrc => unreachable!(),
        Commands::Version => unreachable!(),
    }
}
