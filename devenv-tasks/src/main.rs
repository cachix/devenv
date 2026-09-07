use clap::{Parser, Subcommand, ValueEnum};
use devenv_core::VerbosityLevel;
use devenv_processes::{SupervisionMode, get_process_runtime_dir};
use devenv_tasks::{Config, RunMode, SudoContext, TaskConfig, Tasks, TasksUi, is_tty};
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

        #[clap(long, value_enum, default_value_t = RunMode::Before, help = "The execution mode for tasks (affects dependency resolution)")]
        mode: RunMode,

        #[clap(
            long,
            value_parser,
            env = "DEVENV_TASK_FILE",
            help = "Path to a JSON file containing task definitions"
        )]
        task_file: Option<PathBuf>,

        #[clap(long, help = "Directory for task cache database")]
        cache_dir: PathBuf,

        #[clap(long, help = "Runtime directory for process state")]
        runtime_dir: PathBuf,

        #[clap(
            long,
            value_enum,
            default_value_t = SupervisorArg::Native,
            help = "Who owns restart, readiness, watchdog, and file-watch policy."
        )]
        supervisor: SupervisorArg,

        #[clap(
            long,
            value_enum,
            help = "What to do after all processes settle. Defaults to linger for native and exit for external."
        )]
        on_idle: Option<OnIdleArg>,

        #[clap(
            long,
            help = "Exclude non-root process tasks. External mode enables this automatically."
        )]
        ignore_process_deps: bool,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum SupervisorArg {
    Native,
    External,
}

impl From<SupervisorArg> for SupervisionMode {
    fn from(a: SupervisorArg) -> Self {
        match a {
            SupervisorArg::Native => SupervisionMode::Native,
            SupervisorArg::External => SupervisionMode::External,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum OnIdleArg {
    Exit,
    Linger,
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

fn main() -> Result<()> {
    if let Some(code) = devenv_processes::maybe_run_process_guardian() {
        std::process::exit(code);
    }
    if let Some(code) = devenv_processes::maybe_run_capability_helper() {
        std::process::exit(code);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<()> {
    let shutdown = Shutdown::new();
    shutdown.install_signals().await;
    // A second signal force-exits without running teardown or destructors, and
    // processes live in isolated process scopes, so they would outlive us.
    shutdown.set_pre_exit_hook(devenv_processes::kill_process_scopes);
    watch_parent(&shutdown);

    run_tasks(shutdown.clone()).await?;

    Ok(())
}

/// Request shutdown when Unix reparents this process.
/// Polling supports macOS, which lacks a parent-death signal.
#[cfg(unix)]
fn watch_parent(shutdown: &Arc<Shutdown>) {
    let shutdown = Arc::clone(shutdown);
    let original_parent = nix::unistd::getppid();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            if nix::unistd::getppid() != original_parent {
                shutdown.handle_signal(tokio_shutdown::Signal::SIGTERM);
                break;
            }
        }
    });
}

#[cfg(not(unix))]
fn watch_parent(_shutdown: &Arc<Shutdown>) {}
async fn run_tasks(shutdown: Arc<Shutdown>) -> Result<()> {
    // Drop sudo privileges before creating files.
    let sudo_context = SudoContext::detect();
    if let Some(ref ctx) = sudo_context {
        ctx.drop_privileges()
            .map_err(|e| TaskError::Other(format!("Failed to drop privileges: {}", e)))?;
    }

    let args = Args::parse();

    // Hide plain output behind the TUI, but keep it for process-compose logs.
    let has_tty = is_tty();
    let mut verbosity = if let Ok(cmdline) = env::var("DEVENV_CMDLINE") {
        let cmdline = cmdline.to_lowercase();
        if cmdline.contains("--quiet") || cmdline.contains(" -q ") {
            VerbosityLevel::Quiet
        } else if cmdline.contains("--verbose") || cmdline.contains(" -v ") {
            VerbosityLevel::Verbose
        } else if cmdline.contains("--no-tui") || !has_tty {
            VerbosityLevel::Normal
        } else {
            VerbosityLevel::Quiet
        }
    } else {
        VerbosityLevel::Normal
    };

    // Keep support for the old quiet flag.
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
            cache_dir,
            runtime_dir,
            supervisor,
            on_idle,
            ignore_process_deps,
        } => {
            let supervisor: SupervisionMode = supervisor.into();
            let exit_on_idle = on_idle.map(|on_idle| match on_idle {
                OnIdleArg::Exit => true,
                OnIdleArg::Linger => false,
            });

            let mut tasks: Vec<TaskConfig> = fetch_tasks(&task_file)?;

            if let Ok(cmdline) = env::var("DEVENV_CMDLINE") {
                let cmdline = cmdline.to_lowercase();
                if cmdline.contains("--show-output") {
                    for task in &mut tasks {
                        task.show_output = true;
                    }
                }
            }

            let runtime_dir = get_process_runtime_dir(&runtime_dir).map_err(|e| {
                TaskError::Other(format!("Failed to create runtime directory: {}", e))
            })?;

            let config = Config {
                tasks,
                roots,
                run_mode: mode,
                runtime_dir,
                cache_dir,
                sudo_context: sudo_context.clone(),
                env: std::env::vars().collect(),
                bash: String::new(),
                ignore_process_deps,
                exit_on_idle,
                supervisor,
                capability_broker: None,
            };

            let tasks = Tasks::builder(config, verbosity, Arc::clone(&shutdown))
                .build()
                .await?;

            let (activity_rx, activity_handle) = devenv_activity::init();
            let _activity_guard = activity_handle.install();

            let tasks = Arc::new(tasks);
            let tasks_clone = Arc::clone(&tasks);

            let run_handle = tokio::spawn(async move { tasks_clone.run(false).await });

            let ui = TasksUi::new(Arc::clone(&tasks), activity_rx, verbosity);
            let result = ui.run(run_handle, false).await;

            let cleanup_result = tasks.process_runner().stop_all().await;
            let (status, _) = result?;
            cleanup_result.map_err(|e| {
                TaskError::Other(format!(
                    "Failed to stop processes during task shutdown: {e:?}"
                ))
            })?;

            if shutdown.last_signal().is_some() {
                shutdown.exit_process();
            }

            if status.has_failures() {
                std::process::exit(1);
            }
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
    if let Ok(raw) = env::var("DEVENV_TASKS")
        && !raw.is_empty()
    {
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
