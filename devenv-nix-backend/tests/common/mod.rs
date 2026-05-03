//! Common test utilities shared across test files.
//!
//! `TestEnv::builder()` is the ergonomic entry point — defaults give a
//! complete, runnable backend environment, builder methods customize.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use devenv_core::{
    BootstrapArgs, CacheOptions, CacheSettings, CliOptionsConfig, Config, DevenvPaths, NixArgs,
    NixOptions, NixSettings, PortAllocator, StoreSettings, default_system,
};
use devenv_nix_backend::NixCBackend;
use tempfile::TempDir;

/// Default `devenv.yaml` for tests that don't care about flake input details.
pub const DEFAULT_YAML: &str = r#"inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  git-hooks:
    url: github:cachix/git-hooks.nix
"#;

/// Default `devenv.nix`. Minimal so the bootstrap eval has something to import.
pub const DEFAULT_NIX: &str = "{ ... }: { }";

/// Restores cwd on drop. Backend ops aren't cwd-dependent, but
/// `Config::load_from` falls back to `env::current_dir` when resolving
/// some import paths, so we pin cwd to the temp dir for the duration of
/// the test.
pub struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    pub fn enter(target: &Path) -> Self {
        let original = std::env::current_dir().expect("get cwd");
        std::env::set_current_dir(target).expect("set cwd");
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

/// Build the [`DevenvPaths`] layout under `base` and create the on-disk
/// directories the backend writes into.
pub fn paths_under(base: &Path) -> DevenvPaths {
    let dotfile = base.join(".devenv");
    let paths = DevenvPaths {
        root: base.to_path_buf(),
        dotfile: dotfile.clone(),
        dot_gc: dotfile.join("gc"),
        home_gc: dotfile.join("home-gc"),
        tmp: base.join("tmp"),
        runtime: base.join("runtime"),
        state: None,
        git_root: None,
    };
    std::fs::create_dir_all(&paths.dotfile).expect("create .devenv");
    std::fs::create_dir_all(&paths.dot_gc).expect("create .devenv/gc");
    std::fs::create_dir_all(&paths.home_gc).expect("create .devenv/home-gc");
    paths
}

/// Copy the bundled lock fixture into `dest_dir`. Tests that exercise
/// eval/build paths use this to skip an `update()` round-trip.
pub fn copy_fixture_lock(dest: &Path) {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/devenv.lock");
    std::fs::copy(&fixture, dest.join("devenv.lock")).expect("copy fixture lock");
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=test@devenv",
            "-c",
            "user.name=devenv-test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// Initialize a tiny `git+file://` flake at `dir` whose only output is a
/// marker string. Returns the URL suitable for use in a `devenv.yaml` input.
///
/// The flake declares no derivations, so `update()` against it is a pure
/// local git operation — no network, no eval cost.
pub fn write_local_flake(dir: &Path, marker: &str) -> String {
    std::fs::create_dir_all(dir).expect("create flake dir");
    std::fs::write(
        dir.join("flake.nix"),
        format!("{{ outputs = _: {{ marker = \"{marker}\"; }}; }}\n"),
    )
    .expect("write flake.nix");
    run_git(dir, &["init", "-q"]);
    run_git(dir, &["add", "flake.nix"]);
    run_git(dir, &["commit", "-q", "-m", marker]);
    format!("git+file://{}", dir.display())
}

/// Add a new commit to a flake created with [`write_local_flake`], so that
/// the next `update()` for that input observes a different rev.
pub fn bump_local_flake(dir: &Path, marker: &str) {
    std::fs::write(
        dir.join("flake.nix"),
        format!("{{ outputs = _: {{ marker = \"{marker}\"; }}; }}\n"),
    )
    .expect("rewrite flake.nix");
    run_git(dir, &["commit", "-q", "-am", marker]);
}

/// All the handles a test needs after construction.
///
/// Most tests want `env.backend` and `env.path()`; `config` and `paths`
/// are exposed for tests that need them. `_cwd_guard` and `_temp_dir`
/// are kept alive by ownership.
pub struct TestEnv {
    pub temp_dir: TempDir,
    pub backend: NixCBackend,
    pub config: Config,
    pub paths: DevenvPaths,
    _cwd_guard: CwdGuard,
}

impl TestEnv {
    /// Default env: standard yaml, minimal nix, fixture lock, default options.
    pub async fn new() -> Self {
        Self::builder().build().await
    }

    pub fn builder() -> TestEnvBuilder {
        TestEnvBuilder::default()
    }

    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }
}

#[derive(Clone)]
pub struct TestEnvBuilder {
    yaml: String,
    nix: String,
    extra_files: Vec<(String, String)>,
    nix_options: NixOptions,
    fixture_lock: bool,
}

impl Default for TestEnvBuilder {
    fn default() -> Self {
        Self {
            yaml: DEFAULT_YAML.into(),
            nix: DEFAULT_NIX.into(),
            extra_files: Vec::new(),
            nix_options: NixOptions::default(),
            fixture_lock: true,
        }
    }
}

impl TestEnvBuilder {
    pub fn yaml(mut self, yaml: impl Into<String>) -> Self {
        self.yaml = yaml.into();
        self
    }

    pub fn nix(mut self, nix: impl Into<String>) -> Self {
        self.nix = nix.into();
        self
    }

    /// Add a file under the project root, relative path. Useful for
    /// `imports = [ ./extra.nix ]` style tests.
    pub fn extra_file(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.extra_files.push((name.into(), content.into()));
        self
    }

    pub fn nix_options(mut self, options: NixOptions) -> Self {
        self.nix_options = options;
        self
    }

    /// Skip the fixture lock copy. Use in tests that exercise the
    /// `update()` path or that intentionally start without a lock.
    pub fn no_lock(mut self) -> Self {
        self.fixture_lock = false;
        self
    }

    /// Materialize files only — returns the bits needed to inspect a
    /// bootstrap-time failure (no backend constructed). Use with
    /// [`init_backend`] at the call site to keep the failure observable.
    pub fn build_files(self) -> (TempDir, CwdGuard, DevenvPaths, Config, NixOptions) {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().to_path_buf();
        let cwd_guard = CwdGuard::enter(&path);

        std::fs::write(path.join("devenv.nix"), &self.nix).expect("write devenv.nix");
        std::fs::write(path.join("devenv.yaml"), &self.yaml).expect("write devenv.yaml");
        for (name, content) in &self.extra_files {
            let dest = path.join(name);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).expect("create extra-file parent");
            }
            std::fs::write(&dest, content).expect("write extra file");
        }

        let paths = paths_under(&path);
        let config = Config::load_from(&path).expect("load config");

        if self.fixture_lock {
            copy_fixture_lock(&path);
        }

        (temp_dir, cwd_guard, paths, config, self.nix_options)
    }

    /// Build everything, requiring backend construction to succeed.
    pub async fn build(self) -> TestEnv {
        self.try_build().await.expect("init backend")
    }

    /// Build everything, returning `Err` if backend construction fails.
    /// Use when the test's purpose is to inspect that error.
    pub async fn try_build(self) -> miette::Result<TestEnv> {
        let (temp_dir, cwd_guard, paths, config, nix_cli) = self.build_files();
        let backend = init_backend(paths.clone(), config.clone(), nix_cli)?;
        Ok(TestEnv {
            temp_dir,
            backend,
            config,
            paths,
            _cwd_guard: cwd_guard,
        })
    }
}

fn test_bootstrap_args(paths: &DevenvPaths, config: &Config) -> BootstrapArgs {
    let system = default_system();
    let nixpkgs_config = config.nixpkgs_config(&system);
    let dotfile_relative = PathBuf::from(format!(
        "./{}",
        paths
            .dotfile
            .file_name()
            .expect("dotfile has filename")
            .to_string_lossy()
    ));

    let args = NixArgs {
        version: "1.0.0",
        is_development_version: false,
        require_version_match: false,
        system: &system,
        devenv_root: &paths.root,
        skip_local_src: false,
        devenv_dotfile: &paths.dotfile,
        devenv_dotfile_path: &dotfile_relative,
        devenv_tmpdir: &paths.tmp,
        devenv_runtime: &paths.runtime,
        devenv_istesting: true,
        devenv_direnvrc_latest_version: 5,
        active_profiles: &[],
        cli_options: CliOptionsConfig::default(),
        hostname: None,
        username: None,
        git_root: None,
        secretspec: None,
        devenv_inputs: &config.inputs,
        devenv_imports: &config.imports,
        impure: false,
        nixpkgs_config,
        lock_fingerprint: "",
        devenv_state: None,
    };

    BootstrapArgs::from_serializable(&args).expect("serialize bootstrap args")
}

/// Construct a `NixCBackend` for tests by running the standard
/// setup-then-build flow and skipping lock validation (tests bring
/// their own minimal devenv.nix without flake inputs).
pub fn init_backend(
    paths: DevenvPaths,
    config: Config,
    nix_cli: NixOptions,
) -> miette::Result<NixCBackend> {
    let nix_settings = NixSettings::resolve(nix_cli, &config);
    let cache_settings = CacheSettings::resolve(CacheOptions::default());
    let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);
    let store_settings = StoreSettings::default();

    let gc_registration = devenv_nix_backend::backend::init_nix(&nix_settings, &store_settings)?;
    let store = devenv_nix_backend::backend::open_store(&store_settings)?;
    let (flake_settings, fetchers_settings) = devenv_nix_backend::backend::build_settings()?;
    let logger_setup = devenv_nix_backend::logger::setup_nix_logger()?;

    let bootstrap_args = test_bootstrap_args(&paths, &config);
    NixCBackend::new(
        paths,
        nix_settings,
        cache_settings,
        &nixpkgs_config,
        store,
        flake_settings,
        fetchers_settings,
        gc_registration,
        Arc::new(bootstrap_args),
        Arc::new(PortAllocator::new()),
        None,
        logger_setup,
    )
}
