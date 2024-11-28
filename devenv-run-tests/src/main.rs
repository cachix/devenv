use clap::Parser;
use devenv::{log, Devenv, DevenvOptions};
use std::path::PathBuf;
use std::{env, fs};

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
    println!("Running Tests");

    let cwd = std::env::current_dir()?;

    let mut test_results = vec![];

    for directory in &args.directories {
        println!("Running in directory {}", directory.display());
        let paths = fs::read_dir(directory)?;

        for path in paths {
            let path = path?.path();
            let path = path.as_path();
            if path.is_dir() {
                let dir_name_path = path.file_name().unwrap();
                let dir_name = dir_name_path.to_str().unwrap();

                if !args.only.is_empty() {
                    let mut found = false;
                    for only in &args.only {
                        if path.ends_with(only) {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        continue;
                    }
                } else {
                    for exclude in &args.exclude {
                        if path.ends_with(exclude) {
                            println!("Skipping {}", dir_name);
                            continue;
                        }
                    }
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

                let tmpdir = tempdir::TempDir::new_in(path, ".devenv")
                    .expect("Failed to create temporary directory");

                let options = DevenvOptions {
                    config,
                    devenv_root: Some(cwd.join(path)),
                    devenv_dotfile: Some(tmpdir.path().to_path_buf()),
                    ..Default::default()
                };
                let mut devenv = Devenv::new(options).await;

                println!("  Running {}", dir_name);

                env::set_current_dir(path).expect("failed to set current dir");

                // A script to patch files in the working directory before the shell.
                let patch_script = ".patch.sh";

                // Run .patch.sh if it exists
                if PathBuf::from(patch_script).exists() {
                    println!("    Running {patch_script}");
                    let _ = std::process::Command::new("bash")
                        .arg(patch_script)
                        .status()?;
                }

                // A script to run inside the shell before the test.
                let setup_script = ".setup.sh";

                // Run .setup.sh if it exists
                if PathBuf::from(setup_script).exists() {
                    println!("    Running {setup_script}");
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
                env::set_current_dir(&cwd).expect("failed to set current dir");
            }
        }
    }

    Ok(test_results)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::init_tracing_default();

    let args = Args::parse();

    let executable_path = std::env::current_exe()?;
    let executable_dir = executable_path.parent().unwrap();
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            executable_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );

    let test_results = run_tests_in_directory(&args).await?;
    let num_tests = test_results.len();
    let num_failed_tests = test_results.iter().filter(|r| !r.passed).count();

    println!();

    for result in test_results {
        if !result.passed {
            println!("{}: Failed", result.name);
        };
    }

    println!();
    println!("Ran {} tests, {} failed.", num_tests, num_failed_tests);

    if num_failed_tests > 0 {
        Err("Some tests failed".into())
    } else {
        Ok(())
    }
}
