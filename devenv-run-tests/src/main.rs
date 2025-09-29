use clap::Parser;
use devenv::{Devenv, DevenvOptions, log};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
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
}

#[derive(Parser, Debug)]
struct RunArgs {
    #[clap(long, value_parser, help = "Exclude these tests.")]
    exclude: Vec<PathBuf>,

    #[clap(long, value_parser, help = "Only run these tests.")]
    only: Vec<PathBuf>,

    #[clap(
        short,
        long,
        number_of_values = 2,
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml."
    )]
    override_input: Vec<String>,

    #[clap(value_parser, default_values = DEFAULT_DIRECTORIES)]
    directories: Vec<PathBuf>,
}

#[derive(Parser, Debug)]
struct GenerateJsonArgs {
    #[clap(value_parser, default_values = DEFAULT_DIRECTORIES)]
    directories: Vec<PathBuf>,

    #[clap(long, help = "Include all tests regardless of current system support")]
    all: bool,
}

struct TestResult {
    name: String,
    passed: bool,
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
}

fn default_git_init() -> bool {
    true
}

fn default_use_shell() -> bool {
    true
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            git_init: default_git_init(),
            use_shell: default_use_shell(),
            supported_systems: Vec::new(),
            broken_systems: Vec::new(),
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

fn discover_tests(
    directories: &[PathBuf],
    filter_by_current_system: bool,
) -> Result<Vec<TestInfo>> {
    let mut test_infos = vec![];
    let current_system = get_current_system();

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

            // Skip tests that don't support current system (if filtering is enabled)
            if filter_by_current_system && test_config.should_skip_for_system(&current_system) {
                continue;
            }

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

async fn run_tests_in_directory(args: &RunArgs) -> Result<Vec<TestResult>> {
    let cwd = env::current_dir().into_diagnostic()?;

    // Discover tests (filtered by current system)
    let mut test_infos = discover_tests(&args.directories, true)?;

    // Apply --only and --exclude filters before counting
    test_infos.retain(|test_info| {
        let path = &test_info.path;
        let dir_name = &test_info.name;

        if !args.only.is_empty() {
            if !args.only.iter().any(|only| path.ends_with(only)) {
                return false;
            }
        } else if args.exclude.iter().any(|exclude| path.ends_with(exclude)) {
            eprintln!("Excluding {dir_name}");
            return false;
        }
        true
    });

    let total_tests = test_infos.len();
    eprintln!(
        "Running {} test{}",
        total_tests,
        if total_tests == 1 { "" } else { "s" }
    );

    let mut test_results = vec![];
    let mut current_test_num = 0;

    // Now iterate over the discovered tests
    for test_info in test_infos {
        current_test_num += 1;
        let dir_name = &test_info.name;
        let path = &test_info.path;
        let test_config = &test_info.config;

        eprintln!(
            "\n[{}/{}] Starting: {}",
            current_test_num, total_tests, dir_name
        );
        eprintln!("{}", "-".repeat(50));

        let mut config = devenv::config::Config::load_from(path)?;
        for input in args.override_input.chunks_exact(2) {
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
                &format!("path:{}?dir=src/modules", cwd.display()),
                &[],
            )
            .wrap_err("Failed to add devenv input")?;

        // Create temp directory in system temp dir, not the current directory
        let tmpdir = TempDir::with_prefix(format!("devenv-run-tests-{dir_name}"))
            .map_err(|e| miette::miette!("Failed to create temp directory: {}", e))?;
        let devenv_root = tmpdir.path().to_path_buf();
        let devenv_dotfile = tmpdir.path().join(".devenv");

        // Copy the contents of the test directory to the temporary directory
        let copy_content_status = Command::new("cp")
            .arg("-r")
            .arg(format!("{}/.", path.display()))
            .arg(&devenv_root)
            .status()
            .into_diagnostic()?;
        if !copy_content_status.success() {
            return Err(miette::miette!("Failed to copy test directory"));
        }

        env::set_current_dir(&devenv_root).into_diagnostic()?;

        // Initialize a git repository in the temporary directory if configured to do so.
        // This helps Nix Flakes and git-hooks find the root of the project.
        if test_config.git_init {
            let git_init_status = Command::new("git")
                .arg("init")
                .arg("--initial-branch=main")
                .status()
                .into_diagnostic()?;
            if !git_init_status.success() {
                return Err(miette::miette!("Failed to initialize the git repository"));
            }
        }

        let options = DevenvOptions {
            config,
            devenv_root: Some(devenv_root.clone()),
            devenv_dotfile: Some(devenv_dotfile),
            global_options: Some(devenv::GlobalOptions::default()),
        };
        let devenv = Devenv::new(options).await;

        eprintln!("  Running {dir_name}");

        // A script to patch files in the working directory before the shell.
        let patch_script = ".patch.sh";

        // Run .patch.sh if it exists
        if PathBuf::from(patch_script).exists() {
            eprintln!("    Running {patch_script}");
            let _ = Command::new("bash")
                .arg(patch_script)
                .status()
                .into_diagnostic()?;
        }

        // A script to run inside the shell before the test.
        let setup_script = ".setup.sh";

        // Run .setup.sh if it exists
        if PathBuf::from(setup_script).exists() {
            eprintln!("    Running {setup_script}");
            let output = devenv
                .run_in_shell(format!("./{setup_script}"), &[])
                .await?;
            if !output.status.success() {
                return Err(miette::miette!(
                    "Setup script failed. Status code: {}",
                    output.status.code().unwrap_or(1)
                ));
            }
        }

        let status = if test_config.use_shell {
            devenv.test().await
        } else {
            // Run .test.sh directly - it must exist when run_test_sh is false
            if PathBuf::from(".test.sh").exists() {
                eprintln!("    Running .test.sh directly");
                let output = Command::new("bash")
                    .arg(".test.sh")
                    .status()
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

        let passed = status.is_ok();

        eprintln!("{}", "-".repeat(50));
        if passed {
            eprintln!(
                "✅ [{}/{}] Passed: {}",
                current_test_num, total_tests, dir_name
            );
        } else {
            eprintln!(
                "❌ [{}/{}] Failed: {}",
                current_test_num, total_tests, dir_name
            );
            if let Err(error) = &status {
                eprintln!("    Error: {error:?}");
            }
        }

        let result = TestResult {
            name: dir_name.to_string(),
            passed,
        };
        test_results.push(result);

        // Restore the current directory
        env::set_current_dir(&cwd).into_diagnostic()?;
    }

    Ok(test_results)
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    log::init_tracing_default();

    // If DEVENV_RUN_TESTS is set, run the tests.
    if env::var("DEVENV_RUN_TESTS") == Ok("1".to_string()) {
        let args = Args::parse();
        match execute_command(&args).await {
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
  --override-input devenv 'path:{cwd}?dir=src/modules' \
  "${{override_inputs[@]}}" \
  "${{other_args[@]}}"
"#,
        bin_dir = executable_dir.display(),
        cwd = cwd.display(),
    );

    fs::write(&devenv_wrapper_path, wrapper_content).into_diagnostic()?;
    Command::new("chmod")
        .arg("+x")
        .arg(&devenv_wrapper_path)
        .status()
        .into_diagnostic()?;

    let mut env = vec![
        ("DEVENV_RUN_TESTS", "1".to_string()),
        ("DEVENV_NIX", env::var("DEVENV_NIX").unwrap_or_default()),
        (
            "PATH",
            format!(
                "{}:{}",
                wrapper_dir.path().display(),
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
    }
}

async fn run_tests(args: &RunArgs) -> Result<()> {
    let test_results = run_tests_in_directory(args).await?;
    let num_tests = test_results.len();
    let num_failed_tests = test_results.iter().filter(|r| !r.passed).count();

    eprintln!();

    for result in test_results {
        if !result.passed {
            eprintln!("{}: Failed", result.name);
        };
    }

    eprintln!();
    eprintln!("Ran {num_tests} tests, {num_failed_tests} failed.");

    if num_failed_tests > 0 {
        Err(miette::miette!("Some tests failed"))
    } else {
        Ok(())
    }
}

async fn generate_json(args: &GenerateJsonArgs) -> Result<()> {
    // Discover tests (filter by current system unless --all is specified)
    let test_infos = discover_tests(&args.directories, !args.all)?;

    // Extract just the metadata for JSON output
    let test_metadata: Vec<TestMetadata> =
        test_infos.into_iter().map(|info| info.metadata).collect();

    let json_output = serde_json::to_string(&test_metadata).into_diagnostic()?;
    println!("{json_output}");
    Ok(())
}
