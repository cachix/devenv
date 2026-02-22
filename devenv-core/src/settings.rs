//! Per-concern CLI input structs and resolved settings.
//!
//! Each concern follows the same pattern:
//! - CLI input struct: `#[cfg_attr(feature = "clap", derive(clap::Args))]`
//! - Resolved settings struct: plain Rust, no clap dependency
//! - `resolve()`: takes CLI input by value, merges with Config

use tracing::error;

use crate::config::{Clean, Config, SecretspecConfig};

// --- Utilities (moved from cli.rs) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NixBuildDefaults {
    pub max_jobs: u8,
    pub cores: u8,
}

static NIX_BUILD_DEFAULTS: std::sync::LazyLock<NixBuildDefaults> =
    std::sync::LazyLock::new(NixBuildDefaults::compute);

impl NixBuildDefaults {
    pub fn defaults() -> &'static Self {
        &NIX_BUILD_DEFAULTS
    }

    fn compute() -> Self {
        let total_cores = std::thread::available_parallelism()
            .unwrap_or_else(|e| {
                error!("Failed to get number of logical CPUs: {}", e);
                4.try_into().unwrap()
            })
            .get();

        let max_jobs = (total_cores / 4).max(1);
        let cores = (total_cores / max_jobs).max(1);

        Self {
            max_jobs: max_jobs as u8,
            cores: cores as u8,
        }
    }
}

pub fn default_system() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unknown architecture"
    };

    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "unknown OS"
    };
    format!("{arch}-{os}")
}

// --- Nix ---

#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Clone, Debug)]
pub struct NixCliOptions {
    #[cfg_attr(feature = "clap", arg(short = 'j', long,
        global = true,
        env = "DEVENV_MAX_JOBS",
        help = "Maximum number of Nix builds to run concurrently.",
        default_value_t = NixBuildDefaults::defaults().max_jobs))]
    pub max_jobs: u8,

    #[cfg_attr(feature = "clap", arg(short = 'u', long,
        global = true,
        env = "DEVENV_CORES",
        help = "Number of CPU cores available to each build.",
        default_value_t = NixBuildDefaults::defaults().cores))]
    pub cores: u8,

    #[cfg_attr(feature = "clap", arg(short, long, global = true, default_value_t = default_system()))]
    pub system: String,

    #[cfg_attr(
        feature = "clap",
        arg(
            short,
            long,
            global = true,
            help = "Relax the hermeticity of the environment."
        )
    )]
    pub impure: bool,

    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            help = "Disable substituters and consider all previously downloaded files up-to-date."
        )
    )]
    pub offline: bool,

    #[cfg_attr(feature = "clap", arg(short = 'n', long, global = true, num_args = 2,
        value_names = ["NAME", "VALUE"],
        value_delimiter = ' ',
        help = "Pass additional options to nix commands",
        long_help = "Pass additional options to nix commands.\n\nThese options are passed directly to Nix using the --option flag.\nSee `man nix.conf` for the full list of available options.\n\nExamples:\n  --nix-option sandbox false\n  --nix-option keep-outputs true\n  --nix-option system x86_64-darwin"))]
    pub nix_option: Vec<String>,

    #[cfg_attr(
        feature = "clap",
        arg(long, global = true, help = "Enter the Nix debugger on failure.")
    )]
    pub nix_debugger: bool,
}

impl Default for NixCliOptions {
    fn default() -> Self {
        let defaults = NixBuildDefaults::defaults();
        Self {
            max_jobs: defaults.max_jobs,
            cores: defaults.cores,
            system: default_system(),
            impure: false,
            offline: false,
            nix_option: Vec::new(),
            nix_debugger: false,
        }
    }
}

/// Resolved Nix build settings.
///
/// Produced by `NixSettings::resolve(NixCliOptions, &Config)` as a pure function.
/// Controls how the Nix evaluator and builder behave.
#[derive(Clone, Debug)]
pub struct NixSettings {
    pub impure: bool,
    pub system: String,
    pub max_jobs: u8,
    pub cores: u8,
    pub offline: bool,
    pub nix_option: Vec<String>,
    pub nix_debugger: bool,
}

impl Default for NixSettings {
    fn default() -> Self {
        let defaults = NixBuildDefaults::defaults();
        Self {
            impure: false,
            system: default_system(),
            max_jobs: defaults.max_jobs,
            cores: defaults.cores,
            offline: false,
            nix_option: Vec::new(),
            nix_debugger: false,
        }
    }
}

impl NixSettings {
    /// Resolve Nix build settings from CLI and config sources.
    ///
    /// `impure` uses OR semantics: either CLI or Config can enable it.
    /// All other fields are CLI-only today.
    pub fn resolve(cli: NixCliOptions, config: &Config) -> Self {
        Self {
            impure: cli.impure || config.impure,
            system: cli.system,
            max_jobs: cli.max_jobs,
            cores: cli.cores,
            offline: cli.offline,
            nix_option: cli.nix_option,
            nix_debugger: cli.nix_debugger,
        }
    }
}

// --- Cache ---

#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Clone, Debug)]
pub struct CacheCliOptions {
    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            help = "Cache the results of Nix evaluation.",
            hide = true,
            long_help = "Cache the results of Nix evaluation (deprecated, on by default). Use --no-eval-cache to disable caching.",
            default_value_t = true,
            overrides_with = "no_eval_cache"
        )
    )]
    pub eval_cache: bool,

    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            help = "Force a refresh of the Nix evaluation cache."
        )
    )]
    pub refresh_eval_cache: bool,

    #[cfg_attr(
        feature = "clap",
        arg(long, global = true, help = "Force a refresh of the task cache.")
    )]
    pub refresh_task_cache: bool,
}

impl Default for CacheCliOptions {
    fn default() -> Self {
        Self {
            eval_cache: true,
            refresh_eval_cache: false,
            refresh_task_cache: false,
        }
    }
}

/// Resolved cache settings.
#[derive(Clone, Debug)]
pub struct CacheSettings {
    pub eval_cache: bool,
    pub refresh_eval_cache: bool,
    pub refresh_task_cache: bool,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            eval_cache: true,
            refresh_eval_cache: false,
            refresh_task_cache: false,
        }
    }
}

impl CacheSettings {
    /// Resolve cache settings from CLI source.
    ///
    /// All cache settings are CLI-only today (no config file counterpart).
    pub fn resolve(cli: CacheCliOptions) -> Self {
        Self {
            eval_cache: cli.eval_cache,
            refresh_eval_cache: cli.refresh_eval_cache,
            refresh_task_cache: cli.refresh_task_cache,
        }
    }
}

// --- Shell ---

#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Clone, Debug)]
pub struct ShellCliOptions {
    #[cfg_attr(feature = "clap", arg(short, long, global = true,
        num_args = 0..,
        value_delimiter = ',',
        help = "Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through."))]
    pub clean: Option<Vec<String>>,

    #[cfg_attr(feature = "clap", arg(short = 'P', long, global = true,
        num_args = 1,
        action = clap::ArgAction::Append,
        help = "Activate one or more profiles defined in devenv.nix",
        long_help = "Activate one or more profiles defined in devenv.nix.\n\nProfiles allow you to define different configurations that can be merged with your base configuration.\n\nSee https://devenv.sh/profiles for more information.\n\nExamples:\n  --profile python-3.14\n  --profile backend --profile fast-startup"))]
    pub profile: Vec<String>,

    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            help = "Enable auto-reload when config files change (default).",
            default_value_t = true,
            overrides_with = "no_reload"
        )
    )]
    pub reload: bool,
}

impl Default for ShellCliOptions {
    fn default() -> Self {
        Self {
            clean: None,
            profile: Vec::new(),
            reload: true,
        }
    }
}

/// Resolved shell settings.
#[derive(Clone, Debug)]
pub struct ShellSettings {
    pub clean: Clean,
    pub profiles: Vec<String>,
    pub reload: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            clean: Clean::default(),
            profiles: Vec::new(),
            reload: true,
        }
    }
}

impl ShellSettings {
    /// Resolve shell settings from CLI and config sources.
    ///
    /// Precedence: CLI > Config > Default.
    pub fn resolve(cli: ShellCliOptions, config: &Config) -> Self {
        let clean = if let Some(keep) = cli.clean {
            Clean {
                enabled: true,
                keep,
            }
        } else {
            config.clean.clone().unwrap_or_default()
        };

        let profiles = if !cli.profile.is_empty() {
            cli.profile
        } else if let Some(ref profile) = config.profile {
            vec![profile.clone()]
        } else {
            Vec::new()
        };

        let reload = if !cli.reload {
            false
        } else {
            config.reload.unwrap_or(true)
        };

        Self {
            clean,
            profiles,
            reload,
        }
    }
}

// --- Secrets ---

#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Clone, Debug, Default)]
pub struct SecretCliOptions {
    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            env = "SECRETSPEC_PROVIDER",
            help = "Override the secretspec provider"
        )
    )]
    pub secretspec_provider: Option<String>,

    #[cfg_attr(
        feature = "clap",
        arg(
            long,
            global = true,
            env = "SECRETSPEC_PROFILE",
            help = "Override the secretspec profile"
        )
    )]
    pub secretspec_profile: Option<String>,
}

/// Resolved secret management settings.
#[derive(Clone, Debug, Default)]
pub struct SecretSettings {
    pub secretspec: Option<SecretspecConfig>,
}

impl SecretSettings {
    /// Resolve secret settings from CLI and config sources.
    ///
    /// Precedence: CLI > Config > Default.
    /// If either CLI field is present, merge into config's SecretspecConfig
    /// (or create one) with `enable: true`.
    pub fn resolve(cli: SecretCliOptions, config: &Config) -> Self {
        let has_cli_override =
            cli.secretspec_provider.is_some() || cli.secretspec_profile.is_some();

        let secretspec = if has_cli_override {
            let base = config.secretspec.clone().unwrap_or(SecretspecConfig {
                enable: false,
                profile: None,
                provider: None,
            });
            Some(SecretspecConfig {
                enable: true,
                provider: cli.secretspec_provider.or(base.provider),
                profile: cli.secretspec_profile.or(base.profile),
            })
        } else {
            config.secretspec.clone()
        };

        Self { secretspec }
    }
}

// --- Input overrides ---

#[cfg_attr(feature = "clap", derive(clap::Args))]
#[derive(Clone, Debug, Default)]
pub struct InputOverrides {
    #[cfg_attr(feature = "clap", arg(short, long, global = true,
        num_args = 2,
        value_names = ["NAME", "URI"],
        value_delimiter = ' ',
        help = "Override inputs in devenv.yaml",
        long_help = "Override inputs in devenv.yaml.\n\nExamples:\n  --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable\n  --override-input nixpkgs path:/path/to/local/nixpkgs"))]
    pub override_input: Vec<String>,

    #[cfg_attr(feature = "clap", arg(long = "option", short = 'O', global = true,
        num_args = 2,
        value_names = ["OPTION", "VALUE"],
        help = "Override configuration options with typed values",
        long_help = "Override configuration options with typed values.\n\nOPTION must include a type: <attribute>:<type>\nSupported types: string, int, float, bool, path, pkg, pkgs\n\nExamples:\n  --option languages.rust.channel:string beta\n  --option services.postgres.enable:bool true\n  --option languages.python.version:string 3.10\n  --option packages:pkgs \"ncdu git\""))]
    pub nix_module_options: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn nix_settings_defaults() {
        let cli = NixCliOptions::default();
        let config = Config::default();
        let settings = NixSettings::resolve(cli, &config);
        assert!(!settings.impure);
        assert!(!settings.offline);
        assert!(!settings.nix_debugger);
        assert!(settings.nix_option.is_empty());
    }

    #[test]
    fn nix_settings_impure_from_cli() {
        let cli = NixCliOptions {
            impure: true,
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(cli, &config);
        assert!(settings.impure);
    }

    #[test]
    fn nix_settings_impure_from_config() {
        let cli = NixCliOptions::default();
        let config = Config {
            impure: true,
            ..Default::default()
        };
        let settings = NixSettings::resolve(cli, &config);
        assert!(settings.impure);
    }

    #[test]
    fn nix_settings_impure_is_or() {
        let cli = NixCliOptions {
            impure: false,
            ..Default::default()
        };
        let config = Config {
            impure: true,
            ..Default::default()
        };
        let settings = NixSettings::resolve(cli, &config);
        assert!(settings.impure);
    }

    #[test]
    fn nix_settings_system_from_cli() {
        let cli = NixCliOptions {
            system: "x86_64-linux".into(),
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(cli, &config);
        assert_eq!(settings.system, "x86_64-linux");
    }

    #[test]
    fn nix_settings_cli_fields() {
        let cli = NixCliOptions {
            max_jobs: 4,
            cores: 2,
            offline: true,
            nix_option: vec!["sandbox".into(), "false".into()],
            nix_debugger: true,
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(cli, &config);
        assert_eq!(settings.max_jobs, 4);
        assert_eq!(settings.cores, 2);
        assert!(settings.offline);
        assert_eq!(settings.nix_option, vec!["sandbox", "false"]);
        assert!(settings.nix_debugger);
    }

    #[test]
    fn cache_settings_defaults() {
        let cli = CacheCliOptions::default();
        let settings = CacheSettings::resolve(cli);
        assert!(settings.eval_cache);
        assert!(!settings.refresh_eval_cache);
        assert!(!settings.refresh_task_cache);
    }

    #[test]
    fn cache_settings_eval_cache_disabled() {
        let cli = CacheCliOptions {
            eval_cache: false,
            ..Default::default()
        };
        let settings = CacheSettings::resolve(cli);
        assert!(!settings.eval_cache);
    }

    #[test]
    fn cache_settings_refresh_flags() {
        let cli = CacheCliOptions {
            refresh_eval_cache: true,
            refresh_task_cache: true,
            ..Default::default()
        };
        let settings = CacheSettings::resolve(cli);
        assert!(settings.refresh_eval_cache);
        assert!(settings.refresh_task_cache);
    }

    #[test]
    fn shell_settings_cli_clean_overrides_config() {
        let cli = ShellCliOptions {
            clean: Some(vec!["PATH".into()]),
            ..Default::default()
        };
        let config = Config {
            clean: Some(Clean {
                enabled: true,
                keep: vec!["HOME".into()],
            }),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert!(settings.clean.enabled);
        assert_eq!(settings.clean.keep, vec!["PATH"]);
    }

    #[test]
    fn shell_settings_config_clean_used_when_cli_absent() {
        let cli = ShellCliOptions::default();
        let config = Config {
            clean: Some(Clean {
                enabled: true,
                keep: vec!["HOME".into()],
            }),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert!(settings.clean.enabled);
        assert_eq!(settings.clean.keep, vec!["HOME"]);
    }

    #[test]
    fn shell_settings_clean_defaults_to_disabled() {
        let cli = ShellCliOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(cli, &config);
        assert!(!settings.clean.enabled);
    }

    #[test]
    fn shell_settings_cli_profiles_override_config() {
        let cli = ShellCliOptions {
            profile: vec!["dev".into()],
            ..Default::default()
        };
        let config = Config {
            profile: Some("prod".into()),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert_eq!(settings.profiles, vec!["dev"]);
    }

    #[test]
    fn shell_settings_config_profile_used_when_cli_absent() {
        let cli = ShellCliOptions::default();
        let config = Config {
            profile: Some("prod".into()),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert_eq!(settings.profiles, vec!["prod"]);
    }

    #[test]
    fn shell_settings_no_profiles_by_default() {
        let cli = ShellCliOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(cli, &config);
        assert!(settings.profiles.is_empty());
    }

    #[test]
    fn shell_settings_reload_defaults_to_true() {
        let cli = ShellCliOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(cli, &config);
        assert!(settings.reload);
    }

    #[test]
    fn shell_settings_cli_no_reload_overrides_config() {
        let cli = ShellCliOptions {
            reload: false,
            ..Default::default()
        };
        let config = Config {
            reload: Some(true),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert!(!settings.reload);
    }

    #[test]
    fn shell_settings_config_reload_false_respected() {
        let cli = ShellCliOptions::default();
        let config = Config {
            reload: Some(false),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(cli, &config);
        assert!(!settings.reload);
    }

    #[test]
    fn secret_settings_cli_provider_overrides_config() {
        let cli = SecretCliOptions {
            secretspec_provider: Some("aws".into()),
            ..Default::default()
        };
        let config = Config {
            secretspec: Some(SecretspecConfig {
                enable: true,
                provider: Some("gcp".into()),
                profile: None,
            }),
            ..Default::default()
        };
        let settings = SecretSettings::resolve(cli, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
    }

    #[test]
    fn secret_settings_cli_profile_overrides_config() {
        let cli = SecretCliOptions {
            secretspec_profile: Some("staging".into()),
            ..Default::default()
        };
        let config = Config {
            secretspec: Some(SecretspecConfig {
                enable: true,
                provider: Some("gcp".into()),
                profile: Some("prod".into()),
            }),
            ..Default::default()
        };
        let settings = SecretSettings::resolve(cli, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.profile, Some("staging".into()));
        assert_eq!(sc.provider, Some("gcp".into()));
    }

    #[test]
    fn secret_settings_config_used_when_cli_absent() {
        let cli = SecretCliOptions::default();
        let config = Config {
            secretspec: Some(SecretspecConfig {
                enable: true,
                provider: Some("gcp".into()),
                profile: Some("prod".into()),
            }),
            ..Default::default()
        };
        let settings = SecretSettings::resolve(cli, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("gcp".into()));
        assert_eq!(sc.profile, Some("prod".into()));
    }

    #[test]
    fn secret_settings_cli_enables_when_config_absent() {
        let cli = SecretCliOptions {
            secretspec_provider: Some("aws".into()),
            ..Default::default()
        };
        let config = Config::default();
        let settings = SecretSettings::resolve(cli, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
        assert_eq!(sc.profile, None);
    }

    #[test]
    fn secret_settings_none_when_both_absent() {
        let cli = SecretCliOptions::default();
        let config = Config::default();
        let settings = SecretSettings::resolve(cli, &config);
        assert!(settings.secretspec.is_none());
    }

    #[test]
    fn secret_settings_cli_preserves_config_fields_not_overridden() {
        let cli = SecretCliOptions {
            secretspec_provider: Some("aws".into()),
            ..Default::default()
        };
        let config = Config {
            secretspec: Some(SecretspecConfig {
                enable: false,
                provider: None,
                profile: Some("prod".into()),
            }),
            ..Default::default()
        };
        let settings = SecretSettings::resolve(cli, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
        assert_eq!(sc.profile, Some("prod".into()));
    }
}
