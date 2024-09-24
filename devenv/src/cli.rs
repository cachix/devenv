use clap::{crate_version, Parser, Subcommand};
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
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,

    #[command(flatten)]
    pub(crate) global_options: GlobalOptions,
}

#[derive(Parser, Clone)]
pub struct GlobalOptions {
    #[arg(short, long, global = true, help = "Enable debug log level.")]
    pub verbose: bool,

    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        help = "Disable all logs"
    )]
    pub quiet: bool,

    #[arg(short = 'j', long,
        global = true, help = "Maximum number of Nix builds at any time.",
        default_value_t = max_jobs())]
    pub max_jobs: u8,

    #[arg(
        short = 'u',
        long,
        help = "Maximum number CPU cores being used by a single build..",
        default_value = "2"
    )]
    pub cores: u8,

    #[arg(short, long, global = true, default_value_t = default_system())]
    pub system: String,

    #[arg(
        short,
        long,
        global = true,
        help = "Relax the hermeticity of the environment."
    )]
    pub impure: bool,

    #[arg(long, global = true, help = "Use flake cache for evaluation results.")]
    pub eval_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Disable substituters and consider all previously downloaded files up-to-date."
    )]
    pub offline: bool,

    // TODO: --no-clean?
    #[arg(
        short,
        long,
        global = true,
        num_args = 0..,
        value_delimiter = ',',
        help = "Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through."
    )]
    pub clean: Option<Vec<String>>,

    #[arg(long, global = true, help = "Enter the Nix debugger on failure.")]
    pub nix_debugger: bool,

    #[arg(
        short,
        long,
        global = true,
        num_args = 2,
        value_delimiter = ' ',
        help = "Pass additional options to nix commands, see `man nix.conf` for full list."
    )]
    pub nix_option: Vec<String>,

    #[arg(
        short,
        long,
        global = true,
        num_args = 2,
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml."
    )]
    pub override_input: Vec<String>,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            verbose: false,
            quiet: false,
            max_jobs: max_jobs(),
            cores: 2,
            system: default_system(),
            impure: false,
            eval_cache: false,
            offline: false,
            clean: None,
            nix_debugger: false,
            nix_option: vec![],
            override_input: vec![],
        }
    }
}

#[derive(Subcommand, Clone)]
pub(crate) enum Commands {
    #[command(about = "Scaffold devenv.yaml, devenv.nix, .gitignore and .envrc.")]
    Init {
        target: Option<PathBuf>,
    },

    #[command(about = "Activate the developer environment. https://devenv.sh/basics/")]
    Shell {
        cmd: Option<String>,
        args: Vec<String>,
    },

    #[command(about = "Update devenv.lock from devenv.yaml inputs. http://devenv.sh/inputs/")]
    Update {
        name: Option<String>,
    },

    #[command(
        about = "Search for packages and options in nixpkgs. https://devenv.sh/packages/#searching-for-a-file"
    )]
    Search {
        name: String,
    },

    #[command(
        alias = "show",
        about = "Print information about this developer environment."
    )]
    Info {},

    #[command(about = "Start processes in the foreground. https://devenv.sh/processes/")]
    Up {
        #[arg(help = "Start a specific process.")]
        process: Option<String>,

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
        #[arg(short, long)]
        registry: Option<String>,

        #[arg(long, hide = true)]
        copy: bool,

        #[arg(long, hide = true)]
        docker_run: bool,

        #[arg(long)]
        copy_args: Vec<String>,

        #[arg(hide = true)]
        name: Option<String>,

        #[command(subcommand)]
        command: Option<ContainerCommand>,
    },

    Inputs {
        #[command(subcommand)]
        command: InputsCommand,
    },

    Repl {},

    #[command(
        about = "Deletes previous shell generations. See http://devenv.sh/garbage-collection"
    )]
    Gc {},

    #[command(about = "Build any attribute in devenv.nix.")]
    Build {
        #[arg(num_args=1..)]
        attributes: Vec<String>,
    },

    #[command(about = "Print the version of devenv.")]
    Version {},

    #[clap(hide = true)]
    Assemble,

    #[clap(hide = true)]
    PrintDevEnv {
        #[arg(long)]
        json: bool,
    },

    #[clap(hide = true)]
    GenerateJSONSchema,
}

#[derive(Subcommand, Clone)]
#[clap(about = "Start or stop processes. https://devenv.sh/processes/")]
pub(crate) enum ProcessesCommand {
    #[command(alias = "start", about = "Start processes in the foreground.")]
    Up {
        process: Option<String>,

        #[arg(short, long, help = "Start processes in the background.")]
        detach: bool,
    },

    #[command(alias = "stop", about = "Stop processes running in the background.")]
    Down {},
    // TODO: Status/Attach
}

#[derive(Subcommand, Clone)]
#[clap(about = "Run tasks. https://devenv.sh/tasks/")]
pub(crate) enum TasksCommand {
    #[command(about = "Run tasks.")]
    Run { tasks: Vec<String> },
}

#[derive(Subcommand, Clone)]
#[clap(
    about = "Build, copy, or run a container. https://devenv.sh/containers/",
    arg_required_else_help(true)
)]
pub(crate) enum ContainerCommand {
    #[command(about = "Build a container.")]
    Build { name: String },

    #[command(about = "Copy a container to registry.")]
    Copy { name: String },

    #[command(about = "Run a container.")]
    Run { name: String },
}

#[derive(Subcommand, Clone)]
#[clap(about = "Add an input to devenv.yaml. https://devenv.sh/inputs/")]
pub(crate) enum InputsCommand {
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

pub fn default_system() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unknown architecture"
    };

    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin" // macOS is referred to as "darwin" in target triples
    } else {
        "unknown OS"
    };
    format!("{arch}-{os}")
}

fn max_jobs() -> u8 {
    let num_cpus = std::thread::available_parallelism().unwrap_or_else(|e| {
        eprintln!("Failed to get number of logical CPUs: {}", e);
        std::num::NonZeroUsize::new(1).unwrap()
    });
    (num_cpus.get() / 2).try_into().unwrap()
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
