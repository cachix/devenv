mod cli;
mod command;
mod config;
mod devenv;
mod log;

use clap::{crate_version, Parser};
use cli::{Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand};
use devenv::Devenv;
use miette::Result;
use sha2::Digest;
use std::path::Path;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.global_options.verbose {
        log::Level::Debug
    } else {
        log::Level::Info
    };

    let logger = log::Logger::new(level);

    let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv").unwrap();
    let devenv_home = xdg_dirs.get_data_home();
    let devenv_home_gc = devenv_home.join("gc");
    let devenv_root = std::env::current_dir().expect("Failed to get current directory");
    let devenv_dot_gc = devenv_root.join(".devenv").join("gc");
    let devenv_dotfile = devenv_root.join(".devenv");
    let devenv_tmp = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()));
    // first 7 chars of sha256 hash of devenv_state
    let devenv_state_hash = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    };
    let devenv_runtime = Path::new(&devenv_tmp).join(format!("devenv-{}", &devenv_state_hash[..7]));
    let cachix_trusted_keys = devenv_home.join("cachix_trusted_keys.json");

    let mut config = config::Config::load()?;
    for input in cli.global_options.override_input.chunks_exact(2) {
        config.add_input(&input[0].clone(), &input[1].clone(), &[]);
    }

    let mut devenv = Devenv {
        config,
        global_options: cli.global_options,
        logger: logger.clone(),
        assembled: false,
        dirs_created: false,
        has_processes: None,
        container_name: None,
        devenv_root,
        devenv_dotfile,
        devenv_dot_gc,
        devenv_home_gc,
        devenv_tmp,
        devenv_runtime,
        cachix_caches: None,
        cachix_trusted_keys,
    };

    if !matches!(cli.command, Commands::Version {} | Commands::Gc { .. }) {
        devenv.create_directories()?;
    }

    match cli.command {
        Commands::Shell { cmd, args } => devenv.shell(&cmd, &args, true),
        Commands::Test {
            dont_override_dotfile,
        } => devenv.test(dont_override_dotfile),
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
