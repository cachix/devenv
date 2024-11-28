use crate::{cli, config};
use miette::{bail, IntoDiagnostic, Result, WrapErr};
use nix_conf_parser::NixConf;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

pub struct Nix<'a> {
    pub options: Options<'a>,
    pool: SqlitePool,
    // TODO: all these shouldn't be here
    config: config::Config,
    global_options: cli::GlobalOptions,
    cachix_caches: RefCell<Option<CachixCaches>>,
    cachix_trusted_keys: PathBuf,
    devenv_home_gc: PathBuf,
    devenv_dot_gc: PathBuf,
    devenv_dotfile: PathBuf,
    devenv_root: PathBuf,
}

#[derive(Clone)]
pub struct Options<'a> {
    /// Run `exec` to replace the shell with the command.
    pub replace_shell: bool,
    /// Error out if the command returns a non-zero status code.
    pub bail_on_error: bool,
    /// Cache the output of the command. This is opt-in per command.
    pub cache_output: bool,
    /// Enable logging.
    pub logging: bool,
    /// Log the stdout of the command.
    pub logging_stdout: bool,
    /// Extra flags to pass to nix commands.
    pub nix_flags: &'a [&'a str],
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            replace_shell: false,
            bail_on_error: true,
            // Individual commands opt into caching
            cache_output: false,
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
        }
    }
}

impl<'a> Nix<'a> {
    pub async fn new<P: AsRef<Path>>(
        config: config::Config,
        global_options: cli::GlobalOptions,
        cachix_trusted_keys: P,
        devenv_home_gc: P,
        devenv_dotfile: P,
        devenv_dot_gc: P,
        devenv_root: P,
    ) -> Result<Self> {
        let cachix_trusted_keys = cachix_trusted_keys.as_ref().to_path_buf();
        let devenv_home_gc = devenv_home_gc.as_ref().to_path_buf();
        let devenv_dotfile = devenv_dotfile.as_ref().to_path_buf();
        let devenv_dot_gc = devenv_dot_gc.as_ref().to_path_buf();
        let devenv_root = devenv_root.as_ref().to_path_buf();

        let cachix_caches = RefCell::new(None);
        let options = Options::default();

        let database_url = format!(
            "sqlite:{}/nix-eval-cache.db",
            devenv_dotfile.to_string_lossy()
        );
        let pool = devenv_eval_cache::db::setup_db(database_url)
            .await
            .into_diagnostic()?;

        Ok(Self {
            options,
            pool,
            config,
            global_options,
            cachix_caches,
            cachix_trusted_keys,
            devenv_home_gc,
            devenv_dot_gc,
            devenv_dotfile,
            devenv_root,
        })
    }

    pub async fn develop(
        &self,
        args: &[&str],
        replace_shell: bool,
    ) -> Result<devenv_eval_cache::Output> {
        let options = Options {
            logging_stdout: true,
            // Cannot cache this because we don't get the derivation back.
            // We'd need to switch to print-dev-env and our own `nix develop`.
            cache_output: false,
            bail_on_error: false,
            replace_shell,
            ..self.options
        };
        self.run_nix_with_substituters("nix", args, &options).await
    }

    pub async fn dev_env(
        &self,
        json: bool,
        gc_root: &PathBuf,
    ) -> Result<devenv_eval_cache::Output> {
        let options = Options {
            cache_output: true,
            ..self.options
        };
        let gc_root_str = gc_root.to_str().expect("gc root should be utf-8");
        let mut args: Vec<&str> = vec!["print-dev-env", "--profile", gc_root_str];
        if json {
            args.push("--json");
        }
        let env = self
            .run_nix_with_substituters("nix", &args, &options)
            .await?;

        let options = Options {
            logging: false,
            ..self.options
        };

        let args: Vec<&str> = vec!["-p", gc_root_str, "--delete-generations", "old"];
        self.run_nix("nix-env", &args, &options).await?;
        let now_ns = get_now_with_nanoseconds();
        let target = format!("{}-shell", now_ns);
        symlink_force(
            &fs::canonicalize(gc_root).expect("to resolve gc_root"),
            &self.devenv_home_gc.join(target),
        );

        Ok(env)
    }

    pub async fn add_gc(&self, name: &str, path: &Path) -> Result<()> {
        self.run_nix(
            "nix-store",
            &[
                "--add-root",
                self.devenv_dot_gc.join(name).to_str().unwrap(),
                "-r",
                path.to_str().unwrap(),
            ],
            &self.options,
        )
        .await?;
        let link_path = self
            .devenv_dot_gc
            .join(format!("{}-{}", name, get_now_with_nanoseconds()));
        symlink_force(path, &link_path);
        Ok(())
    }

    pub fn repl(&self) -> Result<()> {
        let mut cmd = self.prepare_command("nix", &["repl", "."], &self.options)?;
        cmd.exec();
        Ok(())
    }

    pub async fn build(&self, attributes: &[&str]) -> Result<Vec<PathBuf>> {
        if attributes.is_empty() {
            return Ok(Vec::new());
        }

        let options = Options {
            cache_output: true,
            ..self.options
        };
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
            .split_whitespace()
            .map(|s| PathBuf::from(s.to_string()))
            .collect())
    }

    pub async fn eval(&self, attributes: &[&str]) -> Result<String> {
        let options = Options {
            cache_output: true,
            ..self.options
        };
        let mut args: Vec<String> = vec!["eval", "--json"]
            .into_iter()
            .map(String::from)
            .collect();
        args.extend(attributes.iter().map(|attr| format!(".#{}", attr)));
        let args = &args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let result = self.run_nix("nix", args, &options).await?;
        String::from_utf8(result.stdout)
            .map_err(|err| miette::miette!("Failed to parse command output as UTF-8: {}", err))
    }

    pub async fn update(&self, input_name: &Option<String>) -> Result<()> {
        let mut args = vec!["flake", "update"];

        if let Some(input_name) = input_name {
            args.push(input_name);
        }

        self.run_nix("nix", &args, &self.options).await?;

        Ok(())
    }

    pub async fn metadata(&self) -> Result<String> {
        let options = Options {
            cache_output: true,
            ..self.options
        };

        // TODO: use --json
        let metadata = self
            .run_nix("nix", &["flake", "metadata"], &options)
            .await?;

        let re = regex::Regex::new(r"(Inputs:.+)$").unwrap();
        let metadata_str = String::from_utf8_lossy(&metadata.stdout);
        let inputs = match re.captures(&metadata_str) {
            Some(captures) => captures.get(1).unwrap().as_str(),
            None => "",
        };

        let info_ = self
            .run_nix("nix", &["eval", "--raw", ".#info"], &options)
            .await?;
        Ok(format!(
            "{}\n{}",
            inputs,
            &String::from_utf8_lossy(&info_.stdout)
        ))
    }

    pub async fn search(&self, name: &str) -> Result<devenv_eval_cache::Output> {
        self.run_nix_with_substituters(
            "nix",
            &["search", "--inputs-from", ".", "--json", "nixpkgs", name],
            &self.options,
        )
        .await
    }

    pub fn gc(&self, paths: Vec<PathBuf>) -> Result<()> {
        let paths: std::collections::HashSet<&str> = paths
            .iter()
            .filter_map(|path_buf| path_buf.to_str())
            .collect();
        for path in paths {
            info!("Deleting {}...", path);
            let args: Vec<&str> = ["store", "delete", path].to_vec();
            let cmd = self.prepare_command("nix", &args, &self.options);
            // we ignore if this command fails, because root might be in use
            let _ = cmd?.output();
        }
        Ok(())
    }

    // Run Nix with debugger capability and return the output
    pub async fn run_nix(
        &self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<devenv_eval_cache::Output> {
        let cmd = self.prepare_command(command, args, options)?;
        self.run_nix_command(cmd, options).await
    }

    pub async fn run_nix_with_substituters(
        &self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<devenv_eval_cache::Output> {
        let cmd = self
            .prepare_command_with_substituters(command, args, options)
            .await?;
        self.run_nix_command(cmd, options).await
    }

    async fn run_nix_command(
        &self,
        mut cmd: std::process::Command,
        options: &Options<'a>,
    ) -> Result<devenv_eval_cache::Output> {
        use devenv_eval_cache::internal_log::Verbosity;
        use devenv_eval_cache::{supports_eval_caching, CachedCommand};

        if options.replace_shell {
            if self.global_options.nix_debugger
                && cmd.get_program().to_string_lossy().ends_with("bin/nix")
            {
                cmd.arg("--debugger");
            }
            let error = cmd.exec();
            error!(
                "Failed to replace shell with `{}`: {error}",
                display_command(&cmd),
            );
            bail!("Failed to replace shell")
        }

        if options.logging {
            cmd.stdin(process::Stdio::inherit())
                .stderr(process::Stdio::inherit());
            if options.logging_stdout {
                cmd.stdout(std::process::Stdio::inherit());
            }
        }

        let result = if self.global_options.eval_cache
            && options.cache_output
            && supports_eval_caching(&cmd)
        {
            let mut cached_cmd = CachedCommand::new(&self.pool);

            cached_cmd.watch_path(self.devenv_root.join("devenv.yaml"));
            cached_cmd.watch_path(self.devenv_root.join("devenv.lock"));

            cached_cmd.unwatch_path(self.devenv_root.join(".devenv.flake.nix"));
            // Ignore anything in .devenv.
            cached_cmd.unwatch_path(&self.devenv_dotfile);

            if self.global_options.refresh_eval_cache {
                cached_cmd.force_refresh();
            }

            if options.logging && !self.global_options.quiet {
                // Show eval and build logs only in verbose mode
                let target_log_level = if self.global_options.verbose {
                    Verbosity::Talkative
                } else {
                    Verbosity::Warn
                };

                cached_cmd.on_stderr(move |log| {
                    if let Some(log) = log.filter_by_level(target_log_level) {
                        if let Some(msg) = log.get_msg() {
                            use devenv_eval_cache::internal_log::InternalLog;
                            match log {
                                InternalLog::Msg { level, .. } => match *level {
                                    Verbosity::Error => error!("{msg}"),
                                    Verbosity::Warn => warn!("{msg}"),
                                    Verbosity::Talkative => debug!("{msg}"),
                                    _ => info!("{msg}"),
                                },
                                _ => info!("{msg}"),
                            };
                        }
                    }
                });
            }

            cached_cmd
                .output(&mut cmd)
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to run command `{}`", display_command(&cmd)))?
        } else {
            let output = cmd
                .output()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to run command `{}`", display_command(&cmd)))?;

            devenv_eval_cache::Output {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                paths: vec![],
            }
        };

        if !result.status.success() {
            let code = match result.status.code() {
                Some(code) => format!("with exit code {}", code),
                None => "without exit code".to_string(),
            };

            if !options.logging {
                error!(
                    "Command produced the following output:\n{}\n{}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr),
                );
            }

            if self.global_options.nix_debugger
                && cmd.get_program().to_string_lossy().ends_with("bin/nix")
            {
                info!("Starting Nix debugger ...");
                cmd.arg("--debugger").exec();
            }

            if options.bail_on_error {
                bail!(format!(
                    "Command `{}` failed with {code}",
                    display_command(&cmd)
                ))
            }
        }

        Ok(result)
    }

    // We have a separate function to avoid recursion as this needs to call self.prepare_command
    pub async fn prepare_command_with_substituters(
        &self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<std::process::Command> {
        let mut final_args = Vec::new();
        let known_keys;
        let pull_caches;
        let mut push_cache = None;

        if !self.global_options.offline {
            let cachix_caches = self.get_cachix_caches().await;

            match cachix_caches {
                Err(e) => {
                    warn!("Failed to get cachix caches due to evaluation error");
                    debug!("{}", e);
                }
                Ok(cachix_caches) => {
                    push_cache = cachix_caches.caches.push.clone();
                    // handle cachix.pull
                    if !cachix_caches.caches.pull.is_empty() {
                        pull_caches = cachix_caches
                            .caches
                            .pull
                            .iter()
                            .map(|cache| format!("https://{}.cachix.org", cache))
                            .collect::<Vec<String>>()
                            .join(" ");
                        final_args.extend_from_slice(&[
                            "--option",
                            "extra-substituters",
                            &pull_caches,
                        ]);
                        known_keys = cachix_caches
                            .known_keys
                            .values()
                            .cloned()
                            .collect::<Vec<String>>()
                            .join(" ");
                        final_args.extend_from_slice(&[
                            "--option",
                            "extra-trusted-public-keys",
                            &known_keys,
                        ]);
                    }
                }
            }
        }

        final_args.extend(args.iter().copied());
        let cmd = self.prepare_command(command, &final_args, options)?;

        // handle cachix.push
        if let Some(push_cache) = push_cache {
            if env::var("CACHIX_AUTH_TOKEN").is_ok() {
                let original_command = cmd.get_program().to_string_lossy().to_string();
                let mut new_cmd = std::process::Command::new("cachix");
                let push_args = vec![
                    "watch-exec".to_string(),
                    push_cache.clone(),
                    "--".to_string(),
                    original_command,
                ];
                new_cmd.args(&push_args);
                new_cmd.args(cmd.get_args());
                // make sure to copy all env vars
                for (key, value) in cmd.get_envs() {
                    if let Some(value) = value {
                        new_cmd.env(key, value);
                    }
                }
                new_cmd.current_dir(cmd.get_current_dir().unwrap_or_else(|| Path::new(".")));
                return Ok(new_cmd);
            } else {
                warn!(
                    "CACHIX_AUTH_TOKEN is not set, but required to push to {}.",
                    push_cache
                );
            }
        }
        Ok(cmd)
    }

    pub fn prepare_command(
        &self,
        command: &str,
        args: &[&str],
        options: &Options<'a>,
    ) -> Result<std::process::Command> {
        let mut flags = options.nix_flags.to_vec();
        flags.push("--max-jobs");
        let max_jobs = self.global_options.max_jobs.to_string();
        flags.push(&max_jobs);

        // Disable the flake eval cache.
        flags.push("--option");
        flags.push("eval-cache");
        flags.push("false");

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
                error!(
            "$DEVENV_NIX is not set, but required as devenv doesn't work without a few Nix patches."
            );
                error!("Please follow https://devenv.sh/getting-started/ to install devenv.");
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
                    .iter()
                    .any(|&arg| arg == "build" || arg == "eval" || arg == "print-dev-env")
            {
                flags.push("--no-pure-eval");
            }
            // set a dummy value to overcome https://github.com/NixOS/nix/issues/10247
            cmd.env("NIX_PATH", ":");
        }
        cmd.args(flags);
        cmd.current_dir(&self.devenv_root);

        if self.global_options.verbose {
            debug!("Running command: {}", display_command(&cmd));
        }
        Ok(cmd)
    }

    async fn get_nix_config(&self) -> Result<NixConf> {
        let options = Options {
            logging: false,
            ..self.options
        };
        let raw_conf = self.run_nix("nix", &["config", "show"], &options).await?;
        let nix_conf = NixConf::parse_stdout(&raw_conf.stdout)?;
        Ok(nix_conf)
    }

    async fn get_cachix_caches(&self) -> Result<Ref<CachixCaches>> {
        if self.cachix_caches.borrow().is_none() {
            let no_logging = Options {
                logging: false,
                ..self.options
            };
            let caches_raw = self.eval(&["devenv.cachix"]).await?;
            let cachix = serde_json::from_str(&caches_raw).expect("Failed to parse JSON");
            let known_keys = if let Ok(known_keys) =
                std::fs::read_to_string(self.cachix_trusted_keys.as_path())
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
                        error!(
                        "Cache {} does not exist or you don't have a CACHIX_AUTH_TOKEN configured.",
                        name
                    );
                        error!("To create a cache, go to https://app.cachix.org/.");
                        bail!("Cache does not exist or you don't have a CACHIX_AUTH_TOKEN configured.")
                    } else {
                        let resp_json =
                            serde_json::from_slice::<CachixResponse>(&resp.bytes().await.unwrap())
                                .expect("Failed to parse JSON");
                        new_known_keys
                            .insert(name.clone(), resp_json.public_signing_keys[0].clone());
                    }
                }
            }

            if !caches.caches.pull.is_empty() {
                let store = self
                    .run_nix("nix", &["store", "ping", "--json"], &no_logging)
                    .await?;
                let trusted = serde_json::from_slice::<StorePing>(&store.stdout)
                    .expect("Failed to parse JSON")
                    .trusted;
                if trusted.is_none() {
                    warn!(
                    "You're using an outdated version of Nix. Please upgrade and restart the nix-daemon.",
                );
                }
                let restart_command = if cfg!(target_os = "linux") {
                    "sudo systemctl restart nix-daemon"
                } else {
                    "sudo launchctl kickstart -k system/org.nixos.nix-daemon"
                };

                info!(
                    devenv.is_user_message = true,
                    "Using Cachix: {}",
                    caches.caches.pull.join(", "),
                );
                if !new_known_keys.is_empty() {
                    for (name, pubkey) in new_known_keys.iter() {
                        info!(
                            "Trusting {}.cachix.org on first use with the public key {}",
                            name, pubkey
                        );
                    }
                    caches.known_keys.extend(new_known_keys);
                }

                std::fs::write(
                    self.cachix_trusted_keys.as_path(),
                    serde_json::to_string(&caches.known_keys).unwrap(),
                )
                .expect("Failed to write cachix caches to file");

                // If the user is not a trusted user, we can't set up the caches for them.
                // Check if all of the requested caches and their public keys are in the substituters and trusted-public-keys lists.
                // If not, suggest actions to remedy the issue.
                if trusted == Some(0) {
                    let mut missing_caches = Vec::new();
                    let mut missing_public_keys = Vec::new();

                    if let Ok(nix_conf) = self.get_nix_config().await {
                        let substituters = nix_conf
                            .get("substituters")
                            .map(|s| s.split_whitespace().collect::<Vec<_>>());

                        if let Some(substituters) = substituters {
                            for cache in caches.caches.pull.iter() {
                                let cache_url = format!("https://{}.cachix.org", cache);
                                if !substituters.iter().any(|s| s == &cache_url) {
                                    missing_caches.push(cache_url);
                                }
                            }
                        }

                        let trusted_public_keys = nix_conf
                            .get("trusted-public-keys")
                            .map(|s| s.split_whitespace().collect::<Vec<_>>());

                        if let Some(trusted_public_keys) = trusted_public_keys {
                            for (_name, key) in caches.known_keys.iter() {
                                if !trusted_public_keys.iter().any(|p| p == key) {
                                    missing_public_keys.push(key.clone());
                                }
                            }
                        }
                    }

                    if !missing_caches.is_empty() || !missing_public_keys.is_empty() {
                        if !Path::new("/etc/NIXOS").exists() {
                            error!("{}", indoc::formatdoc!(
                        "Failed to set up binary caches:

                           {}

                        devenv is configured to automatically manage binary caches with `cachix.enable = true`, but cannot do so because you are not a trusted user of the Nix store.

                        You have several options:

                        a) To let devenv set up the caches for you, add yourself to the trusted-users list in /etc/nix/nix.conf:

                             trusted-users = root {}

                           Then restart the nix-daemon:

                             $ {restart_command}

                        b) Add the missing binary caches to /etc/nix/nix.conf yourself:

                             extra-substituters = {}
                             extra-trusted-public-keys = {}

                        c) Disable automatic cache management in your devenv configuration:

                             {{
                               cachix.enable = false;
                             }}
                    "
                    , missing_caches.join(" ")
                    , whoami::username()
                    , missing_caches.join(" ")
                    , missing_public_keys.join(" ")
                    ));
                        } else {
                            error!("{}", indoc::formatdoc!(
                        "Failed to set up binary caches:

                           {}

                        devenv is configured to automatically manage binary caches with `cachix.enable = true`, but cannot do so because you are not a trusted user of the Nix store.

                        You have several options:

                        a) To let devenv set up the caches for you, add yourself to the trusted-users list in /etc/nix/nix.conf by editing configuration.nix.

                             {{
                               nix.settings.trusted-users = [ \"root\" \"{}\" ];
                             }}

                           Rebuild your system:

                             $ sudo nixos-rebuild switch

                        b) Add the missing binary caches to /etc/nix/nix.conf yourself by editing configuration.nix:

                             {{
                               nix.extraOptions = ''
                                 extra-substituters = {}
                                 extra-trusted-public-keys = {}
                               '';
                             }}

                           Rebuild your system:

                             $ sudo nixos-rebuild switch

                        c) Disable automatic cache management in your devenv configuration:

                             {{
                               cachix.enable = false;
                             }}
                    "
                    , missing_caches.join(" ")
                    , whoami::username()
                    , missing_caches.join(" ")
                    , missing_public_keys.join(" ")
                    ));
                        }

                        bail!("You're not a trusted user of the Nix store.")
                    }
                }
            }

            *self.cachix_caches.borrow_mut() = Some(caches);
        }

        Ok(Ref::map(self.cachix_caches.borrow(), |option| {
            option.as_ref().unwrap()
        }))
    }
}

fn symlink_force(link_path: &Path, target: &Path) {
    let _lock = dotlock::Dotlock::create(target.with_extension("lock")).unwrap();

    debug!(
        "Creating symlink {} -> {}",
        link_path.display(),
        target.display()
    );

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
