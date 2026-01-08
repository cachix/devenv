use crate::tracing as devenv_tracing;
use clap::{Parser, Subcommand, crate_version};
use devenv_core::GlobalOptions;
use devenv_tasks::RunMode;
use std::path::PathBuf;

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
    pub global_options: GlobalOptions,
}

impl Cli {
    /// Parse the CLI arguments with clap and resolve any conflicting options.
    pub fn parse_and_resolve_options() -> Self {
        let mut cli = Self::parse();
        cli.global_options.resolve_overrides();
        cli
    }

    pub fn get_log_level(&self) -> devenv_tracing::Level {
        if self.global_options.verbose {
            devenv_tracing::Level::Debug
        } else if self.global_options.quiet {
            devenv_tracing::Level::Silent
        } else {
            devenv_tracing::Level::default()
        }
    }
}

#[derive(Subcommand, Clone)]
pub enum Commands {
    #[command(about = "Scaffold devenv.yaml, devenv.nix, .gitignore and .envrc.")]
    Init { target: Option<PathBuf> },

    #[command(about = "Generate devenv.yaml and devenv.nix using AI")]
    Generate {
        #[arg(num_args=0.., trailing_var_arg = true)]
        description: Vec<String>,

        #[clap(long, default_value = "https://devenv.new")]
        host: String,

        #[arg(
            long,
            help = "Paths to exclude during generation.",
            value_name = "PATH"
        )]
        exclude: Vec<PathBuf>,

        // https://consoledonottrack.com/
        #[clap(long, env = "DO_NOT_TRACK", action = clap::ArgAction::SetTrue)]
        disable_telemetry: bool,
    },

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

    #[command(
        about = "Print a direnvrc that adds devenv support to direnv. See https://devenv.sh/automatic-shell-activation.",
        long_about = "Print a direnvrc that adds devenv support to direnv.\n\nExample .envrc:\n\n  eval \"$(devenv direnvrc)\"\n\n  # You can pass flags to the devenv command\n  # For example: use devenv --impure --option services.postgres.enable:bool true\n  use devenv\n\nSee https://devenv.sh/automatic-shell-activation."
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
    GenerateJSONSchema,

    #[command(about = "Launch Model Context Protocol server for AI assistants")]
    Mcp {
        #[arg(
            long,
            help = "Run as HTTP server instead of stdio. Optionally specify port (default: 8080)"
        )]
        http: Option<Option<u16>>,
    },

    #[command(
        about = "Start the nixd language server for devenv.nix. https://devenv.sh/editor-support/"
    )]
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
    },

    #[command(alias = "stop", about = "Stop processes running in the background.")]
    Down {},
    // TODO: Status/Attach
}

#[derive(Subcommand, Clone)]
#[clap(about = "Run tasks. https://devenv.sh/tasks/")]
pub enum TasksCommand {
    #[command(about = "Run tasks.")]
    Run {
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
