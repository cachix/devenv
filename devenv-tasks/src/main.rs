use clap::{Parser, Subcommand};
use devenv_tasks::{Config, RunMode, SudoContext, TaskConfig, Tasks, TasksUi, VerbosityLevel};
use std::{env, fmt::Display, fs, path::PathBuf, sync::Arc};
use thiserror::Error;
use tokio_shutdown::Shutdown;

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

type Result<T> = std::result::Result<T, TaskError>;

#[derive(Debug, Clone)]
enum TaskSource {
    EnvVar,
    File(PathBuf),
}

impl Display for TaskSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskSource::EnvVar => write!(f, "DEVENV_TASKS environment variable"),
            TaskSource::File(path) => write!(f, "tasks file at {}", path.display()),
        }
    }
}

#[derive(Debug, Error)]
enum TaskError {
    #[error("Failed to read tasks from {task_source}: {error}")]
    ReadError {
        task_source: TaskSource,
        #[source]
        error: std::io::Error,
    },

    #[error("Failed to parse tasks from {task_source}: {error}")]
    ParseError {
        task_source: TaskSource,
        #[source]
        error: serde_json::Error,
    },

    #[error(
        "No task source provided: DEVENV_TASKS environment variable not set and no task file specified"
    )]
    NoSource,

    #[error("{0}")]
    Other(String),

    #[error(transparent)]
    Tasks(#[from] devenv_tasks::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[tokio::main]
async fn main() -> Result<()> {
    let shutdown = Shutdown::new();
    shutdown.install_signals().await;

    tokio::select! {
        result = run_tasks(shutdown.clone()) => result?,
        _ = shutdown.wait_for_shutdown() => {}
    };

    Ok(())
}
async fn run_tasks(shutdown: Arc<Shutdown>) -> Result<()> {
    // Detect and handle sudo.
    // Drop privileges immediately to avoid creating any files as root.
    let sudo_context = SudoContext::detect();
    if let Some(ref ctx) = sudo_context {
        ctx.drop_privileges()
            .map_err(|e| TaskError::Other(format!("Failed to drop privileges: {}", e)))?;
    }

    let args = Args::parse();

    // Determine verbosity level from DEVENV_CMDLINE
    // TUI is on by default, so we default to Quiet to avoid corrupting the TUI display.
    // Only show output if --no-tui is passed or --verbose is explicitly requested.
    let mut verbosity = if let Ok(cmdline) = env::var("DEVENV_CMDLINE") {
        let cmdline = cmdline.to_lowercase();
        if cmdline.contains("--quiet") || cmdline.contains(" -q ") {
            VerbosityLevel::Quiet
        } else if cmdline.contains("--verbose") || cmdline.contains(" -v ") {
            VerbosityLevel::Verbose
        } else if cmdline.contains("--no-tui") {
            // TUI is disabled, show normal output
            VerbosityLevel::Normal
        } else {
            // TUI is on by default, suppress output
            VerbosityLevel::Quiet
        }
    } else {
        // No DEVENV_CMDLINE means we're likely running standalone, show output
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
            let mut tasks: Vec<TaskConfig> = fetch_tasks(&task_file)?;

            // If --show-output flag is present, enable output for all tasks
            if let Ok(cmdline) = env::var("DEVENV_CMDLINE") {
                let cmdline = cmdline.to_lowercase();
                if cmdline.contains("--show-output") {
                    for task in &mut tasks {
                        task.show_output = true;
                    }
                }
            }

            let config = Config {
                tasks,
                roots,
                run_mode: mode,
                sudo_context: sudo_context.clone(),
            };

            let tasks = Tasks::builder(config, verbosity, Arc::clone(&shutdown))
                .build()
                .await?;

            let (status, _) = TasksUi::new(tasks, verbosity).run().await?;

            if shutdown.last_signal().is_some() {
                shutdown.exit_process();
            }

            if status.has_failures() {
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

/// Fetches task configurations from either the DEVENV_TASKS environment variable or a task file.
///
/// Priority order:
/// 1. DEVENV_TASKS environment variable (takes precedence)
/// 2. Task file specified via --task-file or DEVENV_TASK_FILE
///
/// Returns a vector of task configurations or an error if the source cannot be read or parsed.
fn fetch_tasks(task_file: &Option<PathBuf>) -> Result<Vec<TaskConfig>> {
    let (data, task_source) = read_raw_task_source(task_file)?;
    serde_json::from_str(&data).map_err(|error| TaskError::ParseError { task_source, error })
}

/// Reads the raw task specification string from either the DEVENV_TASKS environment variable or a file.
///
/// Priority order:
/// 1. DEVENV_TASKS environment variable (checked first)
/// 2. Task file path (if provided)
///
/// Returns the raw JSON string and the source it came from, or an error if no source is available.
fn read_raw_task_source(task_file: &Option<PathBuf>) -> Result<(String, TaskSource)> {
    if let Ok(raw) = env::var("DEVENV_TASKS") {
        return Ok((raw, TaskSource::EnvVar));
    }

    match task_file {
        Some(path) => match fs::read_to_string(path) {
            Ok(data) => Ok((data, TaskSource::File(path.clone()))),
            Err(error) => Err(TaskError::ReadError {
                task_source: TaskSource::File(path.clone()),
                error,
            }),
        },
        None => Err(TaskError::NoSource),
    }
}
