use devenv::{app, config, command, log};
use app::{App, Cli, Commands, ContainerCommand, InputsCommand, ProcessesCommand};
use clap::{crate_version, Parser, Subcommand};
use cli_table::{print_stderr, Table, WithTitle};
use include_dir::{include_dir, Dir};
use miette::{bail, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::os::unix::fs::symlink;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
};

pub fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose {
        log::Level::Debug
    } else {
        log::Level::Info
    };

    let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv").unwrap();
    xdg_dirs
        .create_data_directory(Path::new("devenv"))
        .expect("Failed to create DEVENV_HOME directory");
    let devenv_home = xdg_dirs.get_data_home();
    let devenv_home_gc = devenv_home.join("gc");
    std::fs::create_dir_all(&devenv_home_gc).expect("Failed to create DEVENV_HOME_GC directory");
    let devenv_root = std::env::current_dir().expect("Failed to get current directory");
    let devenv_dot_gc = devenv_root.join(".devenv").join("gc");
    std::fs::create_dir_all(&devenv_dot_gc).expect("Failed to create .devenv/gc directory");
    let devenv_dotfile = devenv_root.join(".devenv");
    let cachix_trusted_keys = devenv_home.join("cachix_trusted_keys.json");
    let logger = log::Logger::new(level);
    let mut config = config::Config::load()?;
    for input in cli.override_input.chunks_exact(2) {
        config.add_input(&input[0].clone(), &input[1].clone(), &[]);
    }
    let mut app = App {
        cli,
        config,
        has_processes: None,
        logger,
        container_name: None,
        devenv_root,
        devenv_dotfile,
        devenv_dot_gc,
        devenv_home_gc,
        cachix_trusted_keys,
        cachix_caches: None,
    };

    match app.cli.command.clone() {
        Commands::Shell { cmd, args } => app.shell(&cmd, &args, true),
        Commands::Test {
            dont_override_dotfile,
        } => app.test(dont_override_dotfile),
        Commands::Version {} => Ok(println!("devenv {} ({})", crate_version!(), app.cli.system)),
        Commands::Container {
            registry,
            copy,
            docker_run,
            copy_args,
            name,
            command,
        } => {
            app.container_name = name.clone();
            match name {
                None => {
                    if let Some(c) = command {
                        match c {
                            ContainerCommand::Build { name } => {
                                app.container_name = Some(name.clone());
                                let _ = app.container_build(&name)?;
                            }
                            ContainerCommand::Copy { name } => {
                                app.container_name = Some(name.clone());
                                app.container_copy(&name, &copy_args, registry.as_deref())?;
                            }
                            ContainerCommand::Run { name } => {
                                app.container_name = Some(name.clone());
                                app.container_run(&name, &copy_args, registry.as_deref())?;
                            }
                        }
                    }
                }
                Some(name) => {
                    match (copy, docker_run) {
                        (true, false) => {
                            app.logger.warn(
                                "--copy flag is deprecated, use `devenv container copy` instead",
                            );
                            app.container_copy(&name, &copy_args, registry.as_deref())?;
                        }
                        (_, true) => {
                            app.logger.warn(
                                "--docker-run flag is deprecated, use `devenv container run` instead",
                            );
                            app.container_run(&name, &copy_args, registry.as_deref())?;
                        }
                        _ => {
                            app.logger.warn("Calling without a subcommand is deprecated, use `devenv container build` instead");
                            let _ = app.container_build(&name)?;
                        }
                    };
                }
            };
            Ok(())
        }
        Commands::Init { target } => app.init(&target),
        Commands::Search { name } => app.search(&name),
        Commands::Gc {} => app.gc(),
        Commands::Info {} => app.info(),
        Commands::Build { attributes } => app.build(&attributes),
        Commands::Update { name } => app.update(&name),
        Commands::Up { process, detach } => app.up(process.as_deref(), &detach, &detach),
        Commands::Processes { command } => match command {
            ProcessesCommand::Up { process, detach } => {
                app.up(process.as_deref(), &detach, &detach)
            }
            ProcessesCommand::Down {} => app.down(),
        },
        Commands::Inputs { command } => match command {
            InputsCommand::Add { name, url, follows } => app.inputs_add(&name, &url, &follows),
        },
        // hidden
        Commands::Assemble => app.assemble(),
        Commands::PrintDevEnv { json } => app.print_dev_env(json),
        Commands::GenerateJSONSchema => {
            config::write_json_schema();
            Ok(())
        }
    }
}

