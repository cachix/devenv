use super::{cli, config, log::HumanReadableDuration, nix_backend, tasks, util};
use ::nix::sys::signal;
use ::nix::unistd::Pid;
use clap::crate_version;
use cli_table::Table;
use cli_table::{WithTitle, print_stderr};
use include_dir::{Dir, include_dir};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use once_cell::sync::Lazy;
use secrecy::ExposeSecret;
use serde::Deserialize;
use sha2::Digest;
use similar::{ChangeTag, TextDiff};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process;
use tokio::sync::{OnceCell, RwLock, Semaphore};
use tracing::{Instrument, debug, error, info, info_span, instrument, trace, warn};

// templates
const FLAKE_TMPL: &str = include_str!("flake.tmpl.nix");
const REQUIRED_FILES: [&str; 4] = ["devenv.nix", "devenv.yaml", ".envrc", ".gitignore"];
const EXISTING_REQUIRED_FILES: [&str; 1] = [".gitignore"];
const PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/init");
pub static DIRENVRC: Lazy<String> = Lazy::new(|| {
    include_str!("../direnvrc").replace(
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=0",
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=1",
    )
});
pub static DIRENVRC_VERSION: Lazy<u8> = Lazy::new(|| {
    DIRENVRC
        .lines()
        .find(|line| line.contains("export DEVENV_DIRENVRC_VERSION"))
        .and_then(|line| line.split('=').next_back())
        .map(|version| version.trim())
        .and_then(|version| version.parse().ok())
        .unwrap_or(0)
});
// project vars
pub(crate) const DEVENV_FLAKE: &str = ".devenv.flake.nix";

#[derive(Default, Debug)]
pub struct DevenvOptions {
    pub config: config::Config,
    pub global_options: Option<cli::GlobalOptions>,
    pub devenv_root: Option<PathBuf>,
    pub devenv_dotfile: Option<PathBuf>,
}

#[derive(Default, Debug)]
pub struct ProcessOptions<'a> {
    /// An optional environment map to pass to the process.
    /// If not provided, the process will be executed inside a freshly evaluated shell.
    pub envs: Option<&'a HashMap<String, String>>,
    /// Whether the process should be detached from the current process.
    pub detach: bool,
    /// Whether the process should be logged to a file.
    pub log_to_file: bool,
}

pub struct Devenv {
    pub config: Arc<RwLock<config::Config>>,
    pub global_options: cli::GlobalOptions,

    pub nix: Arc<Box<dyn nix_backend::NixBackend>>,

    // All kinds of paths
    devenv_root: PathBuf,
    devenv_dotfile: PathBuf,
    devenv_dot_gc: PathBuf,
    devenv_home_gc: PathBuf,
    devenv_tmp: String,
    devenv_runtime: PathBuf,

    // Whether assemble has been run.
    // Assemble creates critical runtime directories and files.
    assembled: Arc<AtomicBool>,
    // Semaphore to prevent multiple concurrent assembles
    assemble_lock: Arc<Semaphore>,

    has_processes: Arc<OnceCell<bool>>,

    // Secretspec resolved data to pass to Nix
    secretspec_resolved: Arc<OnceCell<secretspec::Resolved<HashMap<String, String>>>>,

    // TODO: make private.
    // Pass as an arg or have a setter.
    pub container_name: Option<String>,
}

impl Devenv {
    pub async fn new(options: DevenvOptions) -> Self {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv");
        let devenv_home = xdg_dirs
            .get_data_home()
            .expect("Failed to get home directory");
        let cachix_trusted_keys = devenv_home.join("cachix_trusted_keys.json");
        let devenv_home_gc = devenv_home.join("gc");

        let devenv_root = options
            .devenv_root
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
        let devenv_dotfile = options
            .devenv_dotfile
            .map(|p| p.to_path_buf())
            .unwrap_or(devenv_root.join(".devenv"));
        let devenv_dot_gc = devenv_dotfile.join("gc");

        let devenv_tmp = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string()));
        // first 7 chars of sha256 hash of devenv_state
        let devenv_state_hash = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(devenv_dotfile.to_string_lossy().as_bytes());
            let result = hasher.finalize();
            hex::encode(result)
        };
        let devenv_runtime =
            Path::new(&devenv_tmp).join(format!("devenv-{}", &devenv_state_hash[..7]));

        let global_options = options.global_options.unwrap_or_default();

        xdg_dirs
            .create_data_directory(Path::new("devenv"))
            .expect("Failed to create DEVENV_HOME directory");
        tokio::fs::create_dir_all(&devenv_home_gc)
            .await
            .expect("Failed to create DEVENV_HOME_GC directory");

        // Determine backend type from config
        let backend_type = options.config.backend.clone();

        // Create DevenvPaths struct
        let paths = nix_backend::DevenvPaths {
            root: devenv_root.clone(),
            dotfile: devenv_dotfile.clone(),
            dot_gc: devenv_dot_gc.clone(),
            home_gc: devenv_home_gc.clone(),
            cachix_trusted_keys,
        };

        // Create shared secretspec_resolved Arc to share between Devenv and Nix
        let secretspec_resolved = Arc::new(OnceCell::new());

        let nix: Box<dyn nix_backend::NixBackend> = match backend_type {
            config::NixBackendType::Nix => Box::new(
                crate::nix::Nix::new(
                    options.config.clone(),
                    global_options.clone(),
                    paths,
                    secretspec_resolved.clone(),
                )
                .await
                .expect("Failed to initialize Nix backend"),
            ),
            #[cfg(feature = "snix")]
            config::NixBackendType::Snix => Box::new(
                crate::snix_backend::SnixBackend::new(
                    options.config.clone(),
                    global_options.clone(),
                    paths,
                )
                .await
                .expect("Failed to initialize Snix backend"),
            ),
        };

        Self {
            config: Arc::new(RwLock::new(options.config)),
            global_options,
            devenv_root,
            devenv_dotfile,
            devenv_dot_gc,
            devenv_home_gc,
            devenv_tmp,
            devenv_runtime,
            nix: Arc::new(nix),
            assembled: Arc::new(AtomicBool::new(false)),
            assemble_lock: Arc::new(Semaphore::new(1)),
            has_processes: Arc::new(OnceCell::new()),
            secretspec_resolved,
            container_name: None,
        }
    }

    pub fn processes_log(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.log")
    }

    pub fn processes_pid(&self) -> PathBuf {
        self.devenv_dotfile.join("processes.pid")
    }

    pub fn init(&self, target: &Option<PathBuf>) -> Result<()> {
        let target = target.clone().unwrap_or_else(|| {
            std::fs::canonicalize(".").expect("Failed to get current directory")
        });

        // create directory target if not exists
        if !target.exists() {
            std::fs::create_dir_all(&target).expect("Failed to create target directory");
        }

        for filename in REQUIRED_FILES {
            info!("Creating {}", filename);

            let path = PROJECT_DIR
                .get_file(filename)
                .ok_or_else(|| miette::miette!("missing {} in the executable", filename))?;

            // write path.contents to target/filename
            let target_path = target.join(filename);

            // add a check for files like .gitignore to append buffer instead of bailing out
            if target_path.exists() && EXISTING_REQUIRED_FILES.contains(&filename) {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&target_path)
                    .and_then(|mut file| {
                        file.write_all(b"\n")?;
                        file.write_all(path.contents())
                    })
                    .expect("Failed to append to existing file");
            } else if target_path.exists() && !EXISTING_REQUIRED_FILES.contains(&filename) {
                if let Some(utf8_contents) = path.contents_utf8() {
                    confirm_overwrite(&target_path, utf8_contents.to_string())?;
                } else {
                    bail!("Failed to read file contents as UTF-8");
                }
            } else {
                std::fs::write(&target_path, path.contents()).expect("Failed to write file");
            }
        }

        // check if direnv executable is available
        let Ok(direnv) = which::which("direnv") else {
            return Ok(());
        };

        // run direnv allow
        let _ = process::Command::new(direnv)
            .arg("allow")
            .current_dir(&target)
            .spawn();
        Ok(())
    }

    pub async fn inputs_add(&self, name: &str, url: &str, follows: &[String]) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.add_input(name, url, follows)?;
            config.write().await?;
        }
        Ok(())
    }

    pub async fn print_dev_env(&self, json: bool) -> Result<()> {
        let env = self.get_dev_environment(json).await?;
        print!(
            "{}",
            String::from_utf8(env.output).expect("Failed to convert env to utf-8")
        );
        Ok(())
    }

    // TODO: fetch bash from the module system
    async fn get_bash(&self, refresh_cached_output: bool) -> Result<String> {
        let options = nix_backend::Options {
            cache_output: true,
            refresh_cached_output,
            ..Default::default()
        };
        let bash_attr = format!(
            "nixpkgs#legacyPackages.{}.bashInteractive.out",
            self.global_options.system
        );
        String::from_utf8(
            self.nix
                .run_nix(
                    "nix",
                    &[
                        "build",
                        "--inputs-from",
                        ".",
                        "--print-out-paths",
                        "--out-link",
                        &self.devenv_dotfile.join("bash").to_string_lossy(),
                        &bash_attr,
                    ],
                    &options,
                )
                .await?
                .stdout,
        )
        .map(|mut s| {
            let trimmed_len = s.trim_end_matches('\n').len();
            s.truncate(trimmed_len);
            s.push_str("/bin/bash");
            s
        })
        .into_diagnostic()
    }

    #[instrument(skip(self))]
    pub async fn prepare_shell(
        &self,
        cmd: &Option<String>,
        args: &[String],
    ) -> Result<process::Command> {
        let DevEnv { output, .. } = self.get_dev_environment(false).await?;

        let bash = match self.get_bash(false).await {
            Err(e) => {
                trace!("Failed to get bash: {}. Rebuilding.", e);
                self.get_bash(true).await?
            }
            Ok(bash) => bash,
        };

        let mut shell_cmd = process::Command::new(&bash);
        let path = self.devenv_runtime.join("shell");

        // Load the user's bashrc if it exists and if we're in an interactive shell.
        // Disable alias expansion to avoid breaking the dev shell script.
        let mut output = indoc::formatdoc! {
            r#"
            if [ -n "$PS1" ] && [ -e $HOME/.bashrc ]; then
                source $HOME/.bashrc;
            fi

            shopt -u expand_aliases
            {}
            shopt -s expand_aliases
            "#,
            String::from_utf8_lossy(&output)
        };

        match cmd {
            // Non-interactive mode.
            // exec the command at the end of the rcscript.
            Some(cmd) => {
                let command = format!(
                    "\nexec {} {}",
                    cmd,
                    args.iter()
                        .map(|arg| shell_escape::escape(std::borrow::Cow::Borrowed(arg)))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                output.push_str(&command);
                shell_cmd.arg(&path);
            }
            // Interactive mode. Use an rcfile.
            None => {
                shell_cmd.args(["--rcfile", &path.to_string_lossy()]);
            }
        }

        tokio::fs::write(&path, output)
            .await
            .expect("Failed to write the shell script");
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .await
            .expect("Failed to set permissions");

        let config_clean = self.config.read().await.clean.clone().unwrap_or_default();
        if self.global_options.clean.is_some() || config_clean.enabled {
            let keep = match &self.global_options.clean {
                Some(clean) => clean,
                None => &config_clean.keep,
            };

            let filtered_env = std::env::vars().filter(|(k, _)| keep.contains(k));
            shell_cmd.env_clear().envs(filtered_env);
        }

        shell_cmd.env("SHELL", &bash);

        // Pass command args to the shell as DEVENV_CMDLINE
        let cmdline = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        shell_cmd.env("DEVENV_CMDLINE", cmdline);

        Ok(shell_cmd)
    }

    /// Launch an interactive shell (uses exec, never returns).
    pub async fn shell(&self) -> Result<()> {
        self.exec_in_shell(None, &[]).await
    }

    /// Execute a command by replacing the current process using exec.
    ///
    /// This method accepts `Option<String>` for the command to support both:
    /// - Interactive shell: `exec_in_shell(None, &[])`
    /// - Command execution: `exec_in_shell(Some(cmd), args)`
    ///
    /// **Important**: This function never returns `Ok(())` on success because `exec()`
    /// replaces the current process. The `Result<()>` return type only represents
    /// potential errors during setup or if `exec()` fails to start the new process.
    /// On successful exec, this function never returns.
    pub async fn exec_in_shell(&self, cmd: Option<String>, args: &[String]) -> Result<()> {
        let shell_cmd = self.prepare_shell(&cmd, args).await?;
        info!(devenv.is_user_message = true, "Entering shell");
        let err = shell_cmd.into_std().exec();

        let cmd_context = match &cmd {
            Some(c) => format!("command '{c}'"),
            None => "interactive shell".to_string(),
        };
        bail!("Failed to exec into shell with {}: {}", cmd_context, err);
    }

    /// Run a command and return the output.
    ///
    /// This method accepts `String` (not `Option<String>`) because it's specifically
    /// designed for running commands and capturing their output. Unlike `exec_in_shell`,
    /// this method always requires a command and uses `spawn` + `wait_with_output`
    /// to return control to the caller with the command's output.
    pub async fn run_in_shell(&self, cmd: String, args: &[String]) -> Result<Output> {
        let mut shell_cmd = self.prepare_shell(&Some(cmd), args).await?;
        let span = info_span!("running_in_shell", devenv.user_message = "Running in shell");
        // Note that tokio's `output()` always configures stdout/stderr as pipes.
        // Use `spawn` + `wait_with_output` instead.
        let proc = shell_cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .into_diagnostic()?;
        async move { proc.wait_with_output().await.into_diagnostic() }
            .instrument(span)
            .await
    }

    pub async fn update(&self, input_name: &Option<String>) -> Result<()> {
        self.assemble(false).await?;

        let msg = match input_name {
            Some(input_name) => format!("Updating devenv.lock with input {input_name}"),
            None => "Updating devenv.lock".to_string(),
        };

        let span = info_span!("update", devenv.user_message = msg);
        self.nix.update(input_name).instrument(span).await?;

        Ok(())
    }

    #[instrument(
        name = "building_container",
        skip(self),
        fields(devenv.user_message = format!("Building {name} container"))
    )]
    pub async fn container_build(&mut self, name: &str) -> Result<String> {
        // This container name is passed to the flake as an argument and tells the module system
        // that we're 1. building a container 2. which container we're building.
        self.container_name = Some(name.to_string());
        self.assemble(false).await?;

        let sanitized_name = sanitize_container_name(name);
        let gc_root = self
            .devenv_dot_gc
            .join(format!("container-{sanitized_name}-derivation"));
        let host_arch = env!("TARGET_ARCH");
        let host_os = env!("TARGET_OS");
        let target_system = if host_os == "macos" {
            match host_arch {
                "aarch64" => "aarch64-linux",
                "x86_64" => "x86_64-linux",
                _ => bail!("Unsupported container architecture for macOS: {host_arch}"),
            }
        } else {
            &self.global_options.system
        };
        let paths = self
            .nix
            .build(
                &[&format!(
                    "devenv.perSystem.{target_system}.config.containers.{name}.derivation"
                )],
                None,
                Some(&gc_root),
            )
            .await?;
        let container_store_path = &paths[0].to_string_lossy();
        Ok(container_store_path.to_string())
    }

    pub async fn container_copy(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        let spec = self.container_build(name).await?;

        let span = info_span!("copying_container");
        async move {
            let sanitized_name = sanitize_container_name(name);
            let gc_root = self
                .devenv_dot_gc
                .join(format!("container-{sanitized_name}-copy"));
            let paths = self
                .nix
                .build(
                    &[&format!("devenv.config.containers.{name}.copyScript")],
                    None,
                    Some(&gc_root),
                )
                .await?;
            let copy_script = &paths[0];
            let copy_script_string = &copy_script.to_string_lossy();

            let base_args = [spec, registry.unwrap_or("false").to_string()];
            let command_args: Vec<String> = base_args
                .into_iter()
                .chain(copy_args.iter().map(|s| s.to_string()))
                .collect();

            debug!("Running {copy_script_string} {}", command_args.join(" "));

            let status = process::Command::new(copy_script)
                .args(command_args)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .await
                .expect("Failed to run copy script");

            if !status.success() {
                bail!("Failed to copy container")
            } else {
                Ok(())
            }
        }
        .instrument(span)
        .await
    }

    pub async fn container_run(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        if registry.is_some() {
            warn!("Ignoring --registry flag when running container");
        };
        self.container_copy(name, copy_args, Some("docker-daemon:"))
            .await?;

        info!(devenv.is_user_message = true, "Running container {name}",);

        let sanitized_name = sanitize_container_name(name);
        let gc_root = self
            .devenv_dot_gc
            .join(format!("container-{sanitized_name}-run"));
        let paths = self
            .nix
            .build(
                &[&format!("devenv.config.containers.{name}.dockerRun")],
                None,
                Some(&gc_root),
            )
            .await?;

        let err = process::Command::new(&paths[0]).into_std().exec();

        // If exec fails, we return an error.
        bail!("Failed to run container: {}", err);
    }

    pub async fn repl(&self) -> Result<()> {
        self.assemble(false).await?;
        self.nix.repl().await
    }

    pub async fn gc(&self) -> Result<()> {
        let start = std::time::Instant::now();

        let (to_gc, removed_symlinks) = {
            // TODO: No newline
            let span = info_span!(
                "cleanup_symlinks",
                devenv.user_message = format!(
                    "Removing non-existing symlinks in {}",
                    &self.devenv_home_gc.display()
                )
            );
            span.in_scope(|| cleanup_symlinks(&self.devenv_home_gc))
        };
        let to_gc_len = to_gc.len();

        info!("Found {} active environments.", to_gc_len);
        info!(
            "Deleted {} dangling environments (most likely due to previous GC).",
            removed_symlinks.len()
        );

        {
            let span = info_span!(
                "nix_gc",
                devenv.user_message =
                    "Running garbage collection (this process will take some time)"
            );
            info!(
                "If you'd like this to run faster, leave a thumbs up at https://github.com/NixOS/nix/issues/7239"
            );
            self.nix.gc(to_gc).instrument(span).await?;
        }

        let (after_gc, _) = cleanup_symlinks(&self.devenv_home_gc);
        let end = std::time::Instant::now();

        // TODO: newline before or after
        info!(
            "\nDone. Successfully removed {} symlinks in {}s.",
            to_gc_len - after_gc.len(),
            (end - start).as_secs_f32()
        );
        Ok(())
    }

    #[instrument(
        skip(self),
        fields(
            devenv.user_message = "Searching options and packages",
        )
    )]
    pub async fn search(&self, name: &str) -> Result<()> {
        self.assemble(false).await?;

        // Run both searches concurrently
        let (options_results, package_results) =
            tokio::try_join!(self.search_options(name), self.search_packages(name))?;

        let results_options_count = options_results.len();
        let package_results_count = package_results.len();

        if !package_results.is_empty() {
            print_stderr(package_results.with_title()).expect("Failed to print package results");
        }

        if !options_results.is_empty() {
            print_stderr(options_results.with_title()).expect("Failed to print options results");
        }

        info!(
            "Found {package_results_count} packages and {results_options_count} options for '{name}'."
        );
        Ok(())
    }

    async fn search_options(&self, name: &str) -> Result<Vec<DevenvOptionResult>> {
        let build_options = nix_backend::Options {
            logging: false,
            cache_output: true,
            ..Default::default()
        };
        let options = self
            .nix
            .build(&["optionsJSON"], Some(build_options), None)
            .await?;
        let options_path = options[0]
            .join("share")
            .join("doc")
            .join("nixos")
            .join("options.json");
        let options_contents = fs::read(options_path)
            .await
            .expect("Failed to read options.json");
        let options_json: OptionResults =
            serde_json::from_slice(&options_contents).expect("Failed to parse options.json");

        let options_results = options_json
            .0
            .into_iter()
            .filter(|(key, _)| key.contains(name))
            .map(|(key, value)| DevenvOptionResult {
                name: key,
                type_: value.type_,
                default: value.default.unwrap_or_default(),
                description: value.description,
            })
            .collect::<Vec<_>>();

        Ok(options_results)
    }

    async fn search_packages(&self, name: &str) -> Result<Vec<DevenvPackageResult>> {
        let search_options = nix_backend::Options {
            logging: false,
            cache_output: true,
            ..Default::default()
        };
        let search = self.nix.search(name, Some(search_options)).await?;
        let search_json: PackageResults =
            serde_json::from_slice(&search.stdout).expect("Failed to parse search results");
        let search_results = search_json
            .0
            .into_iter()
            .map(|(key, value)| DevenvPackageResult {
                name: format!(
                    "pkgs.{}",
                    key.split('.').skip(2).collect::<Vec<_>>().join(".")
                ),
                version: value.version,
                description: value.description.chars().take(80).collect::<String>(),
            })
            .collect::<Vec<_>>();

        Ok(search_results)
    }

    pub async fn has_processes(&self) -> Result<bool> {
        let value = self
            .has_processes
            .get_or_try_init(|| async {
                let processes = self.nix.eval(&["devenv.config.processes"]).await?;
                Ok::<bool, miette::Report>(processes.trim() != "{}")
            })
            .await?;
        Ok(*value)
    }

    async fn load_tasks(&self) -> Result<Vec<tasks::TaskConfig>> {
        let tasks_json_file = {
            let span = info_span!("load_tasks", devenv.user_message = "Evaluating tasks");
            let gc_root = self.devenv_dot_gc.join("task-config");
            self.nix
                .build(&["devenv.config.task.config"], None, Some(&gc_root))
                .instrument(span)
                .await?
        };
        // parse tasks config
        let tasks_json = fs::read_to_string(&tasks_json_file[0])
            .await
            .expect("Failed to read config file");
        let tasks: Vec<tasks::TaskConfig> =
            serde_json::from_str(&tasks_json).expect("Failed to parse tasks config");
        Ok(tasks)
    }

    pub async fn tasks_run(
        &self,
        roots: Vec<String>,
        run_mode: devenv_tasks::RunMode,
    ) -> Result<()> {
        self.assemble(false).await?;
        if roots.is_empty() {
            bail!("No tasks specified.");
        }

        // Capture the shell environment to ensure tasks run with proper devenv setup
        let envs = self.capture_shell_environment().await?;

        // Set environment variables in the current process
        // This ensures that tasks have access to all devenv environment variables
        for (key, value) in &envs {
            unsafe {
                std::env::set_var(key, value);
            }
        }

        let tasks = self.load_tasks().await?;

        // Convert global options to verbosity level
        let verbosity = if self.global_options.quiet {
            tasks::VerbosityLevel::Quiet
        } else if self.global_options.verbose {
            tasks::VerbosityLevel::Verbose
        } else {
            tasks::VerbosityLevel::Normal
        };

        let config = tasks::Config {
            roots,
            tasks,
            run_mode,
        };
        debug!(
            "Tasks config: {}",
            serde_json::to_string_pretty(&config).unwrap()
        );

        let mut tui = tasks::TasksUi::builder(config, verbosity).build().await?;
        let (tasks_status, outputs) = tui.run().await?;

        if tasks_status.failed > 0 || tasks_status.dependency_failed > 0 {
            miette::bail!("Some tasks failed");
        }

        println!(
            "{}",
            serde_json::to_string(&outputs).expect("parsing of outputs failed")
        );
        Ok(())
    }

    pub async fn tasks_list(&self) -> Result<()> {
        self.assemble(false).await?;

        let tasks = self.load_tasks().await?;

        if tasks.is_empty() {
            println!("No tasks defined.");
            return Ok(());
        }

        // Print the task tree
        print_tasks_tree(&tasks);

        Ok(())
    }

    async fn capture_shell_environment(&self) -> Result<HashMap<String, String>> {
        let temp_dir = tempfile::TempDir::with_prefix("devenv-env")
            .into_diagnostic()
            .wrap_err("Failed to create temporary directory for environment capture")?;

        let script_path = temp_dir.path().join("script");
        let env_path = temp_dir.path().join("env");

        let script = format!("env > {}", env_path.to_string_lossy());
        fs::write(&script_path, script)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write script to {}", script_path.display()))?;
        fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Failed to set execute permissions on {}",
                    script_path.display()
                )
            })?;

        // Run script and capture its environment exports
        self.prepare_shell(&Some(script_path.to_string_lossy().into()), &[])
            .await?
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .spawn()
            .into_diagnostic()
            .wrap_err("Failed to execute environment capture script")?
            .wait()
            .await
            .into_diagnostic()
            .wrap_err("Failed to wait for environment capture script to complete")?;

        // Parse the environment variables
        let file = File::open(&env_path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("Failed to open environment file at {}", env_path.display())
            })?;
        let reader = BufReader::new(file);
        let mut shell_envs = Vec::new();
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut parts = line.splitn(2, '=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                shell_envs.push((key.to_string(), value.to_string()));
            }
        }

        let config_clean = self.config.read().await.clean.clone().unwrap_or_default();
        let mut envs: HashMap<String, String> = {
            let vars = std::env::vars();
            if self.global_options.clean.is_some() || config_clean.enabled {
                let keep = match &self.global_options.clean {
                    Some(clean) => clean,
                    None => &config_clean.keep,
                };
                vars.filter(|(key, _)| !keep.contains(key)).collect()
            } else {
                vars.collect()
            }
        };

        for (key, value) in shell_envs {
            envs.insert(key, value);
        }

        Ok(envs)
    }

    pub async fn test(&self) -> Result<()> {
        self.assemble(true).await?;

        // collect tests
        let test_script = {
            let span = info_span!("test", devenv.user_message = "Building tests");
            let gc_root = self.devenv_dot_gc.join("test");
            let test_script = self
                .nix
                .build(&["devenv.config.test"], None, Some(&gc_root))
                .instrument(span)
                .await?;
            test_script[0].to_string_lossy().to_string()
        };

        let envs = self.capture_shell_environment().await?;

        if self.has_processes().await? {
            let options = ProcessOptions {
                envs: Some(&envs),
                detach: true,
                log_to_file: false,
            };
            self.up(vec![], &options).await?;
        }

        let span = info_span!("test", devenv.user_message = "Running tests");
        let result = async {
            debug!("Running command: {test_script}");
            process::Command::new(&test_script)
                .env_clear()
                .envs(envs)
                .spawn()
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to spawn test process using {test_script}"))?
                .wait_with_output()
                .await
                .into_diagnostic()
                .wrap_err("Failed to get output from test process")
        }
        .instrument(span)
        .await?;

        if self.has_processes().await? {
            self.down().await?;
        }

        if !result.status.success() {
            error!("Tests failed :(");
            bail!("Tests failed");
        } else {
            info!("Tests passed :)");
            Ok(())
        }
    }

    pub async fn info(&self) -> Result<()> {
        self.assemble(false).await?;
        let output = self.nix.metadata().await?;
        println!("{output}");
        Ok(())
    }

    pub async fn build(&self, attributes: &[String]) -> Result<()> {
        let span = info_span!("build", devenv.user_message = "Building");
        async move {
            self.assemble(false).await?;
            let attributes: Vec<String> = if attributes.is_empty() {
                // construct dotted names of all attributes that we need to build
                let build_output = self.nix.eval(&["build"]).await?;
                serde_json::from_str::<serde_json::Value>(&build_output)
                    .map_err(|e| miette::miette!("Failed to parse build output: {}", e))?
                    .as_object()
                    .ok_or_else(|| miette::miette!("Build output is not an object"))?
                    .iter()
                    .flat_map(|(key, value)| {
                        fn flatten_object(prefix: &str, value: &serde_json::Value) -> Vec<String> {
                            match value {
                                serde_json::Value::Object(obj) => obj
                                    .iter()
                                    .flat_map(|(k, v)| flatten_object(&format!("{prefix}.{k}"), v))
                                    .collect(),
                                _ => vec![format!("devenv.config.{}", prefix)],
                            }
                        }
                        flatten_object(key, value)
                    })
                    .collect()
            } else {
                attributes
                    .iter()
                    .map(|attr| format!("devenv.config.{attr}"))
                    .collect()
            };
            let paths = self
                .nix
                .build(
                    &attributes.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                    None,
                    None,
                )
                .await?;
            for path in paths {
                println!("{}", path.display());
            }
            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn up<'a>(
        &self,
        processes: Vec<String>,
        options: &'a ProcessOptions<'a>,
    ) -> Result<()> {
        self.assemble(false).await?;
        if !self.has_processes().await? {
            error!("No 'processes' option defined: https://devenv.sh/processes/");
            bail!("No processes defined");
        }

        let span = info_span!(
            "build_processes",
            devenv.user_message = "Building processes"
        );
        let proc_script_string = async {
            let gc_root = self.devenv_dot_gc.join("procfilescript");
            let paths = self
                .nix
                .build(&["procfileScript"], None, Some(&gc_root))
                .await?;
            let proc_script_string = paths[0].to_string_lossy().to_string();
            Ok::<String, miette::Report>(proc_script_string)
        }
        .instrument(span)
        .await?;

        let span = info_span!("up", devenv.user_message = "Starting processes");
        async {
            let processes = processes.join(" ");

            let processes_script = self.devenv_dotfile.join("processes");
            // we force disable process compose tui if detach is enabled
            let tui = if options.detach {
                "export PC_TUI_ENABLED=0"
            } else {
                ""
            };
            fs::write(
                &processes_script,
                indoc::formatdoc! {"
                #!/usr/bin/env bash
                {tui}
                exec {proc_script_string} {processes}
            "},
            )
            .await
            .expect("Failed to write PROCESSES_SCRIPT");

            fs::set_permissions(&processes_script, std::fs::Permissions::from_mode(0o755))
                .await
                .expect("Failed to set permissions");

            let mut cmd = if let Some(envs) = options.envs {
                let mut cmd = process::Command::new("bash");
                cmd.arg(processes_script.to_string_lossy().to_string())
                    .env_clear()
                    .envs(envs);
                cmd
            } else {
                self.prepare_shell(&Some(processes_script.to_string_lossy().to_string()), &[])
                    .await?
            };

            if options.detach {
                let process = if !options.log_to_file {
                    cmd.stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .spawn()
                        .expect("Failed to spawn process")
                } else {
                    let log_file = std::fs::File::create(self.processes_log())
                        .expect("Failed to create PROCESSES_LOG");
                    cmd.stdout(log_file.try_clone().expect("Failed to clone Stdio"))
                        .stderr(log_file)
                        .spawn()
                        .expect("Failed to spawn process")
                };

                let pid = process
                    .id()
                    .ok_or_else(|| miette!("Failed to get process ID"))?;
                fs::write(self.processes_pid(), pid.to_string())
                    .await
                    .expect("Failed to write PROCESSES_PID");
                info!("PID is {}", pid);
                if options.log_to_file {
                    info!("See logs:  $ tail -f {}", self.processes_log().display());
                }
                info!("Stop:      $ devenv processes stop");
            } else {
                let err = cmd.into_std().exec();
                bail!(err);
            }
            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn down(&self) -> Result<()> {
        if !PathBuf::from(&self.processes_pid()).exists() {
            bail!("No processes running");
        }

        let pid = fs::read_to_string(self.processes_pid())
            .await
            .into_diagnostic()
            .wrap_err("Failed to read processes.pid file")?
            .trim()
            .parse::<i32>()
            .into_diagnostic()
            .wrap_err("Invalid PID in processes.pid file")
            .map(Pid::from_raw)?;

        info!("Stopping process with PID {}", pid);

        match signal::kill(pid, signal::Signal::SIGTERM) {
            Ok(_) => {}
            Err(_) => {
                bail!("Process with PID {} not found.", pid);
            }
        }

        // Wait for the process to actually shut down using exponential backoff
        let start_time = std::time::Instant::now();
        let max_wait = std::time::Duration::from_secs(30);
        let mut wait_interval = std::time::Duration::from_millis(10);
        const MAX_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

        loop {
            // Check if process is still running by sending signal 0 (null signal)
            match signal::kill(pid, None) {
                Ok(_) => {
                    // Process still exists
                    let elapsed = start_time.elapsed();
                    if elapsed >= max_wait {
                        warn!(
                            "Process {} did not shut down gracefully within {} seconds, sending SIGKILL to process group",
                            pid,
                            max_wait.as_secs()
                        );

                        // Send SIGKILL to the entire process group
                        // Negative PID means send to process group
                        let pgid = Pid::from_raw(-pid.as_raw());
                        match signal::kill(pgid, signal::Signal::SIGKILL) {
                            Ok(_) => info!("Sent SIGKILL to process group {}", pid.as_raw()),
                            Err(e) => warn!(
                                "Failed to send SIGKILL to process group {}: {}",
                                pid.as_raw(),
                                e
                            ),
                        }

                        // Give it a moment to die after SIGKILL
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        break;
                    }

                    tokio::time::sleep(wait_interval).await;

                    // Exponential backoff: double the interval up to MAX_INTERVAL
                    wait_interval = wait_interval.mul_f64(1.5).min(MAX_INTERVAL);
                }
                Err(nix::errno::Errno::ESRCH) => {
                    // ESRCH means "No such process" - it has shut down
                    debug!(
                        "Process {} has shut down after {}",
                        pid,
                        HumanReadableDuration(start_time.elapsed())
                    );
                    break;
                }
                Err(e) => {
                    // Some other error occurred
                    warn!("Error checking process {}: {}", pid, e);
                    break;
                }
            }
        }

        fs::remove_file(self.processes_pid())
            .await
            .expect("Failed to remove PROCESSES_PID");
        Ok(())
    }

    pub async fn assemble(&self, is_testing: bool) -> Result<()> {
        if self.assembled.load(Ordering::Acquire) {
            return Ok(());
        }

        let _permit = self.assemble_lock.acquire().await.unwrap();

        // Skip devenv.nix existence check if --option is provided
        if self.global_options.option.is_empty() && !self.devenv_root.join("devenv.nix").exists() {
            bail!(indoc::indoc! {"
            File devenv.nix does not exist. To get started, run:

                $ devenv init
            "});
        }

        fs::create_dir_all(&self.devenv_dot_gc).await.map_err(|e| {
            miette::miette!("Failed to create {}: {}", self.devenv_dot_gc.display(), e)
        })?;

        // Initialise any Nix state
        self.nix.assemble().await?;

        let mut flake_inputs = BTreeMap::new();
        let config = self.config.read().await;
        for (input, attrs) in config.inputs.iter() {
            match config::FlakeInput::try_from(attrs) {
                Ok(flake_input) => {
                    flake_inputs.insert(input.clone(), flake_input);
                }
                Err(e) => {
                    error!("Failed to parse input {}: {}", input, e);
                    bail!("Failed to parse inputs");
                }
            }
        }
        util::write_file_with_lock(
            self.devenv_dotfile.join("flake.json"),
            serde_json::to_string(&flake_inputs).unwrap(),
        )?;
        util::write_file_with_lock(
            self.devenv_dotfile.join("devenv.json"),
            serde_json::to_string(&*config).unwrap(),
        )?;
        // TODO: superceded by eval caching.
        // Remove once direnvrc migration is implemented.
        util::write_file_with_lock(
            self.devenv_dotfile.join("imports.txt"),
            config.imports.join("\n"),
        )?;

        fs::create_dir_all(&self.devenv_runtime)
            .await
            .map_err(|e| {
                miette::miette!("Failed to create {}: {}", self.devenv_runtime.display(), e)
            })?;

        // Check for secretspec.toml and load secrets
        let secretspec_path = self.devenv_root.join("secretspec.toml");
        let secretspec_config_exists = config.secretspec.is_some();
        let secretspec_enabled = config
            .secretspec
            .as_ref()
            .map(|c| c.enable)
            .unwrap_or(false); // Default to false if secretspec config is not present

        if secretspec_path.exists() {
            // Log warning when secretspec.toml exists but is not configured
            if !secretspec_enabled && !secretspec_config_exists {
                info!(
                    "{}",
                    indoc::formatdoc! {"
                    Found secretspec.toml but secretspec integration is not enabled.
                    
                    To enable, add to devenv.yaml:
                      secretspec:
                        enable: true
                    
                    To disable this message:
                      secretspec:
                        enable: false
                    
                    Learn more: https://devenv.sh/integrations/secretspec/
                "}
                );
            }

            if secretspec_enabled {
                // Get profile and provider from devenv.yaml config
                let (profile, provider) = if let Some(ref secretspec_config) = config.secretspec {
                    (
                        secretspec_config.profile.clone(),
                        secretspec_config.provider.clone(),
                    )
                } else {
                    (None, None)
                };

                // Load and validate secrets using SecretSpec API
                let mut secrets = secretspec::Secrets::load()
                    .map_err(|e| miette!("Failed to load secretspec configuration: {}", e))?;

                // Configure provider and profile if specified
                if let Some(ref provider_str) = provider {
                    secrets.set_provider(provider_str);
                }
                if let Some(ref profile_str) = profile {
                    secrets.set_profile(profile_str);
                }

                // Validate secrets
                match secrets.validate()? {
                    Ok(validated_secrets) => {
                        // Store resolved secrets in OnceCell for Nix to use
                        let resolved = secretspec::Resolved {
                            secrets: validated_secrets
                                .resolved
                                .secrets
                                .into_iter()
                                .map(|(k, v)| (k, v.expose_secret().to_string()))
                                .collect(),
                            provider: validated_secrets.resolved.provider,
                            profile: validated_secrets.resolved.profile,
                        };

                        self.secretspec_resolved
                            .set(resolved)
                            .map_err(|_| miette!("Secretspec resolved already set"))?;
                    }
                    Err(validation_errors) => {
                        bail!(
                            "Required secrets are missing: {} (provider: {}, profile: {})",
                            validation_errors.missing_required.join(", "),
                            validation_errors.provider,
                            validation_errors.profile
                        );
                    }
                }
            }
        }

        // Create cli-options.nix if there are CLI options
        if !self.global_options.option.is_empty() {
            let mut cli_options = String::from("{ pkgs, lib, config, ... }: {\n");

            const SUPPORTED_TYPES: &[&str] =
                &["string", "int", "float", "bool", "path", "pkg", "pkgs"];

            for chunk in self.global_options.option.chunks_exact(2) {
                // Parse the path and type from the first value
                let key_parts: Vec<&str> = chunk[0].split(':').collect();
                if key_parts.len() < 2 {
                    miette::bail!(
                        "Invalid option format: '{}'. Must include type, e.g. 'languages.rust.version:string'. Supported types: {}",
                        chunk[0],
                        SUPPORTED_TYPES.join(", ")
                    );
                }

                let path = key_parts[0];
                let type_name = key_parts[1];

                // Format value based on type
                let value = match type_name {
                    "string" => format!("\"{}\"", &chunk[1]),
                    "int" => chunk[1].clone(),
                    "float" => chunk[1].clone(),
                    "bool" => chunk[1].clone(), // true/false will work directly in Nix
                    "path" => format!("./{}", &chunk[1]), // relative path
                    "pkg" => format!("pkgs.{}", &chunk[1]),
                    "pkgs" => {
                        // Split by whitespace and format as a Nix list of package references
                        let items = chunk[1]
                            .split_whitespace()
                            .map(|item| format!("pkgs.{item}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        format!("[ {items} ]")
                    }
                    _ => miette::bail!(
                        "Unsupported type: '{}'. Supported types: {}",
                        type_name,
                        SUPPORTED_TYPES.join(", ")
                    ),
                };

                // Use lib.mkForce for all types except pkgs
                let final_value = if type_name == "pkgs" {
                    value
                } else {
                    format!("lib.mkForce {value}")
                };
                cli_options.push_str(&format!("  {path} = {final_value};\n"));
            }

            cli_options.push_str("}\n");

            util::write_file_with_lock(self.devenv_dotfile.join("cli-options.nix"), &cli_options)?;
        } else {
            // Remove the file if it exists but there are no CLI options
            let cli_options_path = self.devenv_dotfile.join("cli-options.nix");
            if cli_options_path.exists() {
                fs::remove_file(&cli_options_path)
                    .await
                    .expect("Failed to remove cli-options.nix");
            }
        }

        // Create flake.devenv.nix
        //
        // `devenv_root` is an absolute string path to the root of the project directory.
        // `devenv_dotfile` is an absolute string path to the devenv dotfile directory.
        // `devenv_dotfile_path` is a relative Nix path to the dotfile directory.
        //  This is used to load in additional files from the dotfile directory.
        // `devenv_tmpdir` is an absolute string path to the temporary directory for this shell.
        // `devenv_runtime` is an absolute string path to the runtime directory for this shell.
        // `devenv_istesting` is a boolean indicating if the shell is being assembled for testing.
        // `container_name` indicates the name of the container being built, copied, or run, if any.
        let active_profiles = if self.global_options.profile.is_empty() {
            "[ ]".to_string()
        } else {
            format!(
                "[ {} ]",
                self.global_options
                    .profile
                    .iter()
                    .map(|p| format!("\"{p}\""))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        // Get current hostname and username using system APIs
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        let username = whoami::username();

        // Detect git repository root
        let git_root = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(&self.devenv_root)
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    Some(format!(
                        "\"{}\"",
                        String::from_utf8_lossy(&output.stdout).trim()
                    ))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "null".to_string());

        let vars = indoc::formatdoc!(
            "version = \"{version}\";
            system = \"{system}\";
            devenv_root = \"{devenv_root}\";
            devenv_dotfile = \"{devenv_dotfile}\";
            devenv_dotfile_path = ./{devenv_dotfile_name};
            devenv_tmpdir = \"{devenv_tmpdir}\";
            devenv_runtime = \"{devenv_runtime}\";
            devenv_istesting = {devenv_istesting};
            devenv_direnvrc_latest_version = {direnv_version};
            container_name = {container_name};
            active_profiles = {active_profiles};
            hostname = \"{hostname}\";
            username = \"{username}\";
            git_root = {git_root};
            ",
            version = crate_version!(),
            system = self.global_options.system,
            devenv_root = self.devenv_root.display(),
            devenv_dotfile = self.devenv_dotfile.display(),
            devenv_dotfile_name = self
                .devenv_dotfile
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap(),
            container_name = self
                .container_name
                .as_deref()
                .map(|s| format!("\"{s}\""))
                .unwrap_or_else(|| "null".to_string()),
            devenv_tmpdir = self.devenv_tmp,
            devenv_runtime = self.devenv_runtime.display(),
            devenv_istesting = is_testing,
            direnv_version = DIRENVRC_VERSION.to_string(),
            active_profiles = active_profiles,
            hostname = hostname,
            username = username,
            git_root = git_root
        );
        let flake = FLAKE_TMPL.replace("__DEVENV_VARS__", &vars);
        let flake_path = self.devenv_root.join(DEVENV_FLAKE);
        util::write_file_with_lock(&flake_path, &flake)?;

        self.assembled.store(true, Ordering::Release);
        Ok(())
    }

    #[instrument(skip_all,fields(devenv.user_message = "Building shell"))]
    pub async fn get_dev_environment(&self, json: bool) -> Result<DevEnv> {
        self.assemble(false).await?;

        let gc_root = self.devenv_dot_gc.join("shell");
        let span = tracing::debug_span!("evaluating_dev_env");
        let env = self.nix.dev_env(json, &gc_root).instrument(span).await?;

        use devenv_eval_cache::command::{FileInputDesc, Input};
        util::write_file_with_lock(
            self.devenv_dotfile.join("input-paths.txt"),
            env.inputs
                .iter()
                .filter_map(|input| match input {
                    Input::File(FileInputDesc { path, .. }) => {
                        // We include --option in the eval cache, but we don't want it
                        // to trigger direnv reload on each invocation
                        let cli_options_path = self.devenv_dotfile.join("cli-options.nix");
                        if path == &cli_options_path {
                            return None;
                        }
                        Some(path.to_string_lossy().to_string())
                    }
                    // TODO(sander): update direnvrc to handle env vars if possible
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )?;

        Ok(DevEnv { output: env.stdout })
    }
}

fn confirm_overwrite(file: &Path, contents: String) -> Result<()> {
    if std::fs::metadata(file).is_ok() {
        // first output the old version and propose new changes
        let before = std::fs::read_to_string(file).expect("Failed to read file");

        let diff = TextDiff::from_lines(&before, &contents);

        eprintln!("\nChanges that will be made to {}:", file.to_string_lossy());
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "\x1b[31m-\x1b[0m",
                ChangeTag::Insert => "\x1b[32m+\x1b[0m",
                ChangeTag::Equal => " ",
            };
            eprint!("{sign}{change}");
        }

        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "{} already exists. Do you want to overwrite it?",
                file.to_string_lossy()
            ))
            .interact()
            .into_diagnostic()?;

        if confirm {
            std::fs::write(file, contents).into_diagnostic()?;
        }
    } else {
        std::fs::write(file, contents).into_diagnostic()?;
    }
    Ok(())
}

pub struct DevEnv {
    output: Vec<u8>,
}

#[derive(Deserialize)]
struct PackageResults(BTreeMap<String, PackageResult>);

#[derive(Deserialize)]
struct PackageResult {
    version: String,
    description: String,
}

#[derive(Deserialize)]
struct OptionResults(BTreeMap<String, OptionResult>);

#[derive(Deserialize)]
struct OptionResult {
    #[serde(rename = "type")]
    type_: String,
    default: Option<String>,
    description: String,
}

#[derive(Table)]
struct DevenvOptionResult {
    #[table(title = "Option")]
    name: String,
    #[table(title = "Type")]
    type_: String,
    #[table(title = "Default")]
    default: String,
    #[table(title = "Description")]
    description: String,
}

#[derive(Table)]
struct DevenvPackageResult {
    #[table(title = "Package")]
    name: String,
    #[table(title = "Version")]
    version: String,
    #[table(title = "Description")]
    description: String,
}

fn sanitize_container_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
}

fn cleanup_symlinks(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut to_gc = Vec::new();
    let mut removed_symlinks = Vec::new();

    if !root.exists() {
        std::fs::create_dir_all(root).expect("Failed to create gc directory");
    }

    for entry in std::fs::read_dir(root).expect("Failed to read directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_symlink() {
            if !path.exists() {
                removed_symlinks.push(path.clone());
            } else {
                let target = std::fs::canonicalize(&path).expect("Failed to read link");
                to_gc.push(target);
            }
        }
    }

    (to_gc, removed_symlinks)
}

fn print_tasks_tree(tasks: &Vec<tasks::TaskConfig>) {
    // Group tasks by their prefix (namespace)
    let mut namespaces: BTreeMap<String, Vec<&tasks::TaskConfig>> = BTreeMap::new();
    let mut standalone_tasks: Vec<&tasks::TaskConfig> = Vec::new();

    for task in tasks {
        if let Some(colon_pos) = task.name.find(':') {
            let namespace = &task.name[..colon_pos];
            namespaces
                .entry(namespace.to_string())
                .or_default()
                .push(task);
        } else {
            standalone_tasks.push(task);
        }
    }

    // Build dependency information
    let mut task_deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut task_dependents: HashMap<String, Vec<String>> = HashMap::new();
    let task_names: HashSet<String> = tasks.iter().map(|t| t.name.clone()).collect();
    let mut task_configs: HashMap<String, &tasks::TaskConfig> = HashMap::new();

    for task in tasks {
        task_deps.insert(task.name.clone(), task.after.clone());
        task_configs.insert(task.name.clone(), task);

        // Build reverse dependencies (dependents)
        for dep in &task.after {
            task_dependents
                .entry(dep.clone())
                .or_default()
                .push(task.name.clone());
        }

        // Handle "before" dependencies
        for before in &task.before {
            task_deps
                .entry(before.clone())
                .or_default()
                .push(task.name.clone());
            task_dependents
                .entry(task.name.clone())
                .or_default()
                .push(before.clone());
        }
    }

    let mut visited = HashSet::new();

    // Print namespaced tasks grouped by namespace
    for (namespace, tasks_in_ns) in namespaces.iter() {
        println!("{namespace}:");

        // Find roots within this namespace
        let mut ns_roots: Vec<&str> = Vec::new();
        for task in tasks_in_ns {
            let deps = task_deps.get(&task.name).unwrap();
            if deps.is_empty()
                || !deps
                    .iter()
                    .any(|d| task_names.contains(d) && d.starts_with(&format!("{namespace}:")))
            {
                ns_roots.push(&task.name);
            }
        }

        // If no roots found, use all tasks in namespace
        if ns_roots.is_empty() {
            ns_roots = tasks_in_ns.iter().map(|t| t.name.as_str()).collect();
        }

        ns_roots.sort();

        let sub_prefix = "  ";
        for (i, root) in ns_roots.iter().enumerate() {
            if !visited.contains(*root) {
                let is_last = i == ns_roots.len() - 1;
                print_task_tree_with_namespace(
                    root,
                    &task_dependents,
                    &task_configs,
                    &mut visited,
                    sub_prefix,
                    is_last,
                    namespace,
                );
            }
        }
    }

    // Print standalone tasks (without namespace)
    if !standalone_tasks.is_empty() {
        if !namespaces.is_empty() {
            println!("(standalone)");
        }

        // Find roots among standalone tasks
        let mut standalone_roots: Vec<&str> = Vec::new();
        for task in &standalone_tasks {
            let deps = task_deps.get(&task.name).unwrap();
            if deps.is_empty()
                || !deps
                    .iter()
                    .any(|d| task_names.contains(d) && !d.contains(':'))
            {
                standalone_roots.push(&task.name);
            }
        }

        if standalone_roots.is_empty() {
            standalone_roots = standalone_tasks.iter().map(|t| t.name.as_str()).collect();
        }

        standalone_roots.sort();

        let sub_prefix = if namespaces.is_empty() { "" } else { "  " };
        for (i, root) in standalone_roots.iter().enumerate() {
            if !visited.contains(*root) {
                let is_last = i == standalone_roots.len() - 1;
                print_task_tree(
                    root,
                    &task_dependents,
                    &task_configs,
                    &mut visited,
                    sub_prefix,
                    is_last,
                );
            }
        }
    }
}

fn print_task_tree_with_namespace(
    task_name: &str,
    task_dependents: &HashMap<String, Vec<String>>,
    task_configs: &HashMap<String, &tasks::TaskConfig>,
    visited: &mut HashSet<String>,
    prefix: &str,
    is_last: bool,
    namespace: &str,
) {
    if visited.contains(task_name) {
        return;
    }
    visited.insert(task_name.to_string());

    // Print the current task with tree formatting, stripping the namespace prefix
    let connector = if is_last { "└── " } else { "├── " };
    let display_name = task_name
        .strip_prefix(&format!("{namespace}:"))
        .unwrap_or(task_name);
    print!("{prefix}{connector}{display_name}");

    // Add additional info if available
    if let Some(task) = task_configs.get(task_name) {
        let mut extra_info = Vec::new();

        if task.status.is_some() {
            extra_info.push("has status check".to_string());
        }

        if !task.exec_if_modified.is_empty() {
            let files = task.exec_if_modified.join(", ");
            extra_info.push(format!("watches: {files}"));
        }

        if !extra_info.is_empty() {
            print!(" ({})", extra_info.join(", "));
        }
    }

    println!();

    // Get children (tasks that depend on this task) within the same namespace
    let children = task_dependents.get(task_name).cloned().unwrap_or_default();
    let mut children: Vec<_> = children
        .into_iter()
        .filter(|t| task_configs.contains_key(t) && t.starts_with(&format!("{namespace}:")))
        .collect();
    children.sort();

    // Determine the new prefix for children
    let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

    // Print children
    for (i, child) in children.iter().enumerate() {
        let is_last_child = i == children.len() - 1;
        print_task_tree_with_namespace(
            child,
            task_dependents,
            task_configs,
            visited,
            &new_prefix,
            is_last_child,
            namespace,
        );
    }
}

fn print_task_tree(
    task_name: &str,
    task_dependents: &HashMap<String, Vec<String>>,
    task_configs: &HashMap<String, &tasks::TaskConfig>,
    visited: &mut HashSet<String>,
    prefix: &str,
    is_last: bool,
) {
    if visited.contains(task_name) {
        return;
    }
    visited.insert(task_name.to_string());

    // Print the current task with tree formatting
    let connector = if is_last { "└── " } else { "├── " };
    print!("{prefix}{connector}{task_name}");

    // Add additional info if available
    if let Some(task) = task_configs.get(task_name) {
        let mut extra_info = Vec::new();

        if task.status.is_some() {
            extra_info.push("has status check".to_string());
        }

        if !task.exec_if_modified.is_empty() {
            let files = task.exec_if_modified.join(", ");
            extra_info.push(format!("watches: {files}"));
        }

        if !extra_info.is_empty() {
            print!(" ({})", extra_info.join(", "));
        }
    }

    println!();

    // Get children (tasks that depend on this task)
    let children = task_dependents.get(task_name).cloned().unwrap_or_default();
    let mut children: Vec<_> = children
        .into_iter()
        .filter(|t| task_configs.contains_key(t))
        .collect();
    children.sort();

    // Determine the new prefix for children
    let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

    // Print children
    for (i, child) in children.iter().enumerate() {
        let is_last_child = i == children.len() - 1;
        print_task_tree(
            child,
            task_dependents,
            task_configs,
            visited,
            &new_prefix,
            is_last_child,
        );
    }
}
