use clap::Parser;
use devenv::tasks::{Config, TaskConfig, TasksUi};
use std::env;

#[derive(Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(help = "Root directories to search for tasks")]
    roots: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let tasks_json = env::var("DEVENV_TASKS")?;
    let tasks: Vec<TaskConfig> = serde_json::from_str(&tasks_json)?;

    let config = Config {
        tasks,
        roots: args.roots,
    };

    let mut tasks_ui = TasksUi::new(config).await?;
    tasks_ui.run().await?;

    Ok(())
}
