use clap::Parser;
use devenv::log::Level;
use devenv::log::Logger;
use devenv::{Devenv, GlobalOptions};
use std::fs;
use std::path::PathBuf;

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
    let logger = Logger::new(Level::Info);

    logger.info("Running Tests");

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

                println!("  Running {}", dir_name);
                // if .setup.sh exists, run it
                let setup_script = path.join(".setup.sh");
                if setup_script.exists() {
                    println!("    Running .setup.sh");
                    let _ = std::process::Command::new("bash")
                        .arg(".setup.sh")
                        .current_dir(path)
                        .status()?;
                }

                let mut config = devenv::config::Config::load_from(path)?;
                for input in args.override_input.chunks_exact(2) {
                    config.add_input(&input[0].clone(), &input[1].clone(), &[]);
                }

                let tmpdir = tempdir::TempDir::new_in(path, ".devenv")
                    .expect("Failed to create temporary directory");

                // TODO: terrible!
                let global_options = GlobalOptions::parse_from::<[_; 0], String>([]);

                let mut devenv = Devenv::new(
                    config,
                    global_options,
                    Some(&cwd.join(path)),
                    Some(tmpdir.as_ref()),
                    logger.clone(),
                );

                devenv.create_directories();

                let status = devenv.test();

                let result = TestResult {
                    name: dir_name.to_string(),
                    passed: status.is_ok(),
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
