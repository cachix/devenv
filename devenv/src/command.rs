use crate::App;
use miette::{bail, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::os::unix::process::CommandExt;

const NIX_FLAGS: [&str; 12] = [
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
    // always build all dependencies and report errors at the end
    "--keep-going",
];

pub struct Options {
    pub replace_shell: bool,
    pub logging: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            replace_shell: false,
            logging: true,
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
        let prev_logging = self.logger.clone();
        if !options.logging {
            self.logger = crate::log::Logger::new(crate::log::Level::Error);
        }

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
                let code = match result.status.code() {
                    Some(code) => format!("with exit code {}", code),
                    None => "without exit code".to_string(),
                };
                if options.logging {
                    eprintln!();
                    self.logger.error(&format!(
                        "Command produced the following output:\n{}\n{}",
                        String::from_utf8_lossy(&result.stdout),
                        String::from_utf8_lossy(&result.stderr),
                    ));
                }
                if self.cli.nix_debugger && command.ends_with("bin/nix") {
                    self.logger.info("Starting Nix debugger ...");
                    cmd.arg("--debugger").exec();
                }
                bail!(format!(
                    "Command `{} {}` failed with {code}",
                    cmd.get_program().to_string_lossy(),
                    cmd.get_args()
                        .map(|arg| arg.to_str().unwrap())
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
            } else {
                self.logger = prev_logging;
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
                // only pass the impure option to the nix command that supports it.
                // avoid passing it to the older utilities, e.g. like `nix-store` when creating GC roots.
                if command == "nix" {
                    flags.push("--impure");
                }
                // set a dummy value to overcome https://github.com/NixOS/nix/issues/10247
                cmd.env("NIX_PATH", ":");
            }
            cmd.args(flags);

            if args
                .first()
                .map(|arg| arg == &"build" || arg == &"print-dev-env")
                .unwrap_or(false)
            {
                let cachix_caches = self.get_cachix_caches();

                match cachix_caches {
                    Err(e) => {
                        self.logger
                            .warn("Failed to get cachix caches due to evaluation error");
                        self.logger.debug(&format!("{}", e));
                    }
                    Ok(cachix_caches) => {
                        // handle cachix.pull
                        let pull_caches = cachix_caches
                            .caches
                            .pull
                            .iter()
                            .map(|cache| format!("https://{}.cachix.org", cache))
                            .collect::<Vec<String>>()
                            .join(" ");
                        cmd.arg("--option");
                        cmd.arg("extra-substituters");
                        cmd.arg(pull_caches);
                        cmd.arg("--option");
                        cmd.arg("extra-trusted-public-keys");
                        cmd.arg(
                            cachix_caches
                                .known_keys
                                .values()
                                .cloned()
                                .collect::<Vec<String>>()
                                .join(" "),
                        );

                        // handle cachix.push
                        if let Some(push_cache) = &cachix_caches.caches.push {
                            if let Ok(_) = env::var("CACHIX_AUTH_TOKEN") {
                                let args = cmd
                                    .get_args()
                                    .map(|arg| arg.to_str().unwrap())
                                    .collect::<Vec<_>>();
                                let envs = cmd.get_envs().collect::<Vec<_>>();
                                let command_name = cmd.get_program().to_string_lossy();
                                let mut newcmd = std::process::Command::new(format!(
                                    "cachix watch-exec {} {}",
                                    push_cache, command_name
                                ));
                                newcmd.args(args);
                                for (key, value) in envs {
                                    if let Some(value) = value {
                                        newcmd.env(key, value);
                                    }
                                }
                                cmd = newcmd;
                            } else {
                                self.logger.warn(&format!(
                                    "CACHIX_AUTH_TOKEN is not set, but required to push to {}.",
                                    push_cache
                                ));
                            }
                        }
                    }
                }
            }
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

    fn get_cachix_caches(&mut self) -> Result<CachixCaches> {
        match &self.cachix_caches {
            Some(caches) => Ok(caches.clone()),
            None => {
                let no_logging = Options {
                    logging: false,
                    ..Default::default()
                };

                let caches_raw =
                    self.run_nix("nix", &["eval", ".#devenv.cachix", "--json"], &no_logging)?;

                let cachix =
                    serde_json::from_slice(&caches_raw.stdout).expect("Failed to parse JSON");

                let known_keys =
                    if let Ok(known_keys) = std::fs::read_to_string(&self.cachix_trusted_keys) {
                        serde_json::from_str(&known_keys).expect("Failed to parse JSON")
                    } else {
                        HashMap::new()
                    };

                let mut caches = CachixCaches {
                    caches: cachix,
                    known_keys,
                };

                let mut new_known_keys: HashMap<String, String> = HashMap::new();
                for name in caches.caches.pull.iter() {
                    if !caches.known_keys.contains_key(name) {
                        let resp = reqwest::blocking::get(&format!(
                            "https://cachix.org/api/v1/cache/{}",
                            name
                        ))
                        .expect("Failed to get cache");
                        if resp.status().is_client_error() {
                            self.logger.error(&format!(
                                "Cache {} does not exist or you don't have a CACHIX_AUTH_TOKEN configured.",
                                name
                            ));
                            self.logger
                                .error("To create a cache, go to https://app.cachix.org/.");
                            bail!("Cache does not exist or you don't have a CACHIX_AUTH_TOKEN configured.")
                        } else {
                            let resp_json =
                                serde_json::from_slice::<CachixResponse>(&resp.bytes().unwrap())
                                    .expect("Failed to parse JSON");
                            new_known_keys
                                .insert(name.clone(), resp_json.publicSigningKeys[0].clone());
                        }
                    }
                }

                if !caches.caches.pull.is_empty() {
                    let store = self.run_nix("nix", &["store", "ping", "--json"], &no_logging)?;
                    let trusted = serde_json::from_slice::<StorePing>(&store.stdout)
                        .expect("Failed to parse JSON")
                        .trusted;
                    if trusted.is_none() {
                        self.logger
                            .warn("You're using very old version of Nix, please upgrade and restart nix-daemon.");
                    }
                    let restart_command = if cfg!(target_os = "linux") {
                        "sudo systemctl restart nix-daemon"
                    } else {
                        "sudo launchctl kickstart -k system/org.nixos.nix-daemon"
                    };

                    self.logger
                        .info(&format!("Using Cachix: {}", caches.caches.pull.join(", ")));
                    if !new_known_keys.is_empty() {
                        for (name, pubkey) in new_known_keys.iter() {
                            self.logger.info(&format!(
                                "Trusting {}.cachix.org on first use with the public key {}",
                                name, pubkey
                            ));
                        }
                        caches.known_keys.extend(new_known_keys);
                    }

                    std::fs::write(
                        &self.cachix_trusted_keys,
                        serde_json::to_string(&caches.known_keys).unwrap(),
                    )
                    .expect("Failed to write cachix caches to file");

                    if trusted == Some(0) {
                        self.logger.error(&indoc::formatdoc!(
                            "You're not a trusted user of the Nix store. You have the following options:

                            a) Add yourself to the trusted-users list in /etc/nix/nix.conf for devenv to manage caches for you.

                            trusted-users = root {}

                            Restart nix-daemon with:
                            
                              $ {restart_command}

                            b) Add binary caches to /etc/nix/nix.conf yourself:

                            extra-substituters = {}
                            extra-trusted-public-keys = {}

                            And disable automatic cache configuration in `devenv.nix`:

                            {{
                                cachix.enable = false;
                            }}
                        ", whoami::username()
                        , caches.caches.pull.iter().map(|cache| format!("https://{}.cachix.org", cache)).collect::<Vec<String>>().join(" ")
                        , caches.known_keys.values().cloned().collect::<Vec<String>>().join(" ")
                        ));
                        bail!("You're not a trusted user of the Nix store.")
                    }
                }

                self.cachix_caches = Some(caches.clone());
                Ok(caches)
            }
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Cachix {
    pull: Vec<String>,
    push: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct CachixCaches {
    caches: Cachix,
    known_keys: HashMap<String, String>,
}

#[derive(Deserialize, Clone)]
struct CachixResponse {
    publicSigningKeys: Vec<String>,
}

#[derive(Deserialize, Clone)]
struct StorePing {
    trusted: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trusted() {
        let store_ping = r#"{"trusted":1,"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, Some(1));
    }

    #[test]
    fn test_no_trusted() {
        let store_ping = r#"{"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, None);
    }

    #[test]
    fn test_not_trusted() {
        let store_ping = r#"{"trusted":0,"url":"daemon","version":"2.18.1"}"#;
        let store_ping: StorePing = serde_json::from_str(store_ping).unwrap();
        assert_eq!(store_ping.trusted, Some(0));
    }
}
