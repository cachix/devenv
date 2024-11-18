use clap::{Parser, Subcommand};
use devenv_tasks::{Config, TaskConfig, TasksUi};
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
    },
    Export {
        #[clap()]
        strings: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.command {
        Command::Run { roots } => {
            let tasks_json = env::var("DEVENV_TASKS")?;
            let tasks: Vec<TaskConfig> = serde_json::from_str(&tasks_json)?;

            let config = Config { tasks, roots };

            let mut tasks_ui = TasksUi::new(config).await?;
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
