use crate::{cli, config, log};
use miette::{bail, IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Nix<'a> {
    logger: log::Logger,
    pub options: Options<'a>,
    // TODO: all these shouldn't be here
    config: Arc<config::Config>,
    global_options: Arc<cli::GlobalOptions>,
    cachix_caches: Option<CachixCaches>,
    cachix_trusted_keys: Arc<PathBuf>,
    devenv_home_gc: Arc<PathBuf>,
    devenv_dot_gc: Arc<PathBuf>,
    devenv_root: Arc<PathBuf>,
}

#[derive(Copy, Clone)]
pub struct Options<'a> {
    pub replace_shell: bool,
    pub logging: bool,
    pub logging_stdout: bool,
    pub nix_flags: &'a [&'a str],
}

impl<'a> Nix<'a> {
    pub fn new(
        logger: log::Logger,
        config: Arc<config::Config>,
        global_options: Arc<cli::GlobalOptions>,
        cachix_trusted_keys: Arc<PathBuf>,
        devenv_home_gc: Arc<PathBuf>,
        devenv_dot_gc: Arc<PathBuf>,
        devenv_root: Arc<PathBuf>,
    ) -> Self {
        Nix {
            logger,
            cachix_caches: None,
            config,
            global_options,
            options: Options {
                replace_shell: false,
                logging: true,
                logging_stdout: false,
                nix_flags: &[
                    "--show-trace",
                    "--extra-experimental-features",
                    "nix-command",
                    "--extra-experimental-features",
                    "flakes",
                    "--option",
                    "warn-dirty",
                    "false",
                    "--keep-going",
                ],
            },
            cachix_trusted_keys,
            devenv_home_gc,
            devenv_dot_gc,
            devenv_root,
        }
    }

    pub async fn develop(&mut self, args: &[&str], replace_shell: bool) -> Result<process::Output> {
        let options = Options {
            logging_stdout: true,
            replace_shell,
            ..self.options
        };
        self.run_nix_with_substituters("nix", &args, &options).await
    }

    pub async fn dev_env(&mut self, json: bool, gc_root: &PathBuf) -> Result<Vec<u8>> {
        let gc_root_str = gc_root.to_str().expect("gc root should be utf-8");
        let mut args: Vec<&str> = vec!["print-dev-env", "--profile", gc_root_str];
        if json {
            args.push("--json");
        }

        let options = Options { ..self.options };
        let env = self
            .run_nix_with_substituters("nix", &args, &options)
            .await?;

        let options = Options {
            logging: false,
            ..self.options
        };

        let args: Vec<&str> = vec!["-p", gc_root_str, "--delete-generations", "old"];
        self.run_nix("nix-env", &args, &options)?;
        let now_ns = get_now_with_nanoseconds();
        let target = format!("{}-shell", now_ns);
        symlink_force(
            &self.logger,
            &fs::canonicalize(&gc_root).expect("to resolve gc_root"),
            &self.devenv_home_gc.join(target),
        );
        Ok(env.stdout)
    }

    pub fn add_gc(&mut self, name: &str, path: &Path) -> Result<()> {
        let options = self.options;
        self.run_nix(
            "nix-store",
            &[
                "--add-root",
                self.devenv_dot_gc.join(name).to_str().unwrap(),
                "-r",
                path.to_str().unwrap(),
            ],
            &options,
        )?;
        let link_path = self
            .devenv_dot_gc
            .join(format!("{}-{}", name, get_now_with_nanoseconds()));
        symlink_force(&self.logger, path, &link_path);
        Ok(())
    }

    pub fn repl(&mut self) -> Result<()> {
        let options = self.options;
        let mut cmd = self.prepare_command("nix", &["repl", "."], &options)?;
        cmd.exec();
        Ok(())
    }

    pub async fn build(&mut self, attributes: &[&str]) -> Result<Vec<PathBuf>> {
        let options = self.options;
        if !attributes.is_empty() {
            // TODO: use eval underneath
            let mut args: Vec<String> = vec![
                "build".to_string(),
                "--no-link".to_string(),
                "--print-out-paths".to_string(),
            ];
            args.extend(attributes.iter().map(|attr| format!(".#{}", attr)));
            let args_str: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
            let output = self
                .run_nix_with_substituters("nix", &args_str, &options)
                .await?;
            Ok(String::from_utf8_lossy(&output.stdout)
                .to_string()
                .trim()
                .split_whitespace()
                .map(|s| PathBuf::from(s.to_string()))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn eval(&mut self, attributes: &[&str]) -> Result<String> {
        let mut args: Vec<String> = vec!["eval", "--json"]
            .into_iter()
            .map(String::from)
            .collect();
        args.extend(attributes.into_iter().map(|attr| format!(".#{}", attr)));
        let args = &args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let options = self.options;
        let result = self.run_nix("nix", &args, &options)?;
        String::from_utf8(result.stdout)
            .map_err(|err| miette::miette!("Failed to parse command output as UTF-8: {}", err))
    }

    pub fn update(&mut self, input_name: &Option<String>) -> Result<()> {
        let options = self.options;
        match input_name {
            Some(input_name) => {
                self.run_nix(
                    "nix",
                    &["flake", "lock", "--update-input", input_name],
                    &options,
                )?;
            }
            None => {
                self.run_nix("nix", &["flake", "update"], &options)?;
            }
        }
        Ok(())
    }

    pub fn metadata(&mut self) -> Result<String> {
        // TODO: use --json
        let options = self.options;
        let metadata = self.run_nix("nix", &["flake", "metadata"], &options)?;

        let re = regex::Regex::new(r"(Inputs:.+)$").unwrap();
        let metadata_str = String::from_utf8_lossy(&metadata.stdout);
        let inputs = match re.captures(&metadata_str) {
            Some(captures) => captures.get(1).unwrap().as_str(),
            None => "",
        };

        let info_ = self.run_nix("nix", &["eval", "--raw", ".#info"], &options)?;
        Ok(format!(
            "{}\n{}",
            inputs,
            &String::from_utf8_lossy(&info_.stdout)
        ))
    }

    pub async fn search(&mut self, name: &str) -> Result<process::Output> {
        let options = self.options;
        self.run_nix_with_substituters("nix", &["search", "--json", "nixpkgs", name], &options)
            .await
    }

    pub fn gc(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        let options = self.options;
        let paths: std::collections::HashSet<&str> = paths
            .iter()
            .filter_map(|path_buf| path_buf.to_str())
            .collect();
        for path in paths {
            self.logger.info(&format!("Deleting {}...", path));
            let args: Vec<&str> = ["store", "delete", path].iter().copied().collect();
            let cmd = self.prepare_command("nix", &args, &options);
            // we ignore if this command fails, because root might be in use
            let _ = cmd?.output();
        }
        Ok(())
    }

    // Run Nix with debugger capability and return the output
    pub fn run_nix(
        &mut self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<process::Output> {
        let cmd = self.prepare_command(command, args, options)?;
        self.run_nix_command(cmd, options)
    }

    pub async fn run_nix_with_substituters(
        &mut self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<process::Output> {
        let cmd = self
            .prepare_command_with_substituters(command, args, options)
            .await?;
        self.run_nix_command(cmd, options)
    }

    fn run_nix_command(
        &mut self,
        mut cmd: std::process::Command,
        options: &Options<'a>,
    ) -> Result<process::Output> {
        let prev_level = self.logger.level.clone();
        if !options.logging {
            self.logger.level = log::Level::Error;
        }

        if options.replace_shell {
            if self.global_options.nix_debugger
                && cmd.get_program().to_string_lossy().ends_with("bin/nix")
            {
                cmd.arg("--debugger");
            }
            let error = cmd.exec();
            self.logger.error(&format!(
                "Failed to replace shell with `{}`: {error}",
                display_command(&cmd),
            ));
            bail!("Failed to replace shell")
        } else {
            if options.logging {
                cmd.stdin(process::Stdio::inherit())
                    .stderr(process::Stdio::inherit());
                if options.logging_stdout {
                    cmd.stdout(std::process::Stdio::inherit());
                }
            }

            let result = cmd
                .output()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to run command `{}`", display_command(&cmd)))?;

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
                if self.global_options.nix_debugger
                    && cmd.get_program().to_string_lossy().ends_with("bin/nix")
                {
                    self.logger.info("Starting Nix debugger ...");
                    cmd.arg("--debugger").exec();
                }
                bail!(format!(
                    "Command `{}` failed with {code}",
                    display_command(&cmd)
                ))
            } else {
                self.logger.level = prev_level;
                Ok(result)
            }
        }
    }

    // We have a separate function to avoid recursion as this needs to call self.prepare_command
    // TODO: doesn't log the substituters
    pub async fn prepare_command_with_substituters(
        &mut self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<std::process::Command> {
        let mut cmd = self.prepare_command(command, args, options)?;
        if !self.global_options.offline {
            let cachix_caches = self.get_cachix_caches().await;

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
                        if env::var("CACHIX_AUTH_TOKEN").is_ok() {
                            let args = cmd
                                .get_args()
                                .map(|arg| arg.to_str().unwrap())
                                .collect::<Vec<_>>();
                            let envs = cmd.get_envs().collect::<Vec<_>>();
                            let command_name = cmd.get_program().to_string_lossy();
                            let mut newcmd = std::process::Command::new("cachix");
                            newcmd
                                .args(["watch-exec", &push_cache, "--"])
                                .arg(command_name.as_ref())
                                .args(args);
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
        Ok(cmd)
    }

    pub fn prepare_command(
        &mut self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<std::process::Command> {
        let mut flags = options.nix_flags.to_vec();
        flags.push("--max-jobs");
        let max_jobs = self.global_options.max_jobs.to_string();
        flags.push(&max_jobs);

        flags.push("--option");
        flags.push("eval-cache");
        let eval_cache = self.global_options.eval_cache.to_string();
        flags.push(&eval_cache);

        // handle --nix-option key value
        for chunk in self.global_options.nix_option.chunks_exact(2) {
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
                self.logger
                    .error("Please follow https://devenv.sh/getting-started/ to install devenv.");
                bail!("$DEVENV_NIX is not set")
            }
        };

        if self.global_options.offline && command == "nix" {
            flags.push("--offline");
        }

        if self.global_options.impure || self.config.impure {
            // only pass the impure option to the nix command that supports it.
            // avoid passing it to the older utilities, e.g. like `nix-store` when creating GC roots.
            if command == "nix"
                && args
                    .first()
                    .map(|arg| arg == &"build" || arg == &"eval" || arg == &"print-dev-env")
                    .unwrap_or(false)
            {
                flags.push("--no-pure-eval");
            }
            // set a dummy value to overcome https://github.com/NixOS/nix/issues/10247
            cmd.env("NIX_PATH", ":");
        }
        cmd.args(flags);
        cmd.current_dir(&self.devenv_root.as_path());

        if self.global_options.verbose {
            self.logger
                .debug(&format!("Running command: {}", display_command(&cmd)));
        }
        Ok(cmd)
    }

    async fn get_cachix_caches(&mut self) -> Result<CachixCaches> {
        match &self.cachix_caches {
            Some(caches) => Ok(caches.clone()),
            None => {
                let no_logging = Options {
                    logging: false,
                    ..self.options
                };
                let caches_raw = self.eval(&["devenv.cachix"]).await?;
                let cachix = serde_json::from_str(&caches_raw).expect("Failed to parse JSON");
                let known_keys = if let Ok(known_keys) =
                    std::fs::read_to_string(&self.cachix_trusted_keys.as_path())
                {
                    serde_json::from_str(&known_keys).expect("Failed to parse JSON")
                } else {
                    HashMap::new()
                };

                let mut caches = CachixCaches {
                    caches: cachix,
                    known_keys,
                };

                let mut new_known_keys: HashMap<String, String> = HashMap::new();
                let client = reqwest::Client::new();
                for name in caches.caches.pull.iter() {
                    if !caches.known_keys.contains_key(name) {
                        let mut request =
                            client.get(&format!("https://cachix.org/api/v1/cache/{}", name));
                        if let Ok(ret) = env::var("CACHIX_AUTH_TOKEN") {
                            request = request.bearer_auth(ret);
                        }
                        let resp = request.send().await.expect("Failed to get cache");
                        if resp.status().is_client_error() {
                            self.logger.error(&format!(
                                "Cache {} does not exist or you don't have a CACHIX_AUTH_TOKEN configured.",
                                name
                            ));
                            self.logger
                                .error("To create a cache, go to https://app.cachix.org/.");
                            bail!("Cache does not exist or you don't have a CACHIX_AUTH_TOKEN configured.")
                        } else {
                            let resp_json = serde_json::from_slice::<CachixResponse>(
                                &resp.bytes().await.unwrap(),
                            )
                            .expect("Failed to parse JSON");
                            new_known_keys
                                .insert(name.clone(), resp_json.public_signing_keys[0].clone());
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
                        &self.cachix_trusted_keys.as_path(),
                        serde_json::to_string(&caches.known_keys).unwrap(),
                    )
                    .expect("Failed to write cachix caches to file");

                    if trusted == Some(0) {
                        if !Path::new("/etc/NIXOS").exists() {
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
                        } else {
                            self.logger.error(&indoc::formatdoc!(
                            "You're not a trusted user of the Nix store. You have the following options:

                            a) Add yourself to the trusted-users list in /etc/nix/nix.conf by editing configuration.nix for devenv to manage caches for you.

                            {{
                                nix.extraOptions = ''
                                    trusted-users = root {}
                                '';
                            }}

                            b) Add binary caches to /etc/nix/nix.conf yourself by editing configuration.nix:
                            {{
                                nix.extraOptions = ''
                                    extra-substituters = {};
                                    extra-trusted-public-keys = {};
                                '';
                            }}

                            Lastly rebuild your system

                            $ sudo nixos-rebuild switch
                        ", whoami::username()
                        , caches.caches.pull.iter().map(|cache| format!("https://{}.cachix.org", cache)).collect::<Vec<String>>().join(" ")
                        , caches.known_keys.values().cloned().collect::<Vec<String>>().join(" ")
                        ));
                        }
                        bail!("You're not a trusted user of the Nix store.")
                    }
                }

                self.cachix_caches = Some(caches.clone());
                Ok(caches)
            }
        }
    }
}

fn symlink_force(logger: &log::Logger, link_path: &Path, target: &Path) {
    let _lock = dotlock::Dotlock::create(target.with_extension("lock")).unwrap();
    logger.debug(&format!(
        "Creating symlink {} -> {}",
        link_path.display(),
        target.display()
    ));

    if target.exists() {
        fs::remove_file(target).unwrap_or_else(|_| panic!("Failed to remove {}", target.display()));
    }

    symlink(link_path, target).unwrap_or_else(|_| {
        panic!(
            "Failed to create symlink: {} -> {}",
            link_path.display(),
            target.display()
        )
    });
}

fn get_now_with_nanoseconds() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    let secs = duration.as_secs();
    let nanos = duration.subsec_nanos();
    format!("{}.{}", secs, nanos)
}

// Display a command as a pretty string.
fn display_command(cmd: &std::process::Command) -> String {
    let command = cmd.get_program().to_string_lossy();
    let args = cmd
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{command} {args}")
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
    #[serde(rename = "publicSigningKeys")]
    public_signing_keys: Vec<String>,
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
