mod pty;

use clap::Parser;
use devenv::activity::ActivityLevel;
use devenv::{
    Config, Devenv, DevenvOptions, NixSettings, SecretOptions, SecretSettings, VerbosityLevel,
    activity as devenv_activity, console as devenv_console, tracing as devenv_tracing,
};
use devenv_mailbox::FrontendCommand;
use globset::{Glob, GlobSet, GlobSetBuilder};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const ALL_SYSTEMS: &[&str] = &[
    "x86_64-linux",
    "aarch64-linux",
    "x86_64-darwin",
    "aarch64-darwin",
];
const DEFAULT_DIRECTORIES: &[&str] = &["examples", "tests"];

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Run tests
    #[clap(name = "run")]
    Run(RunArgs),
    /// Generate JSON metadata for tests
    #[clap(name = "generate-json")]
    GenerateJson(GenerateJsonArgs),
    /// Run a command on a fresh PTY, relaying stdin and capturing output
    #[clap(name = "pty")]
    Pty(PtyArgs),
}

#[derive(Parser, Debug)]
struct RunArgs {
    #[clap(
        long,
        value_parser,
        help = "Exclude tests matching these glob patterns (e.g. 'python-*')."
    )]
    exclude: Vec<String>,

    #[clap(
        long,
        value_parser,
        help = "Only run tests matching these glob patterns (e.g. 'python-*')."
    )]
    only: Vec<String>,

    #[clap(
        short,
        long = "override-input",
        number_of_values = 2,
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml."
    )]
    override_inputs: Vec<String>,

    #[clap(value_parser, default_values = DEFAULT_DIRECTORIES)]
    directories: Vec<PathBuf>,
}

#[derive(Parser, Debug)]
struct PtyArgs {
    #[clap(value_parser, help = "File to capture PTY output to")]
    transcript: PathBuf,

    #[clap(value_parser, help = "Shell command to run on the PTY")]
    command: String,

    #[clap(
        long,
        default_value_t = 30,
        help = "Seconds allowed per expect: directive and for the final drain (exit 124)"
    )]
    step_timeout: u64,
}

#[derive(Parser, Debug)]
struct GenerateJsonArgs {
    #[clap(value_parser, default_values = DEFAULT_DIRECTORIES)]
    directories: Vec<PathBuf>,

    #[clap(long, help = "Include all tests regardless of current system support")]
    all: bool,

    #[clap(
        long,
        conflicts_with = "all",
        help = "Filter tests for this system instead of the current system (e.g. aarch64-darwin)"
    )]
    system: Option<String>,
}

enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

struct TestResult {
    name: String,
    status: TestStatus,
    duration: Option<Duration>,
    /// Closure size of the shell the test built, in bytes.
    closure_size: Option<u64>,
    /// Extra context shown next to the status, e.g. why a test failed.
    note: Option<String>,
}

impl TestResult {
    fn skipped(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: TestStatus::Skipped,
            duration: None,
            closure_size: None,
            note: None,
        }
    }
}

#[derive(Serialize, Debug)]
struct TestMetadata {
    name: String,
    path: String,
    supported_systems: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct TestConfig {
    /// Whether to initialize a git repository for the test
    #[serde(default = "default_git_init")]
    git_init: bool,
    /// Whether to run .test.sh inside the shell automatically (default: true)
    #[serde(default = "default_use_shell")]
    use_shell: bool,
    /// Systems that this test supports (empty means all systems supported)
    #[serde(default)]
    supported_systems: Vec<String>,
    /// Systems where this test is known to be broken (empty means no broken systems)
    #[serde(default)]
    broken_systems: Vec<String>,
    /// Whether to run the test in a temporary directory (default: true)
    #[serde(default = "default_use_tmp_dir")]
    use_tmp_dir: bool,
    /// Fail the test if the shell's closure is larger than this (e.g. "500 MB", "1.5 GiB")
    #[serde(default, deserialize_with = "deserialize_size")]
    max_closure_size: Option<u64>,
}

/// Accept a byte count or a string with a unit (`B`, `KB`, `MB`, `GB`, `TB`, `KiB`, `MiB`, `GiB`, `TiB`).
fn deserialize_size<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Size {
        Bytes(u64),
        Text(String),
    }

    match Option::<Size>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Size::Bytes(bytes)) => Ok(Some(bytes)),
        Some(Size::Text(text)) => parse_size(&text)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn parse_size(text: &str) -> std::result::Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number
        .parse()
        .map_err(|_| format!("invalid size '{text}': expected a number followed by a unit"))?;
    let multiplier: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1000,
        "mb" => 1000_u64.pow(2),
        "gb" => 1000_u64.pow(3),
        "tb" => 1000_u64.pow(4),
        "kib" => 1024,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        other => return Err(format!("invalid size '{text}': unknown unit '{other}'")),
    };
    Ok((number * multiplier as f64) as u64)
}

fn format_size(bytes: u64) -> String {
    const GB: f64 = 1e9;
    const MB: f64 = 1e6;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs >= 60.0 {
        format!("{}m{:02}s", (secs / 60.0) as u64, (secs % 60.0) as u64)
    } else {
        format!("{secs:.1}s")
    }
}

/// Closure size in bytes of the shell behind the `shell` GC root, or `None` if no shell was built.
async fn shell_closure_size(dot_gc: &Path) -> Result<Option<u64>> {
    let gc_root = dot_gc.join("shell");
    let store_path = match tokio::fs::canonicalize(&gc_root).await {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(miette::miette!(
                "Failed to resolve shell GC root {}: {e}",
                gc_root.display()
            ));
        }
    };
    let output = tokio::process::Command::new("nix")
        .args(["--extra-experimental-features", "nix-command"])
        .args(["path-info", "--closure-size"])
        .arg(&store_path)
        .stdin(Stdio::null())
        .output()
        .await
        .into_diagnostic()
        .wrap_err("Failed to run `nix path-info --closure-size`")?;
    if !output.status.success() {
        return Err(miette::miette!(
            "`nix path-info --closure-size {}` failed: {}",
            store_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().last())
        .and_then(|size| size.parse().ok())
        .map(Some)
        .ok_or_else(|| miette::miette!("Unexpected `nix path-info` output: {stdout}"))
}

fn default_git_init() -> bool {
    true
}

fn default_use_shell() -> bool {
    true
}

fn default_use_tmp_dir() -> bool {
    true
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            git_init: default_git_init(),
            use_shell: default_use_shell(),
            supported_systems: Vec::new(),
            broken_systems: Vec::new(),
            use_tmp_dir: default_use_tmp_dir(),
            max_closure_size: None,
        }
    }
}

impl TestConfig {
    fn load_from_path(path: &std::path::Path) -> Result<Self> {
        // Try different config file extensions
        let config_paths = [
            path.join(".test-config.yml"),
            path.join(".test-config.yaml"),
        ];

        for config_path in &config_paths {
            if config_path.exists() {
                let content = fs::read_to_string(config_path)
                    .into_diagnostic()
                    .wrap_err("Failed to read .test-config file")?;
                return serde_yaml::from_str(&content)
                    .into_diagnostic()
                    .wrap_err("Failed to parse .test-config YAML");
            }
        }

        Ok(Self::default())
    }

    fn should_skip_for_system(&self, current_system: &str) -> bool {
        // Skip if the test explicitly lists broken systems and current system is broken
        if !self.broken_systems.is_empty()
            && self.broken_systems.contains(&current_system.to_string())
        {
            return true;
        }

        // Skip if the test lists supported systems and current system is not supported
        if !self.supported_systems.is_empty()
            && !self.supported_systems.contains(&current_system.to_string())
        {
            return true;
        }

        false
    }
}

fn get_current_system() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    match (arch, os) {
        ("x86_64", "linux") => "x86_64-linux".to_string(),
        ("aarch64", "linux") => "aarch64-linux".to_string(),
        ("x86_64", "macos") => "x86_64-darwin".to_string(),
        ("aarch64", "macos") => "aarch64-darwin".to_string(),
        _ => panic!("Unsupported system: {arch}-{os}"),
    }
}

fn get_supported_systems_for_config(test_config: &TestConfig) -> Vec<String> {
    if test_config.supported_systems.is_empty() && test_config.broken_systems.is_empty() {
        // If no systems specified, support all known systems
        ALL_SYSTEMS.iter().map(|s| s.to_string()).collect()
    } else if !test_config.supported_systems.is_empty() {
        // Use explicitly supported systems
        test_config.supported_systems.clone()
    } else {
        // Start with all systems, remove broken ones
        ALL_SYSTEMS
            .iter()
            .filter(|sys| !test_config.broken_systems.contains(&sys.to_string()))
            .map(|s| s.to_string())
            .collect()
    }
}

struct TestInfo {
    name: String,
    path: PathBuf,
    config: TestConfig,
    metadata: TestMetadata,
}

fn discover_tests(directories: &[PathBuf]) -> Result<Vec<TestInfo>> {
    let mut test_infos = vec![];

    for directory in directories {
        let paths = fs::read_dir(directory).into_diagnostic()?;

        for path in paths {
            let path = path.into_diagnostic()?.path();
            let path = path.as_path();

            // Skip files
            if !path.is_dir() {
                continue;
            }

            let Some(dir_name_path) = path.file_name() else {
                continue;
            };
            let Some(dir_name) = dir_name_path.to_str() else {
                eprintln!("Warning: skipping directory with non-UTF8 name: {dir_name_path:?}",);
                continue;
            };

            // Load test configuration
            let test_config = TestConfig::load_from_path(path)?;

            let supported_systems = get_supported_systems_for_config(&test_config);
            let metadata = TestMetadata {
                name: dir_name.to_string(),
                path: path.display().to_string(),
                supported_systems,
            };

            let test_info = TestInfo {
                name: dir_name.to_string(),
                path: path.to_path_buf(),
                config: test_config,
                metadata,
            };
            test_infos.push(test_info);
        }
    }

    // Sort tests by path for consistent ordering
    test_infos.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(test_infos)
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            Glob::new(pattern)
                .into_diagnostic()
                .wrap_err_with(|| format!("Invalid glob pattern: {pattern}"))?,
        );
    }
    builder.build().into_diagnostic()
}

async fn run_tests_in_directory(args: &RunArgs) -> Result<Vec<TestResult>> {
    let cwd = env::current_dir().into_diagnostic()?;

    let mut test_infos = discover_tests(&args.directories)?;
    let current_system = get_current_system();
    let mut test_results = vec![];

    let only_set = build_glob_set(&args.only)?;
    let exclude_set = build_glob_set(&args.exclude)?;

    test_infos.retain(|test_info| {
        let name = &test_info.name;

        if !only_set.is_empty() && !only_set.is_match(name) {
            return false;
        }

        if exclude_set.is_match(name) {
            devenv_activity::message(ActivityLevel::Info, format!("Excluding {name}"));
            return false;
        }

        if test_info.config.should_skip_for_system(&current_system) {
            devenv_activity::message(
                ActivityLevel::Info,
                format!("Skipping {name} (unsupported system {current_system})"),
            );
            test_results.push(TestResult::skipped(name));
            return false;
        }

        true
    });

    let total_tests = test_infos.len();
    let num_skipped = test_results.len();
    devenv_activity::message(
        ActivityLevel::Info,
        format!(
            "Running {} test{}, {} skipped",
            total_tests,
            if total_tests == 1 { "" } else { "s" },
            num_skipped
        ),
    );

    let mut current_test_num = 0;

    // Now iterate over the discovered tests
    for test_info in test_infos {
        current_test_num += 1;
        let dir_name = &test_info.name;
        let path = &test_info.path;
        let test_config = &test_info.config;

        devenv_activity::message(
            ActivityLevel::Info,
            format!("[{current_test_num}/{total_tests}] Starting: {dir_name}"),
        );
        let started = Instant::now();

        // Determine whether to use a temporary directory
        let (devenv_root, devenv_dotfile, _tmpdir) = if test_config.use_tmp_dir {
            // Create temp directory in system temp dir, not the current directory
            let tmpdir = TempDir::with_prefix(format!("devenv-run-tests-{dir_name}"))
                .map_err(|e| miette::miette!("Failed to create temp directory: {}", e))?;
            let devenv_root = tmpdir.path().to_path_buf();
            let devenv_dotfile = tmpdir.path().join(".devenv");

            // Copy the contents of the test directory to the temporary directory
            let copy_content_status = tokio::process::Command::new("cp")
                .arg("-r")
                .arg(format!("{}/.", path.display()))
                .arg(&devenv_root)
                .status()
                .await
                .into_diagnostic()?;
            if !copy_content_status.success() {
                return Err(miette::miette!("Failed to copy test directory"));
            }

            env::set_current_dir(&devenv_root).into_diagnostic()?;

            // Initialize a git repository in the temporary directory if configured to do so.
            // This helps Nix Flakes and git-hooks find the root of the project.
            if test_config.git_init {
                let git_init_status = tokio::process::Command::new("git")
                    .arg("init")
                    .arg("--initial-branch=main")
                    .status()
                    .await
                    .into_diagnostic()?;
                if !git_init_status.success() {
                    return Err(miette::miette!("Failed to initialize the git repository"));
                }
            }

            (devenv_root, devenv_dotfile, Some(tmpdir))
        } else {
            // Run tests directly in the test directory
            let devenv_root = cwd.join(path);
            let devenv_dotfile = devenv_root.join(".devenv");

            env::set_current_dir(&devenv_root).into_diagnostic()?;

            // Note: git_init is ignored when use_tmp_dir is false, as we assume
            // the test directory is already set up correctly

            (devenv_root, devenv_dotfile, None)
        };

        // Run .patch.sh if it exists (must run before loading config)
        let patch_script = PathBuf::from(".patch.sh");
        if patch_script.exists() {
            devenv_activity::message(ActivityLevel::Info, "Running .patch.sh");
            let _ = tokio::process::Command::new("bash")
                .arg(&patch_script)
                .status()
                .await
                .into_diagnostic()?;
        }

        // A script to run inside the shell before the test.
        let setup_script = ".setup.sh";

        let status: miette::Result<()> = if test_config.use_shell {
            // Now load config from the current directory (which might be temp dir)
            let mut config = Config::load_from(&devenv_root)?;
            for input in args.override_inputs.chunks_exact(2) {
                config
                    .override_input_url(&input[0], &input[1])
                    .wrap_err(format!(
                        "Failed to override input {} with {}",
                        &input[0], &input[1]
                    ))?;
            }

            // Override the input for the devenv module
            config
                .add_input(
                    "devenv",
                    &format!("git+file:{}?dir=src/modules", cwd.display()),
                    &[],
                )
                .wrap_err("Failed to add devenv input")?;

            let nix_settings = NixSettings {
                backend: config.backend.clone(),
                ..NixSettings::default()
            };
            let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);
            let secret_settings = SecretSettings::resolve(SecretOptions::default(), &config);
            let options = DevenvOptions {
                inputs: config.inputs,
                imports: config.imports,
                git_root: config.git_root,
                nixpkgs_config,
                nix_settings,
                secret_settings,
                devenv_root: Some(devenv_root.clone()),
                devenv_dotfile: Some(devenv_dotfile.clone()),
                ..Default::default()
            };
            let devenv = Devenv::new(options, devenv::tokio_shutdown::Shutdown::new()).await?;

            // Run .setup.sh if it exists
            if PathBuf::from(setup_script).exists() {
                devenv_activity::message(ActivityLevel::Info, format!("Running {setup_script}"));
                let output = devenv
                    .run_in_shell(
                        format!("./{setup_script}"),
                        &[],
                        Some("Running setup script"),
                    )
                    .await?;
                if !output.status.success() {
                    return Err(miette::miette!(
                        "Setup script failed. Status code: {}",
                        output.status.code().unwrap_or(1)
                    ));
                }
            }

            devenv.test(devenv::VerbosityLevel::Normal).await
        } else {
            // Run .test.sh directly - it must exist when run_test_sh is false
            if PathBuf::from(".test.sh").exists() {
                devenv_activity::message(ActivityLevel::Info, "Running .test.sh directly");
                let output = tokio::process::Command::new("bash")
                    .arg(".test.sh")
                    .status()
                    .await
                    .into_diagnostic()?;
                if output.success() {
                    Ok(())
                } else {
                    Err(miette::miette!(
                        "Test script failed. Status code: {}",
                        output.code().unwrap_or(1)
                    ))
                }
            } else {
                Err(miette::miette!(
                    ".test.sh file is required when use_shell is disabled"
                ))
            }
        };

        let duration = started.elapsed();

        // `gc/shell` points at the last shell built in this directory.
        let closure_size = match shell_closure_size(&devenv_dotfile.join("gc")).await {
            Ok(size) => size,
            Err(error) => {
                devenv_activity::message(
                    ActivityLevel::Warn,
                    format!("Could not determine the shell closure size: {error}"),
                );
                None
            }
        };

        let status = match (status, test_config.max_closure_size, closure_size) {
            (Ok(()), Some(limit), Some(size)) if size > limit => Err(miette::miette!(
                "Shell closure is {}, exceeding max_closure_size of {}",
                format_size(size),
                format_size(limit)
            )),
            (Ok(()), Some(_), None) => Err(miette::miette!(
                "max_closure_size is set but the shell closure size could not be determined"
            )),
            (status, _, _) => status,
        };

        let mut stats = vec![format_duration(duration)];
        if let Some(size) = closure_size {
            stats.push(format!("closure {}", format_size(size)));
        }
        let stats = stats.join(", ");

        // Queue status through the activity pipeline to preserve diagnostic ordering.
        let (test_status, note) = match status {
            Ok(()) => {
                devenv_activity::message(
                    ActivityLevel::Info,
                    format!("[{current_test_num}/{total_tests}] Passed: {dir_name} ({stats})"),
                );
                (TestStatus::Passed, None)
            }
            Err(error) => {
                devenv_activity::message(
                    ActivityLevel::Error,
                    format!("[{current_test_num}/{total_tests}] Failed: {dir_name} ({stats})"),
                );
                devenv_activity::message(ActivityLevel::Error, format!("{error:?}"));
                (TestStatus::Failed, Some(error.to_string()))
            }
        };

        test_results.push(TestResult {
            name: dir_name.to_string(),
            status: test_status,
            duration: Some(duration),
            closure_size,
            note,
        });

        // Restore the current directory
        env::set_current_dir(&cwd).into_diagnostic()?;
    }

    Ok(test_results)
}

fn main() -> Result<ExitCode> {
    // Dispatch before the Nix/GC runtime and the DEVENV_RUN_TESTS re-exec.
    if let Commands::Pty(pty_args) = Args::parse().command {
        let code = pty::run(
            &pty_args.transcript,
            &pty_args.command,
            std::time::Duration::from_secs(pty_args.step_timeout),
        )?;
        return Ok(ExitCode::from((code & 0xff) as u8));
    }

    // Nix evaluation recurses deeply enough to overflow the default 8MB main
    // thread stack, so the whole run happens on a thread sized for Nix.
    let thread = std::thread::Builder::new()
        .name("devenv-run-tests".into())
        .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .spawn(|| {
            // `block_on` polls the (!Send) backend futures on this thread, so it
            // needs GC registration just like the runtime's worker threads.
            devenv_nix_backend::gc_register_current_thread()
                .map_err(|e| miette::miette!("Failed to register thread with GC: {e}"))?;
            build_gc_runtime()?.block_on(async_main())
        })
        .into_diagnostic()
        .wrap_err("Failed to spawn devenv-run-tests thread")?;

    thread.join().map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| format!("{payload:?}"));
        miette::miette!("devenv-run-tests thread panicked: {message}")
    })?
}

/// Create a tokio runtime with worker threads registered with Boehm GC.
///
/// Nix uses Boehm GC with parallel marking. During stop-the-world collection,
/// only registered threads are paused. This ensures all tokio worker threads
/// are properly registered to avoid race conditions.
fn build_gc_runtime() -> Result<tokio::runtime::Runtime> {
    devenv_nix_backend::nix_init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("devenv-run-tests-worker")
        .thread_stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .on_thread_start(|| {
            let _ = devenv_nix_backend::gc_register_current_thread();
        })
        .build()
        .into_diagnostic()
        .wrap_err("Failed to create tokio runtime")
}

async fn async_main() -> Result<ExitCode> {
    let _tracing_guard = devenv_tracing::init_tracing_default();

    // If DEVENV_RUN_TESTS is set, run the tests.
    if env::var("DEVENV_RUN_TESTS") == Ok("1".to_string()) {
        let args = Args::parse();

        // Wire activity events to ConsoleOutput so devenv.test()'s build/eval/
        // process output surfaces to stderr. Without this, events go nowhere.
        let (activity_rx, handle) = devenv_activity::init();
        let activity_guard = handle.install();
        let (frontend_tx, frontend_rx) = tokio::sync::mpsc::channel(1);
        let console_task = tokio::spawn(async move {
            devenv_console::ConsoleOutput::new(activity_rx, frontend_rx, VerbosityLevel::Normal)
                .run()
                .await;
        });

        let result = execute_command(&args).await;

        let _ = frontend_tx.send(FrontendCommand::ExitRenderer).await;
        drop(activity_guard);
        let _ = console_task.await;

        match result {
            Ok(_) => return Ok(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("Error: {err}");
                return Ok(ExitCode::FAILURE);
            }
        };
    }

    // Otherwise, run the tests in a subprocess with a fresh environment.
    let executable_path = env::current_exe().into_diagnostic()?;
    let executable_dir = executable_path.parent().unwrap();
    let cwd = env::current_dir().into_diagnostic()?;

    // Create a wrapper for devenv that adds --override-input
    let wrapper_dir = TempDir::new().into_diagnostic()?;
    let devenv_wrapper_path = wrapper_dir.path().join("devenv");

    // NOTE: clap has a bug where multiple global arguments aren't resolved properly across subcommand boundaries.
    // We parse out all overrides and add them before the command to allow invocations to provide their own overrides.
    // Similar issue: https://github.com/clap-rs/clap/issues/6049
    let wrapper_content = format!(
        r#"#!/usr/bin/env bash

# Parse arguments to extract --override-input and reposition them
override_inputs=()
other_args=()

i=0
while [ $i -lt $# ]; do
    case "${{@:$((i+1)):1}}" in
        --override-input)
            # Add --override-input and its two values (name and URL)
            override_inputs+=("--override-input")
            override_inputs+=("${{@:$((i+2)):1}}")
            override_inputs+=("${{@:$((i+3)):1}}")
            i=$((i+3))
            ;;
        *)
            other_args+=("${{@:$((i+1)):1}}")
            i=$((i+1))
            ;;
    esac
done

# Execute devenv with our devenv override first, then user overrides, then other arguments
exec '{bin_dir}/devenv' \
  --override-input devenv 'git+file:{cwd}?dir=src/modules' \
  "${{override_inputs[@]}}" \
  "${{other_args[@]}}"
"#,
        bin_dir = executable_dir.display(),
        cwd = cwd.display(),
    );

    tokio::fs::write(&devenv_wrapper_path, wrapper_content)
        .await
        .into_diagnostic()?;
    tokio::process::Command::new("chmod")
        .arg("+x")
        .arg(&devenv_wrapper_path)
        .status()
        .await
        .into_diagnostic()?;

    let mut env = vec![
        ("DEVENV_RUN_TESTS", "1".to_string()),
        ("DEVENV_NIX", env::var("DEVENV_NIX").unwrap_or_default()),
        // Path to the devenv repo being tested, for tests that need to use it as an input
        ("DEVENV_REPO", cwd.display().to_string()),
        (
            "PATH",
            format!(
                "{}:{}:{}",
                wrapper_dir.path().display(),
                executable_dir.display(),
                env::var("PATH").unwrap_or_default()
            ),
        ),
        (
            "HOME",
            env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        ),
        (
            "USER",
            env::var("USER").unwrap_or_else(|_| "nobody".to_string()),
        ),
    ];

    // Pass through optional environment variables only if they exist
    // TERM is essential for many programs, provide a safe default if not set
    env.push((
        "TERM",
        env::var("TERM").unwrap_or_else(|_| "dumb".to_string()),
    ));
    // SHELL is needed by many programs that spawn subshells
    env.push((
        "SHELL",
        env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
    ));
    if let Ok(lang) = env::var("LANG") {
        env.push(("LANG", lang));
    }
    if let Ok(lc_all) = env::var("LC_ALL") {
        env.push(("LC_ALL", lc_all));
    }
    if let Ok(tzdir) = env::var("TZDIR") {
        env.push(("TZDIR", tzdir));
    }
    if let Ok(auth_sock) = env::var("SSH_AUTH_SOCK") {
        env.push(("SSH_AUTH_SOCK", auth_sock));
    }
    // Only pass through RUST_LOG if explicitly set in the parent environment.
    // Do not default it — setting RUST_LOG=info would suppress debug-level trace
    // output from devenv --verbose, breaking tests that grep trace logs.
    if let Ok(rust_log) = env::var("RUST_LOG") {
        env.push(("RUST_LOG", rust_log));
    }

    let mut cmd = Command::new(&executable_path);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(env::args().skip(1))
        .env_clear()
        .envs(env);

    let output = cmd.output().into_diagnostic()?;
    if output.status.success() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

async fn execute_command(args: &Args) -> Result<()> {
    match &args.command {
        Commands::Run(run_args) => run_tests(run_args).await,
        Commands::GenerateJson(gen_args) => generate_json(gen_args).await,
        // Dispatched in main() before the runtime starts.
        Commands::Pty(_) => unreachable!("pty is handled in main"),
    }
}

/// One line per test: status, name, duration and shell closure size.
fn format_results_table(results: &[TestResult]) -> String {
    let name_width = results.iter().map(|r| r.name.len()).max().unwrap_or(0);
    results
        .iter()
        .map(|result| {
            let status = match result.status {
                TestStatus::Passed => "passed ",
                TestStatus::Failed => "FAILED ",
                TestStatus::Skipped => "skipped",
            };
            let duration = result
                .duration
                .map(format_duration)
                .unwrap_or_else(|| "-".to_string());
            let closure = result
                .closure_size
                .map(format_size)
                .unwrap_or_else(|| "-".to_string());
            let mut line = format!(
                "{status}  {:<name_width$}  {duration:>8}  {closure:>9}",
                result.name
            );
            if let Some(note) = &result.note {
                line.push_str("  ");
                line.push_str(note.lines().next().unwrap_or_default());
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn run_tests(args: &RunArgs) -> Result<()> {
    let test_results = run_tests_in_directory(args).await?;

    let mut num_passed = 0;
    let mut num_failed = 0;
    let mut num_skipped = 0;

    for result in &test_results {
        match &result.status {
            TestStatus::Passed => num_passed += 1,
            TestStatus::Failed => num_failed += 1,
            TestStatus::Skipped => num_skipped += 1,
        }
    }

    if !test_results.is_empty() {
        devenv_activity::message_with_details(
            ActivityLevel::Info,
            "Test results:",
            Some(format_results_table(&test_results)),
        );
    }

    let num_ran = num_passed + num_failed;
    devenv_activity::message(
        if num_failed > 0 {
            ActivityLevel::Error
        } else {
            ActivityLevel::Info
        },
        format!("Ran {num_ran} tests, {num_failed} failed, {num_skipped} skipped."),
    );

    if num_failed > 0 {
        Err(miette::miette!("Some tests failed"))
    } else {
        Ok(())
    }
}

async fn generate_json(args: &GenerateJsonArgs) -> Result<()> {
    let mut test_infos = discover_tests(&args.directories)?;

    if !args.all {
        let system = args.system.clone().unwrap_or_else(get_current_system);
        test_infos.retain(|info| !info.config.should_skip_for_system(&system));
    }

    // Extract just the metadata for JSON output
    let test_metadata: Vec<TestMetadata> =
        test_infos.into_iter().map(|info| info.metadata).collect();

    let json_output = serde_json::to_string(&test_metadata).into_diagnostic()?;
    println!("{json_output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes_with_units() {
        assert_eq!(parse_size("500 MB").unwrap(), 500_000_000);
        assert_eq!(parse_size("1.5 GiB").unwrap(), 1_610_612_736);
        assert_eq!(parse_size("2gb").unwrap(), 2_000_000_000);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert!(parse_size("1 parsec").is_err());
        assert!(parse_size("MB").is_err());
    }

    #[test]
    fn test_config_accepts_size_strings_and_bytes() {
        let config: TestConfig = serde_yaml::from_str("max_closure_size: 2 GB").unwrap();
        assert_eq!(config.max_closure_size, Some(2_000_000_000));
        let config: TestConfig = serde_yaml::from_str("max_closure_size: 4096").unwrap();
        assert_eq!(config.max_closure_size, Some(4096));
        let config: TestConfig = serde_yaml::from_str("use_shell: false").unwrap();
        assert_eq!(config.max_closure_size, None);
    }

    #[test]
    fn formats_sizes_and_durations() {
        assert_eq!(format_size(361_000_000), "361 MB");
        assert_eq!(format_size(1_700_000_000), "1.7 GB");
        assert_eq!(format_duration(Duration::from_millis(6_240)), "6.2s");
        assert_eq!(format_duration(Duration::from_secs(103)), "1m43s");
    }
}
