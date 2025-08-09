use clap::Parser;
use devenv::{log, Devenv, DevenvOptions};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};
use tempfile::TempDir;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
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

    #[clap(value_parser, default_values = vec!["examples", "tests"])]
    directories: Vec<PathBuf>,
}

struct TestResult {
    name: String,
    passed: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct TestConfig {
    /// Whether to initialize a git repository for the test
    #[serde(default = "default_git_init")]
    git_init: bool,
    /// Whether to run .test.sh inside the shell automatically (default: true)
    #[serde(default = "default_use_shell")]
    use_shell: bool,
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
}

async fn run_tests_in_directory(args: &Args) -> Result<Vec<TestResult>> {
    eprintln!("Running Tests");

    let cwd = env::current_dir().into_diagnostic()?;

    let mut test_results = vec![];

    for directory in &args.directories {
        eprintln!("Running in directory {}", directory.display());
        let paths = fs::read_dir(directory).into_diagnostic()?;

        for path in paths {
            let path = path.into_diagnostic()?.path();
            let path = path.as_path();

            // Skip files
            if !path.is_dir() {
                continue;
            }

            let dir_name_path = path.file_name().unwrap();
            let dir_name = dir_name_path.to_str().unwrap();

            if !args.only.is_empty() {
                if !args.only.iter().any(|only| path.ends_with(only)) {
                    continue;
                }
            } else if args.exclude.iter().any(|exclude| path.ends_with(exclude)) {
                eprintln!("Skipping {}", dir_name);
                continue;
            }

            // Load test configuration
            let test_config = TestConfig::load_from_path(path)?;

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
                    &format!("path:{:}?dir=src/modules", cwd.to_str().unwrap()),
                    &[],
                )
                .wrap_err("Failed to add devenv input")?;

            // Create temp directory in system temp dir, not the current directory
            let tmpdir = TempDir::with_prefix(format!("devenv-run-tests-{}", dir_name))
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
                global_options: Some(devenv::GlobalOptions {
                    // Avoid caching between setup and shell.
                    // Because setup runs inside the shell, we can cache the shell before it's fully set up (e.g. dotenv test)
                    // TODO(sander): remove once `pathExists` can be cache-busted
                    eval_cache: false,
                    ..Default::default()
                }),
            };
            let devenv = Devenv::new(options).await;

            eprintln!("  Running {}", dir_name);

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

            // TODO: wait for processes to shut down before exiting
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
            if let Err(error) = &status {
                eprintln!("    Error in {}: {:?}", dir_name, error);
            }

            let result = TestResult {
                name: dir_name.to_string(),
                passed,
            };
            test_results.push(result);

            // Restore the current directory
            env::set_current_dir(&cwd).into_diagnostic()?;
        }
    }

    Ok(test_results)
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    log::init_tracing_default();

    // If DEVENV_RUN_TESTS is set, run the tests.
    if env::var("DEVENV_RUN_TESTS") == Ok("1".to_string()) {
        let args = Args::parse();
        match run(&args).await {
            Ok(_) => return Ok(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("Error: {}", err);
                return Ok(ExitCode::FAILURE);
            }
        };
    }

    // Otherwise, run the tests in a subprocess with a fresh environment.
    let executable_path = env::current_exe().into_diagnostic()?;
    let executable_dir = executable_path.parent().unwrap();

    let mut env = vec![
        ("DEVENV_RUN_TESTS", "1".to_string()),
        ("DEVENV_NIX", env::var("DEVENV_NIX").unwrap_or_default()),
        (
            "PATH",
            format!(
                "{}:{}",
                executable_dir.display(),
                env::var("PATH").unwrap_or_default()
            ),
        ),
        (
            "HOME", 
            env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
        ),
        (
            "USER", 
            env::var("USER").unwrap_or_else(|_| "nobody".to_string())
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

async fn run(args: &Args) -> Result<()> {
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
    eprintln!("Ran {} tests, {} failed.", num_tests, num_failed_tests);

    if num_failed_tests > 0 {
        Err(miette::miette!("Some tests failed"))
    } else {
        Ok(())
    }
}
