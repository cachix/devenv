//! Per-concern option structs and resolved settings.
//!
//! Each concern follows the same pattern:
//! - Options struct: all `Option<T>` fields, no clap dependency
//! - Resolved settings struct: plain Rust with concrete types
//! - `resolve()`: takes options by value, merges with Config

use tracing::error;

use crate::config::{Clean, Config, NixBackendType, SecretspecConfig};

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

/// Resolve a boolean flag pair (`--flag` / `--no-flag`).
///
/// Returns `Some(true)` if `--flag` was set, `Some(false)` if `--no-flag` was set,
/// `None` if neither was set (defer to config, then to default).
///
/// The logical default lives in `.unwrap_or(default)` at the call site,
/// not in clap's `default_value_t`.
pub fn flag(yes: bool, no: bool) -> Option<bool> {
    match (yes, no) {
        (_, true) => Some(false),
        (true, _) => Some(true),
        (false, false) => None,
    }
}

/// Combine two values, preferring `self` (higher precedence).
pub(crate) trait Combine: Sized {
    fn combine(self, other: Self) -> Self;
}

impl<T> Combine for Option<T> {
    fn combine(self, other: Self) -> Self {
        self.or(other)
    }
}

impl<T> Combine for Vec<T> {
    /// Prefer self if non-empty; fall back to other.
    fn combine(self, other: Self) -> Self {
        if !self.is_empty() { self } else { other }
    }
}

// --- Nix ---

#[derive(Clone, Debug, Default)]
pub struct NixOptions {
    pub max_jobs: Option<u8>,
    pub cores: Option<u8>,
    pub system: Option<String>,
    pub impure: Option<bool>,
    pub offline: Option<bool>,
    pub nix_option: Vec<String>,
    pub nix_debugger: Option<bool>,
    pub backend: Option<NixBackendType>,
}

/// Resolved Nix build settings.
///
/// Produced by `NixSettings::resolve(NixOptions, &Config)` as a pure function.
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
    pub backend: NixBackendType,
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
            backend: NixBackendType::default(),
        }
    }
}

impl NixSettings {
    /// Resolve Nix build settings from options and config sources.
    ///
    /// `impure` uses option-wins semantics: `Some(true)`/`Some(false)` override config,
    /// falling back to `config.impure` when `None`.
    pub fn resolve(options: NixOptions, config: &Config) -> Self {
        let defaults = NixBuildDefaults::defaults();
        Self {
            impure: options.impure.unwrap_or(config.impure),
            system: options.system.unwrap_or_else(default_system),
            max_jobs: options.max_jobs.unwrap_or(defaults.max_jobs),
            cores: options.cores.unwrap_or(defaults.cores),
            offline: options.offline.unwrap_or(false),
            nix_option: options.nix_option,
            nix_debugger: options.nix_debugger.unwrap_or(false),
            backend: options.backend.unwrap_or_else(|| config.backend.clone()),
        }
    }
}

// --- Cache ---

#[derive(Clone, Debug, Default)]
pub struct CacheOptions {
    pub eval_cache: Option<bool>,
    pub refresh_eval_cache: Option<bool>,
    pub refresh_task_cache: Option<bool>,
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
    /// Resolve cache settings from options.
    ///
    /// All cache settings are CLI-only today (no config file counterpart).
    pub fn resolve(options: CacheOptions) -> Self {
        Self {
            eval_cache: options.eval_cache.unwrap_or(true),
            refresh_eval_cache: options.refresh_eval_cache.unwrap_or(false),
            refresh_task_cache: options.refresh_task_cache.unwrap_or(false),
        }
    }
}

// --- Shell ---

#[derive(Clone, Debug, Default)]
pub struct ShellOptions {
    pub clean: Option<Vec<String>>,
    pub profiles: Vec<String>,
    pub reload: Option<bool>,
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
    /// Resolve shell settings from options and config sources.
    ///
    /// Precedence: Options > Config > Default.
    pub fn resolve(options: ShellOptions, config: &Config) -> Self {
        let clean = if let Some(keep) = options.clean {
            Clean {
                enabled: true,
                keep,
            }
        } else {
            config.clean.clone().unwrap_or_default()
        };

        let config_profiles: Vec<String> = config.profile.iter().cloned().collect();
        let profiles = options.profiles.combine(config_profiles);

        let reload = options.reload.combine(config.reload).unwrap_or(true);

        Self {
            clean,
            profiles,
            reload,
        }
    }
}

// --- Secrets ---

#[derive(Clone, Debug, Default)]
pub struct SecretOptions {
    pub secretspec_provider: Option<String>,
    pub secretspec_profile: Option<String>,
}

/// Resolved secret management settings.
#[derive(Clone, Debug, Default)]
pub struct SecretSettings {
    pub secretspec: Option<SecretspecConfig>,
}

impl SecretSettings {
    /// Resolve secret settings from options and config sources.
    ///
    /// Precedence: Options > Config > Default.
    /// If option fields are present, they override the matching config fields.
    /// When no config exists, `enable` defaults to `true` so the user can
    /// pass `--secretspec-provider` without also adding secretspec config.
    /// When config explicitly sets `enable: false`, that value is preserved.
    pub fn resolve(options: SecretOptions, config: &Config) -> Self {
        let has_override =
            options.secretspec_provider.is_some() || options.secretspec_profile.is_some();

        let secretspec = if has_override {
            let base = config.secretspec.clone().unwrap_or_default();
            let enable = config.secretspec.as_ref().is_none_or(|c| c.enable);
            Some(SecretspecConfig {
                enable,
                provider: options.secretspec_provider.or(base.provider),
                profile: options.secretspec_profile.or(base.profile),
            })
        } else {
            config.secretspec.clone()
        };

        Self { secretspec }
    }
}

// --- Input overrides ---

#[derive(Clone, Debug, Default)]
pub struct InputOverrides {
    pub override_input: Vec<String>,
    pub nix_module_options: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn nix_settings_defaults() {
        let options = NixOptions::default();
        let config = Config::default();
        let settings = NixSettings::resolve(options, &config);
        assert!(!settings.impure);
        assert!(!settings.offline);
        assert!(!settings.nix_debugger);
        assert!(settings.nix_option.is_empty());
    }

    #[test]
    fn nix_settings_impure_from_options() {
        let options = NixOptions {
            impure: Some(true),
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(options, &config);
        assert!(settings.impure);
    }

    #[test]
    fn nix_settings_impure_from_config() {
        let options = NixOptions::default();
        let config = Config {
            impure: true,
            ..Default::default()
        };
        let settings = NixSettings::resolve(options, &config);
        assert!(settings.impure);
    }

    #[test]
    fn nix_settings_no_impure_overrides_config() {
        let options = NixOptions {
            impure: Some(false),
            ..Default::default()
        };
        let config = Config {
            impure: true,
            ..Default::default()
        };
        let settings = NixSettings::resolve(options, &config);
        assert!(!settings.impure);
    }

    #[test]
    fn nix_settings_system_from_options() {
        let options = NixOptions {
            system: Some("x86_64-linux".into()),
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(options, &config);
        assert_eq!(settings.system, "x86_64-linux");
    }

    #[test]
    fn nix_settings_option_fields() {
        let options = NixOptions {
            max_jobs: Some(4),
            cores: Some(2),
            offline: Some(true),
            nix_option: vec!["sandbox".into(), "false".into()],
            nix_debugger: Some(true),
            ..Default::default()
        };
        let config = Config::default();
        let settings = NixSettings::resolve(options, &config);
        assert_eq!(settings.max_jobs, 4);
        assert_eq!(settings.cores, 2);
        assert!(settings.offline);
        assert_eq!(settings.nix_option, vec!["sandbox", "false"]);
        assert!(settings.nix_debugger);
    }

    #[test]
    fn cache_settings_defaults() {
        let options = CacheOptions::default();
        let settings = CacheSettings::resolve(options);
        assert!(settings.eval_cache);
        assert!(!settings.refresh_eval_cache);
        assert!(!settings.refresh_task_cache);
    }

    #[test]
    fn cache_settings_eval_cache_disabled() {
        let options = CacheOptions {
            eval_cache: Some(false),
            ..Default::default()
        };
        let settings = CacheSettings::resolve(options);
        assert!(!settings.eval_cache);
    }

    #[test]
    fn cache_settings_refresh_flags() {
        let options = CacheOptions {
            refresh_eval_cache: Some(true),
            refresh_task_cache: Some(true),
            ..Default::default()
        };
        let settings = CacheSettings::resolve(options);
        assert!(settings.refresh_eval_cache);
        assert!(settings.refresh_task_cache);
    }

    #[test]
    fn shell_settings_options_clean_overrides_config() {
        let options = ShellOptions {
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
        let settings = ShellSettings::resolve(options, &config);
        assert!(settings.clean.enabled);
        assert_eq!(settings.clean.keep, vec!["PATH"]);
    }

    #[test]
    fn shell_settings_config_clean_used_when_options_absent() {
        let options = ShellOptions::default();
        let config = Config {
            clean: Some(Clean {
                enabled: true,
                keep: vec!["HOME".into()],
            }),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(options, &config);
        assert!(settings.clean.enabled);
        assert_eq!(settings.clean.keep, vec!["HOME"]);
    }

    #[test]
    fn shell_settings_clean_defaults_to_disabled() {
        let options = ShellOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(options, &config);
        assert!(!settings.clean.enabled);
    }

    #[test]
    fn shell_settings_options_profiles_override_config() {
        let options = ShellOptions {
            profiles: vec!["dev".into()],
            ..Default::default()
        };
        let config = Config {
            profile: Some("prod".into()),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(options, &config);
        assert_eq!(settings.profiles, vec!["dev"]);
    }

    #[test]
    fn shell_settings_config_profile_used_when_options_absent() {
        let options = ShellOptions::default();
        let config = Config {
            profile: Some("prod".into()),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(options, &config);
        assert_eq!(settings.profiles, vec!["prod"]);
    }

    #[test]
    fn shell_settings_no_profiles_by_default() {
        let options = ShellOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(options, &config);
        assert!(settings.profiles.is_empty());
    }

    #[test]
    fn shell_settings_reload_defaults_to_true() {
        let options = ShellOptions::default();
        let config = Config::default();
        let settings = ShellSettings::resolve(options, &config);
        assert!(settings.reload);
    }

    #[test]
    fn shell_settings_no_reload_overrides_config() {
        let options = ShellOptions {
            reload: Some(false),
            ..Default::default()
        };
        let config = Config {
            reload: Some(true),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(options, &config);
        assert!(!settings.reload);
    }

    #[test]
    fn shell_settings_config_reload_false_respected() {
        let options = ShellOptions::default();
        let config = Config {
            reload: Some(false),
            ..Default::default()
        };
        let settings = ShellSettings::resolve(options, &config);
        assert!(!settings.reload);
    }

    #[test]
    fn secret_settings_options_provider_overrides_config() {
        let options = SecretOptions {
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
        let settings = SecretSettings::resolve(options, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
    }

    #[test]
    fn secret_settings_options_profile_overrides_config() {
        let options = SecretOptions {
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
        let settings = SecretSettings::resolve(options, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.profile, Some("staging".into()));
        assert_eq!(sc.provider, Some("gcp".into()));
    }

    #[test]
    fn secret_settings_config_used_when_options_absent() {
        let options = SecretOptions::default();
        let config = Config {
            secretspec: Some(SecretspecConfig {
                enable: true,
                provider: Some("gcp".into()),
                profile: Some("prod".into()),
            }),
            ..Default::default()
        };
        let settings = SecretSettings::resolve(options, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("gcp".into()));
        assert_eq!(sc.profile, Some("prod".into()));
    }

    #[test]
    fn secret_settings_options_enables_when_config_absent() {
        let options = SecretOptions {
            secretspec_provider: Some("aws".into()),
            ..Default::default()
        };
        let config = Config::default();
        let settings = SecretSettings::resolve(options, &config);
        let sc = settings.secretspec.unwrap();
        assert!(sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
        assert_eq!(sc.profile, None);
    }

    #[test]
    fn secret_settings_none_when_both_absent() {
        let options = SecretOptions::default();
        let config = Config::default();
        let settings = SecretSettings::resolve(options, &config);
        assert!(settings.secretspec.is_none());
    }

    #[test]
    fn secret_settings_options_preserves_config_enable_false() {
        let options = SecretOptions {
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
        let settings = SecretSettings::resolve(options, &config);
        let sc = settings.secretspec.unwrap();
        assert!(!sc.enable);
        assert_eq!(sc.provider, Some("aws".into()));
        assert_eq!(sc.profile, Some("prod".into()));
    }
}
