use crate::tracing as devenv_tracing;
use clap::{Parser, Subcommand, crate_version};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use devenv_core::config::NixBackendType;
use devenv_core::settings::{
    CacheOptions, InputOverrides, NixOptions, SecretOptions, ShellOptions, flag,
};
use devenv_tasks::RunMode;
use std::env;
use std::ffi::OsStr;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::str::FromStr;

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

// --- Domain CLI args (clap-derived, converted to *Options for resolve()) ---

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Nix options")]
pub struct NixCliArgs {
    #[arg(
        short = 'j',
        long,
        global = true,
        env = "DEVENV_MAX_JOBS",
        help = "Maximum number of Nix builds to run concurrently.",
        long_help = "Maximum number of Nix builds to run concurrently.\n\nDefaults to 1/4 of available CPU cores (minimum 1)."
    )]
    pub max_jobs: Option<u8>,

    #[arg(
        short = 'u',
        long,
        global = true,
        env = "DEVENV_CORES",
        help = "Number of CPU cores available to each build.",
        long_help = "Number of CPU cores available to each build.\n\nDefaults to available cores divided by max-jobs (minimum 1)."
    )]
    pub cores: Option<u8>,

    #[arg(
        short,
        long,
        global = true,
        help = "Override the target system.",
        long_help = "Override the target system.\n\nDefaults to the host system (e.g. aarch64-darwin, x86_64-linux)."
    )]
    pub system: Option<String>,

    #[arg(
        short,
        long,
        global = true,
        help = "Relax the hermeticity of the environment."
    )]
    pub impure: bool,

    #[arg(
        long,
        global = true,
        help = "Force a hermetic environment, overriding config."
    )]
    pub no_impure: bool,

    #[arg(
        long,
        global = true,
        help = "Disable substituters and consider all previously downloaded files up-to-date."
    )]
    pub offline: bool,

    #[arg(long, global = true, num_args = 2,
        value_names = ["NAME", "VALUE"],
        value_delimiter = ' ',
        help = "Pass additional options to nix commands",
        long_help = "Pass additional options to nix commands.\n\nThese options are passed directly to Nix using the --option flag.\nSee `man nix.conf` for the full list of available options.\n\nExamples:\n  --nix-option sandbox false\n  --nix-option keep-outputs true\n  --nix-option system x86_64-darwin")]
    pub nix_option: Vec<String>,

    #[arg(long, global = true, help = "Enter the Nix debugger on failure.")]
    pub nix_debugger: bool,

    #[arg(
        long,
        global = true,
        value_enum,
        hide = true,
        help = "Nix backend to use."
    )]
    pub backend: Option<NixBackendType>,
}

impl From<NixCliArgs> for NixOptions {
    fn from(cli: NixCliArgs) -> Self {
        Self {
            max_jobs: cli.max_jobs,
            cores: cli.cores,
            system: cli.system,
            impure: flag(cli.impure, cli.no_impure),
            offline: cli.offline.then_some(true),
            nix_option: cli.nix_option,
            nix_debugger: cli.nix_debugger.then_some(true),
            backend: cli.backend,
        }
    }
}

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Cache options")]
pub struct CacheCliArgs {
    #[arg(
        long,
        global = true,
        help = "Enable caching of Nix evaluation results (default)."
    )]
    pub eval_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Disable caching of Nix evaluation results."
    )]
    pub no_eval_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Force a refresh of the Nix evaluation cache."
    )]
    pub refresh_eval_cache: bool,

    #[arg(long, global = true, help = "Force a refresh of the task cache.")]
    pub refresh_task_cache: bool,
}

impl From<CacheCliArgs> for CacheOptions {
    fn from(cli: CacheCliArgs) -> Self {
        Self {
            eval_cache: flag(cli.eval_cache, cli.no_eval_cache),
            refresh_eval_cache: cli.refresh_eval_cache.then_some(true),
            refresh_task_cache: cli.refresh_task_cache.then_some(true),
        }
    }
}

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Shell options")]
pub struct ShellCliArgs {
    #[arg(short, long, global = true,
        num_args = 0..,
        value_delimiter = ',',
        help = "Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through.")]
    pub clean: Option<Vec<String>>,

    #[arg(short = 'P', long, global = true,
        num_args = 1,
        action = clap::ArgAction::Append,
        help = "Activate one or more profiles defined in devenv.nix",
        long_help = "Activate one or more profiles defined in devenv.nix.\n\nProfiles allow you to define different configurations that can be merged with your base configuration.\n\nSee https://devenv.sh/profiles for more information.\n\nExamples:\n  --profile python-3.14\n  --profile backend --profile fast-startup")]
    pub profile: Vec<String>,

    #[arg(
        long,
        global = true,
        help = "Enable auto-reload when config files change (default)."
    )]
    pub reload: bool,

    #[arg(
        long,
        global = true,
        help = "Disable auto-reload when config files change."
    )]
    pub no_reload: bool,
}

impl From<ShellCliArgs> for ShellOptions {
    fn from(cli: ShellCliArgs) -> Self {
        Self {
            clean: cli.clean,
            profiles: cli.profile,
            reload: flag(cli.reload, cli.no_reload),
        }
    }
}

#[derive(clap::Args, Clone, Debug, Default)]
#[command(next_help_heading = "Secretspec options")]
pub struct SecretCliArgs {
    #[arg(
        long,
        global = true,
        env = "SECRETSPEC_PROVIDER",
        help = "Override the secretspec provider"
    )]
    pub secretspec_provider: Option<String>,

    #[arg(
        long,
        global = true,
        env = "SECRETSPEC_PROFILE",
        help = "Override the secretspec profile"
    )]
    pub secretspec_profile: Option<String>,
}

impl From<SecretCliArgs> for SecretOptions {
    fn from(cli: SecretCliArgs) -> Self {
        Self {
            secretspec_provider: cli.secretspec_provider,
            secretspec_profile: cli.secretspec_profile,
        }
    }
}

#[derive(clap::Args, Clone, Debug, Default)]
#[command(next_help_heading = "Input overrides")]
pub struct InputOverrideCliArgs {
    #[arg(short, long, global = true,
        num_args = 2,
        value_names = ["NAME", "URI"],
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml",
        long_help = "Override inputs in devenv.yaml.\n\nExamples:\n  --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable\n  --override-input nixpkgs path:/path/to/local/nixpkgs")]
    pub override_input: Vec<String>,

    #[arg(long = "option", short = 'O', global = true,
        num_args = 2,
        value_names = ["OPTION:TYPE", "VALUE"],
        help = "Override configuration options with typed values",
        long_help = "Override configuration options with typed values.\n\nOPTION must include a type: <attribute>:<type>\nSupported types: string, int, float, bool, path, pkg, pkgs\n\nExamples:\n  --option languages.rust.channel:string beta\n  --option services.postgres.enable:bool true\n  --option languages.python.version:string 3.10\n  --option packages:pkgs \"ncdu git\"")]
    pub nix_module_options: Vec<String>,
}

impl From<InputOverrideCliArgs> for InputOverrides {
    fn from(cli: InputOverrideCliArgs) -> Self {
        Self {
            override_input: cli.override_input,
            nix_module_options: cli.nix_module_options,
        }
    }
}

// --- CLI-only options ---

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Tracing options")]
pub struct TracingCliArgs {
    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_OUTPUT",
        help = "Enable tracing and set the output destination: stdout, stderr, or file:<path>. Tracing is disabled by default."
    )]
    pub trace_output: Option<TraceOutput>,

    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_FORMAT",
        help = "Set the trace output format. Only takes effect when tracing is enabled via --trace-output.",
        default_value_t,
        value_enum
    )]
    pub trace_format: TraceFormat,
}

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Global options")]
pub struct CliOptions {
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

    #[arg(short, long, action = clap::ArgAction::Help, global = true, help = "Print help (see a summary with '-h')")]
    pub help: Option<bool>,

    #[arg(
        short = 'V',
        long,
        global = true,
        help = "Print version information",
        long_help = "Print version information and exit"
    )]
    pub version: bool,
}

impl CliOptions {
    /// Returns true if legacy CLI mode should be used (instead of TUI).
    pub fn use_legacy_cli(&self) -> bool {
        !self.tui || self.log_format == Some(LegacyLogFormat::Cli)
    }
}

impl TracingCliArgs {
    /// Returns true if tracing-only mode should be used.
    pub fn use_tracing_mode(&self) -> bool {
        matches!(
            self.trace_output,
            Some(TraceOutput::Stdout) | Some(TraceOutput::Stderr)
        )
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
    disable_help_flag = true,
    // for --clean to work with subcommands
    subcommand_precedence_over_arg = true,
    dont_delimit_trailing_values = true,
    about = format!("https://devenv.sh {}: Fast, Declarative, Reproducible, and Composable Developer Environments", crate_version!())
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(
        long,
        global = true,
        help_heading = "Input overrides",
        help = "Source for devenv.nix (flake input reference or path)",
        long_help = "Source for devenv.nix.\n\nCan be either a filesystem path (with path: prefix) or a flake input reference.\n\nExamples:\n  --from github:cachix/devenv\n  --from github:cachix/devenv?dir=examples/simple\n  --from path:/absolute/path/to/project\n  --from path:./relative/path"
    )]
    pub from: Option<String>,

    #[command(flatten)]
    pub input_overrides: InputOverrideCliArgs,

    #[command(flatten)]
    pub nix_args: NixCliArgs,

    #[command(flatten)]
    pub shell_args: ShellCliArgs,

    #[command(flatten)]
    pub cache_args: CacheCliArgs,

    #[command(flatten)]
    pub secret_args: SecretCliArgs,

    #[command(flatten)]
    pub tracing_args: TracingCliArgs,

    #[command(flatten)]
    pub cli_options: CliOptions,
}

impl Cli {
    /// Parse the CLI arguments with clap and resolve any conflicting options.
    pub fn parse_args() -> Self {
        let mut cli = Self::parse();

        cli.cli_options.tui = match flag(cli.cli_options.tui, cli.cli_options.no_tui) {
            Some(v) => v,
            // Default: enable TUI only when running interactively outside CI.
            None => {
                let is_ci = env::var_os("CI").is_some();
                let is_tty = io::stdin().is_terminal() && io::stderr().is_terminal();
                !is_ci && is_tty
            }
        };

        // Handle deprecated --log-format
        if let Some(format) = cli.cli_options.log_format {
            eprintln!("Warning: --log-format is deprecated, use --trace-format instead");
            if let Ok(trace_format) = format.try_into() {
                cli.tracing_args.trace_format = trace_format;
            }
        }

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
