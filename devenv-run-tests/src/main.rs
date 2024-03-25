use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};

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

    #[clap(value_parser, required = true)]
    directories: Vec<PathBuf>,
}

struct TestResult {
    name: String,
    passed: bool,
}

fn run_tests_in_directory(args: &Args) -> Result<Vec<TestResult>, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let cwd = cwd.display();

    let mut test_results = vec![];

    for directory in &args.directories {
        println!("Running in directory {}", directory.display());
        let paths = fs::read_dir(directory)?;

        for path in paths {
            let path = path?.path();
            if path.is_dir() {
                let dir_name_path = path.file_name().unwrap();
                let dir_name = dir_name_path.to_str().unwrap();

                if !args.only.is_empty() {
                    let mut found = false;
                    for only in &args.only {
                        if path.as_path().ends_with(only) {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        continue;
                    }
                } else {
                    for exclude in &args.exclude {
                        if path.as_path().ends_with(exclude) {
                            println!("Skipping {}", dir_name);
                            continue;
                        }
                    }
                }

                println!("  Running {}", dir_name);
                // if .setup.sh exists, run it
                let setup_script = path.join(".setup.sh");
                if setup_script.exists() {
                    println!("    Running .setup.sh");
                    let _ = std::process::Command::new("bash")
                        .arg(".setup.sh")
                        .current_dir(&path)
                        .status()?;
                }
                let overrides = args.override_input.iter().enumerate().flat_map(|(i, arg)| {
                    if i % 2 == 0 {
                        vec!["--override-input", arg.as_str()]
                    } else {
                        vec![arg.as_str()]
                    }
                });
                // TODO: use as a library
                let status = std::process::Command::new("devenv")
                    .args([
                        "--override-input",
                        "devenv",
                        &format!("path:{cwd}?dir=src/modules"),
                    ])
                    .args(overrides)
                    .arg("test")
                    .current_dir(&path)
                    .status()?;

                let result = TestResult {
                    name: dir_name.to_string(),
                    passed: status.success(),
                };
                test_results.push(result);
            }
        }
    }

    Ok(test_results)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let test_results = run_tests_in_directory(&args)?;
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
