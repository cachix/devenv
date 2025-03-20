use super::{cli, cnix, config, tasks};
use clap::crate_version;
use cli_table::Table;
use cli_table::{print_stderr, WithTitle};
use include_dir::{include_dir, Dir};
use miette::{bail, IntoDiagnostic, Result};
use nix::sys::signal;
use nix::unistd::Pid;
use once_cell::sync::Lazy;
use serde::Deserialize;
use sha2::Digest;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::{debug, error, info, info_span, warn, Instrument};

// templates
const FLAKE_TMPL: &str = include_str!("flake.tmpl.nix");
const REQUIRED_FILES: [&str; 4] = ["devenv.nix", "devenv.yaml", ".envrc", ".gitignore"];
const EXISTING_REQUIRED_FILES: [&str; 1] = [".gitignore"];
const PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/init");
pub static DIRENVRC: Lazy<String> = Lazy::new(|| {
    include_str!("../../direnvrc").replace(
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=0",
        "DEVENV_DIRENVRC_ROLLING_UPGRADE=1",
    )
});
pub static DIRENVRC_VERSION: Lazy<u8> = Lazy::new(|| {
    DIRENVRC
        .lines()
        .find(|line| line.contains("export DEVENV_DIRENVRC_VERSION"))
        .map(|line| line.split('=').last().unwrap().trim())
        .and_then(|version| version.parse().ok())
        .unwrap_or(0)
});
// project vars
const DEVENV_FLAKE: &str = ".devenv.flake.nix";

#[derive(Default)]
pub struct DevenvOptions {
    pub config: config::Config,
    pub global_options: Option<cli::GlobalOptions>,
    pub devenv_root: Option<PathBuf>,
    pub devenv_dotfile: Option<PathBuf>,
}

pub struct Devenv {
    pub config: config::Config,
    pub global_options: cli::GlobalOptions,

    nix: cnix::Nix,

    // All kinds of paths
    devenv_root: PathBuf,
    devenv_dotfile: PathBuf,
    devenv_dot_gc: PathBuf,
    devenv_home_gc: PathBuf,
    devenv_tmp: String,
    devenv_runtime: PathBuf,

    assembled: bool,
    has_processes: Option<bool>,

    // TODO: make private.
    // Pass as an arg or have a setter.
    pub container_name: Option<String>,
}

impl Devenv {
    pub async fn new(options: DevenvOptions) -> Self {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv").unwrap();
        let devenv_home = xdg_dirs.get_data_home();
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
        std::fs::create_dir_all(&devenv_home_gc)
            .expect("Failed to create DEVENV_HOME_GC directory");

        let nix = cnix::Nix::new(
            options.config.clone(),
            global_options.clone(),
            cachix_trusted_keys,
            devenv_home_gc.clone(),
            devenv_dotfile.clone(),
            devenv_dot_gc.clone(),
            devenv_root.clone(),
        )
        .await
        .expect("Failed to initialize Nix");

        Self {
            config: options.config,
            global_options,
            devenv_root,
            devenv_dotfile,
            devenv_dot_gc,
            devenv_home_gc,
            devenv_tmp,
            devenv_runtime,
            nix,
            assembled: false,
            has_processes: None,
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
        let target = target
            .clone()
            .unwrap_or_else(|| fs::canonicalize(".").expect("Failed to get current directory"));

        // create directory target if not exists
        if !target.exists() {
            std::fs::create_dir_all(&target).expect("Failed to create target directory");
        }

        for filename in REQUIRED_FILES {
            info!("Creating {}", filename);

            let path = PROJECT_DIR
                .get_file(filename)
                .unwrap_or_else(|| panic!("missing {} in the executable", filename));

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
        let _ = std::process::Command::new(direnv)
            .arg("allow")
            .current_dir(&target)
            .exec();
        Ok(())
    }

    pub fn inputs_add(&mut self, name: &str, url: &str, follows: &[String]) -> Result<()> {
        self.config.add_input(name, url, follows);
        self.config.write();
        Ok(())
    }

    pub async fn print_dev_env(&mut self, json: bool) -> Result<()> {
        let env = self.get_dev_environment(json).await?;
        print!(
            "{}",
            String::from_utf8(env.output).expect("Failed to convert env to utf-8")
        );
        Ok(())
    }

    pub async fn shell(
        &mut self,
        cmd: &Option<String>,
        args: &[String],
        replace_shell: bool,
    ) -> Result<()> {
        let develop_args = self.prepare_develop_args(cmd, args).await?;

        let develop_args = develop_args
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>();

        let span = info_span!("Entering shell", devenv.user_message = "Entering shell");
        self.nix
            .develop(&develop_args, replace_shell)
            .instrument(span)
            .await?;
        Ok(())
    }

    pub async fn prepare_develop_args(
        &mut self,
        cmd: &Option<String>,
        args: &[String],
    ) -> Result<Vec<String>> {
        self.assemble(false).await?;
        let env = self.get_dev_environment(false).await?;

        let mut develop_args = vec![
            "develop",
            env.gc_root.to_str().expect("gc root should be utf-8"),
        ];

        let default_clean = config::Clean {
            enabled: false,
            keep: vec![],
        };
        let config_clean = self.config.clean.as_ref().unwrap_or(&default_clean);
        if self.global_options.clean.is_some() || config_clean.enabled {
            develop_args.push("--ignore-environment");

            let keep = match &self.global_options.clean {
                Some(clean) => clean,
                None => &config_clean.keep,
            };

            for env in keep {
                develop_args.push("--keep");
                develop_args.push(env);
            }

            develop_args.push("-c");
            develop_args.push("bash");
            develop_args.push("--norc");
            develop_args.push("--noprofile")
        }

        if let Some(cmd) = cmd {
            develop_args.push("-c");
            develop_args.push(cmd);
            let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
            develop_args.extend_from_slice(&args);
        }

        Ok(develop_args.into_iter().map(|s| s.to_string()).collect())
    }

    pub async fn update(&mut self, input_name: &Option<String>) -> Result<()> {
        self.assemble(false).await?;

        let msg = match input_name {
            Some(input_name) => format!("Updating devenv.lock with input {input_name}"),
            None => "Updating devenv.lock".to_string(),
        };

        let span = info_span!("update", devenv.user_message = msg);
        self.nix.update(input_name).instrument(span).await?;

        Ok(())
    }

    pub async fn container_build(&mut self, name: &str) -> Result<String> {
        if cfg!(target_os = "macos") {
            bail!("Containers are not supported on macOS yet: https://github.com/cachix/devenv/issues/430");
        }

        let span = info_span!(
            "building_container",
            devenv.user_message = format!("Building {name} container")
        );

        async move {
            self.assemble(false).await?;

            let container_store_path = self
                .nix
                .build(&[&format!("devenv.containers.{name}.derivation")], None)
                .await?;
            let container_store_path = container_store_path[0]
                .to_str()
                .expect("Failed to get container store path");
            println!("{}", &container_store_path);
            Ok(container_store_path.to_string())
        }
        .instrument(span)
        .await
    }

    pub async fn container_copy(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        let spec = self.container_build(name).await?;

        // TODO: No newline
        let span = info_span!(
            "copying_container",
            devenv.user_message = format!("Copying {name} container")
        );

        async move {
            let copy_script = self
                .nix
                .build(&[&format!("devenv.containers.{name}.copyScript")], None)
                .await?;
            let copy_script = &copy_script[0];
            let copy_script_string = &copy_script.to_string_lossy();

            let base_args = [spec, registry.unwrap_or("false").to_string()];
            let command_args: Vec<String> = base_args
                .into_iter()
                .chain(copy_args.iter().map(|s| s.to_string()))
                .collect();

            info!("Running {copy_script_string} {}", command_args.join(" "));

            let status = std::process::Command::new(copy_script)
                .args(command_args)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
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

        let span = info_span!(
            "running_container",
            devenv.user_message = format!("Running {name} container")
        );

        async move {
            let run_script = self
                .nix
                .build(&[&format!("devenv.containers.{name}.dockerRun")], None)
                .await?;

            let status = std::process::Command::new(&run_script[0])
                .status()
                .expect("Failed to run container script");

            if !status.success() {
                bail!("Failed to run container")
            } else {
                Ok(())
            }
        }
        .instrument(span)
        .await
    }

    pub async fn repl(&mut self) -> Result<()> {
        self.assemble(false).await?;
        self.nix.repl()
    }

    pub fn gc(&mut self) -> Result<()> {
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
            info!("If you'd like this to run faster, leave a thumbs up at https://github.com/NixOS/nix/issues/7239");
            span.in_scope(|| self.nix.gc(to_gc))?;
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

    pub async fn search(&mut self, name: &str) -> Result<()> {
        self.assemble(false).await?;

        let build_options = cnix::Options {
            logging: false,
            cache_output: true,
            ..Default::default()
        };
        let options = self
            .nix
            .build(&["optionsJSON"], Some(build_options))
            .await?;
        let options_path = options[0]
            .join("share")
            .join("doc")
            .join("nixos")
            .join("options.json");
        let options_contents = fs::read(options_path).expect("Failed to read options.json");
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
        let results_options_count = options_results.len();

        let search = self.nix.search(name).await?;
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
        let search_results_count = search_results.len();

        if !search_results.is_empty() {
            print_stderr(search_results.with_title()).expect("Failed to print search results");
        }

        if !options_results.is_empty() {
            print_stderr(options_results.with_title()).expect("Failed to print options results");
        }

        info!("Found {search_results_count} packages and {results_options_count} options for '{name}'.");
        Ok(())
    }

    pub async fn has_processes(&mut self) -> Result<bool> {
        if self.has_processes.is_none() {
            let processes = self.nix.eval(&["devenv.processes"]).await?;
            self.has_processes = Some(processes.trim() != "{}");
        }
        Ok(self.has_processes.unwrap())
    }

    pub async fn tasks_run(&mut self, roots: Vec<String>) -> Result<()> {
        self.assemble(false).await?;
        if roots.is_empty() {
            bail!("No tasks specified.");
        }
        let tasks_json_file = {
            // TODO: No newline
            let span = info_span!("tasks_run", devenv.user_message = "Evaluating tasks");
            self.nix
                .build(&["devenv.task.config"], None)
                .instrument(span)
                .await?
        };
        // parse tasks config
        let tasks_json =
            std::fs::read_to_string(&tasks_json_file[0]).expect("Failed to read config file");
        let tasks: Vec<tasks::TaskConfig> =
            serde_json::from_str(&tasks_json).expect("Failed to parse tasks config");
        // run tasks
        let config = tasks::Config { roots, tasks };
        debug!(
            "Tasks config: {}",
            serde_json::to_string_pretty(&config).unwrap()
        );
        let mut tui = tasks::TasksUi::new(config).await?;
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

    pub async fn test(&mut self) -> Result<()> {
        self.assemble(true).await?;

        // collect tests
        let test_script = {
            let span = info_span!("test", devenv.user_message = "Building tests");
            self.nix
                .build(&["devenv.test"], None)
                .instrument(span)
                .await?
        };
        let test_script = test_script[0].to_string_lossy().to_string();

        if self.has_processes().await? {
            self.up(None, &true, &false).await?;
        }

        let span = info_span!("test", devenv.user_message = "Running tests");
        let result = async {
            debug!("Running command: {test_script}");
            let develop_args = self.prepare_develop_args(&Some(test_script), &[]).await?;
            // TODO: replace_shell?
            self.nix
                .develop(
                    &develop_args
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<&str>>(),
                    false, // replace_shell
                )
                .await
        }
        .instrument(span)
        .await?;

        if self.has_processes().await? {
            self.down()?;
        }

        if !result.status.success() {
            error!("Tests failed :(");
            bail!("Tests failed");
        } else {
            info!("Tests passed :)");
            Ok(())
        }
    }

    pub async fn info(&mut self) -> Result<()> {
        self.assemble(false).await?;
        let output = self.nix.metadata().await?;
        println!("{}", output);
        Ok(())
    }

    pub async fn build(&mut self, attributes: &[String]) -> Result<()> {
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
                                .flat_map(|(k, v)| flatten_object(&format!("{}.{}", prefix, k), v))
                                .collect(),
                            _ => vec![format!("devenv.{}", prefix)],
                        }
                    }
                    flatten_object(key, value)
                })
                .collect()
        } else {
            attributes
                .iter()
                .map(|attr| format!("devenv.{}", attr))
                .collect()
        };
        let paths = self
            .nix
            .build(
                &attributes.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                None,
            )
            .await?;
        for path in paths {
            println!("{}", path.display());
        }
        Ok(())
    }

    pub async fn up(
        &mut self,
        process: Option<&str>,
        detach: &bool,
        log_to_file: &bool,
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
            let proc_script = self.nix.build(&["procfileScript"], None).await?;
            let proc_script_string = proc_script[0]
                .to_str()
                .expect("Failed to get proc script path")
                .to_string();
            self.nix.add_gc("procfilescript", &proc_script[0]).await?;
            Ok::<String, miette::Report>(proc_script_string)
        }
        .instrument(span)
        .await?;

        let span = info_span!("up", devenv.user_message = "Starting processes");
        async {
            let process = process.unwrap_or("");

            let processes_script = self.devenv_dotfile.join("processes");
            // we force disable process compose tui if detach is enabled
            let tui = if *detach {
                "export PC_TUI_ENABLED=0"
            } else {
                ""
            };
            fs::write(
                &processes_script,
                indoc::formatdoc! {"
                #!/usr/bin/env bash
                {tui}
                exec {proc_script_string} {process}
            "},
            )
            .expect("Failed to write PROCESSES_SCRIPT");

            std::fs::set_permissions(&processes_script, std::fs::Permissions::from_mode(0o755))
                .expect("Failed to set permissions");

            let develop_args = self
                .prepare_develop_args(&Some(processes_script.to_str().unwrap().to_string()), &[])
                .await?;

            let span = info_span!("Entering shell");
            let mut cmd = self
                .nix
                .prepare_command_with_substituters(
                    "nix",
                    &develop_args
                        .iter()
                        .map(AsRef::as_ref)
                        .collect::<Vec<&str>>(),
                    &self.nix.options,
                )
                .instrument(span)
                .await?;

            if *detach {
                let log_file = std::fs::File::create(self.processes_log())
                    .expect("Failed to create PROCESSES_LOG");
                let process = if !*log_to_file {
                    cmd.stdout(std::process::Stdio::inherit())
                        .stderr(std::process::Stdio::inherit())
                        .spawn()
                        .expect("Failed to spawn process")
                } else {
                    cmd.stdout(log_file.try_clone().expect("Failed to clone Stdio"))
                        .stderr(log_file)
                        .spawn()
                        .expect("Failed to spawn process")
                };

                std::fs::write(self.processes_pid(), process.id().to_string())
                    .expect("Failed to write PROCESSES_PID");
                info!("PID is {}", process.id());
                if *log_to_file {
                    info!("See logs:  $ tail -f {}", self.processes_log().display());
                }
                info!("Stop:      $ devenv processes stop");
            } else {
                let err = cmd.exec();
                bail!(err);
            }
            Ok(())
        }
        .instrument(span)
        .await
    }

    pub fn down(&self) -> Result<()> {
        if !PathBuf::from(&self.processes_pid()).exists() {
            error!("No processes running.");
            bail!("No processes running");
        }

        let pid = std::fs::read_to_string(self.processes_pid())
            .expect("Failed to read PROCESSES_PID")
            .parse::<i32>()
            .expect("Failed to parse PROCESSES_PID");

        info!("Stopping process with PID {}", pid);

        let pid = Pid::from_raw(pid);
        match signal::kill(pid, signal::Signal::SIGTERM) {
            Ok(_) => {}
            Err(_) => {
                error!("Process with PID {} not found.", pid);
                bail!("Process not found");
            }
        }

        std::fs::remove_file(self.processes_pid()).expect("Failed to remove PROCESSES_PID");
        Ok(())
    }

    pub async fn assemble(&mut self, is_testing: bool) -> Result<()> {
        if self.assembled {
            return Ok(());
        }

        if !self.devenv_root.join("devenv.nix").exists() {
            bail!(indoc::indoc! {"
            File devenv.nix does not exist. To get started, run:

                $ devenv init
            "});
        }

        fs::create_dir_all(&self.devenv_dot_gc)
            .unwrap_or_else(|_| panic!("Failed to create {}", self.devenv_dot_gc.display()));

        // Initialise any Nix state
        self.nix.assemble().await?;

        let mut flake_inputs = HashMap::new();
        for (input, attrs) in self.config.inputs.iter() {
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
        fs::write(
            self.devenv_dotfile.join("flake.json"),
            serde_json::to_string(&flake_inputs).unwrap(),
        )
        .expect("Failed to write flake.json");
        fs::write(
            self.devenv_dotfile.join("devenv.json"),
            serde_json::to_string(&self.config).unwrap(),
        )
        .expect("Failed to write devenv.json");
        // TODO: superceded by eval caching.
        // Remove once direnvrc migration is implemented.
        fs::write(
            self.devenv_dotfile.join("imports.txt"),
            self.config.imports.join("\n"),
        )
        .expect("Failed to write imports.txt");

        // create flake.devenv.nix
        let vars = indoc::formatdoc!(
            "version = \"{}\";
            system = \"{}\";
            devenv_root = \"{}\";
            devenv_dotfile = ./{};
            devenv_dotfile_string = \"{}\";
            container_name = {};
            devenv_tmpdir = \"{}\";
            devenv_runtime = \"{}\";
            devenv_istesting = {};
            devenv_direnvrc_latest_version = {};
            ",
            crate_version!(),
            self.global_options.system,
            self.devenv_root.display(),
            self.devenv_dotfile.file_name().unwrap().to_str().unwrap(),
            self.devenv_dotfile.file_name().unwrap().to_str().unwrap(),
            self.container_name
                .as_deref()
                .map(|s| format!("\"{}\"", s))
                .unwrap_or_else(|| "null".to_string()),
            self.devenv_tmp,
            self.devenv_runtime.display(),
            is_testing,
            DIRENVRC_VERSION.to_string()
        );
        let flake = FLAKE_TMPL.replace("__DEVENV_VARS__", &vars);
        let flake_path = self.devenv_root.join(DEVENV_FLAKE);

        // Avoid writing the flake if it hasn't changed.
        // direnv's watch_file triggers a reload based solely on mtime, which becomes annoying if we constantly touch this file.
        let existing_flake = fs::read_to_string(&flake_path).unwrap_or_default();
        if flake != existing_flake {
            fs::write(flake_path, flake).expect("Failed to write flake.nix");
        }

        self.assembled = true;
        Ok(())
    }

    pub async fn get_dev_environment(&mut self, json: bool) -> Result<DevEnv> {
        self.assemble(false).await?;

        let gc_root = self.devenv_dot_gc.join("shell");
        let span = tracing::info_span!("building_shell", devenv.user_message = "Building shell",);
        let env = self.nix.dev_env(json, &gc_root).instrument(span).await?;

        use devenv_eval_cache::command::{FileInputDesc, Input};
        fs::write(
            self.devenv_dotfile.join("input-paths.txt"),
            env.inputs
                .iter()
                .filter_map(|input| match input {
                    Input::File(FileInputDesc { path, .. }) => {
                        Some(path.to_string_lossy().to_string())
                    }
                    // TODO(sander): update direnvrc to handle env vars if possible
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("Failed to write input-paths.txt");

        Ok(DevEnv {
            output: env.stdout,
            gc_root,
        })
    }
}

fn confirm_overwrite(file: &Path, contents: String) -> Result<()> {
    if std::fs::metadata(file).is_ok() {
        // first output the old version and propose new changes
        let before = std::fs::read_to_string(file).expect("Failed to read file");

        let diff = TextDiff::from_lines(&before, &contents);

        println!("\nChanges that will be made to {}:", file.to_string_lossy());
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "\x1b[31m-\x1b[0m",
                ChangeTag::Insert => "\x1b[32m+\x1b[0m",
                ChangeTag::Equal => " ",
            };
            print!("{}{}", sign, change);
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
    gc_root: PathBuf,
}

#[derive(Deserialize)]
struct PackageResults(HashMap<String, PackageResult>);

#[derive(Deserialize)]
struct PackageResult {
    version: String,
    description: String,
}

#[derive(Deserialize)]
struct OptionResults(HashMap<String, OptionResult>);

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

fn cleanup_symlinks(root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut to_gc = Vec::new();
    let mut removed_symlinks = Vec::new();

    if !root.exists() {
        std::fs::create_dir_all(root).expect("Failed to create gc directory");
    }

    for entry in fs::read_dir(root).expect("Failed to read directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_symlink() {
            if !path.exists() {
                removed_symlinks.push(path.clone());
            } else {
                let target = fs::canonicalize(&path).expect("Failed to read link");
                to_gc.push(target);
            }
        }
    }

    (to_gc, removed_symlinks)
}
