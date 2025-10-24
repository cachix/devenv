use crate::log::LogFormat;
use clap::{Parser, Subcommand, crate_version};
use devenv_tasks::RunMode;
use std::path::PathBuf;
use tracing::error;

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
}

#[derive(Clone, Debug, Parser)]
pub struct GlobalOptions {
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
        help = "Configure the output format of the logs.",
        default_value_t,
        value_enum
    )]
    pub log_format: LogFormat,

    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_EXPORT_FILE",
        help = "Path to export traces.",
        hide = true
    )]
    pub trace_export_file: Option<PathBuf>,

    #[arg(short = 'j', long,
        global = true,
        env = "DEVENV_MAX_JOBS",
        help = "Maximum number of Nix builds to run concurrently.",
        default_value_t = NixBuildDefaults::compute().max_jobs)]
    pub max_jobs: u8,

    #[arg(
        short = 'u',
        long,
        global = true,
        env = "DEVENV_CORES",
        help = "Number of CPU cores available to each build.",
        default_value_t = NixBuildDefaults::compute().cores
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

    #[arg(
        long,
        global = true,
        help = "Cache the results of Nix evaluation.",
        hide = true
    )]
    #[arg(
        long_help = "Cache the results of Nix evaluation (deprecated, on by default). Use --no-eval-cache to disable caching."
    )]
    #[arg(default_value_t = true, overrides_with = "no_eval_cache")]
    pub eval_cache: bool,

    /// Disable the evaluation cache. Sets `eval_cache` to false.
    #[arg(
        long,
        global = true,
        help = "Disable caching of Nix evaluation results."
    )]
    #[arg(overrides_with = "eval_cache")]
    pub no_eval_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Force a refresh of the Nix evaluation cache."
    )]
    pub refresh_eval_cache: bool,

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
        value_names = ["NAME", "VALUE"],
        value_delimiter = ' ',
        help = "Pass additional options to nix commands",
        long_help = "Pass additional options to nix commands.\n\nThese options are passed directly to Nix using the --option flag.\nSee `man nix.conf` for the full list of available options.\n\nExamples:\n  --nix-option sandbox false\n  --nix-option keep-outputs true\n  --nix-option system x86_64-darwin"
    )]
    pub nix_option: Vec<String>,

    #[arg(
        short,
        long,
        global = true,
        num_args = 2,
        value_names = ["NAME", "URI"],
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml",
        long_help = "Override inputs in devenv.yaml.\n\nExamples:\n  --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable\n  --override-input nixpkgs path:/path/to/local/nixpkgs"
    )]
    pub override_input: Vec<String>,

    #[arg(
        long,
        short = 'O',
        global = true,
        num_args = 2,
        value_names = ["OPTION", "VALUE"],
        help = "Override configuration options with typed values",
        long_help = "Override configuration options with typed values.\n\nOPTION must include a type: <attribute>:<type>\nSupported types: string, int, float, bool, path, pkg, pkgs\n\nExamples:\n  --option languages.rust.channel:string beta\n  --option services.postgres.enable:bool true\n  --option languages.python.version:string 3.10\n  --option packages:pkgs \"ncdu git\""
    )]
    pub option: Vec<String>,

    #[arg(
        short = 'P',
        long,
        global = true,
        num_args = 1,
        action = clap::ArgAction::Append,
        help = "Activate one or more profiles defined in devenv.nix",
        long_help = "Activate one or more profiles defined in devenv.nix.\n\nProfiles allow you to define different configurations that can be merged with your base configuration.\n\nSee https://devenv.sh/profiles for more information.\n\nExamples:\n  --profile python-3.14\n  --profile backend --profile fast-startup"
    )]
    pub profile: Vec<String>,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        let defaults = NixBuildDefaults::compute();
        Self {
            version: false,
            verbose: false,
            quiet: false,
            log_format: LogFormat::default(),
            trace_export_file: None,
            max_jobs: defaults.max_jobs,
            cores: defaults.cores,
            system: default_system(),
            impure: false,
            eval_cache: true,
            no_eval_cache: false,
            refresh_eval_cache: false,
            offline: false,
            clean: None,
            nix_debugger: false,
            nix_option: vec![],
            override_input: vec![],
            option: vec![],
            profile: vec![],
        }
    }
}

impl GlobalOptions {
    /// Resolve conflicting options.
    // TODO: https://github.com/clap-rs/clap/issues/815
    pub fn resolve_overrides(&mut self) {
        if self.no_eval_cache {
            self.eval_cache = false;
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
    Mcp {},
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
    Copy { name: String },

    #[command(about = "Run a container.")]
    Run { name: String },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NixBuildDefaults {
    pub max_jobs: u8,
    pub cores: u8,
}

impl NixBuildDefaults {
    pub fn compute() -> Self {
        let total_cores = std::thread::available_parallelism()
            .unwrap_or_else(|e| {
                error!("Failed to get number of logical CPUs: {}", e);
                4.try_into().unwrap()
            })
            .get();

        let max_jobs = (total_cores / 4).max(1);
        let cores = (total_cores / max_jobs).max(1);

        Self {
            max_jobs: max_jobs as u8,
            cores: cores as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, NixBuildDefaults};

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert()
    }

    #[test]
    fn test_nix_build_defaults_low_cores() {
        // 1 core: containers or minimal VMs
        assert_eq!(
            compute_with_cores(1),
            NixBuildDefaults {
                max_jobs: 1,
                cores: 1
            }
        );
        // 2 cores: small VMs or older machines
        assert_eq!(
            compute_with_cores(2),
            NixBuildDefaults {
                max_jobs: 1,
                cores: 2
            }
        );
        // 4 cores: older laptops or small VMs
        assert_eq!(
            compute_with_cores(4),
            NixBuildDefaults {
                max_jobs: 1,
                cores: 4
            }
        );
    }

    #[test]
    fn test_nix_build_defaults_medium_cores() {
        // 8 cores: typical modern machines
        assert_eq!(
            compute_with_cores(8),
            NixBuildDefaults {
                max_jobs: 2,
                cores: 4
            }
        );
        // 10 cores: common on ARM-based laptops
        assert_eq!(
            compute_with_cores(10),
            NixBuildDefaults {
                max_jobs: 2,
                cores: 5
            }
        );
        // 12 cores: performance machines
        assert_eq!(
            compute_with_cores(12),
            NixBuildDefaults {
                max_jobs: 3,
                cores: 4
            }
        );
        // 16 cores: high-end machines and workstations
        assert_eq!(
            compute_with_cores(16),
            NixBuildDefaults {
                max_jobs: 4,
                cores: 4
            }
        );
    }

    #[test]
    fn test_nix_build_defaults_high_cores() {
        // 32 cores: workstations and servers
        assert_eq!(
            compute_with_cores(32),
            NixBuildDefaults {
                max_jobs: 8,
                cores: 4
            }
        );
        // 64 cores: high-end workstations and servers
        assert_eq!(
            compute_with_cores(64),
            NixBuildDefaults {
                max_jobs: 16,
                cores: 4
            }
        );
        // 128 cores: servers and cloud instances
        assert_eq!(
            compute_with_cores(128),
            NixBuildDefaults {
                max_jobs: 32,
                cores: 4
            }
        );
    }

    #[test]
    fn test_nix_build_defaults_full_utilization() {
        for cores in [1, 2, 4, 8, 10, 12, 16, 20, 32, 64, 128] {
            let defaults = compute_with_cores(cores);
            assert_eq!(
                defaults.max_jobs as usize * defaults.cores as usize,
                cores,
                "Full utilization check failed for {} cores: got max_jobs={}, cores={}",
                cores,
                defaults.max_jobs,
                defaults.cores
            );
        }
    }

    fn compute_with_cores(total_cores: usize) -> NixBuildDefaults {
        let max_jobs = (total_cores / 4).max(1);
        let cores = (total_cores / max_jobs).max(1);
        NixBuildDefaults {
            max_jobs: max_jobs as u8,
            cores: cores as u8,
        }
    }
}
