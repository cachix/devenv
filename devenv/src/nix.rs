use crate::{
    cli, config, devenv,
    nix_backend::{self, NixBackend},
};
use async_trait::async_trait;
use futures::future;
use miette::{bail, IntoDiagnostic, Result, WrapErr};
use nix_conf_parser::NixConf;
use secretspec;
use serde::Deserialize;
use serde_json;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::OnceCell;
use tracing::{debug, debug_span, error, info, instrument, warn, Instrument};

pub struct Nix {
    pub options: nix_backend::Options,
    pool: Arc<OnceCell<SqlitePool>>,
    database_url: String,
    // TODO: all these shouldn't be here
    config: config::Config,
    global_options: cli::GlobalOptions,
    cachix_caches: Arc<OnceCell<CachixCaches>>,
    netrc_path: Arc<OnceCell<String>>,
    paths: nix_backend::DevenvPaths,
    secretspec_resolved: Arc<OnceCell<secretspec::Resolved<HashMap<String, String>>>>,
}

impl Nix {
    pub async fn new(
        config: config::Config,
        global_options: cli::GlobalOptions,
        paths: nix_backend::DevenvPaths,
        secretspec_resolved: Arc<OnceCell<secretspec::Resolved<HashMap<String, String>>>>,
    ) -> Result<Self> {
        let options = nix_backend::Options::default();

        let database_url = format!(
            "sqlite:{}/nix-eval-cache.db",
            paths.dotfile.to_string_lossy()
        );

        Ok(Self {
            options,
            pool: Arc::new(OnceCell::new()),
            database_url,
            config,
            global_options,
            cachix_caches: Arc::new(OnceCell::new()),
            netrc_path: Arc::new(OnceCell::new()),
            paths,
            secretspec_resolved,
        })
    }

    // Defer creating local project state
    pub async fn assemble(&self) -> Result<()> {
        self.pool
            .get_or_try_init(|| async {
                // Extract database path from URL
                let path = PathBuf::from(self.database_url.trim_start_matches("sqlite:"));

                // Connect to database and run migrations in one step
                let db =
                    devenv_cache_core::db::Database::new(path, &devenv_eval_cache::db::MIGRATIONS)
                        .await
                        .map_err(|e| miette::miette!("Failed to initialize database: {}", e))?;

                Ok::<_, miette::Report>(db.pool().clone())
            })
            .await?;

        Ok(())
    }

    pub async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<devenv_eval_cache::Output> {
        // Refresh the cache if the GC root is not a valid path.
        // This can happen if the store path is forcefully removed: GC'd or the Nix store is
        // tampered with.
        let refresh_cached_output = fs::canonicalize(gc_root).await.is_err();
        let options = nix_backend::Options {
            cache_output: true,
            refresh_cached_output,
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

        // Delete any old generations of this profile.
        let options = nix_backend::Options {
            logging: false,
            ..self.options
        };
        let args: Vec<&str> = vec!["-p", gc_root_str, "--delete-generations", "old"];
        self.run_nix("nix-env", &args, &options).await?;

        // Save the GC root for this profile.
        let now_ns = get_now_with_nanoseconds();
        let target = format!("{}-shell", now_ns);
        if let Ok(resolved_gc_root) = fs::canonicalize(gc_root).await {
            symlink_force(&resolved_gc_root, &self.paths.home_gc.join(target)).await?;
        } else {
            warn!(
                "Failed to resolve the GC root path to the Nix store: {}. Try running devenv again with --refresh-eval-cache.",
                gc_root.display()
            );
        }

        Ok(env)
    }

    /// Add a GC root for the given path.
    ///
    /// SAFETY
    ///
    /// You should prefer protecting build outputs with options like `--out-link` to avoid race conditions.
    /// A untimely GC run -- the usual culprit is auto-gc with min-free -- could delete the store
    /// path you're trying to protect.
    ///
    /// The `build` command supports an optional `gc_root` argument.
    pub async fn add_gc(&self, name: &str, path: &Path) -> Result<()> {
        self.run_nix(
            "nix-store",
            &[
                "--add-root",
                self.paths.dot_gc.join(name).to_str().unwrap(),
                "-r",
                path.to_str().unwrap(),
            ],
            &self.options,
        )
        .await?;
        let link_path = self
            .paths
            .dot_gc
            .join(format!("{}-{}", name, get_now_with_nanoseconds()));
        symlink_force(path, &link_path).await?;
        Ok(())
    }

    pub async fn repl(&self) -> Result<()> {
        let mut cmd = self.prepare_command("nix", &["repl", "."], &self.options)?;
        let _ = cmd.exec();
        Ok(())
    }

    pub async fn build(
        &self,
        attributes: &[&str],
        options: Option<nix_backend::Options>,
        gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>> {
        if attributes.is_empty() {
            return Ok(Vec::new());
        }

        let options = options.unwrap_or(nix_backend::Options {
            cache_output: true,
            ..self.options
        });

        // TODO: use eval underneath
        let mut args: Vec<String> = vec!["build".to_string()];

        // Add GC root or --no-link
        match gc_root {
            Some(root) => {
                args.push("--out-link".to_string());
                args.push(root.to_string_lossy().to_string());
            }
            None => {
                args.push("--no-link".to_string());
            }
        }

        args.push("--print-out-paths".to_string());
        args.push("-L".to_string());

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
        let options = nix_backend::Options {
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
        let options = nix_backend::Options {
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

    pub async fn search(
        &self,
        name: &str,
        options: Option<nix_backend::Options>,
    ) -> Result<devenv_eval_cache::Output> {
        let opts = options.as_ref().unwrap_or(&self.options);
        self.run_nix_with_substituters(
            "nix",
            &[
                "search",
                "--inputs-from",
                ".",
                "--quiet",
                "--option",
                "eval-cache",
                "true",
                "--json",
                "nixpkgs",
                name,
            ],
            opts,
        )
        .await
    }

    pub async fn gc(&self, paths: Vec<PathBuf>) -> Result<()> {
        let paths: std::collections::HashSet<&str> = paths
            .iter()
            .filter_map(|path_buf| path_buf.to_str())
            .collect();
        for path in paths {
            info!("Deleting {}...", path);
            let args: Vec<&str> = ["store", "delete", path].to_vec();
            // we ignore if this command fails, because root might be in use
            let _ = self.run_nix("nix", &args, &self.options).await;
        }
        Ok(())
    }

    // Run Nix with debugger capability and return the output
    pub async fn run_nix(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<devenv_eval_cache::Output> {
        let cmd = self.prepare_command(command, args, options)?;
        self.run_nix_command(cmd, options).await
    }

    pub async fn run_nix_with_substituters(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<devenv_eval_cache::Output> {
        let cmd = self
            .prepare_command_with_substituters(command, args, options)
            .await?;
        self.run_nix_command(cmd, options).await
    }

    #[instrument(skip(self), fields(output, cache_status))]
    async fn run_nix_command(
        &self,
        mut cmd: std::process::Command,
        options: &nix_backend::Options,
    ) -> Result<devenv_eval_cache::Output> {
        use devenv_eval_cache::internal_log::Verbosity;
        use devenv_eval_cache::{supports_eval_caching, CachedCommand};

        if options.replace_shell {
            if self.global_options.nix_debugger
                && cmd.get_program().to_string_lossy().ends_with("bin/nix")
            {
                cmd.arg("--debugger");
            }

            debug!("Running command: {}", display_command(&cmd));

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
            && self.pool.get().is_some()
        {
            let pool = self.pool.get().unwrap();
            let mut cached_cmd = CachedCommand::new(pool);

            cached_cmd.watch_path(self.paths.root.join(devenv::DEVENV_FLAKE));
            cached_cmd.watch_path(self.paths.root.join("devenv.yaml"));
            cached_cmd.watch_path(self.paths.root.join("devenv.lock"));
            cached_cmd.watch_path(self.paths.dotfile.join("flake.json"));
            cached_cmd.watch_path(self.paths.dotfile.join("cli-options.nix"));

            // Ignore anything in .devenv except for the specifically watched files above.
            cached_cmd.unwatch_path(&self.paths.dotfile);

            if self.global_options.refresh_eval_cache || options.refresh_cached_output {
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

            let pretty_cmd = display_command(&cmd);
            let span = debug_span!(
                "Running command",
                command = pretty_cmd.as_str(),
                devenv.user_message = format!("Running command: {}", pretty_cmd)
            );
            let output = cached_cmd
                .output(&mut cmd)
                .instrument(span)
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to run command `{}`", display_command(&cmd)))?;

            if output.cache_hit {
                tracing::Span::current().record(
                    "cache_status",
                    if output.cache_hit { "hit" } else { "miss" },
                );
            }

            output
        } else {
            let pretty_cmd = display_command(&cmd);
            let span = debug_span!(
                "Running command",
                command = pretty_cmd.as_str(),
                devenv.user_message = format!("Running command: {}", pretty_cmd)
            );
            let output = span.in_scope(|| {
                cmd.output()
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to run command `{}`", display_command(&cmd)))
            })?;

            devenv_eval_cache::Output {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                inputs: vec![],
                cache_hit: false,
            }
        };

        tracing::Span::current().record("output", format!("{:?}", result));

        if !result.status.success() {
            let code = match result.status.code() {
                Some(code) => format!("with exit code {}", code),
                None => "without an exit code".to_string(),
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
                let _ = cmd.arg("--debugger").exec();
            }

            if options.bail_on_error {
                bail!(format!("Command `{}` failed {code}", display_command(&cmd)))
            }
        }

        Ok(result)
    }

    // We have a separate function to avoid recursion as this needs to call self.prepare_command
    pub async fn prepare_command_with_substituters(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<std::process::Command> {
        let mut final_args = Vec::new();
        let known_keys_str;
        let pull_caches_str;
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
                        let mut pull_caches = cachix_caches
                            .caches
                            .pull
                            .iter()
                            .map(|cache| format!("https://{}.cachix.org", cache))
                            .collect::<Vec<String>>();
                        pull_caches.sort();
                        pull_caches_str = pull_caches.join(" ");
                        final_args.extend_from_slice(&[
                            "--option",
                            "extra-substituters",
                            &pull_caches_str,
                        ]);

                        let mut known_keys = cachix_caches
                            .known_keys
                            .values()
                            .cloned()
                            .collect::<Vec<String>>();
                        known_keys.sort();
                        known_keys_str = known_keys.join(" ");
                        final_args.extend_from_slice(&[
                            "--option",
                            "extra-trusted-public-keys",
                            &known_keys_str,
                        ]);
                    }

                    // Configure a netrc file with the auth token if available
                    if !cachix_caches.caches.pull.is_empty() {
                        if let Ok(auth_token) = env::var("CACHIX_AUTH_TOKEN") {
                            let netrc_path = self
                                .netrc_path
                                .get_or_try_init(|| async {
                                    let netrc_path = self.paths.dotfile.join("netrc");
                                    let netrc_path_str = netrc_path.to_string_lossy().to_string();

                                    self.create_netrc_file(
                                        &netrc_path,
                                        &cachix_caches.caches.pull,
                                        &auth_token,
                                    )
                                    .await?;

                                    Ok::<String, miette::Report>(netrc_path_str)
                                })
                                .await;

                            match netrc_path {
                                Ok(netrc_path) => {
                                    final_args.extend_from_slice(&[
                                        "--option",
                                        "netrc-file",
                                        netrc_path,
                                    ]);
                                }
                                Err(e) => {
                                    warn!("${e}")
                                }
                            }
                        }
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

    fn prepare_command(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<std::process::Command> {
        let mut flags = options.nix_flags.to_vec();
        flags.push("--max-jobs");
        let max_jobs = self.global_options.max_jobs.to_string();
        flags.push(&max_jobs);

        // Disable the flake eval cache.
        flags.push("--option");
        flags.push("eval-cache");
        flags.push("false");

        // Always allow substitutes to ensure Nix can download dependencies
        // See https://github.com/NixOS/nix/issues/4442
        flags.push("--option");
        flags.push("always-allow-substitutes");
        flags.push("true");

        // Set http-connections to 100 for better parallelism
        flags.push("--option");
        flags.push("http-connections");
        flags.push("100");

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

        // Pass secretspec data to Nix if available
        if let Some(resolved) = self.secretspec_resolved.get() {
            let secrets_data = serde_json::json!({
                "secrets": resolved.secrets,
                "profile": resolved.profile,
                "provider": resolved.provider
            });
            if let Ok(secrets_json) = serde_json::to_string(&secrets_data) {
                cmd.env("SECRETSPEC_SECRETS", secrets_json);
            }
        }

        cmd.args(flags);
        cmd.current_dir(&self.paths.root);

        Ok(cmd)
    }

    async fn get_nix_config(&self) -> Result<NixConf> {
        let options = nix_backend::Options {
            logging: false,
            ..self.options
        };
        let raw_conf = self.run_nix("nix", &["config", "show"], &options).await?;
        let nix_conf = NixConf::parse_stdout(&raw_conf.stdout)?;
        Ok(nix_conf)
    }

    async fn create_netrc_file(
        &self,
        netrc_path: &Path,
        pull_caches: &[String],
        auth_token: &str,
    ) -> Result<()> {
        let mut netrc_content = String::new();

        for cache in pull_caches {
            netrc_content.push_str(&format!(
                "machine {cache}.cachix.org\nlogin token\npassword {auth_token}\n\n",
            ));
        }

        // Create netrc file with restrictive permissions (600)
        {
            use tokio::io::AsyncWriteExt;

            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(netrc_path)
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to create netrc file at {}", netrc_path.display())
                })?;

            file.write_all(netrc_content.as_bytes())
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!("Failed to write netrc content to {}", netrc_path.display())
                })?;
        }

        Ok(())
    }

    async fn get_cachix_caches(&self) -> Result<CachixCaches> {
        self.cachix_caches
            .get_or_try_init(|| async {
        let no_logging = nix_backend::Options {
            logging: false,
            ..self.options
        };

        // Run Nix evaluation and file I/O concurrently
        let cachix_eval_future = self.eval(&["devenv.config.cachix"]);
        let trusted_keys_path = self.paths.cachix_trusted_keys.clone();
        let known_keys_future = tokio::fs::read_to_string(&trusted_keys_path);

        let (caches_raw, known_keys_result) = tokio::join!(cachix_eval_future, known_keys_future);

        let caches_raw = caches_raw?;
        let cachix_config: CachixConfig = serde_json::from_str(&caches_raw)
            .into_diagnostic()
            .wrap_err("Failed to parse the cachix configuration")?;

                // Return empty caches if the Cachix integration is disabled
                if !cachix_config.enable {
                    return Ok(CachixCaches::default());
                }

        let known_keys: BTreeMap<String, String> = known_keys_result
            .into_diagnostic()
            .and_then(|content| serde_json::from_str(&content).into_diagnostic())
            .unwrap_or_else(|e| {
                if let Some(source) = e.chain().find_map(|s| s.downcast_ref::<std::io::Error>()) {
                    if source.kind() != std::io::ErrorKind::NotFound {
                        error!(
                            "Failed to load cachix trusted keys from {}:\n{}.",
                            trusted_keys_path.display(),
                            e
                        );
                    }
                }
                BTreeMap::new()
            });

        let mut caches = CachixCaches {
            caches: cachix_config.caches,
            known_keys,
        };

        let client = reqwest::Client::builder()
            .use_preconfigured_tls(http_client_tls::tls_config())
            .build()
            .into_diagnostic()
            .wrap_err("Failed to create HTTP client to query the Cachix API")?;
        let mut new_known_keys: BTreeMap<String, String> = BTreeMap::new();

        // Collect caches that need their keys fetched
        let caches_to_fetch: Vec<&String> = caches
            .caches
            .pull
            .iter()
            .filter(|name| !caches.known_keys.contains_key(*name))
            .collect();

        if !caches_to_fetch.is_empty() {
            // Create futures for all HTTP requests
            let auth_token = env::var("CACHIX_AUTH_TOKEN").ok();
            let fetch_futures: Vec<_> = caches_to_fetch.into_iter().map(|name| {
                let client = &client;
                let auth_token = auth_token.as_ref();
                let name = name.clone();
                async move {
                    let result = async {
                        let mut request = client.get(format!("https://cachix.org/api/v1/cache/{}", name));
                        if let Some(token) = auth_token {
                            request = request.bearer_auth(token);
                        }
                        let resp = request.send().await.into_diagnostic().wrap_err_with(|| {
                            format!("Failed to fetch information for cache '{}'", name)
                        })?;
                        if resp.status().is_client_error() {
                            error!(
                                "Cache {} does not exist or you don't have a CACHIX_AUTH_TOKEN configured.",
                                name
                            );
                            error!("To create a cache, go to https://app.cachix.org/.");
                            bail!("Cache does not exist or you don't have a CACHIX_AUTH_TOKEN configured.")
                        } else {
                            let resp_json: CachixResponse =
                                resp.json().await.into_diagnostic().wrap_err_with(|| {
                                    format!("Failed to parse Cachix API response for cache '{name}'")
                                })?;
                            Ok::<String, miette::Report>(resp_json.public_signing_keys[0].clone())
                        }
                    }.await;

                    match result {
                        Ok(key) => Ok((name.clone(), key)),
                        Err(e) => Err(e.wrap_err(format!("Failed to fetch cache '{}'", name)))
                    }
                }
            }).collect();

            // Execute all requests concurrently
            let results = future::join_all(fetch_futures).await;

            for result in results {
                match result {
                    Ok((name, key)) => {
                        new_known_keys.insert(name, key);
                    }
                    Err(e) => {
                        error!("{}", e);
                    }
                }
            }
        }

        if !caches.caches.pull.is_empty() {
            // Run store ping and file write operations concurrently
            let store_ping_future = self.run_nix("nix", &["store", "ping", "--json"], &no_logging);
            let trusted_keys_path = self.paths.cachix_trusted_keys.clone();
            let write_keys_future = async {
                if !new_known_keys.is_empty() {
                    caches.known_keys.extend(new_known_keys.clone());
                    let json_content = serde_json::to_string(&caches.known_keys)
                        .into_diagnostic()
                        .wrap_err("Failed to serialize cachix trusted keys")?;
                    tokio::fs::write(&trusted_keys_path, json_content)
                        .await
                        .into_diagnostic()
                        .wrap_err_with(|| {
                            format!(
                                "Failed to write cachix trusted keys to {}",
                                trusted_keys_path.display()
                            )
                        })?;
                }
                Ok::<(), miette::Report>(())
            };

            let (store_result, write_result) = tokio::join!(store_ping_future, write_keys_future);
            let store = store_result?;
            write_result?;

            let store_ping = serde_json::from_slice::<StorePing>(&store.stdout)
                .into_diagnostic()
                .wrap_err("Failed to query the Nix store")?;
            let trusted = store_ping.trusted;
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
                "Using Cachix caches: {}",
                caches.caches.pull.join(", "),
            );
            if !new_known_keys.is_empty() {
                for (name, pubkey) in new_known_keys.iter() {
                    info!(
                        "Trusting {}.cachix.org on first use with the public key {}",
                        name, pubkey
                    );
                }
            }

            // If the user is not a trusted user, we can't set up the caches for them.
            // Check if all of the requested caches and their public keys are in the substituters and trusted-public-keys lists.
            // If not, suggest actions to remedy the issue.
            if trusted == Some(0) {
                let (missing_caches, missing_public_keys) = self
                    .get_nix_config()
                    .await
                    .map(|nix_conf| detect_missing_caches(&caches, nix_conf))
                    .unwrap_or_default();

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

                Ok::<_, miette::Report>(caches)
            })
            .await.cloned()
    }

    /// Clean up the netrc file if it was created during this session.
    ///
    /// This method safely removes the netrc file containing auth tokens,
    /// handling race conditions where the file might already be removed.
    /// Only attempts cleanup if a netrc file was actually created.
    fn cleanup_netrc(&self) {
        if let Some(netrc_path_str) = self.netrc_path.get() {
            let netrc_path = Path::new(netrc_path_str);
            match std::fs::remove_file(netrc_path) {
                Ok(()) => debug!("Removed netrc file: {}", netrc_path_str),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => warn!("Failed to remove netrc file {}: {}", netrc_path_str, e),
            }
        }
    }

    fn name(&self) -> &'static str {
        "nix"
    }
}

impl Drop for Nix {
    fn drop(&mut self) {
        self.cleanup_netrc();
    }
}

#[async_trait(?Send)]
impl NixBackend for Nix {
    async fn assemble(&self) -> Result<()> {
        self.assemble().await
    }

    async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<devenv_eval_cache::Output> {
        self.dev_env(json, gc_root).await
    }

    async fn add_gc(&self, name: &str, path: &Path) -> Result<()> {
        self.add_gc(name, path).await
    }

    async fn repl(&self) -> Result<()> {
        self.repl().await
    }

    async fn build(
        &self,
        attributes: &[&str],
        options: Option<nix_backend::Options>,
        gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>> {
        self.build(attributes, options, gc_root).await
    }

    async fn eval(&self, attributes: &[&str]) -> Result<String> {
        self.eval(attributes).await
    }

    async fn update(&self, input_name: &Option<String>) -> Result<()> {
        self.update(input_name).await
    }

    async fn metadata(&self) -> Result<String> {
        self.metadata().await
    }

    async fn search(
        &self,
        name: &str,
        options: Option<nix_backend::Options>,
    ) -> Result<devenv_eval_cache::Output> {
        self.search(name, options).await
    }

    async fn gc(&self, paths: Vec<PathBuf>) -> Result<()> {
        self.gc(paths).await
    }

    fn name(&self) -> &'static str {
        self.name()
    }

    async fn run_nix(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<devenv_eval_cache::Output> {
        self.run_nix(command, args, options).await
    }

    async fn run_nix_with_substituters(
        &self,
        command: &str,
        args: &[&str],
        options: &nix_backend::Options,
    ) -> Result<devenv_eval_cache::Output> {
        self.run_nix_with_substituters(command, args, options).await
    }
}

async fn symlink_force(link_path: &Path, target: &Path) -> Result<()> {
    let _lock = dotlock::Dotlock::create(target.with_extension("lock")).unwrap();

    debug!(
        "Creating symlink {} -> {}",
        link_path.display(),
        target.display()
    );

    if target.exists() {
        fs::remove_file(target)
            .await
            .map_err(|e| miette::miette!("Failed to remove {}: {}", target.display(), e))?;
    }

    symlink(link_path, target).map_err(|e| {
        miette::miette!(
            "Failed to create symlink: {} -> {}: {}",
            link_path.display(),
            target.display(),
            e
        )
    })?;

    Ok(())
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

/// The Cachix module configuration
#[derive(Deserialize, Default, Clone)]
pub struct CachixConfig {
    enable: bool,
    #[serde(flatten)]
    caches: Cachix,
}

#[derive(Deserialize, Default, Clone)]
pub struct Cachix {
    pub pull: Vec<String>,
    pub push: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct CachixCaches {
    caches: Cachix,
    known_keys: BTreeMap<String, String>,
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

fn detect_missing_caches(caches: &CachixCaches, nix_conf: NixConf) -> (Vec<String>, Vec<String>) {
    let mut missing_caches = Vec::new();
    let mut missing_public_keys = Vec::new();

    let substituters = nix_conf
        .get("substituters")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let extra_substituters = nix_conf
        .get("extra-substituters")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let all_substituters = substituters
        .into_iter()
        .flatten()
        .chain(extra_substituters.into_iter().flatten())
        .collect::<Vec<_>>();

    for cache in caches.caches.pull.iter() {
        let cache_url = format!("https://{}.cachix.org", cache);
        if !all_substituters.iter().any(|s| s == &cache_url) {
            missing_caches.push(cache_url);
        }
    }

    let trusted_public_keys = nix_conf
        .get("trusted-public-keys")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let extra_trusted_public_keys = nix_conf
        .get("extra-trusted-public-keys")
        .map(|s| s.split_whitespace().collect::<Vec<_>>());
    let all_trusted_public_keys = trusted_public_keys
        .into_iter()
        .flatten()
        .chain(extra_trusted_public_keys.into_iter().flatten())
        .collect::<Vec<_>>();

    for (_name, key) in caches.known_keys.iter() {
        if !all_trusted_public_keys.iter().any(|p| p == key) {
            missing_public_keys.push(key.clone());
        }
    }

    (missing_caches, missing_public_keys)
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

    #[test]
    fn test_missing_substituters() {
        let mut cachix = CachixCaches::default();
        cachix.caches.pull = vec!["cache1".to_string(), "cache2".to_string()];
        cachix
            .known_keys
            .insert("cache1".to_string(), "key1".to_string());
        cachix
            .known_keys
            .insert("cache2".to_string(), "key2".to_string());
        let nix_conf = NixConf::parse_stdout(
            r#"
              substituters = https://cache1.cachix.org https://cache3.cachix.org
              trusted-public-keys = key1 key3
            "#
            .as_bytes(),
        )
        .expect("Failed to parse NixConf");
        assert_eq!(
            detect_missing_caches(&cachix, nix_conf),
            (
                vec!["https://cache2.cachix.org".to_string()],
                vec!["key2".to_string()]
            )
        );
    }

    #[test]
    fn test_extra_missing_substituters() {
        let mut cachix = CachixCaches::default();
        cachix.caches.pull = vec!["cache1".to_string(), "cache2".to_string()];
        cachix
            .known_keys
            .insert("cache1".to_string(), "key1".to_string());
        cachix
            .known_keys
            .insert("cache2".to_string(), "key2".to_string());
        let nix_conf = NixConf::parse_stdout(
            r#"
              extra-substituters = https://cache1.cachix.org https://cache3.cachix.org
              extra-trusted-public-keys = key1 key3
            "#
            .as_bytes(),
        )
        .expect("Failed to parse NixConf");
        assert_eq!(
            detect_missing_caches(&cachix, nix_conf),
            (
                vec!["https://cache2.cachix.org".to_string()],
                vec!["key2".to_string()]
            )
        );
    }
}
