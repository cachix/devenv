use clap::{Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use devenv_core::config::NixBackendType;
use devenv_core::settings::{
    CacheOptions, InputOverrides, NixOptions, SecretOptions, ShellOptions, flag,
};
use devenv_tasks::RunMode;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fmt, fs};
use url::Url;

/// Rendering format for tracing-subscriber layers writing to a `TraceSink`.
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

impl fmt::Display for TraceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("full"),
            Self::Json => f.write_str("json"),
            Self::Pretty => f.write_str("pretty"),
        }
    }
}

impl FromStr for TraceFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "full" => Ok(Self::Full),
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            _ => Err(()),
        }
    }
}

/// OpenTelemetry OTLP wire protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
    HttpJson,
}

impl fmt::Display for OtlpProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grpc => f.write_str("otlp-grpc"),
            Self::HttpProtobuf => f.write_str("otlp-http-protobuf"),
            Self::HttpJson => f.write_str("otlp-http-json"),
        }
    }
}

impl FromStr for OtlpProtocol {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "otlp-grpc" => Ok(Self::Grpc),
            "otlp-http-protobuf" => Ok(Self::HttpProtobuf),
            "otlp-http-json" => Ok(Self::HttpJson),
            _ => Err(()),
        }
    }
}

impl OtlpProtocol {
    /// Default endpoint when none is specified in the spec.
    pub fn default_endpoint(&self) -> Url {
        let s = match self {
            Self::Grpc => "http://localhost:4317",
            Self::HttpProtobuf | Self::HttpJson => "http://localhost:4318",
        };
        s.parse()
            .expect("hard-coded OTLP default endpoint must parse")
    }
}

/// A byte sink for rendered trace output.
///
/// Accepts the following syntax:
/// - `stdout` - write to standard output
/// - `stderr` - write to standard error
/// - `file:/path/to/file` - write to the specified file path
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TraceSink {
    #[default]
    Stderr,
    Stdout,
    File(PathBuf),
}

impl fmt::Display for TraceSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceSink::Stdout => f.write_str("stdout"),
            TraceSink::Stderr => f.write_str("stderr"),
            TraceSink::File(p) => write!(f, "file:{}", p.display()),
        }
    }
}

impl FromStr for TraceSink {
    type Err = ParseTraceOutputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stderr" => Ok(TraceSink::Stderr),
            "stdout" => Ok(TraceSink::Stdout),
            s if s.starts_with("file:") => Ok(TraceSink::File(PathBuf::from(&s[5..]))),
            _ => Err(ParseTraceOutputError::UnsupportedFormat(s.to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseTraceOutputError {
    #[error("unsupported trace output format '{0}', expected 'stdout', 'stderr', or 'file:<path>'")]
    UnsupportedFormat(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("bare format name '{0}' is not a destination; use '{0}:<destination>'")]
    BareFormatName(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TracingArgsError {
    #[error("DEVENV_TRACE_TO: {0}")]
    EnvParse(#[source] ParseTraceOutputError),
    #[error("duplicate trace destination '{spec}' (would interleave output)")]
    DuplicateDestination { spec: TraceOutputSpec },
}

/// A trace output specification.
///
/// Parsed from `[format:]destination` syntax. When format is omitted, defaults to JSON.
/// The two variants reflect different tracing subsystems:
/// - `Render`: tracing-subscriber Layer that writes formatted text to a `TraceSink`.
/// - `Otlp`: OpenTelemetry exporter sending spans over a wire protocol to a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceOutputSpec {
    Render(TraceFormat, TraceSink),
    Otlp(OtlpProtocol, Url),
}

impl TraceOutputSpec {
    /// Returns true if this spec writes to a terminal (stdout/stderr).
    pub fn targets_terminal(&self) -> bool {
        matches!(self, Self::Render(_, TraceSink::Stdout | TraceSink::Stderr))
    }

    /// Returns true if two specs would write to the same destination
    /// (and therefore produce interleaved output).
    fn same_destination(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Render(_, a), Self::Render(_, b)) => a == b,
            (Self::Otlp(_, a), Self::Otlp(_, b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for TraceOutputSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(format, sink) => write!(f, "{format}:{sink}"),
            Self::Otlp(proto, url) => write!(f, "{proto}:{url}"),
        }
    }
}

impl FromStr for TraceOutputSpec {
    type Err = ParseTraceOutputError;

    /// Parse `[format:]destination`.
    ///
    /// Dispatch is "try each kind in turn": OTLP protocol → render format → bare sink.
    /// Format names never collide with sink prefixes (`file:`, `stdout`, `stderr`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (prefix, rest) = match s.split_once(':') {
            Some((p, r)) => (p, r),
            None => (s, ""),
        };

        if let Ok(proto) = prefix.parse::<OtlpProtocol>() {
            let url = if rest.is_empty() {
                proto.default_endpoint()
            } else {
                rest.parse::<Url>()
                    .map_err(|e| ParseTraceOutputError::InvalidUrl(e.to_string()))?
            };
            return Ok(Self::Otlp(proto, url));
        }

        if let Ok(format) = prefix.parse::<TraceFormat>() {
            if rest.is_empty() {
                return Err(ParseTraceOutputError::BareFormatName(prefix.to_string()));
            }
            return Ok(Self::Render(format, rest.parse()?));
        }

        // Bare destination (no recognized format prefix) — default to JSON.
        Ok(Self::Render(TraceFormat::Json, s.parse()?))
    }
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

    #[arg(long = "nix-option", global = true, num_args = 2,
        value_names = ["NAME", "VALUE"],
        value_delimiter = ' ',
        help = "Pass additional options to nix commands",
        long_help = "Pass additional options to nix commands.\n\nThese options are passed directly to Nix using the --option flag.\nSee `man nix.conf` for the full list of available options.\n\nExamples:\n  --nix-option sandbox false\n  --nix-option keep-outputs true\n  --nix-option system x86_64-darwin")]
    pub nix_options: Vec<String>,

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
            nix_options: cli.nix_options,
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

    #[arg(short = 'P', long = "profile", global = true,
        num_args = 1,
        action = clap::ArgAction::Append,
        help = "Activate one or more profiles defined in devenv.nix",
        long_help = "Activate one or more profiles defined in devenv.nix.\n\nProfiles allow you to define different configurations that can be merged with your base configuration.\n\nSee https://devenv.sh/profiles for more information.\n\nExamples:\n  --profile python-3.14\n  --profile backend --profile fast-startup")]
    pub profiles: Vec<String>,

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

    #[arg(
        long = "shell",
        global = true,
        env = "DEVENV_SHELL_TYPE",
        value_parser = clap::builder::PossibleValuesParser::new(["bash", "zsh", "fish", "nu"]),
        help = "Shell to use for interactive sessions (bash, zsh, fish, nu)."
    )]
    pub shell_type: Option<String>,
}

impl From<ShellCliArgs> for ShellOptions {
    fn from(cli: ShellCliArgs) -> Self {
        Self {
            clean: cli.clean,
            profiles: cli.profiles,
            reload: flag(cli.reload, cli.no_reload),
            shell: cli.shell_type,
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
    #[arg(short, long = "override-input", global = true,
        num_args = 2,
        value_names = ["NAME", "URI"],
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml",
        long_help = "Override inputs in devenv.yaml.\n\nExamples:\n  --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable\n  --override-input nixpkgs path:/path/to/local/nixpkgs")]
    pub override_inputs: Vec<String>,

    #[arg(long = "option", short = 'O', global = true,
        num_args = 2,
        value_names = ["OPTION:TYPE", "VALUE"],
        help = "Override configuration options with typed values",
        long_help = "Override configuration options with typed values.\n\nOPTION must include a type: <attribute>:<type>\nSupported types: string, int, float, bool, path, pkg, pkgs\n\nList types (pkgs) append to existing values by default.\nAdd a ! suffix to replace instead: pkgs!\n\nExamples:\n  --option languages.rust.channel:string beta\n  --option services.postgres.enable:bool true\n  --option languages.python.version:string 3.10\n  --option packages:pkgs \"ncdu git\"       (appends to packages)\n  --option packages:pkgs! \"ncdu git\"      (replaces all packages)")]
    pub nix_module_options: Vec<String>,
}

impl From<InputOverrideCliArgs> for InputOverrides {
    fn from(cli: InputOverrideCliArgs) -> Self {
        Self {
            override_inputs: cli.override_inputs,
            nix_module_options: cli.nix_module_options,
        }
    }
}

// --- CLI-only options ---

#[derive(clap::Args, Clone, Debug)]
#[command(next_help_heading = "Tracing options")]
pub struct TracingCliArgs {
    #[arg(
        long = "trace-to",
        global = true,
        action = clap::ArgAction::Append,
        help = "Enable tracing. Repeatable. Syntax: [format:]destination. [env: DEVENV_TRACE_TO=]",
        long_help = "Enable tracing and set output destination(s). Can be repeated for multiple outputs.\n\n\
            Syntax: [format:]destination\n\n\
            Examples:\n  \
            --trace-to stderr                              # json to stderr\n  \
            --trace-to pretty:stderr                       # pretty format to stderr\n  \
            --trace-to json:file:/tmp/trace.json           # JSON to file\n  \
            --trace-to otlp-grpc                           # OTLP gRPC to default endpoint\n  \
            --trace-to otlp-grpc:http://collector:4317     # OTLP gRPC to custom endpoint\n\n\
            Multiple outputs:\n  \
            --trace-to pretty:stderr --trace-to json:file:/tmp/t.json\n\n\
            Destinations: stdout, stderr, file:<path>, http(s)://<host>:<port>\n\
            Formats: json (default), pretty, full, otlp-grpc, otlp-http-protobuf, otlp-http-json\n\n\
            When format is omitted, defaults to json.\n\
            Tracing is disabled by default.\n\n\
            [env: DEVENV_TRACE_TO=] (comma-separated)"
    )]
    pub trace_to: Vec<TraceOutputSpec>,

    // Legacy flags — hidden, kept for backward compatibility.
    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_OUTPUT",
        hide = true,
        help = "Legacy: use --trace-to instead."
    )]
    pub trace_output: Option<TraceSink>,

    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_FORMAT",
        hide = true,
        help = "Legacy: use --trace-to instead.",
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
        value_parser = clap::builder::BoolishValueParser::new(),
        help = "Enable the interactive terminal interface (default when interactive)."
    )]
    pub tui: Option<bool>,

    #[arg(
        long,
        global = true,
        help = "Disable the interactive terminal interface."
    )]
    pub no_tui: bool,

    #[arg(short, long, action = clap::ArgAction::Help, global = true, help = "Print help (see a summary with '-h')")]
    pub help: Option<bool>,
}

impl TracingCliArgs {
    /// Parse `DEVENV_TRACE_TO` env var (comma-separated `[format:]destination` specs).
    fn specs_from_env() -> Result<Vec<TraceOutputSpec>, TracingArgsError> {
        let val = match env::var("DEVENV_TRACE_TO") {
            Ok(v) if !v.is_empty() => v,
            _ => return Ok(Vec::new()),
        };
        val.split(',')
            .map(|s| {
                s.trim()
                    .parse::<TraceOutputSpec>()
                    .map_err(TracingArgsError::EnvParse)
            })
            .collect()
    }

    /// Merge all trace sources: `DEVENV_TRACE_TO` env var, `--trace-to` CLI flags,
    /// and legacy `--trace-output`/`--trace-format`.
    ///
    /// No validation step — specs are valid by construction (the type system
    /// rules out incompatible format/destination combinations).
    pub fn resolve(&self) -> Result<Vec<TraceOutputSpec>, TracingArgsError> {
        let mut specs = Self::specs_from_env()?;

        // CLI --trace-to flags append after env var specs
        specs.extend(self.trace_to.iter().cloned());

        // Legacy --trace-output/--trace-format: local-only by type, no validation needed.
        if let Some(ref sink) = self.trace_output {
            specs.push(TraceOutputSpec::Render(self.trace_format, sink.clone()));
        }

        // Reject duplicate destinations (e.g. json:stderr + pretty:stderr).
        for (i, a) in specs.iter().enumerate() {
            for b in &specs[i + 1..] {
                if a.same_destination(b) {
                    return Err(TracingArgsError::DuplicateDestination { spec: a.clone() });
                }
            }
        }

        Ok(specs)
    }
}

/// Complete task names by reading from .devenv/task-names.txt cache file.
/// Walks up from current directory to find the project root.
/// If cache doesn't exist, spawns `devenv tasks list` in background to populate it.
fn complete_task_names(current: &OsStr) -> Vec<CompletionCandidate> {
    let current_str = current.to_str().unwrap_or("");

    // Walk up from current directory to find .devenv directory or devenv.nix/devenv.yaml
    let mut dir = env::current_dir().ok();
    while let Some(d) = dir {
        let cache_path = d.join(".devenv").join("task-names.txt");
        let is_devenv_project = d.join("devenv.nix").exists() || d.join("devenv.yaml").exists();

        if cache_path.exists() {
            if let Ok(content) = fs::read_to_string(&cache_path) {
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
    version = env!("DEVENV_VERSION_STRING"),
    color = clap::ColorChoice::Auto,
    disable_help_flag = true,
    arg_required_else_help = true,
    // for --clean to work with subcommands
    subcommand_precedence_over_arg = true,
    dont_delimit_trailing_values = true,
    about = format!("https://devenv.sh {}: Fast, Declarative, Reproducible, and Composable Developer Environments", env!("DEVENV_VERSION_STRING"))
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

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
    /// Parse from `env::args_os()` after merging `--profile <name>` / `-P <name>`
    /// into the `=` form, so a profile whose name shadows a subcommand
    /// (e.g. `devenv --profile test test`) isn't mistaken for the subcommand by
    /// clap's `subcommand_precedence_over_arg`.
    pub fn parse_preprocessed() -> Self {
        Self::parse_from(preprocess_profile_args(env::args_os()))
    }
}

/// Merge `--profile X` → `--profile=X` and `-P X` → `-PX` so clap's
/// `subcommand_precedence_over_arg` doesn't steal the value when it matches a
/// subcommand name. See https://github.com/cachix/devenv/issues/2821.
fn preprocess_profile_args<I>(args: I) -> Vec<std::ffi::OsString>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let mut out = Vec::new();
    let mut iter = args.into_iter().peekable();
    while let Some(arg) = iter.next() {
        let kind = match arg.to_str() {
            Some("--profile") => Some(true), // long form, use `=`
            Some("-P") => Some(false),       // short form, no separator
            _ => None,
        };
        match kind {
            Some(use_equals) if iter.peek().is_some_and(|n| !is_flag(n)) => {
                let value = iter.next().expect("peeked");
                let mut merged = arg;
                if use_equals {
                    merged.push("=");
                }
                merged.push(&value);
                out.push(merged);
            }
            _ => out.push(arg),
        }
    }
    out
}

fn is_flag(arg: &std::ffi::OsString) -> bool {
    arg.to_str().is_some_and(|s| s.starts_with('-') && s != "-")
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
        #[command(flatten)]
        up_args: UpArgs,
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
        #[arg(
            long,
            help = "Override .devenv with a temporary directory for isolation."
        )]
        override_dotfile: bool,
        #[arg(
            long,
            help = "Deprecated: no-op flag kept for backward compatibility.",
            hide = true
        )]
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

    #[clap(hide = true)]
    GenerateYamlOptionsDoc,

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

    #[cfg(feature = "lsp")]
    #[command(about = "Start the nixd language server for devenv.nix.")]
    Lsp {
        #[arg(long, help = "Print nixd configuration and exit")]
        print_config: bool,
    },

    #[command(
        about = "Print shell hook for auto-activation on directory change.",
        long_about = "Print shell hook for auto-activation on directory change.\n\nAdd to your shell config:\n\n  bash:    eval \"$(devenv hook bash)\"     # in ~/.bashrc\n  zsh:     eval \"$(devenv hook zsh)\"      # in ~/.zshrc\n  fish:    devenv hook fish | source       # in ~/.config/fish/config.fish\n  nushell: see devenv hook nu              # in config.nu"
    )]
    Hook {
        #[arg(value_enum)]
        shell: HookShell,
    },

    #[command(about = "Allow auto-activation for the current directory.")]
    Allow,

    #[command(about = "Revoke auto-activation for the current directory.")]
    Revoke,

    /// Internal: check if hook should activate devenv in current directory
    #[clap(hide = true)]
    HookShouldActivate,

    /// Internal: run native process manager as a daemon (used by `devenv up -d`)
    #[clap(hide = true)]
    DaemonProcesses {
        /// Path to the serialized task config JSON file
        config_file: PathBuf,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HookShell {
    Bash,
    Zsh,
    Fish,
    Nu,
}

#[derive(clap::Args, Clone, Debug)]
pub struct UpArgs {
    #[arg(help = "Start a specific process(es).")]
    pub processes: Vec<String>,

    #[arg(short, long, help = "Start processes in the background.")]
    pub detach: bool,

    #[arg(
        short,
        long,
        help = "The execution mode for process tasks (affects dependency resolution)",
        value_enum,
        default_value_t = RunMode::Before
    )]
    pub mode: RunMode,

    #[arg(
        long,
        help = "Error if a port is already in use instead of auto-allocating the next available port."
    )]
    pub strict_ports: bool,

    #[arg(
        long,
        help = "Disable strict port mode, overriding strict_ports from devenv.yaml."
    )]
    pub no_strict_ports: bool,
}

#[derive(Subcommand, Clone)]
#[clap(about = "Start or stop processes. https://devenv.sh/processes/")]
pub enum ProcessesCommand {
    #[command(about = "Start processes in the foreground.")]
    Up {
        #[command(flatten)]
        up_args: UpArgs,
    },

    #[command(about = "Stop processes running in the background.")]
    Down {},

    #[command(about = "Wait for all processes to be ready.")]
    Wait {
        #[arg(long, default_value = "120", help = "Timeout in seconds.")]
        timeout: u64,
    },

    #[command(about = "List all managed processes and their status.")]
    List {},

    #[command(about = "Get the status of a process.")]
    Status {
        #[arg(help = "Name of the process.")]
        name: String,
    },

    #[command(about = "Get logs for a process.")]
    Logs {
        #[arg(help = "Name of the process.")]
        name: String,

        #[arg(
            short = 'n',
            long,
            default_value = "100",
            help = "Number of lines to show."
        )]
        lines: usize,

        #[arg(long, conflicts_with = "stderr", help = "Show only stdout.")]
        stdout: bool,

        #[arg(long, conflicts_with = "stdout", help = "Show only stderr.")]
        stderr: bool,
    },

    #[command(about = "Restart a process.")]
    Restart {
        #[arg(help = "Name of the process.")]
        name: String,
    },

    #[command(about = "Start a process (or all processes if no name given).")]
    Start {
        #[arg(help = "Name of the process. If omitted, starts all processes (same as 'up').")]
        name: Option<String>,

        #[arg(short, long, help = "Start processes in the background.")]
        detach: bool,
    },

    #[command(about = "Stop a running process (or all processes if no name given).")]
    Stop {
        #[arg(help = "Name of the process. If omitted, stops all processes (same as 'down').")]
        name: Option<String>,
    },
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
            default_value_t = RunMode::Before
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
    use super::*;
    use clap::{Parser, crate_version};

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert()
    }

    #[test]
    fn trace_output_spec_bare_destination_defaults_to_json() {
        let spec: TraceOutputSpec = "stderr".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Render(TraceFormat::Json, TraceSink::Stderr)
        );
    }

    #[test]
    fn trace_output_spec_format_prefix() {
        let spec: TraceOutputSpec = "pretty:stderr".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
    }

    #[test]
    fn trace_output_spec_json_file() {
        let spec: TraceOutputSpec = "json:file:/tmp/trace.json".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Render(
                TraceFormat::Json,
                TraceSink::File(PathBuf::from("/tmp/trace.json"))
            )
        );
    }

    #[test]
    fn trace_output_spec_bare_file_destination() {
        let spec: TraceOutputSpec = "file:/tmp/trace.json".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Render(
                TraceFormat::Json,
                TraceSink::File(PathBuf::from("/tmp/trace.json"))
            )
        );
    }

    #[test]
    fn trace_output_spec_bare_otlp() {
        let spec: TraceOutputSpec = "otlp-grpc".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Otlp(OtlpProtocol::Grpc, "http://localhost:4317".parse().unwrap())
        );
    }

    #[test]
    fn trace_output_spec_otlp_with_url() {
        let spec: TraceOutputSpec = "otlp-grpc:http://collector:4317".parse().unwrap();
        assert_eq!(
            spec,
            TraceOutputSpec::Otlp(OtlpProtocol::Grpc, "http://collector:4317".parse().unwrap())
        );
    }

    #[test]
    fn trace_output_spec_bare_format_name_errors() {
        let result: Result<TraceOutputSpec, _> = "json".parse();
        assert!(result.is_err());
    }

    #[test]
    fn trace_to_bare_defaults_to_json() {
        let args = TracingCliArgs {
            trace_to: vec!["stderr".parse().unwrap()],
            trace_output: None,
            trace_format: TraceFormat::Pretty, // should NOT affect --trace-to
        };
        let specs = args.resolve().unwrap();
        assert_eq!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Json, TraceSink::Stderr)
        );
    }

    #[test]
    fn trace_to_preserves_explicit_format() {
        let args = TracingCliArgs {
            trace_to: vec!["pretty:stderr".parse().unwrap()],
            trace_output: None,
            trace_format: TraceFormat::Json,
        };
        let specs = args.resolve().unwrap();
        assert_eq!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
    }

    #[test]
    fn legacy_trace_output_uses_trace_format() {
        let args = TracingCliArgs {
            trace_to: vec![],
            trace_output: Some(TraceSink::Stderr),
            trace_format: TraceFormat::Pretty,
        };
        let specs = args.resolve().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
    }

    #[test]
    fn legacy_and_new_merge_different_destinations() {
        let args = TracingCliArgs {
            trace_to: vec!["json:file:/tmp/t.json".parse().unwrap()],
            trace_output: Some(TraceSink::Stderr),
            trace_format: TraceFormat::Pretty,
        };
        let specs = args.resolve().unwrap();
        assert_eq!(specs.len(), 2);
        assert!(matches!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Json, _)
        ));
        assert!(matches!(
            specs[1],
            TraceOutputSpec::Render(TraceFormat::Pretty, _)
        ));
    }

    #[test]
    fn duplicate_destination_rejected() {
        let args = TracingCliArgs {
            trace_to: vec![
                "json:stderr".parse().unwrap(),
                "pretty:stderr".parse().unwrap(),
            ],
            trace_output: None,
            trace_format: TraceFormat::Json,
        };
        let err = args.resolve().unwrap_err();
        assert!(
            matches!(err, TracingArgsError::DuplicateDestination { .. }),
            "{err}"
        );
    }

    #[test]
    fn duplicate_destination_legacy_and_new() {
        let args = TracingCliArgs {
            trace_to: vec!["json:stderr".parse().unwrap()],
            trace_output: Some(TraceSink::Stderr),
            trace_format: TraceFormat::Pretty,
        };
        let err = args.resolve().unwrap_err();
        assert!(
            matches!(err, TracingArgsError::DuplicateDestination { .. }),
            "{err}"
        );
    }

    #[test]
    fn trace_to_multiple_from_cli() {
        let cli = Cli::parse_from([
            "devenv",
            "--trace-to",
            "json:file:/tmp/trace.json",
            "--trace-to",
            "pretty:stderr",
            "shell",
        ]);
        let specs = cli.tracing_args.resolve().unwrap();
        assert_eq!(specs.len(), 2);
        assert!(matches!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Json, TraceSink::File(_))
        ));
        assert_eq!(
            specs[1],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
    }

    #[test]
    fn legacy_trace_output_from_cli() {
        let cli = Cli::parse_from([
            "devenv",
            "--trace-output",
            "stderr",
            "--trace-format",
            "pretty",
            "shell",
        ]);
        let specs = cli.tracing_args.resolve().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
    }

    #[test]
    fn parse_comma_separated_trace_specs() {
        let specs: Vec<TraceOutputSpec> = "pretty:stderr,json:file:/tmp/t.json"
            .split(',')
            .map(|s| s.trim().parse().unwrap())
            .collect();
        assert_eq!(specs.len(), 2);
        assert_eq!(
            specs[0],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        );
        assert_eq!(
            specs[1],
            TraceOutputSpec::Render(
                TraceFormat::Json,
                TraceSink::File(PathBuf::from("/tmp/t.json"))
            )
        );
    }

    #[test]
    fn parse_comma_separated_with_otlp() {
        let specs: Vec<TraceOutputSpec> = "otlp-grpc,pretty:stderr"
            .split(',')
            .map(|s| s.trim().parse().unwrap())
            .collect();
        assert_eq!(specs.len(), 2);
        assert!(matches!(
            specs[0],
            TraceOutputSpec::Otlp(OtlpProtocol::Grpc, _)
        ));
        assert!(matches!(
            specs[1],
            TraceOutputSpec::Render(TraceFormat::Pretty, TraceSink::Stderr)
        ));
    }

    #[test]
    fn version_flag_short_circuits_subcommand_requirement() {
        // `devenv --version` must work without a subcommand even though
        // `arg_required_else_help = true` is set on the top-level command.
        // https://github.com/cachix/devenv/issues/2791
        for flag in ["--version", "-V"] {
            let err = match Cli::try_parse_from(["devenv", flag]) {
                Ok(_) => panic!("expected --version/-V to short-circuit, but parsing succeeded"),
                Err(e) => e,
            };
            assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
            assert!(
                err.to_string().contains(crate_version!()),
                "expected version output to contain crate version, got: {err}"
            );
        }
    }

    #[test]
    fn up_accepts_no_strict_ports() {
        let cli = Cli::parse_from(["devenv", "up", "--no-strict-ports"]);

        match cli.command {
            Commands::Up { up_args } => {
                assert!(!up_args.strict_ports);
                assert!(up_args.no_strict_ports);
            }
            _ => panic!("expected `devenv up` command"),
        }
    }

    #[test]
    fn processes_up_accepts_no_strict_ports() {
        let cli = Cli::parse_from(["devenv", "processes", "up", "--no-strict-ports"]);

        match cli.command {
            Commands::Processes {
                command: ProcessesCommand::Up { up_args },
            } => {
                assert!(!up_args.strict_ports);
                assert!(up_args.no_strict_ports);
            }
            _ => panic!("expected `devenv processes up` command"),
        }
    }

    fn osargs<const N: usize>(args: [&str; N]) -> Vec<std::ffi::OsString> {
        args.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn preprocess_profile_long_form_with_subcommand_name() {
        // https://github.com/cachix/devenv/issues/2821
        let out = preprocess_profile_args(osargs(["devenv", "--profile", "test", "test"]));
        assert_eq!(out, osargs(["devenv", "--profile=test", "test"]));
    }

    #[test]
    fn preprocess_profile_short_form_with_subcommand_name() {
        let out = preprocess_profile_args(osargs(["devenv", "-P", "test", "test"]));
        assert_eq!(out, osargs(["devenv", "-Ptest", "test"]));
    }

    #[test]
    fn preprocess_profile_already_uses_equals() {
        let out = preprocess_profile_args(osargs(["devenv", "--profile=test", "test"]));
        assert_eq!(out, osargs(["devenv", "--profile=test", "test"]));
    }

    #[test]
    fn preprocess_profile_followed_by_flag_is_untouched() {
        // Don't swallow the following flag as a value.
        let out = preprocess_profile_args(osargs(["devenv", "--profile", "--verbose"]));
        assert_eq!(out, osargs(["devenv", "--profile", "--verbose"]));
    }

    #[test]
    fn preprocess_profile_at_end_is_untouched() {
        let out = preprocess_profile_args(osargs(["devenv", "--profile"]));
        assert_eq!(out, osargs(["devenv", "--profile"]));
    }

    #[test]
    fn preprocess_handles_multiple_profile_flags() {
        let out = preprocess_profile_args(osargs(["devenv", "--profile", "a", "-P", "b", "shell"]));
        assert_eq!(out, osargs(["devenv", "--profile=a", "-Pb", "shell"]));
    }

    #[test]
    fn cli_profile_before_subcommand_shadowing_name() {
        // https://github.com/cachix/devenv/issues/2821
        let argv = preprocess_profile_args(osargs(["devenv", "--profile", "test", "test"]));
        let cli = Cli::parse_from(argv);
        assert_eq!(cli.shell_args.profiles, vec!["test".to_string()]);
        assert!(matches!(cli.command, Commands::Test { .. }));
    }

    #[test]
    fn cli_profile_short_before_subcommand_shadowing_name() {
        let argv = preprocess_profile_args(osargs(["devenv", "-P", "test", "test"]));
        let cli = Cli::parse_from(argv);
        assert_eq!(cli.shell_args.profiles, vec!["test".to_string()]);
        assert!(matches!(cli.command, Commands::Test { .. }));
    }
}
