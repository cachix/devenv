//! Common test utilities shared across test files.

#![allow(dead_code)]

use devenv_core::cachix::{CachixManager, CachixPaths};
use devenv_core::{
    BootstrapArgs, CacheOptions, CacheSettings, CliOptionsConfig, Config, DevenvPaths, NixArgs,
    NixOptions, NixSettings, PortAllocator, StoreSettings,
};
use devenv_nix_backend::NixCBackend;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn paths_under(base: &Path) -> DevenvPaths {
    let dotfile = base.join(".devenv");
    DevenvPaths {
        root: base.to_path_buf(),
        dotfile: dotfile.clone(),
        dot_gc: dotfile.join("gc"),
        home_gc: dotfile.join("home-gc"),
        tmp: base.join("tmp"),
        runtime: base.join("runtime"),
        state: None,
        git_root: None,
    }
}

pub fn get_current_system() -> &'static str {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (arch, os) {
        ("x86_64", "linux") => "x86_64-linux",
        ("aarch64", "linux") => "aarch64-linux",
        ("x86_64", "macos") => "x86_64-darwin",
        ("aarch64", "macos") => "aarch64-darwin",
        _ => panic!("Unsupported system: {arch}-{os}"),
    }
}

pub fn create_test_cachix_manager(
    base_dir: &Path,
    daemon_socket: Option<PathBuf>,
) -> Arc<CachixManager> {
    let cachix_paths = CachixPaths {
        trusted_keys: base_dir.join(".devenv/cachix/trusted-keys.json"),
        netrc: base_dir.join(".devenv/netrc"),
        daemon_socket,
    };
    Arc::new(CachixManager::new(cachix_paths))
}

pub struct TestNixArgs {
    tmpdir: PathBuf,
    runtime: PathBuf,
    dotfile_path: PathBuf,
}

impl TestNixArgs {
    pub fn new(paths: &DevenvPaths) -> Self {
        let dotfile_name = paths
            .dotfile
            .file_name()
            .expect("dotfile should have a file name")
            .to_string_lossy();
        TestNixArgs {
            tmpdir: paths.root.join("tmp"),
            runtime: paths.root.join("runtime"),
            dotfile_path: PathBuf::from(format!("./{}", dotfile_name)),
        }
    }

    pub fn to_nix_args<'a>(
        &'a self,
        paths: &'a DevenvPaths,
        config: &'a Config,
        nixpkgs_config: devenv_core::config::NixpkgsConfig,
    ) -> NixArgs<'a> {
        NixArgs {
            version: "1.0.0",
            is_development_version: false,
            require_version_match: false,
            system: get_current_system(),
            devenv_root: &paths.root,
            skip_local_src: false,
            devenv_dotfile: &paths.dotfile,
            devenv_dotfile_path: &self.dotfile_path,
            devenv_tmpdir: &self.tmpdir,
            devenv_runtime: &self.runtime,
            devenv_istesting: true,
            devenv_direnvrc_latest_version: 5,
            container_name: None,
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
        }
    }
}

pub fn test_bootstrap_args(paths: &DevenvPaths, config: &Config) -> BootstrapArgs {
    let test_args = TestNixArgs::new(paths);
    let nix_args =
        test_args.to_nix_args(paths, config, config.nixpkgs_config(get_current_system()));
    BootstrapArgs::from_serializable(&nix_args).expect("Failed to serialize bootstrap args")
}

/// Construct a `NixCBackend` for tests by running the standard
/// setup-then-build flow and skipping lock validation (tests bring
/// their own minimal devenv.nix without flake inputs).
pub fn create_backend(
    paths: DevenvPaths,
    config: Config,
    nix_cli: NixOptions,
    _cachix_manager: Arc<CachixManager>,
) -> miette::Result<NixCBackend> {
    let nix_settings = NixSettings::resolve(nix_cli, &config);
    let cache_settings = CacheSettings::resolve(CacheOptions::default());
    let nixpkgs_config = config.nixpkgs_config(&nix_settings.system);
    let store_settings = StoreSettings::default();

    let gc_registration = devenv_nix_backend::backend::init_nix(&nix_settings, &store_settings)?;
    let store = devenv_nix_backend::backend::open_store(&store_settings)?;
    let (flake_settings, fetchers_settings) = devenv_nix_backend::backend::build_settings()?;

    // Tests bring their own minimal devenv.nix without flake inputs, so
    // skip lock validation entirely.
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
    )
}

/// Same as [`create_backend`]; tests historically called this `init_backend`.
pub fn init_backend(
    paths: DevenvPaths,
    config: Config,
    nix_cli: NixOptions,
    cachix_manager: Arc<CachixManager>,
) -> miette::Result<NixCBackend> {
    create_backend(paths, config, nix_cli, cachix_manager)
}
