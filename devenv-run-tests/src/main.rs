use clap::Parser;
use devenv::{log, Devenv, DevenvOptions};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};

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

async fn run_tests_in_directory(
    args: &Args,
) -> Result<Vec<TestResult>, Box<dyn std::error::Error>> {
    eprintln!("Running Tests");

    let cwd = env::current_dir()?;

    let mut test_results = vec![];

    for directory in &args.directories {
        eprintln!("Running in directory {}", directory.display());
        let paths = fs::read_dir(directory)?;

        for path in paths {
            let path = path?.path();
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

            let mut config = devenv::config::Config::load_from(path)?;
            for input in args.override_input.chunks_exact(2) {
                config.add_input(&input[0].clone(), &input[1].clone(), &[]);
            }

            // Override the input for the devenv module
            config.add_input(
                "devenv",
                &format!("path:{:}?dir=src/modules", cwd.to_str().unwrap()),
                &[],
            );

            let tmpdir = tempdir::TempDir::new(&format!("devenv-run-tests-{}", dir_name))?;
            let devenv_root = tmpdir.path().to_path_buf();
            let devenv_dotfile = tmpdir.path().join(".devenv");

            // Copy the contents of the test directory to the temporary directory
            let copy_content_status = Command::new("cp")
                .arg("-r")
                .arg(format!("{}/.", path.display()))
                .arg(&devenv_root)
                .status()?;
            if !copy_content_status.success() {
                return Err("Failed to copy test directory".into());
            }

            env::set_current_dir(&devenv_root)?;

            // Initialize a git repository in the temporary directory.
            // This helps Nix Flakes and git-hooks find the root of the project.
            let git_init_status = Command::new("git")
                .arg("init")
                .arg("--initial-branch=main")
                .status()?;
            if !git_init_status.success() {
                return Err("Failed to initialize the git repository".into());
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
            let mut devenv = Devenv::new(options).await;

            eprintln!("  Running {}", dir_name);

            // A script to patch files in the working directory before the shell.
            let patch_script = ".patch.sh";

            // Run .patch.sh if it exists
            if PathBuf::from(patch_script).exists() {
                eprintln!("    Running {patch_script}");
                let _ = Command::new("bash").arg(patch_script).status()?;
            }

            // A script to run inside the shell before the test.
            let setup_script = ".setup.sh";

            // Run .setup.sh if it exists
            if PathBuf::from(setup_script).exists() {
                eprintln!("    Running {setup_script}");
                devenv
                    .shell(&Some(format!("./{setup_script}")), &[], false)
                    .await?;
            }

            // TODO: wait for processes to shut down before exiting
            let status = devenv.test().await;
            let result = TestResult {
                name: dir_name.to_string(),
                passed: status.is_ok(),
            };
            test_results.push(result);

            // Restore the current directory
            env::set_current_dir(&cwd)?;
        }
    }

    Ok(test_results)
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
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
    let executable_path = env::current_exe()?;
    let executable_dir = executable_path.parent().unwrap();
    let path = format!(
        "{}:{}",
        executable_dir.display(),
        env::var("PATH").unwrap_or_default()
    );

    let mut cmd = Command::new(&executable_path);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(env::args().skip(1));
    cmd.env_clear()
        .env("DEVENV_RUN_TESTS", "1")
        .env("DEVENV_NIX", env::var("DEVENV_NIX").unwrap_or_default())
        .env("PATH", path)
        .env("HOME", env::var("HOME").unwrap_or_default());

    let output = cmd.output()?;
    if output.status.success() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

async fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
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
        Err("Some tests failed".into())
    } else {
        Ok(())
    }
}
