use clap::{Parser, Subcommand};
use devenv_tasks::{
    signal_handler::SignalHandler, Config, RunMode, TaskConfig, TasksUi, VerbosityLevel,
};
use std::env;

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
    },
    Export {
        #[clap()]
        strings: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    if let Ok(quiet_var) = env::var("DEVENV_TASKS_QUIET") {
        if quiet_var == "true" || quiet_var == "1" {
            verbosity = VerbosityLevel::Quiet;
        }
    }

    match args.command {
        Command::Run { roots, mode } => {
            let tasks_json = env::var("DEVENV_TASKS")?;
            let tasks: Vec<TaskConfig> = serde_json::from_str(&tasks_json)?;

            let config = Config {
                tasks,
                roots,
                run_mode: mode,
            };

            // Create shared signal handler
            let signal_handler = SignalHandler::start();
            let cancellation_token = signal_handler.cancellation_token();

            let mut tasks_ui = TasksUi::builder(config, verbosity)
                .with_cancellation_token(cancellation_token)
                .build()
                .await?;
            let (status, _outputs) = tasks_ui.run().await?;

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
