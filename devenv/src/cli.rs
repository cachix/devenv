use crate::tracing as devenv_tracing;
use clap::{Parser, Subcommand, crate_version};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use devenv_tasks::RunMode;
use std::env;
use std::ffi::OsStr;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::str::FromStr;

// --- Trace types (moved from devenv-core) ---

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceFormat {
    /// A verbose structured log format used for debugging.
    Full,
    /// A JSON log format used for machine consumption.
    #[default]
    Json,
    /// A pretty human-readable log format used for debugging.
    Pretty,
}

/// Deprecated: use TraceFormat instead.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegacyLogFormat {
    #[default]
    Cli,
    TracingFull,
    TracingPretty,
    TracingJson,
}

impl TryFrom<LegacyLogFormat> for TraceFormat {
    type Error = ();

    fn try_from(format: LegacyLogFormat) -> Result<Self, Self::Error> {
        match format {
            LegacyLogFormat::TracingFull => Ok(TraceFormat::Full),
            LegacyLogFormat::TracingJson => Ok(TraceFormat::Json),
            LegacyLogFormat::TracingPretty => Ok(TraceFormat::Pretty),
            LegacyLogFormat::Cli => Err(()),
        }
    }
}

/// Specifies where trace output should be written.
///
/// Accepts the following formats:
/// - `stdout` - write to standard output
/// - `stderr` - write to standard error
/// - `file:/path/to/file` - write to the specified file path
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TraceOutput {
    #[default]
    Stderr,
    Stdout,
    File(PathBuf),
}

impl FromStr for TraceOutput {
    type Err = ParseTraceOutputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stderr" => Ok(TraceOutput::Stderr),
            "stdout" => Ok(TraceOutput::Stdout),
            s if s.starts_with("file:") => Ok(TraceOutput::File(PathBuf::from(&s[5..]))),
            _ => Err(ParseTraceOutputError::UnsupportedFormat(s.to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseTraceOutputError {
    #[error("unsupported trace output format '{0}', expected 'stdout', 'stderr', or 'file:<path>'")]
    UnsupportedFormat(String),
}

// --- CLI-only options ---

#[derive(clap::Args, Clone, Debug)]
pub struct CliOptions {
    #[arg(
        short = 'V',
        long,
        global = true,
        help = "Print version information",
        long_help = "Print version information and exit"
    )]
    pub version: bool,

    #[arg(short, long, global = true, help = "Enable additional debug logs.")]
    pub verbose: bool,

    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        help = "Silence all logs"
    )]
    pub quiet: bool,

    #[arg(
        long,
        global = true,
        env = "DEVENV_TUI",
        help = "Enable the interactive terminal interface (default when interactive)."
    )]
    pub tui: bool,

    #[arg(
        long,
        global = true,
        help = "Disable the interactive terminal interface."
    )]
    pub no_tui: bool,

    #[arg(
        long,
        global = true,
        help = "Deprecated: use --trace-format instead.",
        value_enum,
        hide = true
    )]
    pub log_format: Option<LegacyLogFormat>,

    #[arg(
        long,
        global = true,
        help_heading = "Tracing",
        env = "DEVENV_TRACE_OUTPUT",
        help = "Enable tracing and set the output destination: stdout, stderr, or file:<path>. Tracing is disabled by default."
    )]
    pub trace_output: Option<TraceOutput>,

    #[arg(
        long,
        global = true,
        help_heading = "Tracing",
        env = "DEVENV_TRACE_FORMAT",
        help = "Set the trace output format. Only takes effect when tracing is enabled via --trace-output.",
        default_value_t,
        value_enum
    )]
    pub trace_format: TraceFormat,
}

impl CliOptions {
    /// Resolve conflicting/derived options.
    pub fn resolve_overrides(&mut self) {
        self.tui = match devenv_core::flag(self.tui, self.no_tui) {
            Some(v) => v,
            // Default: enable TUI only when running interactively outside CI.
            None => {
                let is_ci = env::var_os("CI").is_some();
                let is_tty = io::stdin().is_terminal() && io::stderr().is_terminal();
                !is_ci && is_tty
            }
        };

        // Handle deprecated --log-format (except Cli which is handled separately)
        if let Some(format) = self.log_format {
            eprintln!("Warning: --log-format is deprecated, use --trace-format instead");
            if let Ok(trace_format) = format.try_into() {
                self.trace_format = trace_format;
            }
        }
    }

    /// Returns true if tracing-only mode should be used.
    pub fn use_tracing_mode(&self) -> bool {
        matches!(
            self.trace_output,
            Some(TraceOutput::Stdout) | Some(TraceOutput::Stderr)
        )
    }

    /// Returns true if legacy CLI mode should be used (instead of TUI).
    pub fn use_legacy_cli(&self) -> bool {
        !self.tui || self.log_format == Some(LegacyLogFormat::Cli)
    }
}

/// Complete task names by reading from .devenv/task-names.txt cache file.
/// Walks up from current directory to find the project root.
/// If cache doesn't exist, spawns `devenv tasks list` in background to populate it.
fn complete_task_names(current: &OsStr) -> Vec<CompletionCandidate> {
    let current_str = current.to_str().unwrap_or("");

    // Walk up from current directory to find .devenv directory or devenv.nix/devenv.yaml
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let cache_path = d.join(".devenv").join("task-names.txt");
        let is_devenv_project = d.join("devenv.nix").exists() || d.join("devenv.yaml").exists();

        if cache_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&cache_path) {
                return content
                    .lines()
                    .filter(|name| !name.is_empty() && name.starts_with(current_str))
                    .map(CompletionCandidate::new)
                    .collect();
            }
        } else if is_devenv_project {
            // Cache doesn't exist but this is a devenv project - spawn background task to populate it
            let _ = std::process::Command::new("devenv")
                .args(["tasks", "list"])
                .current_dir(&d)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            // Return empty for now - next completion will have the cache
            return Vec::new();
        }

        dir = d.parent().map(|p| p.to_path_buf());
    }

    Vec::new()
}

#[derive(Parser)]
#[command(
    name = "devenv",
    color = clap::ColorChoice::Auto,
    // for --clean to work with subcommands
    subcommand_precedence_over_arg = true,
    dont_delimit_trailing_values = true,
    about = format!("https://devenv.sh {}: Fast, Declarative, Reproducible, and Composable Developer Environments", crate_version!())
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub cli_options: CliOptions,

    #[arg(
        long,
        global = true,
        help = "Source for devenv.nix (flake input reference or path)",
        long_help = "Source for devenv.nix.\n\nCan be either a filesystem path (with path: prefix) or a flake input reference.\n\nExamples:\n  --from github:cachix/devenv\n  --from github:cachix/devenv?dir=examples/simple\n  --from path:/absolute/path/to/project\n  --from path:./relative/path"
    )]
    pub from: Option<String>,

    #[command(flatten)]
    pub input_overrides: devenv_core::InputOverrides,

    #[command(flatten)]
    pub nix_cli: devenv_core::NixCliOptions,

    #[command(flatten)]
    pub shell_cli: devenv_core::ShellCliOptions,

    #[command(flatten)]
    pub cache_cli: devenv_core::CacheCliOptions,

    #[command(flatten)]
    pub secret_cli: devenv_core::SecretCliOptions,
}

impl Cli {
    /// Parse the CLI arguments with clap and resolve any conflicting options.
    pub fn parse_and_resolve_options() -> Self {
        let mut cli = Self::parse();
        cli.cli_options.resolve_overrides();
        cli
    }

    pub fn get_log_level(&self) -> devenv_tracing::Level {
        if self.cli_options.verbose {
            devenv_tracing::Level::Debug
        } else if self.cli_options.quiet {
            devenv_tracing::Level::Silent
        } else {
            devenv_tracing::Level::default()
        }
    }
}

#[derive(Subcommand, Clone)]
pub enum Commands {
    #[command(about = "Scaffold devenv.yaml, devenv.nix, and .gitignore.")]
    Init { target: Option<PathBuf> },

    #[command(about = "Generate devenv.yaml and devenv.nix using AI")]
    Generate,

    #[command(about = "Activate the developer environment. https://devenv.sh/basics/")]
    Shell {
        cmd: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    #[command(about = "Update devenv.lock from devenv.yaml inputs. http://devenv.sh/inputs/")]
    Update { name: Option<String> },

    #[command(
        about = "Search for packages and options in nixpkgs. https://devenv.sh/packages/#searching-for-a-file"
    )]
    Search { name: String },

    #[command(
        alias = "show",
        about = "Print information about this developer environment."
    )]
    Info {},

    #[command(about = "Start processes in the foreground. https://devenv.sh/processes/")]
    Up {
        #[arg(help = "Start a specific process(es).")]
        processes: Vec<String>,

        #[arg(short, long, help = "Start processes in the background.")]
        detach: bool,

        #[arg(
            long,
            help = "Error if a port is already in use instead of auto-allocating the next available port."
        )]
        strict_ports: bool,
    },

    Processes {
        #[command(subcommand)]
        command: ProcessesCommand,
    },

    #[command(about = "Run tasks. https://devenv.sh/tasks/")]
    Tasks {
        #[command(subcommand)]
        command: TasksCommand,
    },

    #[command(about = "Run tests. http://devenv.sh/tests/", alias = "ci")]
    Test {
        #[arg(short, long, help = "Don't override .devenv to a temporary directory.")]
        dont_override_dotfile: bool,
    },

    Container {
        #[command(subcommand)]
        command: ContainerCommand,
    },

    Inputs {
        #[command(subcommand)]
        command: InputsCommand,
    },

    #[command(about = "Show relevant changelogs.")]
    Changelogs {},

    #[command(about = "Launch an interactive environment for inspecting the devenv configuration.")]
    Repl {},

    #[command(
        about = "Delete previous shell generations. See https://devenv.sh/garbage-collection"
    )]
    Gc {},

    #[command(about = "Build any attribute in devenv.nix.")]
    Build {
        #[arg(num_args=1..)]
        attributes: Vec<String>,
    },

    #[command(about = "Evaluate any attribute in devenv.nix and return JSON.")]
    Eval {
        #[arg(num_args=1..)]
        attributes: Vec<String>,
    },

    #[command(
        about = "Print a direnvrc that adds devenv support to direnv. See https://devenv.sh/integrations/direnv/.",
        long_about = "Print a direnvrc that adds devenv support to direnv.\n\nExample .envrc:\n\n  eval \"$(devenv direnvrc)\"\n\n  # You can pass flags to the devenv command\n  # For example: use devenv --impure --option services.postgres.enable:bool true\n  use devenv\n\nSee https://devenv.sh/integrations/direnv/."
    )]
    Direnvrc,

    #[command(about = "Print the version of devenv.")]
    Version,

    #[clap(hide = true)]
    Assemble,

    #[clap(hide = true)]
    PrintDevEnv {
        #[arg(long)]
        json: bool,
    },

    #[clap(hide = true)]
    DirenvExport,

    #[clap(hide = true)]
    GenerateJSONSchema,

    /// Print computed paths (dotfile, gc, etc.) for shell integration
    #[clap(hide = true)]
    PrintPaths,

    #[command(about = "Launch Model Context Protocol server for AI assistants")]
    Mcp {
        #[arg(
            long,
            help = "Run as HTTP server instead of stdio. Optionally specify port (default: 8080)"
        )]
        http: Option<Option<u16>>,
    },

    #[command(about = "Start the nixd language server for devenv.nix.")]
    Lsp {
        #[arg(long, help = "Print nixd configuration and exit")]
        print_config: bool,
    },
}

#[derive(Subcommand, Clone)]
#[clap(about = "Start or stop processes. https://devenv.sh/processes/")]
pub enum ProcessesCommand {
    #[command(alias = "start", about = "Start processes in the foreground.")]
    Up {
        processes: Vec<String>,

        #[arg(short, long, help = "Start processes in the background.")]
        detach: bool,

        #[arg(
            long,
            help = "Error if a port is already in use instead of auto-allocating the next available port."
        )]
        strict_ports: bool,
    },

    #[command(alias = "stop", about = "Stop processes running in the background.")]
    Down {},

    #[command(about = "Wait for all processes to be ready.")]
    Wait {
        #[arg(long, default_value = "120", help = "Timeout in seconds.")]
        timeout: u64,
    },
    // TODO: Status/Attach
}

#[derive(Subcommand, Clone)]
#[clap(about = "Run tasks. https://devenv.sh/tasks/")]
pub enum TasksCommand {
    #[command(about = "Run tasks.")]
    Run {
        #[arg(add = ArgValueCompleter::new(complete_task_names))]
        tasks: Vec<String>,

        #[arg(
            short,
            long,
            help = "The execution mode for tasks (affects dependency resolution)",
            value_enum,
            default_value_t = RunMode::Single
        )]
        mode: RunMode,

        #[arg(
            long,
            help = "Show task output for all tasks (equivalent to --verbose for tasks)"
        )]
        show_output: bool,

        #[arg(
            long = "input",
            value_name = "KEY=VALUE",
            help = "Set a task input value (repeatable, value parsed as JSON if valid, otherwise string)"
        )]
        input: Vec<String>,

        #[arg(
            long = "input-json",
            value_name = "JSON",
            help = "Set task inputs from a JSON object string"
        )]
        input_json: Option<String>,
    },
    #[command(about = "List all available tasks.")]
    List {},
}

#[derive(Subcommand, Clone)]
#[clap(
    about = "Build, copy, or run a container. https://devenv.sh/containers/",
    arg_required_else_help(true)
)]
pub enum ContainerCommand {
    #[command(about = "Build a container.")]
    Build { name: String },

    #[command(about = "Copy a container to registry.")]
    Copy {
        name: String,

        #[arg(long)]
        copy_args: Vec<String>,

        #[arg(short, long)]
        registry: Option<String>,
    },

    #[command(about = "Run a container.")]
    Run {
        name: String,

        #[arg(long)]
        copy_args: Vec<String>,
    },
}

#[derive(Subcommand, Clone)]
#[clap(about = "Add an input to devenv.yaml. https://devenv.sh/inputs/")]
pub enum InputsCommand {
    #[command(about = "Add an input to devenv.yaml.")]
    Add {
        #[arg(help = "The name of the input.")]
        name: String,

        #[arg(
            help = "See https://devenv.sh/reference/yaml-options/#inputsnameurl for possible values."
        )]
        url: String,

        #[arg(short, long, help = "What inputs should follow your inputs?")]
        follows: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::Cli;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert()
    }
}
