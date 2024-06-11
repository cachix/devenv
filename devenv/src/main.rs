mod cli;
mod command;
mod config;
mod devenv;
mod log;

use clap::{crate_version, Parser};
use cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand};
use devenv::Devenv;
use miette::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.global_options.verbose {
        log::Level::Debug
    } else {
        log::Level::Info
    };

    let logger = log::Logger::new(level);

    let mut config = config::Config::load()?;
    for input in cli.global_options.override_input.chunks_exact(2) {
        config.add_input(&input[0].clone(), &input[1].clone(), &[]);
    }

    let options = devenv::DevenvOptions {
        logger: Some(logger.clone()),
        global_options: Some(cli.global_options),
        config,
        ..Default::default()
    };

    let mut devenv = Devenv::new(options);

    if !matches!(cli.command, Commands::Version {} | Commands::Gc { .. }) {
        devenv.create_directories()?;
    }

    match cli.command {
        Commands::Shell { cmd, args } => devenv.shell(&cmd, &args, true),
        Commands::Test {
            dont_override_dotfile,
        } => {
            let tmpdir = tempdir::TempDir::new_in(devenv.devenv_root(), ".devenv")
                .expect("Failed to create temporary directory");
            if !dont_override_dotfile {
                logger.info(&format!(
                    "Overriding .devenv to {}",
                    tmpdir.path().file_name().unwrap().to_str().unwrap()
                ));
                devenv.update_devenv_dotfile(tmpdir.as_ref());
            }
            devenv.test()
        }
        Commands::Version {} => Ok(println!(
            "devenv {} ({})",
            crate_version!(),
            devenv.global_options.system
        )),
        Commands::Container {
            registry,
            copy,
            docker_run,
            copy_args,
            name,
            command,
        } => {
            devenv.container_name = name.clone();
            match name {
                None => {
                    if let Some(c) = command {
                        match c {
                            ContainerCommand::Build { name } => {
                                devenv.container_name = Some(name.clone());
                                let _ = devenv.container_build(&name)?;
                            }
                            ContainerCommand::Copy { name } => {
                                devenv.container_name = Some(name.clone());
                                devenv.container_copy(&name, &copy_args, registry.as_deref())?;
                            }
                            ContainerCommand::Run { name } => {
                                devenv.container_name = Some(name.clone());
                                devenv.container_run(&name, &copy_args, registry.as_deref())?;
                            }
                        }
                    }
                }
                Some(name) => {
                    match (copy, docker_run) {
                        (true, false) => {
                            logger.warn(
                                "--copy flag is deprecated, use `devenv container copy` instead",
                            );
                            devenv.container_copy(&name, &copy_args, registry.as_deref())?;
                        }
                        (_, true) => {
                            logger.warn(
                                "--docker-run flag is deprecated, use `devenv container run` instead",
                            );
                            devenv.container_run(&name, &copy_args, registry.as_deref())?;
                        }
                        _ => {
                            logger.warn("Calling without a subcommand is deprecated, use `devenv container build` instead");
                            let _ = devenv.container_build(&name)?;
                        }
                    };
                }
            };
            Ok(())
        }
        Commands::Init { target } => devenv.init(&target),
        Commands::Search { name } => devenv.search(&name),
        Commands::Gc {} => devenv.gc(),
        Commands::Info {} => devenv.info(),
        Commands::Repl {} => devenv.repl(),
        Commands::Build { attributes } => devenv.build(&attributes),
        Commands::Update { name } => devenv.update(&name),
        Commands::Up { process, detach } => devenv.up(process.as_deref(), &detach, &detach),
        Commands::Processes { command } => match command {
            ProcessesCommand::Up { process, detach } => {
                devenv.up(process.as_deref(), &detach, &detach)
            }
            ProcessesCommand::Down {} => devenv.down(),
        },
        Commands::Inputs { command } => match command {
            InputsCommand::Add { name, url, follows } => devenv.inputs_add(&name, &url, &follows),
        },

        // hidden
        Commands::Assemble => devenv.assemble(false),
        Commands::PrintDevEnv { json } => devenv.print_dev_env(json),
        Commands::GenerateJSONSchema => {
            config::write_json_schema();
            Ok(())
        }
    }
}
