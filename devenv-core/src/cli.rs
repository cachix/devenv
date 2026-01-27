//! CLI-related types and utilities for devenv

use clap::Parser;
use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::str::FromStr;
use tracing::error;

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceFormat {
    /// A verbose structured log format used for debugging (default).
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
        env = "DEVENV_TUI",
        help = "Enable the interactive terminal interface.",
        default_value_t = true,
        overrides_with = "no_tui"
    )]
    pub tui: bool,

    #[arg(
        long,
        global = true,
        help = "Disable the interactive terminal interface.",
        overrides_with = "tui"
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
        env = "DEVENV_TRACE_FORMAT",
        help = "Configure the output format of traces.",
        default_value_t,
        value_enum
    )]
    pub trace_format: TraceFormat,

    #[arg(
        long,
        global = true,
        env = "DEVENV_TRACE_OUTPUT",
        help = "Where to export traces (stdout, stderr, or file path).",
        hide = true
    )]
    pub trace_output: Option<TraceOutput>,

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

    #[arg(
        long,
        global = true,
        help = "Source for devenv.nix (flake input reference or path)",
        long_help = "Source for devenv.nix.\n\nCan be either a filesystem path or a flake input reference.\n\nExamples:\n  --from myinput\n  --from myinput/subdir\n  --from /absolute/path/to/project"
    )]
    pub from: Option<String>,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        let defaults = NixBuildDefaults::compute();
        Self {
            version: false,
            verbose: false,
            quiet: false,
            tui: true,
            no_tui: false,
            log_format: None,
            trace_format: TraceFormat::default(),
            trace_output: None,
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
            from: None,
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

        if self.no_tui {
            self.tui = false;
        }

        // Disable TUI in CI environments or when not running in a TTY
        if self.tui {
            let is_ci = env::var("CI")
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false);
            let is_tty = io::stdin().is_terminal() && io::stderr().is_terminal();
            if is_ci || !is_tty {
                self.tui = false;
            }
        }

        // Handle deprecated --log-format (except Cli which is handled separately)
        if let Some(format) = self.log_format {
            eprintln!("Warning: --log-format is deprecated, use --trace-format instead");
            if let Ok(trace_format) = format.try_into() {
                self.trace_format = trace_format;
            }
        }
    }

    /// Returns true if tracing-only mode should be used.
    ///
    /// Tracing mode is used when trace_output is Stdout or Stderr,
    /// as these would conflict with TUI or legacy CLI output.
    pub fn use_tracing_mode(&self) -> bool {
        matches!(
            self.trace_output,
            Some(TraceOutput::Stdout) | Some(TraceOutput::Stderr)
        )
    }

    /// Returns true if legacy CLI mode should be used (instead of TUI).
    ///
    /// Legacy CLI mode is used when:
    /// - `--no-tui` is passed
    /// - `--log-format cli` is passed (deprecated)
    pub fn use_legacy_cli(&self) -> bool {
        !self.tui || self.log_format == Some(LegacyLogFormat::Cli)
    }
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
