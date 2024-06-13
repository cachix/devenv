use super::{cli, command, config, log};
use clap::crate_version;
use cli_table::Table;
use cli_table::{print_stderr, WithTitle};
use include_dir::{include_dir, Dir};
use miette::{bail, IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    fs,
    path::{Path, PathBuf},
};

// templates
const FLAKE_TMPL: &str = include_str!("flake.tmpl.nix");
const REQUIRED_FILES: [&str; 4] = ["devenv.nix", "devenv.yaml", ".envrc", ".gitignore"];
const EXISTING_REQUIRED_FILES: [&str; 1] = [".gitignore"];
const PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/init");
// project vars
const DEVENV_FLAKE: &str = ".devenv.flake.nix";

pub struct Devenv {
    pub config: config::Config,
    pub global_options: cli::GlobalOptions,

    pub logger: log::Logger,

    // All kinds of paths
    pub devenv_root: PathBuf,
    pub devenv_dotfile: PathBuf,
    pub devenv_dot_gc: PathBuf,
    pub devenv_home_gc: PathBuf,
    pub devenv_tmp: String,
    pub devenv_runtime: PathBuf,

    // Caching
    pub cachix_caches: Option<command::CachixCaches>,
    pub cachix_trusted_keys: PathBuf,

    pub assembled: bool,
    pub dirs_created: bool,
    pub has_processes: Option<bool>,

    pub container_name: Option<String>,
}

impl Devenv {
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

        // fails if any of the required files already exists
        for filename in REQUIRED_FILES {
            let file_path = target.join(filename);
            if file_path.exists() && !EXISTING_REQUIRED_FILES.contains(&filename) {
                bail!("File already exists {}", file_path.display());
            }
        }

        for filename in REQUIRED_FILES {
            self.logger.info(&format!("Creating {}", filename));

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
                    .and_then(|mut file| file.write_all(path.contents()))
                    .expect("Failed to append to existing file");
            } else {
                std::fs::write(&target_path, path.contents()).expect("Failed to write file");
            }
        }

        // check if direnv executable is available
        let Ok(direnv) = which::which("direnv") else {
            return Ok(());
        };

        // run direnv allow
        std::process::Command::new(direnv)
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

    pub fn print_dev_env(&mut self, json: bool) -> Result<()> {
        let (env, _) = self.get_dev_environment(json, false)?;
        print!(
            "{}",
            String::from_utf8(env).expect("Failed to convert env to utf-8")
        );
        Ok(())
    }

    pub fn shell(
        &mut self,
        cmd: &Option<String>,
        args: &[String],
        replace_shell: bool,
    ) -> Result<()> {
        let develop_args = self.prepare_shell(cmd, args)?;

        let options = command::Options {
            replace_shell,
            ..command::Options::default()
        };

        let develop_args = develop_args
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>();

        self.run_nix("nix", &develop_args, &options)?;
        Ok(())
    }

    pub fn prepare_shell(&mut self, cmd: &Option<String>, args: &[String]) -> Result<Vec<String>> {
        self.assemble(false)?;
        let (_, gc_root) = self.get_dev_environment(false, true)?;

        let mut develop_args = vec![
            "develop",
            gc_root.to_str().expect("gc root should be utf-8"),
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

        match cmd {
            Some(cmd) => {
                develop_args.push("-c");
                develop_args.push(cmd);
                let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
                develop_args.extend_from_slice(&args);
            }
            None => {
                self.logger.info("Entering shell");
            }
        };

        Ok(develop_args.into_iter().map(|s| s.to_string()).collect())
    }

    pub fn update(&mut self, input_name: &Option<String>) -> Result<()> {
        let msg = match input_name {
            Some(input_name) => format!("Updating devenv.lock with input {input_name}"),
            None => "Updating devenv.lock".to_string(),
        };
        let _logprogress = log::LogProgress::new(&msg, true);
        self.assemble(false)?;

        match input_name {
            Some(input_name) => {
                self.run_nix(
                    "nix",
                    &["flake", "lock", "--update-input", input_name],
                    &command::Options::default(),
                )?;
            }
            None => {
                self.run_nix("nix", &["flake", "update"], &command::Options::default())?;
            }
        }
        Ok(())
    }

    pub fn container_build(&mut self, name: &str) -> Result<String> {
        if cfg!(target_os = "macos") {
            bail!("Containers are not supported on macOS yet: https://github.com/cachix/devenv/issues/430");
        }

        let _logprogress = log::LogProgress::new(&format!("Building {name} container"), true);

        self.assemble(false)?;

        let container_store_path = self.run_nix(
            "nix",
            &[
                "build",
                "--print-out-paths",
                "--no-link",
                &format!(".#devenv.containers.{name}.derivation"),
            ],
            &command::Options::default(),
        )?;

        let container_store_path_string = String::from_utf8_lossy(&container_store_path.stdout)
            .to_string()
            .trim()
            .to_string();
        println!("{}", container_store_path_string);
        Ok(container_store_path_string)
    }

    pub fn container_copy(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        let spec = self.container_build(name)?;

        let _logprogress = log::LogProgress::new(&format!("Copying {name} container"), false);

        let copy_script = self.run_nix(
            "nix",
            &[
                "build",
                "--print-out-paths",
                "--no-link",
                &format!(".#devenv.containers.{name}.copyScript"),
            ],
            &command::Options::default(),
        )?;

        let copy_script = String::from_utf8_lossy(&copy_script.stdout)
            .to_string()
            .trim()
            .to_string();

        let copy_args = [
            spec,
            registry.unwrap_or("false").to_string(),
            copy_args.join(" "),
        ];

        self.logger
            .info(&format!("Running {copy_script} {}", copy_args.join(" ")));

        let status = std::process::Command::new(copy_script)
            .args(copy_args)
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

    pub fn container_run(
        &mut self,
        name: &str,
        copy_args: &[String],
        registry: Option<&str>,
    ) -> Result<()> {
        if registry.is_some() {
            self.logger
                .warn("Ignoring --registry flag when running container");
        };
        self.container_copy(name, copy_args, Some("docker-daemon:"))?;

        let _logprogress = log::LogProgress::new(&format!("Running {name} container"), false);

        let run_script = self.run_nix(
            "nix",
            &[
                "build",
                "--print-out-paths",
                "--no-link",
                &format!(".#devenv.containers.{name}.dockerRun"),
            ],
            &command::Options::default(),
        )?;

        let run_script = String::from_utf8_lossy(&run_script.stdout)
            .to_string()
            .trim()
            .to_string();

        let status = std::process::Command::new(run_script)
            .status()
            .expect("Failed to run container script");

        if !status.success() {
            bail!("Failed to run container")
        } else {
            Ok(())
        }
    }

    pub fn repl(&mut self) -> Result<()> {
        self.assemble(false)?;

        let mut cmd = self.prepare_command("nix", &["repl", "."])?;
        cmd.exec();
        Ok(())
    }

    pub fn gc(&mut self) -> Result<()> {
        let start = std::time::Instant::now();

        let (to_gc, removed_symlinks) = {
            let _logprogress = log::LogProgress::new(
                &format!(
                    "Removing non-existing symlinks in {} ...",
                    &self.devenv_home_gc.display()
                ),
                false,
            );
            cleanup_symlinks(&self.devenv_home_gc)
        };

        self.logger
            .info(&format!("Found {} active environments.", to_gc.len()));
        self.logger.info(&format!(
            "Deleted {} dangling environments (most likely due to previous GC).",
            removed_symlinks.len()
        ));

        {
            let _logprogress = log::LogProgress::new(
                "Running garbage collection (this process may take some time) ...",
                false,
            );
            let paths: Vec<&str> = to_gc
                .iter()
                .filter_map(|path_buf| path_buf.to_str())
                .collect();
            let args: Vec<&str> = ["store", "gc"]
                .iter()
                .chain(paths.iter())
                .copied()
                .collect();
            self.run_nix("nix", &args, &command::Options::default())?;
        }

        let (after_gc, _) = cleanup_symlinks(&self.devenv_home_gc);
        let end = std::time::Instant::now();

        eprintln!();
        self.logger.info(&format!(
            "Done. Successfully removed {} symlinks in {}s.",
            to_gc.len() - after_gc.len(),
            (end - start).as_secs_f32()
        ));
        Ok(())
    }

    pub fn search(&mut self, name: &str) -> Result<()> {
        self.assemble(false)?;

        let options = self.run_nix(
            "nix",
            &["build", "--no-link", "--print-out-paths", ".#optionsJSON"],
            &command::Options::default(),
        )?;

        let options_str = std::str::from_utf8(&options.stdout).unwrap().trim();
        let options_path = PathBuf::from_str(options_str)
            .expect("options store path should be a utf-8")
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

        let search = self.run_nix(
            "nix",
            &["search", "--json", "nixpkgs", name],
            &command::Options::default(),
        )?;
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

        self.logger.info(&format!("Found {search_results_count} packages and {results_options_count} options for '{name}'."));
        Ok(())
    }

    pub fn has_processes(&mut self) -> Result<bool> {
        if self.has_processes.is_none() {
            let result = self.run_nix(
                "nix",
                &["eval", ".#devenv.processes", "--json"],
                &command::Options::default(),
            )?;
            let processes = String::from_utf8_lossy(&result.stdout).to_string();
            self.has_processes = Some(processes.trim() != "{}");
        }
        Ok(self.has_processes.unwrap())
    }

    pub fn test(&mut self, dont_override_dotfile: bool) -> Result<()> {
        let tmpdir = tempdir::TempDir::new_in(&self.devenv_root, ".devenv")
            .expect("Failed to create temporary directory");
        if !dont_override_dotfile {
            self.logger.info(&format!(
                "Overriding .devenv to {}",
                tmpdir.path().file_name().unwrap().to_str().unwrap()
            ));
            self.devenv_dotfile = tmpdir.as_ref().to_path_buf();
            // TODO: don't add gc roots for tests
            self.devenv_dot_gc = self.devenv_dotfile.join("gc");
        }
        self.assemble(true)?;

        // collect tests
        let test_script = {
            let _logprogress = log::LogProgress::new("Building tests", true);
            self.run_nix(
                "nix",
                &["build", ".#devenv.test", "--no-link", "--print-out-paths"],
                &command::Options::default(),
            )?
        };

        let test_script_string = String::from_utf8_lossy(&test_script.stdout)
            .to_string()
            .trim()
            .to_string();
        if test_script_string.is_empty() {
            self.logger.error("No tests found.");
            bail!("No tests found");
        }

        if self.has_processes()? {
            self.up(None, &true, &false)?;
        }

        let result = {
            let _logprogress = log::LogProgress::new("Running tests", true);

            self.logger
                .debug(&format!("Running command: {test_script_string}"));

            let develop_args = self.prepare_shell(&Some(test_script_string), &[])?;
            let develop_args = develop_args
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>();
            let mut cmd = self.prepare_command("nix", &develop_args)?;
            cmd.stdin(std::process::Stdio::inherit());
            cmd.stderr(std::process::Stdio::inherit());
            cmd.stdout(std::process::Stdio::inherit());
            cmd.output().expect("Failed to run tests")
        };

        if self.has_processes()? {
            self.down()?;
        }

        if !result.status.success() {
            self.logger.error("Tests failed :(");
            bail!("Tests failed");
        } else {
            self.logger.info("Tests passed :)");
            Ok(())
        }
    }

    pub fn info(&mut self) -> Result<()> {
        self.assemble(false)?;

        // TODO: use --json
        let metadata = self.run_nix("nix", &["flake", "metadata"], &command::Options::default())?;

        let re = regex::Regex::new(r"(Inputs:.+)$").unwrap();
        let metadata_str = String::from_utf8_lossy(&metadata.stdout);
        let inputs = match re.captures(&metadata_str) {
            Some(captures) => captures.get(1).unwrap().as_str(),
            None => "",
        };

        let info_ = self.run_nix(
            "nix",
            &["eval", "--raw", ".#info"],
            &command::Options::default(),
        )?;
        println!("{}\n{}", inputs, &String::from_utf8_lossy(&info_.stdout));
        Ok(())
    }

    pub fn build(&mut self, attributes: &[String]) -> Result<()> {
        self.assemble(false)?;

        let formatted_strings: Vec<String> = attributes
            .iter()
            .map(|attr| format!("#.devenv.{}", attr))
            .collect();

        let mut args: Vec<&str> = formatted_strings.iter().map(|s| s.as_str()).collect();

        args.insert(0, "build");
        self.run_nix("nix", &args, &command::Options::default())?;
        Ok(())
    }

    pub fn add_gc(&mut self, name: &str, path: &Path) -> Result<()> {
        self.run_nix(
            "nix-store",
            &[
                "--add-root",
                self.devenv_dot_gc.join(name).to_str().unwrap(),
                "-r",
                path.to_str().unwrap(),
            ],
            &command::Options::default(),
        )?;
        let link_path = self
            .devenv_dot_gc
            .join(format!("{}-{}", name, get_now_with_nanoseconds()));
        symlink_force(&self.logger, path, &link_path);
        Ok(())
    }

    pub fn up(&mut self, process: Option<&str>, detach: &bool, log_to_file: &bool) -> Result<()> {
        self.assemble(false)?;
        if !self.has_processes()? {
            self.logger
                .error("No 'processes' option defined: https://devenv.sh/processes/");
            bail!("No processes defined");
        }

        let proc_script_string: String;
        {
            let _logprogress = log::LogProgress::new("Building processes", true);

            let proc_script = self.run_nix(
                "nix",
                &[
                    "build",
                    "--no-link",
                    "--print-out-paths",
                    ".#procfileScript",
                ],
                &command::Options::default(),
            )?;

            proc_script_string = String::from_utf8_lossy(&proc_script.stdout)
                .to_string()
                .trim()
                .to_string();
            self.add_gc("procfilescript", Path::new(&proc_script_string))?;
        }

        {
            let _logprogress = log::LogProgress::new("Starting processes", true);

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

            let args =
                self.prepare_shell(&Some(processes_script.to_str().unwrap().to_string()), &[])?;
            let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
            let mut cmd = self.prepare_command("nix", &args)?;

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
                self.logger.info(&format!("PID is {}", process.id()));
                if *log_to_file {
                    self.logger.info(&format!(
                        "See logs:  $ tail -f {}",
                        self.processes_log().display()
                    ));
                }
                self.logger.info("Stop:      $ devenv processes stop");
            } else {
                cmd.exec();
            }
            Ok(())
        }
    }

    pub fn down(&self) -> Result<()> {
        if !PathBuf::from(&self.processes_pid()).exists() {
            self.logger.error("No processes running.");
            bail!("No processes running");
        }

        let pid = std::fs::read_to_string(self.processes_pid())
            .expect("Failed to read PROCESSES_PID")
            .parse::<i32>()
            .expect("Failed to parse PROCESSES_PID");

        self.logger
            .info(&format!("Stopping process with PID {}", pid));

        let pid = nix::unistd::Pid::from_raw(pid);
        match nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM) {
            Ok(_) => {}
            Err(_) => {
                self.logger
                    .error(&format!("Process with PID {} not found.", pid));
                bail!("Process not found");
            }
        }

        std::fs::remove_file(self.processes_pid()).expect("Failed to remove PROCESSES_PID");
        Ok(())
    }

    pub fn create_directories(&mut self) -> Result<()> {
        if !self.dirs_created {
            let xdg_dirs = xdg::BaseDirectories::with_prefix("devenv")
                .into_diagnostic()
                .wrap_err("Failed to get XDG directories")?;
            xdg_dirs
                .create_data_directory(Path::new("devenv"))
                .into_diagnostic()
                .wrap_err("Failed to create DEVENV_HOME directory")?;
            std::fs::create_dir_all(&self.devenv_home_gc)
                .into_diagnostic()
                .wrap_err("Failed to create DEVENV_HOME_GC directory")?;
            std::fs::create_dir_all(&self.devenv_dot_gc)
                .into_diagnostic()
                .wrap_err("Failed to create .devenv/gc directory")?;
            self.dirs_created = true;
        }
        Ok(())
    }

    pub fn assemble(&mut self, is_testing: bool) -> Result<()> {
        if !self.assembled {
            if !PathBuf::from("devenv.nix").exists() {
                bail!(indoc::indoc! {"
                File devenv.nix does not exist. To get started, run:

                    $ devenv init
                "});
            }
            std::fs::create_dir_all(&self.devenv_dot_gc)
                .unwrap_or_else(|_| panic!("Failed to create {}", self.devenv_dot_gc.display()));

            let mut flake_inputs = HashMap::new();
            for (input, attrs) in self.config.inputs.iter() {
                match config::FlakeInput::try_from(attrs) {
                    Ok(flake_input) => {
                        flake_inputs.insert(input.clone(), flake_input);
                    }
                    Err(e) => {
                        self.logger
                            .error(&format!("Failed to parse input {}: {}", input, e));
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
                is_testing
            );
            let flake = FLAKE_TMPL.replace("__DEVENV_VARS__", &vars);
            std::fs::write(DEVENV_FLAKE, flake).expect("Failed to write flake.nix");
        }
        self.assembled = true;
        Ok(())
    }

    pub fn get_dev_environment(&mut self, json: bool, logging: bool) -> Result<(Vec<u8>, PathBuf)> {
        self.assemble(false)?;
        let _logprogress = if logging {
            Some(log::LogProgress::new("Building shell", true))
        } else {
            None
        };
        let gc_root = self.devenv_dot_gc.join("shell");
        let gc_root_str = gc_root.to_str().expect("gc root should be utf-8");

        let mut args: Vec<&str> = vec!["print-dev-env", "--profile", gc_root_str];
        if json {
            args.push("--json");
        }

        let env = self.run_nix("nix", &args, &command::Options::default())?;

        let options = command::Options {
            logging: false,
            ..command::Options::default()
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

        Ok((env.stdout, gc_root))
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

fn get_now_with_nanoseconds() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    let secs = duration.as_secs();
    let nanos = duration.subsec_nanos();
    format!("{}.{}", secs, nanos)
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
