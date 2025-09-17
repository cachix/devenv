use clap::{Parser, Subcommand};
use devenv_tasks::{
    Config, RunMode, SudoContext, TaskConfig, TasksUi, VerbosityLevel,
    signal_handler::SignalHandler,
};
use std::{env, fs, path::PathBuf};

#[derive(Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run {
        #[clap()]
        roots: Vec<String>,

        #[clap(long, value_enum, default_value_t = RunMode::Single, help = "The execution mode for tasks (affects dependency resolution)")]
        mode: RunMode,

        #[clap(
            long,
            value_parser,
            env = "DEVENV_TASK_FILE",
            help = "Path to a JSON file containing task definitions"
        )]
        task_file: Option<PathBuf>,
    },
    Export {
        #[clap()]
        strings: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Detect and handle sudo.
    // Drop privileges immediately to avoid creating any files as root.
    let sudo_context = SudoContext::detect();
    if let Some(ref ctx) = sudo_context {
        ctx.drop_privileges()
            .map_err(|e| format!("Failed to drop privileges: {}", e))?;
    }

    let args = Args::parse();

    // Determine verbosity level from DEVENV_CMDLINE
    let mut verbosity = if let Ok(cmdline) = env::var("DEVENV_CMDLINE") {
        let cmdline = cmdline.to_lowercase();
        if cmdline.contains("--quiet") || cmdline.contains(" -q ") {
            VerbosityLevel::Quiet
        } else if cmdline.contains("--verbose") || cmdline.contains(" -v ") {
            VerbosityLevel::Verbose
        } else {
            VerbosityLevel::Normal
        }
    } else {
        VerbosityLevel::Normal
    };

    // Keeping backwards compatibility for existing scripts that might set DEVENV_TASKS_QUIET
    if let Ok(quiet_var) = env::var("DEVENV_TASKS_QUIET")
        && (quiet_var == "true" || quiet_var == "1")
    {
        verbosity = VerbosityLevel::Quiet;
    }

    match args.command {
        Command::Run {
            roots,
            mode,
            task_file,
        } => {
            let tasks: Vec<TaskConfig> = {
                let task_source = || {
                    task_file
                        .as_ref()
                        .map(|p| format!("tasks file at {}", p.display()))
                        .unwrap_or_else(|| "DEVENV_TASKS".to_string())
                };

                let data = env::var("DEVENV_TASKS").or_else(|_| {
                    task_file
                        .as_ref()
                        .ok_or_else(|| {
                            "No task file specified and DEVENV_TASKS environment variable not set"
                                .to_string()
                        })
                        .and_then(|path| {
                            fs::read_to_string(path)
                                .map_err(|e| format!("Failed to read {}: {e}", task_source()))
                        })
                })?;
                serde_json::from_str(&data)
                    .map_err(|e| format!("Failed to parse {} as JSON: {e}", task_source()))?
            };
            let config = Config {
                tasks,
                roots,
                run_mode: mode,
                sudo_context: sudo_context.clone(),
            };

            // Create a global signal handler
            let signal_handler = SignalHandler::start();

            let mut tasks_ui = TasksUi::builder(config, verbosity)
                .with_cancellation_token(signal_handler.cancellation_token())
                .build()
                .await?;
            let (status, _outputs) = tasks_ui.run().await?;

            if signal_handler.last_signal().is_some() {
                signal_handler.exit_process();
            }

            if status.failed + status.dependency_failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Export { strings } => {
            let output_file =
                env::var("DEVENV_TASK_OUTPUT_FILE").expect("DEVENV_TASK_OUTPUT_FILE not set");
            let mut output: serde_json::Value = std::fs::read_to_string(&output_file)
                .map(|content| serde_json::from_str(&content).unwrap_or(serde_json::json!({})))
                .unwrap_or(serde_json::json!({}));

            let mut exported_vars = serde_json::Map::new();
            for var in strings {
                if let Ok(value) = env::var(&var) {
                    exported_vars.insert(var, serde_json::Value::String(value));
                }
            }

            if !output.as_object().unwrap().contains_key("devenv") {
                output["devenv"] = serde_json::json!({});
            }
            if !output["devenv"].as_object().unwrap().contains_key("env") {
                output["devenv"]["env"] = serde_json::json!({});
            }
            output["devenv"]["env"] = serde_json::Value::Object(
                output["devenv"]["env"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .chain(exported_vars)
                    .collect(),
            );
            std::fs::write(output_file, serde_json::to_string_pretty(&output)?)?;
        }
    }

    Ok(())
}
