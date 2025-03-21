use landlock::{
    path_beneath_rules, Access, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetError,
    RulesetStatus, ABI,
};
use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <command> [args...]", args[0]);
        std::process::exit(1);
    }

    // Get DEVENV_ROOT from environment
    let devenv_root = match env::var("DEVENV_ROOT") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("DEVENV_ROOT environment variable is not set");
            std::process::exit(1);
        }
    };
    
    let runtime_dir = match env::var("XDG_RUNTIME_DIR") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("XDG_RUNTIME_DIR environment variable is not set");
            std::process::exit(1);
        }
    };
    
    // TODO: make sure that the user cannot modify this within the shell
    let home_dir = match env::var("HOME") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("HOME environment variable is not set");
            std::process::exit(1);
        }
    };

    // Verify the path exists
    let devenv_path = PathBuf::from(&devenv_root);
    if !devenv_path.exists() {
        eprintln!("DEVENV_ROOT path does not exist: {}", devenv_root);
        std::process::exit(1);
    }

    // Set up landlock sandboxing
    match setup_landlock_sandbox(&devenv_root, &runtime_dir, &home_dir) {
        Ok(status) => match status {
            RulesetStatus::FullyEnforced => {
                println!("Landlock: Fully sandboxed to {}", devenv_root)
            }
            RulesetStatus::PartiallyEnforced => {
                println!("Landlock: Partially sandboxed to {}", devenv_root)
            }
            RulesetStatus::NotEnforced => {
                println!("Landlock: Not sandboxed! Please update your kernel.")
            }
        },
        Err(e) => {
            eprintln!("Failed to set up landlock sandbox: {}", e);
            std::process::exit(1);
        }
    }

    // Execute the command
    let command = &args[1];
    let command_args = if args.len() > 2 { &args[2..] } else { &[] };

    let status = match execute_command(command, command_args) {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Failed to execute command: {}", e);
            std::process::exit(1);
        }
    };

    // Exit with the same status as the executed command
    std::process::exit(status.code().unwrap_or(1));
}

fn setup_landlock_sandbox(devenv_root: &str, runtime_dir: &str, home: &str) -> Result<RulesetStatus, RulesetError> {
    let abi = ABI::V2;

    // Create a ruleset that only allows access to the DEVENV_ROOT directory
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?
        .add_rules(path_beneath_rules(&[
            devenv_root, 
            runtime_dir, 
            // for Python uv
            &(home.to_owned()+"/.cache/uv"), 
            // error without: GC_push_all_stacks: pthread_getattr_np failed!
            "/proc", // TODO: limit further. I was too lazy.
            // process-compose logs here
            "/tmp", // TODO: can we limit this further?
            // required by process-compose for tui
            "/dev/tty", 
            // for redirecting output into /dev/null
            "/dev/null",
        ], AccessFs::from_all(abi)))?
        .add_rules(path_beneath_rules(&[
            "/nix", 
            "/proc/stat",
        ], AccessFs::from_read(abi)))?
        .restrict_self()?;

    Ok(status.ruleset)
}

fn execute_command(command: &str, args: &[String]) -> Result<ExitStatus, std::io::Error> {
    let status = Command::new(command).args(args).status()?;

    Ok(status)
}

