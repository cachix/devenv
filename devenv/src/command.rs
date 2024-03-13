use crate::App;
use miette::{bail, Result};
use std::env;
use std::os::unix::process::CommandExt;

const NIX_FLAGS: [&str; 11] = [
    "--show-trace",
    "--extra-experimental-features",
    "nix-command",
    "--extra-experimental-features",
    "flakes",
    // remove unnecessary warnings
    "--option",
    "warn-dirty",
    "false",
    // flake caching is too aggressive
    "--option",
    "eval-cache",
    "false",
];

pub struct Options {
    pub replace_shell: bool,
    pub use_cachix: bool,
    pub logging: bool,
    pub dont_exit: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            replace_shell: false,
            use_cachix: false,
            logging: true,
            dont_exit: false,
        }
    }
}

impl App {
    pub fn run_nix(
        &mut self,
        command: &str,
        args: &[&str],
        options: &Options,
    ) -> Result<std::process::Output> {
        let mut cmd = self.prepare_command(command, args)?;

        if options.replace_shell {
            if self.cli.nix_debugger && command.ends_with("bin/nix") {
                cmd.arg("--debugger");
            }
            let error = cmd.exec();
            self.logger.error(&format!(
                "Failed to replace shell with `{} {}`: {error}",
                cmd.get_program().to_string_lossy(),
                cmd.get_args()
                    .map(|arg| arg.to_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
            bail!("Failed to replace shell")
        } else {
            let result = if options.logging {
                cmd.stdin(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .output()
                    .expect("Failed to run command")
            } else {
                cmd.output().expect("Failed to run command")
            };
            if !result.status.success() {
                if options.logging {
                    println!();
                    let code = match result.status.code() {
                        Some(code) => format!("with exit code {}", code),
                        None => "without exit code".to_string(),
                    };
                    self.logger.error(&format!(
                        "Command `{} {}` failed {code} and produced the following output:\n{}\n{}",
                        cmd.get_program().to_string_lossy(),
                        cmd.get_args()
                            .map(|arg| arg.to_str().unwrap())
                            .collect::<Vec<_>>()
                            .join(" "),
                        String::from_utf8_lossy(&result.stdout),
                        String::from_utf8_lossy(&result.stderr),
                    ));
                }
                if self.cli.nix_debugger && command.ends_with("bin/nix") {
                    self.logger.info("Starting Nix debugger ...");
                    cmd.arg("--debugger").exec();
                }
                if !options.dont_exit {
                    bail!("Command failed")
                } else {
                    Ok(result)
                }
            } else {
                Ok(result)
            }
        }
    }

    pub fn prepare_command(
        &mut self,
        command: &str,
        args: &[&str],
    ) -> Result<std::process::Command> {
        let cmd = if command.starts_with("nix") {
            let mut flags = NIX_FLAGS.to_vec();
            flags.push("--max-jobs");
            let max_jobs = self.cli.max_jobs.to_string();
            flags.push(&max_jobs);

            // handle --nix-option key value
            for chunk in self.cli.nix_option.chunks_exact(2) {
                flags.push("--option");
                flags.push(&chunk[0]);
                flags.push(&chunk[1]);
            }

            flags.extend_from_slice(args);

            let mut cmd = match env::var("DEVENV_NIX") {
                Ok(devenv_nix) => std::process::Command::new(format!("{devenv_nix}/bin/{command}")),
                Err(_) => {
                    self.logger.error(
                "$DEVENV_NIX is not set, but required as devenv doesn't work without a few Nix patches."
                );
                    self.logger.error(
                        "Please follow https://devenv.sh/getting-started/ to install devenv.",
                    );
                    bail!("$DEVENV_NIX is not set")
                }
            };

            if self.cli.impure || self.config.impure {
                flags.push("--impure");
            }

            cmd.args(flags);
            cmd
        } else {
            let mut cmd = std::process::Command::new(command);
            cmd.args(args);
            cmd
        };

        if self.cli.verbose {
            self.logger.debug(&format!(
                "Running command: {} {}",
                command,
                cmd.get_args()
                    .map(|arg| arg.to_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }

        Ok(cmd)
    }
}
