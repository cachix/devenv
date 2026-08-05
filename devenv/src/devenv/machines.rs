use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::cli::{DiskoMode, InstallPhase};

use cli_table::{Table, WithTitle};
use devenv_activity::{ActivityInstrument, activity};
use futures::stream::StreamExt;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use secrecy::{ExposeSecret, SecretSlice};
use serde::Deserialize;
use tokio::process;

use super::Devenv;

/// Default SSH options applied to every connection opened by `devenv
/// machines`. OpenSSH keeps the first value it obtains for most options, so
/// configured `target.sshOpts` must be placed before these defaults.
const DEFAULT_SSH_OPTS: &[&str] = &[
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "ConnectTimeout=10",
];

/// Non-overridable SSH policy for an install that will transmit local file
/// payloads. It is prepended even before configured options because OpenSSH's
/// first-value-wins semantics make these settings authoritative.
const SENSITIVE_INSTALL_SSH_OPTS: &[&str] = &[
    "-o",
    "StrictHostKeyChecking=yes",
    "-o",
    "ClearAllForwardings=yes",
    "-o",
    "ForwardAgent=no",
    "-o",
    "ForwardX11=no",
    "-o",
    "PermitLocalCommand=no",
    "-o",
    "RequestTTY=no",
];

type BootstrapValues = HashMap<String, SecretSlice<u8>>;
type TargetBootstrapManifests = HashMap<String, SecretSlice<u8>>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SecretspecExecution {
    #[default]
    Local,
    Target,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MachineSecretspec {
    #[serde(default)]
    execution: SecretspecExecution,
    provider: Option<String>,
    profile: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MachineMeta {
    system: String,
    target: MachineTarget,
    #[serde(rename = "hasNixos")]
    has_nixos: bool,
    #[serde(rename = "hasNixDarwin")]
    has_nix_darwin: bool,
    #[serde(rename = "hasHomeManager")]
    has_home_manager: bool,
    #[serde(rename = "kexecImage")]
    kexec_image: Option<String>,
    #[serde(rename = "kexecPostSshPort")]
    kexec_post_ssh_port: Option<u16>,
    #[serde(rename = "copyHostKeys")]
    copy_host_keys: bool,
    #[serde(default)]
    secretspec: MachineSecretspec,
    #[serde(rename = "secrets", default)]
    bootstrap_secrets: Vec<BootstrapSecret>,
    #[serde(rename = "extraFiles", default)]
    extra_files: BTreeMap<String, ExtraFile>,
    #[serde(rename = "encryptionKeys", default)]
    encryption_keys: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExtraFile {
    source: String,
    owner: String,
    mode: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BootstrapSecret {
    target: String,
    secret: String,
    owner: String,
    mode: String,
}

struct TargetBootstrapInstall<'a> {
    secrets: &'a [BootstrapSecret],
    settings: &'a MachineSecretspec,
    toplevel: &'a Path,
    manifest: &'a SecretSlice<u8>,
}

/// Sanity check surfaced from the machine's evaluated NixOS config. Loaded
/// lazily per machine at install time through the same `_nixosEval` thunk that
/// produces the toplevel.
#[derive(Debug, Clone, Deserialize)]
struct MachineInstallCheck {
    #[serde(rename = "hasRootAuth")]
    has_root_auth: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct MachineTarget {
    host: Option<String>,
    #[serde(rename = "sshOpts")]
    ssh_opts: Vec<String>,
}

/// Facts collected from a preflight SSH probe on the install target.
/// Parsed from key=value lines emitted by the probe script. `uid` remains
/// unset for empty or malformed output so install preflight fails closed.
#[derive(Debug, Default)]
struct HostFacts {
    uid: Option<u32>,
    has_tar: bool,
    has_curl: bool,
}

impl HostFacts {
    fn parse(output: &str) -> Self {
        let mut facts = HostFacts::default();
        for line in output.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("user=") {
                facts.uid = val.parse().ok();
            } else if let Some(val) = line.strip_prefix("has_tar=") {
                facts.has_tar = val == "1";
            } else if let Some(val) = line.strip_prefix("has_curl=") {
                facts.has_curl = val == "1";
            }
        }
        facts
    }
}

/// A parsed SSH destination for a machine's `target.host`.
///
/// Accepts `user@host`, `user@host:port`, and `ssh://user@host:port`.
/// `user` is optional to match SSH's own semantics, but devenv itself never
/// produces a target without a user — doc: "logs in as `root` and does not
/// escalate with sudo".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SshTarget {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

impl SshTarget {
    pub(crate) fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("target.host cannot be empty");
        }
        let rest = trimmed.strip_prefix("ssh://").unwrap_or(trimmed);

        let (user, after_user) = match rest.split_once('@') {
            Some((u, r)) => (Some(u.to_string()), r),
            None => (None, rest),
        };

        let (host, port) = match after_user.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p.parse().map_err(|_| {
                    miette!(
                        "target.host '{input}' has an invalid port: {p:?}. Expected a number 1-65535."
                    )
                })?;
                (h.to_string(), Some(port))
            }
            None => (after_user.to_string(), None),
        };

        if host.is_empty() {
            bail!("target.host '{input}' has an empty host component");
        }

        Ok(SshTarget { user, host, port })
    }

    /// Destination string suitable as the positional argument to `ssh`.
    pub(crate) fn ssh_destination(&self) -> String {
        match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        }
    }

    /// URI suitable as the argument to `nix copy --to`.
    pub(crate) fn nix_copy_uri(&self) -> String {
        let mut uri = String::from("ssh://");
        if let Some(u) = &self.user {
            uri.push_str(u);
            uri.push('@');
        }
        uri.push_str(&self.host);
        if let Some(p) = self.port {
            uri.push(':');
            uri.push_str(&p.to_string());
        }
        uri
    }

    pub(crate) fn port(&self) -> Option<u16> {
        self.port
    }
}

/// Build a Nix `builders` config line from the machines metadata. Each
/// machine with `target.host` set and a matching `system` becomes a
/// candidate remote builder. Returns `None` if no builders are available.
///
/// Format: `ssh://user@host system` (one entry per builder), joined by `;`
/// for Nix's `builders` setting.
fn resolve_builders_config(meta: &BTreeMap<String, MachineMeta>) -> Option<String> {
    let entries: Vec<String> = meta
        .values()
        .filter(|m| m.target.host.is_some())
        .filter_map(|m| {
            let host = m.target.host.as_deref()?;
            let target = SshTarget::parse(host).ok()?;
            Some(format!("{} {}", target.nix_copy_uri(), m.system))
        })
        .collect();
    if entries.is_empty() {
        None
    } else {
        Some(entries.join(" ; "))
    }
}

fn configure_remote_builders(devenv: &Devenv, meta: &BTreeMap<String, MachineMeta>) -> Result<()> {
    let Some(builders) = resolve_builders_config(meta) else {
        return Ok(());
    };
    if devenv.cnix().is_none() {
        bail!("--use-machines-as-builders currently requires the C-Nix backend");
    }
    devenv_nix_backend::backend::apply_remote_builders(&builders)
}

/// Return the default nixos-images kexec tarball URL for the given system.
fn kexec_url(system: &str) -> Result<String> {
    let arch = match system {
        "x86_64-linux" => "x86_64-linux",
        "aarch64-linux" => "aarch64-linux",
        other => bail!(
            "No default kexec image for system '{other}'. \
             Only x86_64-linux and aarch64-linux are supported. \
             For other architectures, set `install.kexec.image` on the machine."
        ),
    };
    Ok(format!(
        "https://github.com/nix-community/nixos-images/releases/download/nixos-unstable/nixos-kexec-installer-noninteractive-{arch}.tar.gz"
    ))
}

/// Build the argv fragment for SSH options. OpenSSH uses the first obtained
/// value for most settings, so explicit machine options precede our defaults.
fn ssh_opts_argv(user_opts: &[String]) -> Vec<String> {
    let mut v = user_opts.to_vec();
    v.extend(DEFAULT_SSH_OPTS.iter().map(|s| s.to_string()));
    v
}

/// Force fail-closed host authentication and disable forwarding features for
/// every connection in an install that will transmit local file payloads.
fn sensitive_install_ssh_opts(user_opts: &[String]) -> Vec<String> {
    let mut v: Vec<String> = SENSITIVE_INSTALL_SSH_OPTS
        .iter()
        .map(|s| s.to_string())
        .collect();
    v.extend(user_opts.iter().cloned());
    v
}

fn install_transmits_local_files(
    phases: &HashSet<InstallPhase>,
    has_encryption_keys: bool,
    has_extra_files: bool,
    has_bootstrap_secrets: bool,
) -> bool {
    (phases.contains(&InstallPhase::Disko) && has_encryption_keys)
        || (phases.contains(&InstallPhase::Install) && (has_extra_files || has_bootstrap_secrets))
}

/// Machine names are interpolated into bare Nix attr paths. Keep them to a
/// deliberately small set that cannot change attr-path or shell semantics.
fn validate_machine_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Machine name cannot be empty");
    }
    let first = name.chars().next().expect("non-empty machine name");
    if !(first.is_ascii_alphabetic() || first == '_') {
        bail!(
            "Invalid machine name {name:?}: must start with a letter or underscore, \
             then only letters, digits, '_' or '-'."
        );
    }
    for c in name.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            bail!(
                "Invalid machine name {name:?}: contains '{c}'. \
                 Only letters, digits, '_' and '-' are allowed."
            );
        }
    }
    Ok(())
}

/// Kexec images are fetched by curl on the target. Limit overrides to plain
/// HTTP(S) URLs and reject characters with shell semantics.
fn validate_kexec_url(url: &str) -> Result<()> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!(
            "install.kexec.image {url:?} must be an http:// or https:// URL — \
             the kexec phase fetches it with curl on the target."
        );
    }
    for c in url.chars() {
        let ok = c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '.'
                    | '_'
                    | '~'
                    | ':'
                    | '/'
                    | '?'
                    | '#'
                    | '['
                    | ']'
                    | '@'
                    | '&'
                    | '+'
                    | ','
                    | '='
                    | '%'
            );
        if !ok {
            bail!(
                "install.kexec.image {url:?} contains character {c:?} which is \
                 not allowed in a URL used by the kexec phase."
            );
        }
    }
    Ok(())
}

/// POSIX single-quote a value interpolated into a remote shell command.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Build the privileged nix-darwin activation command. Remote macOS deploys
/// commonly log in as an administrator rather than root, so activation must
/// actually invoke the passwordless `sudo` that the user configured. Root
/// targets skip sudo, and both paths use an explicit nix-env discovered before
/// privilege escalation so sudo's restricted PATH cannot hide Nix.
fn nix_darwin_activation_script(toplevel: &Path) -> String {
    let toplevel = shell_quote(&toplevel.display().to_string());
    format!(
        "set -eu; p={toplevel}; \
         nix_env=$(command -v nix-env 2>/dev/null || true); \
         if [ -z \"$nix_env\" ]; then nix_env=/nix/var/nix/profiles/default/bin/nix-env; fi; \
         if [ ! -x \"$nix_env\" ]; then echo 'nix-env is not available on the target' >&2; exit 1; fi; \
         if [ \"$(id -u)\" -eq 0 ]; then \
           \"$nix_env\" --profile /nix/var/nix/profiles/system --set \"$p\" && \
             HOME=/var/root \"$p/activate\"; \
         else \
           sudo -H -- \"$nix_env\" --profile /nix/var/nix/profiles/system --set \"$p\" && \
             sudo -H -- /usr/bin/env HOME=/var/root \"$p/activate\"; \
         fi"
    )
}

/// Validate a configured file path written on the installer or below `/mnt`.
fn validate_target_file_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path == "/" {
        bail!("target file path {path:?} must be an absolute file path");
    }
    if path.chars().any(char::is_control) {
        bail!("target file path {path:?} cannot contain control characters");
    }
    if path.split('/').any(|component| component == "..") {
        bail!("target file path {path:?} cannot contain '..' components");
    }
    Ok(())
}

fn validate_file_owner(owner: &str) -> Result<()> {
    let Some((uid, gid)) = owner.split_once(':') else {
        bail!("file owner {owner:?} must use numeric uid:gid format");
    };
    if uid.is_empty()
        || gid.is_empty()
        || !uid.chars().all(|c| c.is_ascii_digit())
        || !gid.chars().all(|c| c.is_ascii_digit())
    {
        bail!("file owner {owner:?} must use numeric uid:gid format");
    }
    Ok(())
}

fn validate_file_mode(mode: &str) -> Result<()> {
    if !(3..=4).contains(&mode.len()) || !mode.chars().all(|c| matches!(c, '0'..='7')) {
        bail!("file mode {mode:?} must be a 3- or 4-digit octal mode");
    }
    Ok(())
}

/// Bootstrap credentials should not accidentally be installed with special,
/// executable, group-writable, or world-accessible permission bits. Group
/// read remains available for services that use a dedicated credentials
/// group (for example, 0640).
fn validate_secret_file_mode(mode: &str) -> Result<()> {
    validate_file_mode(mode)?;
    let parsed = u32::from_str_radix(mode, 8).expect("validated octal mode");
    if parsed & 0o7000 != 0 || parsed & 0o111 != 0 || parsed & 0o020 != 0 || parsed & 0o007 != 0 {
        bail!(
            "secret file mode {mode:?} is too permissive: special and execute bits, group write, and all permissions for other users are forbidden"
        );
    }
    Ok(())
}

/// Build the target-side receiver for one install-time file. The payload is
/// framed as `<decimal byte length>\n<raw bytes>`. It is written to a private
/// temporary file, checked for truncation, and atomically renamed over the
/// destination only after its metadata is correct.
fn install_file_receiver_script(
    allowed_root: &str,
    target: &str,
    mode: &str,
    owner: &str,
) -> String {
    let root = shell_quote(allowed_root);
    let target = shell_quote(target);
    let mode = shell_quote(mode);
    let owner = shell_quote(owner);
    format!(
        "set -eu; umask 077; \
         root={root}; dest={target}; dir=$(dirname -- \"$dest\"); \
         mkdir -p -- \"$dir\"; \
         resolved_root=$(realpath -e -- \"$root\"); \
         resolved_dir=$(realpath -e -- \"$dir\"); \
         if [ \"$resolved_root\" != / ]; then \
           case \"$resolved_dir/\" in \"$resolved_root\"/*) ;; \
             *) echo 'File destination escapes its allowed root' >&2; exit 1 ;; \
           esac; \
         fi; \
         tmp=$(mktemp -- \"$dir/.devenv-secret.XXXXXX\"); \
         cleanup() {{ if [ -n \"$tmp\" ]; then rm -f -- \"$tmp\"; fi; }}; \
         trap cleanup EXIT HUP INT TERM; \
         IFS= read -r expected; \
         case \"$expected\" in ''|*[!0-9]*) echo 'Invalid secret payload length' >&2; exit 1 ;; esac; \
         head -c \"$expected\" > \"$tmp\"; \
         actual=$(stat -c %s -- \"$tmp\"); \
         if [ \"$actual\" != \"$expected\" ]; then \
           echo 'Truncated secret payload' >&2; exit 1; \
         fi; \
         chmod -- {mode} \"$tmp\"; \
         chown -- {owner} \"$tmp\"; \
         sync -f \"$tmp\"; \
         mv -T -- \"$tmp\" \"$dest\"; \
         tmp=; \
         sync -f \"$dir\""
    )
}

/// Produce a self-contained SecretSpec manifest containing only the active and
/// default profiles. Configuration inheritance is flattened structurally on
/// the workstation; provider access is deliberately not performed here. Every
/// requested entry is forced to `as_path`, allowing the target receiver to copy
/// exact bytes without placing a value on stdout. Other declarations remain so
/// composed secrets and profile-wide constraints retain their semantics, but
/// the receiver asks SecretSpec to resolve only the explicitly requested names.
fn target_secretspec_manifest(
    secretspec_path: &Path,
    profile_name: &str,
    secret_names: impl IntoIterator<Item = String>,
) -> Result<Vec<u8>> {
    let mut config = secretspec::Config::try_from(secretspec_path)
        .map_err(|e| miette!("Failed to load SecretSpec configuration: {e}"))?;
    if !config.profiles.contains_key(profile_name) {
        let mut available: Vec<&str> = config.profiles.keys().map(String::as_str).collect();
        available.sort_unstable();
        return Err(miette!(
            "SecretSpec profile {profile_name:?} is not defined. Available profiles: {}",
            available.join(", ")
        ));
    }

    let mut missing = Vec::new();
    let mut names: Vec<String> = secret_names.into_iter().collect();
    names.sort();
    names.dedup();

    for name in names {
        let mut found = false;
        if let Some(secret) = config
            .profiles
            .get_mut(profile_name)
            .and_then(|profile| profile.secrets.get_mut(&name))
        {
            secret.as_path = Some(true);
            found = true;
        }
        if profile_name != "default"
            && let Some(secret) = config
                .profiles
                .get_mut("default")
                .and_then(|profile| profile.secrets.get_mut(&name))
        {
            secret.as_path = Some(true);
            found = true;
        }
        if !found {
            missing.push(name);
        }
    }

    if !missing.is_empty() {
        bail!(
            "SecretSpec profile {profile_name:?} does not declare bootstrap secret(s): {}",
            missing.join(", ")
        );
    }

    config.project.extends = None;
    config
        .profiles
        .retain(|name, _| name == profile_name || name == "default");
    config.scopes = None;
    let manifest = toml::to_string(&config)
        .into_diagnostic()
        .wrap_err("Failed to serialize target SecretSpec manifest")?;
    Ok(manifest.into_bytes())
}

/// Build the installer-side program for target-resolved secrets. Stdin carries
/// only a reduced SecretSpec manifest, framed as `<length>\n<bytes>`. SecretSpec
/// writes each fetched value into a private temporary file below a controlled
/// TMPDIR; this script copies it to a same-directory temporary destination and
/// atomically renames it into the installed system.
fn target_secretspec_installer_script(
    allowed_root: &str,
    secretspec_bin: &Path,
    machine_name: &str,
    profile: &str,
    provider: Option<&str>,
    secrets: &[BootstrapSecret],
) -> String {
    let root = shell_quote(allowed_root);
    let resolver = shell_quote(&secretspec_bin.display().to_string());
    let reason = shell_quote(&format!("bootstrap machines.{machine_name}"));
    let profile = shell_quote(profile);
    let mut script = format!(
        "set -eu; ulimit -c 0; umask 077; \
         root={root}; resolver={resolver}; \
         work=$(mktemp -d /tmp/devenv-secretspec.XXXXXX); \
         manifest=\"$work/secretspec.toml\"; tmp=; resolved=; \
         cleanup() {{ \
           if [ -n \"$tmp\" ]; then rm -f -- \"$tmp\"; fi; \
           if [ -n \"$resolved\" ]; then \
             case \"$resolved\" in \"$work\"/*) rm -f -- \"$resolved\" ;; esac; \
           fi; \
           if [ -n \"$work\" ]; then rm -rf -- \"$work\"; fi; \
         }}; \
         trap cleanup EXIT HUP INT TERM; \
         IFS= read -r expected; \
         case \"$expected\" in ''|*[!0-9]*) echo 'Invalid SecretSpec manifest length' >&2; exit 1 ;; esac; \
         head -c \"$expected\" > \"$manifest\"; \
         actual=$(stat -c %s -- \"$manifest\"); \
         if [ \"$actual\" != \"$expected\" ]; then \
           echo 'Truncated SecretSpec manifest' >&2; exit 1; \
         fi; \
         resolved_root=$(realpath -e -- \"$root\"); \
         export TMPDIR=\"$work\"; "
    );

    for secret in secrets {
        let name = shell_quote(&secret.secret);
        let dest = shell_quote(&format!("{allowed_root}{}", secret.target));
        let mode = shell_quote(&secret.mode);
        let owner = shell_quote(&secret.owner);
        let provider_arg = provider
            .map(|value| format!(" --provider {}", shell_quote(value)))
            .unwrap_or_default();
        script.push_str(&format!(
            "resolved=$(\"$resolver\" --file \"$manifest\" --reason {reason} \
               get {name} --profile {profile}{provider_arg}); \
             resolved=$(realpath -e -- \"$resolved\"); \
             case \"$resolved\" in \"$work\"/*) ;; \
               *) echo 'SecretSpec returned a file outside its private directory' >&2; exit 1 ;; \
             esac; \
             if [ ! -f \"$resolved\" ]; then echo 'SecretSpec did not return a regular file' >&2; exit 1; fi; \
             dest={dest}; dir=$(dirname -- \"$dest\"); mkdir -p -- \"$dir\"; \
             resolved_dir=$(realpath -e -- \"$dir\"); \
             case \"$resolved_dir/\" in \"$resolved_root\"/*) ;; \
               *) echo 'Secret destination escapes the installed system' >&2; exit 1 ;; \
             esac; \
             tmp=$(mktemp -- \"$dir/.devenv-secret.XXXXXX\"); \
             expected_value=$(stat -c %s -- \"$resolved\"); \
             cp -- \"$resolved\" \"$tmp\"; \
             actual_value=$(stat -c %s -- \"$tmp\"); \
             if [ \"$actual_value\" != \"$expected_value\" ]; then echo 'Truncated secret copy' >&2; exit 1; fi; \
             rm -f -- \"$resolved\"; resolved=; \
             chmod -- {mode} \"$tmp\"; chown -- {owner} \"$tmp\"; \
             sync -f \"$tmp\"; mv -T -- \"$tmp\" \"$dest\"; tmp=; sync -f \"$dir\"; "
        ));
    }

    script.push_str("rm -rf -- \"$work\"; work=; trap - EXIT HUP INT TERM");
    script
}

/// Build the `NIX_SSHOPTS` env var value used by `nix copy`. Nix parses this
/// as a shell-style word list, so joining with spaces is correct as long as
/// individual option tokens do not contain spaces — which is true for the
/// defaults and for the usual `-o Key=Value` shape users pass.
fn nix_ssh_opts_env(user_opts: &[String]) -> String {
    ssh_opts_argv(user_opts).join(" ")
}

/// One row in the `devenv machines info` table. Columns follow the
/// user-facing vocabulary of `machines.<name>` so the table doubles as a
/// quick schema reference.
#[derive(Table)]
struct MachineInfoRow {
    #[table(title = "Name")]
    name: String,
    #[table(title = "System")]
    system: String,
    #[table(title = "Target")]
    target: String,
    #[table(title = "Roles")]
    roles: String,
}

/// Render the role list for a machine into a comma-separated string,
/// matching the schema attribute names exactly.
fn format_roles(m: &MachineMeta) -> String {
    let mut roles = Vec::with_capacity(3);
    if m.has_nixos {
        roles.push("nixos");
    }
    if m.has_nix_darwin {
        roles.push("nix-darwin");
    }
    if m.has_home_manager {
        roles.push("home-manager");
    }
    if roles.is_empty() {
        "(none)".to_string()
    } else {
        roles.join(", ")
    }
}

impl Devenv {
    pub async fn machines_info(&self, names: &[String]) -> Result<String> {
        let meta = self.load_machines_meta().await?;

        // Resolve the working set. With no explicit names, show every
        // machine. With explicit names, validate each one up front so a
        // typo produces the same "Unknown machine(s)" error shape as
        // `deploy`.
        let working_set: Vec<&str> = if names.is_empty() {
            meta.keys().map(String::as_str).collect()
        } else {
            let mut missing: Vec<&str> = Vec::new();
            for name in names {
                if !meta.contains_key(name) {
                    missing.push(name.as_str());
                }
            }
            if !missing.is_empty() {
                let available: Vec<String> = meta.keys().cloned().collect();
                let available_str = if available.is_empty() {
                    "(none defined in devenv.nix)".to_string()
                } else {
                    available.join(", ")
                };
                bail!(
                    "Unknown machine(s): {}. Available: {}",
                    missing.join(", "),
                    available_str
                );
            }
            names.iter().map(String::as_str).collect()
        };

        if working_set.is_empty() {
            return Ok("No machines defined in devenv.nix.\n".to_string());
        }

        let rows: Vec<MachineInfoRow> = working_set
            .into_iter()
            .map(|name| {
                let m = &meta[name];
                MachineInfoRow {
                    name: name.to_string(),
                    system: m.system.clone(),
                    target: m
                        .target
                        .host
                        .clone()
                        .unwrap_or_else(|| "(no target)".to_string()),
                    roles: format_roles(m),
                }
            })
            .collect();

        let table = rows
            .with_title()
            .table()
            .display()
            .map_err(|e| miette!("Failed to format machines info table: {e}"))?;
        Ok(format!("{table}\n"))
    }

    pub async fn machines_install(
        &self,
        names: &[String],
        max_concurrent: Option<usize>,
        phases: &HashSet<InstallPhase>,
        disko_mode: DiskoMode,
        use_machines_as_builders: bool,
    ) -> Result<()> {
        let meta = self.load_machines_meta().await?;

        // Configure remote builders if requested (same mechanism as deploy).
        if use_machines_as_builders {
            configure_remote_builders(self, &meta)?;
        }

        // Validate names
        let mut missing: Vec<&str> = Vec::new();
        for name in names {
            if !meta.contains_key(name) {
                missing.push(name.as_str());
            }
        }
        if !missing.is_empty() {
            let available: Vec<String> = meta.keys().cloned().collect();
            let available_str = if available.is_empty() {
                "(none defined in devenv.nix)".to_string()
            } else {
                available.join(", ")
            };
            bail!(
                "Unknown machine(s): {}. Available: {}",
                missing.join(", "),
                available_str
            );
        }

        // Per-machine validation: install only applies to NixOS roles
        // over SSH. Catch configuration mistakes before starting any work.
        for name in names {
            let m = &meta[name];
            if !m.has_nixos {
                bail!(
                    "machines.{name} does not have a `nixos` module set. \
                     `devenv machines install` only applies to NixOS machines. \
                     Use `devenv machines deploy` for home-manager and nix-darwin."
                );
            }
            if m.target.host.is_none() {
                bail!(
                    "machines.{name} does not have `target.host` set. \
                     `devenv machines install` always operates over SSH."
                );
            }

            if phases.contains(&InstallPhase::Install) {
                for (target, file) in &m.extra_files {
                    validate_target_file_path(target).wrap_err_with(|| {
                        format!("Invalid machines.{name}.install.extraFiles target")
                    })?;
                    validate_file_owner(&file.owner).wrap_err_with(|| {
                        format!("Invalid owner for machines.{name}.install.extraFiles.{target:?}")
                    })?;
                    validate_file_mode(&file.mode).wrap_err_with(|| {
                        format!("Invalid mode for machines.{name}.install.extraFiles.{target:?}")
                    })?;
                }
                for secret in &m.bootstrap_secrets {
                    validate_target_file_path(&secret.target).wrap_err_with(|| {
                        format!("Invalid machines.{name}.install.secrets target")
                    })?;
                    validate_file_owner(&secret.owner).wrap_err_with(|| {
                        format!(
                            "Invalid owner for machines.{name}.install.secrets.{:?}",
                            secret.target
                        )
                    })?;
                    validate_secret_file_mode(&secret.mode).wrap_err_with(|| {
                        format!(
                            "Invalid mode for machines.{name}.install.secrets.{:?}",
                            secret.target
                        )
                    })?;
                }
            }
            if phases.contains(&InstallPhase::Disko) {
                for target in m.encryption_keys.keys() {
                    validate_target_file_path(target).wrap_err_with(|| {
                        format!("Invalid machines.{name}.install.encryptionKeys target")
                    })?;
                }
            }
        }

        // Prepare every bootstrap input before starting any per-machine job so
        // a missing declaration fails before preflight, kexec, or disko touches
        // a target. Local machines resolve values now. Target machines receive
        // only a reduced manifest and do not contact the local provider.
        let mut bootstrap_values = HashMap::new();
        let mut target_manifests = TargetBootstrapManifests::new();
        if phases.contains(&InstallPhase::Install) {
            let has_bootstrap_secrets = names
                .iter()
                .any(|name| !meta[name].bootstrap_secrets.is_empty());
            let secretspec_enabled = self
                .secret_settings
                .secretspec
                .as_ref()
                .is_some_and(|config| config.enable);
            if has_bootstrap_secrets && !secretspec_enabled {
                bail!(
                    "Machine bootstrap secrets require SecretSpec. Add a secretspec.toml and enable it under `secretspec:` in devenv.yaml, or pass --secretspec-provider/--secretspec-profile."
                );
            }

            let local_names: Vec<&String> = names
                .iter()
                .filter(|name| {
                    !meta[*name].bootstrap_secrets.is_empty()
                        && meta[*name].secretspec.execution == SecretspecExecution::Local
                })
                .collect();
            if !local_names.is_empty() {
                let mut resolved_cell = tokio::sync::OnceCell::new();
                let mut as_paths = HashSet::new();
                super::resolve_secretspec_into(
                    &self.devenv_root,
                    &self.secret_settings,
                    &mut resolved_cell,
                    &mut as_paths,
                )?;
                let resolved = resolved_cell.get().ok_or_else(|| {
                    miette!("Local machine bootstrap secrets require an enabled secretspec.toml")
                })?;

                let mut missing = Vec::new();
                for name in &local_names {
                    for secret in &meta[*name].bootstrap_secrets {
                        if !resolved.secrets.contains_key(&secret.secret) {
                            missing.push(format!(
                                "machines.{name}.install.secrets.{:?} references {:?}",
                                secret.target, secret.secret
                            ));
                        }
                    }
                }
                if !missing.is_empty() {
                    bail!(
                        "SecretSpec did not resolve {} bootstrap secret reference(s):\n- {}",
                        missing.len(),
                        missing.join("\n- ")
                    );
                }

                for name in local_names {
                    for secret in &meta[name].bootstrap_secrets {
                        if bootstrap_values.contains_key(&secret.secret) {
                            continue;
                        }
                        let value = resolved
                            .secrets
                            .get(&secret.secret)
                            .expect("SecretSpec reference checked above");
                        let bytes = if as_paths.contains(&secret.secret) {
                            tokio::fs::read(value)
                                .await
                                .into_diagnostic()
                                .wrap_err_with(|| {
                                    format!(
                                        "Failed to read temporary file for SecretSpec secret {}",
                                        secret.secret
                                    )
                                })?
                        } else {
                            value.as_bytes().to_vec()
                        };
                        bootstrap_values.insert(secret.secret.clone(), SecretSlice::from(bytes));
                    }
                }
            }

            let manifest_path = self.devenv_root.join("secretspec.toml");
            for name in names {
                let machine = &meta[name];
                if machine.bootstrap_secrets.is_empty()
                    || machine.secretspec.execution != SecretspecExecution::Target
                {
                    continue;
                }
                if !manifest_path.exists() {
                    bail!(
                        "machines.{name} uses target-side SecretSpec resolution, but {} does not exist",
                        manifest_path.display()
                    );
                }
                let profile = machine.secretspec.profile.as_deref().unwrap_or("default");
                let manifest = target_secretspec_manifest(
                    &manifest_path,
                    profile,
                    machine
                        .bootstrap_secrets
                        .iter()
                        .map(|secret| secret.secret.clone()),
                )
                .wrap_err_with(|| format!("Invalid target SecretSpec setup for machines.{name}"))?;
                target_manifests.insert(name.clone(), SecretSlice::from(manifest));
            }
        }

        let concurrency = max_concurrent
            .unwrap_or(names.len())
            .max(1)
            .min(names.len());

        let jobs = names.iter().map(|name| {
            let machine = &meta[name];
            let bootstrap_values = &bootstrap_values;
            let target_manifests = &target_manifests;
            let target_label = machine.target.host.as_deref().unwrap_or("(no target)");
            let activity = activity!(
                INFO,
                operation,
                format!("Installing {name} ({target_label})")
            );
            async move {
                let outcome = async {
                    self.install_one_machine(
                        name,
                        machine,
                        phases,
                        disko_mode,
                        bootstrap_values,
                        target_manifests,
                    )
                    .await
                }
                .in_activity(&activity)
                .await;
                if outcome.is_err() {
                    activity.fail();
                }
                (name.clone(), outcome)
            }
        });

        let results: Vec<(String, Result<()>)> = futures::stream::iter(jobs)
            .buffer_unordered(concurrency)
            .collect()
            .await;

        let failures: Vec<(String, miette::Report)> = results
            .into_iter()
            .filter_map(|(n, r)| r.err().map(|e| (n, e)))
            .collect();

        if !failures.is_empty() {
            let details = failures
                .iter()
                .map(|(n, e)| format!("- {n}: {e}"))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "{} machine(s) failed to install:\n{}",
                failures.len(),
                details
            );
        }

        Ok(())
    }

    // ── Install pipeline ─────────────────────────────────────────────

    async fn install_one_machine(
        &self,
        name: &str,
        machine: &MachineMeta,
        phases: &HashSet<InstallPhase>,
        disko_mode: DiskoMode,
        bootstrap_values: &BootstrapValues,
        target_manifests: &TargetBootstrapManifests,
    ) -> Result<()> {
        let host = machine
            .target
            .host
            .as_deref()
            .expect("validated in machines_install");
        let pre_kexec_target = SshTarget::parse(host)?;
        // A machine that will receive any local file payload must use pinned
        // host identity verification from the very first connection. Both
        // encryptionKeys and extraFiles commonly contain credentials, so they
        // get the same policy as SecretSpec bootstrap values.
        let transmits_local_files = install_transmits_local_files(
            phases,
            !machine.encryption_keys.is_empty(),
            !machine.extra_files.is_empty(),
            !machine.bootstrap_secrets.is_empty(),
        );
        let ssh_opts = if transmits_local_files {
            sensitive_install_ssh_opts(&machine.target.ssh_opts)
        } else {
            machine.target.ssh_opts.clone()
        };

        // Preflight: probe the target before any destructive work. The
        // probe is lightweight (one SSH round-trip) and catches the most
        // common misconfiguration — not running as root — before kexec
        // wipes the boot environment. Only runs when kexec is in the
        // phase set (if the user is resuming from a later phase, the
        // target is already in installer state and the probe is moot).
        if phases.contains(&InstallPhase::Kexec) {
            let act = activity!(INFO, operation, "Preflight check");
            async {
                self.install_preflight(name, &pre_kexec_target, &ssh_opts)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // Phase 1: kexec (uses the pre-kexec target; after kexec the port
        // may change if install.kexec.postSshPort is set).
        if phases.contains(&InstallPhase::Kexec) {
            let act = activity!(INFO, operation, "kexec");
            async {
                self.install_kexec(name, &pre_kexec_target, &ssh_opts, machine)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // After kexec, use the post-kexec target (may differ in port).
        let target = match machine.kexec_post_ssh_port {
            Some(port) => SshTarget {
                port: Some(port),
                ..pre_kexec_target
            },
            None => pre_kexec_target,
        };

        // Phase 2: facter
        if phases.contains(&InstallPhase::Facter) {
            let act = activity!(INFO, operation, "Probing hardware (nixos-facter)");
            async { self.install_facter(name, &target, &ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        // Encryption keys are dropped onto the installer BEFORE disko runs
        // so LUKS layouts with `passwordFile` can reference them.
        if phases.contains(&InstallPhase::Disko) && !machine.encryption_keys.is_empty() {
            let act = activity!(INFO, operation, "Copying encryption keys");
            async {
                self.install_encryption_keys(&target, &ssh_opts, &machine.encryption_keys)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // Phase 3: disko
        if phases.contains(&InstallPhase::Disko) {
            let act = activity!(INFO, operation, "Partitioning (disko)");
            async {
                self.install_disko(name, &target, &ssh_opts, disko_mode)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // `nixos-install --no-root-password` must not create a system with no
        // usable root authentication. Evaluate this only when Install runs.
        if phases.contains(&InstallPhase::Install) {
            let check = self.load_machine_install_check(name).await?;
            if !check.has_root_auth {
                bail!(
                    "machines.{name}: refusing to install. The NixOS config \
                     declares no authentication for root, and `nixos-install` \
                     runs with `--no-root-password`, so the target would boot \
                     locked out. Set one of:\n\
                     \x20 - users.users.root.openssh.authorizedKeys.keys\n\
                     \x20 - users.users.root.openssh.authorizedKeys.keyFiles\n\
                     \x20 - users.users.root.hashedPassword\n\
                     \x20 - users.users.root.hashedPasswordFile\n\
                     \x20 - users.users.root.initialHashedPassword"
                );
            }
        }

        // Phase 4: install
        let installed_toplevel = if phases.contains(&InstallPhase::Install) {
            let act = activity!(INFO, operation, "Installing NixOS");
            Some(
                async { self.install_nixos(name, &target, &ssh_opts).await }
                    .in_activity(&act)
                    .await?,
            )
        } else {
            None
        };

        // Extra files are copied after nixos-install (system is at /mnt)
        // Extra files and host keys are part of the install phase conceptually
        // (after nixos-install, before reboot).
        if phases.contains(&InstallPhase::Install) && !machine.extra_files.is_empty() {
            let act = activity!(INFO, operation, "Copying extra files");
            async {
                self.install_extra_files(&target, &ssh_opts, &machine.extra_files)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        if phases.contains(&InstallPhase::Install) && !machine.bootstrap_secrets.is_empty() {
            match machine.secretspec.execution {
                SecretspecExecution::Local => {
                    let act = activity!(INFO, operation, "Copying SecretSpec bootstrap secrets");
                    async {
                        self.install_bootstrap_secrets(
                            name,
                            &target,
                            &ssh_opts,
                            &machine.bootstrap_secrets,
                            bootstrap_values,
                        )
                        .await
                    }
                    .in_activity(&act)
                    .await?;
                }
                SecretspecExecution::Target => {
                    let act = activity!(
                        INFO,
                        operation,
                        "Resolving SecretSpec bootstrap secrets on target"
                    );
                    let toplevel = installed_toplevel
                        .as_ref()
                        .expect("the install phase produced a NixOS toplevel");
                    let manifest = target_manifests
                        .get(name)
                        .expect("target manifest prepared before machine jobs");
                    async {
                        self.install_target_bootstrap_secrets(
                            name,
                            &target,
                            &ssh_opts,
                            TargetBootstrapInstall {
                                secrets: &machine.bootstrap_secrets,
                                settings: &machine.secretspec,
                                toplevel,
                                manifest,
                            },
                        )
                        .await
                    }
                    .in_activity(&act)
                    .await?;
                }
            }
        }

        if phases.contains(&InstallPhase::Install) && machine.copy_host_keys {
            let act = activity!(INFO, operation, "Copying SSH host keys");
            async { self.install_copy_host_keys(&target, &ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        // Phase 5: reboot
        if phases.contains(&InstallPhase::Reboot) {
            let act = activity!(INFO, operation, "Rebooting");
            async { self.install_reboot(&target, &ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        Ok(())
    }

    /// Preflight probe: run a single SSH command that checks key
    /// prerequisites on the target before any destructive work begins.
    async fn install_preflight(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
    ) -> Result<()> {
        // Single script that outputs key=value lines. Avoids multiple
        // SSH round-trips.
        let probe_script = r#"
            echo "user=$(id -u)"
            echo "has_tar=$(command -v tar >/dev/null 2>&1 && echo 1 || echo 0)"
            echo "has_curl=$(command -v curl >/dev/null 2>&1 && echo 1 || echo 0)"
        "#;

        let output = self
            .ssh_run_capture(target, ssh_opts, probe_script)
            .await
            .wrap_err_with(|| {
                format!(
                    "Preflight probe failed on machines.{name} — \
                     is root SSH enabled and TCP 22 reachable?"
                )
            })?;

        let facts = HostFacts::parse(&output);

        match facts.uid {
            None => bail!(
                "machines.{name}: preflight probe did not return a `user=` line. \
                 The SSH command ran but produced unexpected output, so devenv \
                 cannot confirm the target is in a safe state. Re-check SSH \
                 connectivity and that the remote shell allows `id -u`."
            ),
            Some(uid) if uid != 0 => bail!(
                "machines.{name}: install requires root SSH access, but the \
                 current user on the target has uid {uid}. Either SSH in as \
                 root or configure root login on the target."
            ),
            Some(_) => {}
        }
        if !facts.has_tar {
            bail!(
                "machines.{name}: `tar` is not available on the target. \
                 The kexec phase needs tar to extract the installer tarball."
            );
        }
        if !facts.has_curl {
            bail!(
                "machines.{name}: `curl` is not available on the target. \
                 The kexec phase needs curl to download the installer tarball."
            );
        }

        Ok(())
    }

    /// Phase 1: kexec into the NixOS installer on the target.
    ///
    /// Downloads the nixos-images kexec tarball for the machine's system,
    /// extracts it, and runs `kexec`. After kexec, polls SSH until the
    /// installer comes up (or a timeout expires).
    async fn install_kexec(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
        machine: &MachineMeta,
    ) -> Result<()> {
        let url = match &machine.kexec_image {
            Some(custom) => {
                validate_kexec_url(custom)
                    .wrap_err_with(|| format!("install.kexec.image on machines.{name}"))?;
                custom.clone()
            }
            None => kexec_url(&machine.system)?,
        };
        let url_q = shell_quote(&url);
        let script = format!(
            "set -euo pipefail && \
             curl --fail -L {url_q} | tar xzf - -C /root && \
             /root/kexec/run"
        );
        self.ssh_run(target, ssh_opts, &script)
            .await
            .wrap_err_with(|| {
                format!("kexec failed on machines.{name} — is root SSH enabled and the target reachable?")
            })?;

        // After kexec the target reboots into the NixOS installer. Wait
        // for SSH to come back. If install.kexec.postSshPort is set, probe
        // on that port instead of the original.
        let wait_target = match machine.kexec_post_ssh_port {
            Some(port) => SshTarget {
                port: Some(port),
                ..target.clone()
            },
            None => target.clone(),
        };
        self.ssh_wait_ready(&wait_target, ssh_opts, 300)
            .await
            .wrap_err_with(|| {
                format!(
                    "Timed out waiting for machines.{name} to come back after kexec. \
                 If the target got a new IP from DHCP, update target.host and re-run."
                )
            })?;

        Ok(())
    }

    /// Phase 2: probe hardware via nixos-facter on the kexec'd installer.
    ///
    /// Writes the JSON report to `.machines/<name>/facter.json` in the
    /// project root and stages it with `git add --intent-to-add`.
    async fn install_facter(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
    ) -> Result<()> {
        let json = self
            .ssh_run_capture(target, ssh_opts, "nixos-facter --json")
            .await
            .wrap_err_with(|| {
                format!("nixos-facter failed on machines.{name} — is the NixOS installer running?")
            })?;

        // Write report to .machines/<name>/facter.json
        let machines_dir = self.devenv_root.join(".machines").join(name);
        tokio::fs::create_dir_all(&machines_dir)
            .await
            .into_diagnostic()?;
        let facter_path = machines_dir.join("facter.json");
        tokio::fs::write(&facter_path, &json)
            .await
            .into_diagnostic()?;

        // Stage with git add --intent-to-add so the file is tracked
        let status = process::Command::new("git")
            .arg("add")
            .arg("--intent-to-add")
            .arg(&facter_path)
            .current_dir(&self.devenv_root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .into_diagnostic()?;
        if !status.success() {
            // Non-fatal: the file is written, just not staged. The user
            // can `git add` manually.
            tracing::warn!(
                "git add --intent-to-add {} exited with {status}",
                facter_path.display()
            );
        }

        Ok(())
    }

    /// Phase 3: partition and format disks via disko.
    async fn install_disko(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
        mode: DiskoMode,
    ) -> Result<()> {
        let role = match mode {
            DiskoMode::Disko => "diskoScript",
            DiskoMode::Format => "diskoFormatScript",
            DiskoMode::Mount => "diskoMountScript",
        };
        let disko_script = self.build_machine_role(name, role).await?;
        self.nix_copy(target, &disko_script, ssh_opts)
            .await
            .wrap_err("Failed to copy disko script to target")?;
        let disko_str = disko_script.display().to_string();
        self.ssh_run(target, ssh_opts, &disko_str)
            .await
            .wrap_err_with(|| format!("disko partitioning failed on machines.{name}"))?;
        Ok(())
    }

    /// Phase 4: copy the NixOS toplevel and run nixos-install.
    async fn install_nixos(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
    ) -> Result<PathBuf> {
        let toplevel = self.build_machine_role(name, "nixos").await?;
        self.nix_copy(target, &toplevel, ssh_opts)
            .await
            .wrap_err("Failed to copy NixOS closure to target")?;
        let toplevel_str = toplevel.display().to_string();
        let script =
            format!("nixos-install --system {toplevel_str} --no-root-password --no-channel-copy");
        self.ssh_run(target, ssh_opts, &script)
            .await
            .wrap_err_with(|| format!("nixos-install failed on machines.{name}"))?;
        Ok(toplevel)
    }

    /// Copy encryption keyfiles to the installer BEFORE disko runs, so LUKS
    /// layouts with `passwordFile` can unlock during partitioning.
    async fn install_encryption_keys(
        &self,
        target: &SshTarget,
        ssh_opts: &[String],
        keys: &BTreeMap<String, String>,
    ) -> Result<()> {
        for (target_path, local_source) in keys {
            let contents = SecretSlice::from(
                tokio::fs::read(local_source)
                    .await
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to read encryption key {local_source}"))?,
            );
            self.stream_install_file(
                target,
                ssh_opts,
                "/",
                target_path,
                "0600",
                "0:0",
                contents.expose_secret(),
                &format!("encryption key at {target_path}"),
            )
            .await?;
        }
        Ok(())
    }

    /// Copy extra files to the installed system (mounted at /mnt) after
    /// `nixos-install` but before reboot.
    async fn install_extra_files(
        &self,
        target: &SshTarget,
        ssh_opts: &[String],
        files: &BTreeMap<String, ExtraFile>,
    ) -> Result<()> {
        for (target_path, file) in files {
            let contents = SecretSlice::from(
                tokio::fs::read(&file.source)
                    .await
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to read {}", file.source))?,
            );
            // Write below /mnt since the installed system is mounted there.
            let mnt_path = format!("/mnt{target_path}");
            self.stream_install_file(
                target,
                ssh_opts,
                "/mnt",
                &mnt_path,
                &file.mode,
                &file.owner,
                contents.expose_secret(),
                &format!("extra file at {target_path}"),
            )
            .await?;
        }
        Ok(())
    }

    /// Materialize SecretSpec values in the installed system. Values were
    /// resolved and copied into `bootstrap_values` before any machine job
    /// started; only their bytes travel over SSH stdin.
    async fn install_bootstrap_secrets(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
        secrets: &[BootstrapSecret],
        bootstrap_values: &BootstrapValues,
    ) -> Result<()> {
        for secret in secrets {
            let contents = bootstrap_values.get(&secret.secret).ok_or_else(|| {
                miette!(
                    "SecretSpec secret {:?} for machines.{name} was not resolved",
                    secret.secret
                )
            })?;

            let mnt_path = format!("/mnt{}", secret.target);
            self.stream_install_file(
                target,
                ssh_opts,
                "/mnt",
                &mnt_path,
                &secret.mode,
                &secret.owner,
                contents.expose_secret(),
                &format!(
                    "SecretSpec secret {:?} at {} on machines.{name}",
                    secret.secret, secret.target
                ),
            )
            .await?;
        }

        Ok(())
    }

    /// Stream exact bytes over SSH stdin and atomically install them with the
    /// requested metadata. No payload is placed in argv or the remote script.
    #[allow(clippy::too_many_arguments)]
    async fn stream_install_file(
        &self,
        target: &SshTarget,
        ssh_opts: &[String],
        allowed_root: &str,
        destination: &str,
        mode: &str,
        owner: &str,
        contents: &[u8],
        description: &str,
    ) -> Result<()> {
        let script = install_file_receiver_script(allowed_root, destination, mode, owner);
        let mut cmd = process::Command::new("ssh");
        for opt in ssh_opts_argv(ssh_opts) {
            cmd.arg(opt);
        }
        if let Some(port) = target.port() {
            cmd.arg("-p").arg(port.to_string());
        }
        cmd.arg(target.ssh_destination()).arg(&script);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        cmd.kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to start SSH while installing {description}"))?;
        let write_result = async {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take().ok_or_else(|| {
                miette!("SSH stdin was not available while installing {description}")
            })?;
            let header = format!("{}\n", contents.len());
            stdin.write_all(header.as_bytes()).await.into_diagnostic()?;
            stdin.write_all(contents).await.into_diagnostic()?;
            stdin.shutdown().await.into_diagnostic()?;
            Ok::<(), miette::Report>(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error).wrap_err_with(|| format!("Failed to stream {description}"));
        }
        let status = child.wait().await.into_diagnostic()?;
        if !status.success() {
            bail!("Failed to install {description}");
        }
        Ok(())
    }

    /// Resolve bootstrap values on the installer itself. The workstation sends
    /// only the reduced SecretSpec manifest; provider authentication and secret
    /// bytes remain on the target. The resolver is part of the just-installed
    /// NixOS toplevel and therefore already present in the copied closure.
    async fn install_target_bootstrap_secrets(
        &self,
        name: &str,
        target: &SshTarget,
        ssh_opts: &[String],
        install: TargetBootstrapInstall<'_>,
    ) -> Result<()> {
        let profile = install.settings.profile.as_deref().unwrap_or("default");
        let secretspec_bin = install.toplevel.join("sw/bin/secretspec");
        let script = target_secretspec_installer_script(
            "/mnt",
            &secretspec_bin,
            name,
            profile,
            install.settings.provider.as_deref(),
            install.secrets,
        );

        let mut cmd = process::Command::new("ssh");
        for opt in ssh_opts_argv(ssh_opts) {
            cmd.arg(opt);
        }
        if let Some(port) = target.port() {
            cmd.arg("-p").arg(port.to_string());
        }
        cmd.arg(target.ssh_destination()).arg(&script);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().into_diagnostic().wrap_err_with(|| {
            format!("Failed to start target-side SecretSpec for machines.{name}")
        })?;
        let manifest = install.manifest.expose_secret();
        let write_result = async {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| miette!("SSH stdin was not available for machines.{name}"))?;
            let header = format!("{}\n", manifest.len());
            stdin.write_all(header.as_bytes()).await.into_diagnostic()?;
            stdin.write_all(manifest).await.into_diagnostic()?;
            stdin.shutdown().await.into_diagnostic()?;
            Ok::<(), miette::Report>(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error).wrap_err_with(|| {
                format!("Failed to send SecretSpec manifest to machines.{name}")
            });
        }

        let status = child.wait().await.into_diagnostic()?;
        if !status.success() {
            bail!("Target-side SecretSpec bootstrap failed for machines.{name}");
        }
        Ok(())
    }

    /// Keep the post-kexec SSH identity across first boot by copying host keys
    /// from the installer's `/etc/ssh/` into `/mnt/etc/ssh/`.
    async fn install_copy_host_keys(&self, target: &SshTarget, ssh_opts: &[String]) -> Result<()> {
        self.ssh_run(
            target,
            ssh_opts,
            "mkdir -p /mnt/etc/ssh && cp /etc/ssh/ssh_host_* /mnt/etc/ssh/",
        )
        .await
        .wrap_err("Failed to copy SSH host keys to installed system")?;
        Ok(())
    }

    /// Phase 5: reboot into the installed system.
    async fn install_reboot(&self, target: &SshTarget, ssh_opts: &[String]) -> Result<()> {
        self.ssh_run_capture(target, ssh_opts, "true")
            .await
            .wrap_err(
                "Cannot reach target to reboot — SSH connection failed before issuing reboot",
            )?;
        // The probe above separates a dial failure from the expected
        // connection drop after issuing reboot.
        let _ = self.ssh_run(target, ssh_opts, "reboot").await;
        Ok(())
    }

    /// Like `ssh_run` but captures stdout into a String.
    async fn ssh_run_capture(
        &self,
        target: &SshTarget,
        user_ssh_opts: &[String],
        script: &str,
    ) -> Result<String> {
        let mut cmd = process::Command::new("ssh");
        for opt in ssh_opts_argv(user_ssh_opts) {
            cmd.arg(opt);
        }
        if let Some(port) = target.port() {
            cmd.arg("-p").arg(port.to_string());
        }
        cmd.arg(target.ssh_destination()).arg(script);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let output = cmd
            .output()
            .await
            .into_diagnostic()
            .wrap_err("Failed to spawn ssh")?;
        if !output.status.success() {
            bail!(
                "ssh {} '{}' exited with {}",
                target.ssh_destination(),
                script,
                output.status
            );
        }
        String::from_utf8(output.stdout)
            .into_diagnostic()
            .wrap_err("ssh stdout was not valid UTF-8")
    }

    /// Poll SSH connectivity until the target responds or the timeout
    /// expires. Used after kexec to wait for the NixOS installer to boot.
    async fn ssh_wait_ready(
        &self,
        target: &SshTarget,
        user_ssh_opts: &[String],
        timeout_secs: u64,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        let probe_interval = std::time::Duration::from_secs(5);
        // Use a short ConnectTimeout for probes so we don't block the
        // full interval on an unreachable host.
        let mut probe_opts = vec!["-o".to_string(), "ConnectTimeout=5".to_string()];
        probe_opts.extend(user_ssh_opts.iter().cloned());
        loop {
            let mut cmd = process::Command::new("ssh");
            for opt in ssh_opts_argv(&probe_opts) {
                cmd.arg(opt);
            }
            if let Some(port) = target.port() {
                cmd.arg("-p").arg(port.to_string());
            }
            cmd.arg(target.ssh_destination()).arg("true");
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Ok(status) = cmd.status().await
                && status.success()
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "SSH to {} did not become ready within {timeout_secs}s",
                    target.ssh_destination()
                );
            }
            tokio::time::sleep(probe_interval).await;
        }
    }

    pub async fn machines_deploy(
        &self,
        names: &[String],
        max_concurrent: Option<usize>,
        use_machines_as_builders: bool,
    ) -> Result<()> {
        // Configure the live C-Nix builder settings before parallel jobs.
        if use_machines_as_builders {
            let meta = self.load_machines_meta().await?;
            configure_remote_builders(self, &meta)?;
            return self
                .machines_deploy_inner(names, max_concurrent, &meta)
                .await;
        }

        let meta = self.load_machines_meta().await?;
        self.machines_deploy_inner(names, max_concurrent, &meta)
            .await
    }

    async fn machines_deploy_inner(
        &self,
        names: &[String],
        max_concurrent: Option<usize>,
        meta: &BTreeMap<String, MachineMeta>,
    ) -> Result<()> {
        // Resolve the working set. Explicit names are used as-is; bulk runs
        // (no names) pick every machine in the attrset, per doc:
        //
        //     `devenv machines deploy` without arguments deploys every
        //     machine in the attrset that has `target.host` set. Machines
        //     without a host are skipped with an informational line, which
        //     is how you opt a local-only home-manager entry out of bulk
        //     deploys.
        //
        // "Skipping" here means we still create the per-machine activity so
        // the TUI shows the entry as Skipped, rather than silently dropping
        // it from the report.
        let bulk = names.is_empty();
        let working_set: Vec<String> = if bulk {
            meta.keys().cloned().collect()
        } else {
            // Validate every requested name up front so a typo never
            // partially-deploys a bulk run.
            let mut missing: Vec<&str> = Vec::new();
            for name in names {
                if !meta.contains_key(name) {
                    missing.push(name.as_str());
                }
            }
            if !missing.is_empty() {
                let available: Vec<String> = meta.keys().cloned().collect();
                let available_str = if available.is_empty() {
                    "(none defined in devenv.nix)".to_string()
                } else {
                    available.join(", ")
                };
                bail!(
                    "Unknown machine(s): {}. Available: {}",
                    missing.join(", "),
                    available_str
                );
            }
            names.to_vec()
        };

        if working_set.is_empty() {
            return Ok(());
        }

        // Parallel execution. Doc: "Machines are deployed in parallel by
        // default. Each machine runs its own build, copy, and activation
        // pipeline independently, and the summary printed at the end shows
        // the outcome for each one. Pass `--max-concurrent N` to cap how many
        // machines run at once; `--max-concurrent 1` runs them one at a time."
        //
        // Default cap = working_set.len() (every machine runs concurrently);
        // an explicit `--max-concurrent N` clamps it. A limit of 1 gives
        // deterministic sequential ordering, which matches the doc's
        // "watching a single host closely" use case.
        let concurrency = max_concurrent
            .unwrap_or(working_set.len())
            .max(1)
            .min(working_set.len());

        // Build a stream of per-machine deploy futures. Each future creates
        // its own activity, so the TUI shows a tree of concurrent machines
        // under the top-level `devenv machines deploy` operation, and marks
        // each one `skipped` / `fail` independently.
        //
        // Doc: "A failure on one machine does not stop the others. Machines
        // that already activated stay applied, machines still running
        // finish their own pipelines, and every outcome lands in the final
        // summary."
        let jobs = working_set.iter().map(|name| {
            let machine = &meta[name];
            let target_label = machine.target.host.as_deref().unwrap_or("(no target)");
            let activity = activity!(
                INFO,
                operation,
                format!("Deploying {name} ({target_label})")
            );
            let should_skip = bulk && machine.target.host.is_none();
            async move {
                if should_skip {
                    activity.skipped();
                    return (name.clone(), Ok(()));
                }
                let outcome = async { self.deploy_one_machine(name, machine).await }
                    .in_activity(&activity)
                    .await;
                if outcome.is_err() {
                    activity.fail();
                }
                (name.clone(), outcome)
            }
        });

        let results: Vec<(String, Result<()>)> = futures::stream::iter(jobs)
            .buffer_unordered(concurrency)
            .collect()
            .await;

        let failures: Vec<(String, miette::Report)> = results
            .into_iter()
            .filter_map(|(n, r)| r.err().map(|e| (n, e)))
            .collect();

        if !failures.is_empty() {
            let details = failures
                .iter()
                .map(|(n, e)| format!("- {n}: {e}"))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "{} machine(s) failed to deploy:\n{}",
                failures.len(),
                details
            );
        }

        Ok(())
    }

    /// Load the metadata attrset for every machine defined in `devenv.nix`.
    ///
    /// Goes through the `machinesMeta` option specifically (not `machines`)
    /// so that `devenv eval` does not try to serialise user-supplied NixOS
    /// module functions or force `build.*` closures for unrelated machines.
    async fn load_machine_install_check(&self, name: &str) -> Result<MachineInstallCheck> {
        let attr = format!("devenv.config.machines.{name}.installCheck");
        let json = self.backend.eval_devenv(&[attr.as_str()]).await?;
        serde_json::from_str(&json)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to parse installCheck JSON for machines.{name}"))
    }

    async fn load_machines_meta(&self) -> Result<BTreeMap<String, MachineMeta>> {
        let json = self
            .backend
            .eval_devenv(&["devenv.config.machinesMeta"])
            .await
            .wrap_err("Failed to load machinesMeta from devenv.nix")?;
        let meta: BTreeMap<String, MachineMeta> = serde_json::from_str(&json)
            .into_diagnostic()
            .wrap_err("Failed to parse machinesMeta JSON")?;
        for name in meta.keys() {
            validate_machine_name(name)
                .wrap_err("Rejecting machine with an unsafe attribute name")?;
        }
        Ok(meta)
    }

    async fn deploy_one_machine(&self, name: &str, machine: &MachineMeta) -> Result<()> {
        if !machine.has_nixos && !machine.has_nix_darwin && !machine.has_home_manager {
            bail!(
                "machines.{name} has no deployable role set (nixos, nix-darwin, or home-manager)"
            );
        }

        // Doc: "Roles activate in a fixed order: NixOS (or nix-darwin) first,
        // then home-manager. home-manager depends on the user existing on the
        // target, so running it after the system switch is the only order
        // that works for a fresh entry."
        if machine.has_nixos {
            self.deploy_nixos(name, machine).await?;
        }
        if machine.has_nix_darwin {
            self.deploy_nix_darwin(name, machine).await?;
        }
        if machine.has_home_manager {
            self.deploy_home_manager(name, machine).await?;
        }

        Ok(())
    }

    async fn deploy_nixos(&self, name: &str, machine: &MachineMeta) -> Result<()> {
        let host = machine.target.host.as_deref().ok_or_else(|| {
            miette!(
                "machines.{name}.nixos requires target.host to be set — NixOS deploys always go over SSH."
            )
        })?;
        let target = SshTarget::parse(host)
            .wrap_err_with(|| format!("Failed to parse machines.{name}.target.host"))?;

        // Intentionally no wrap_err on the build call: if the inner error is
        // a missing-input hint (`devenv inputs add disko …`) or a disko
        // evaluation failure, we want the user to see it directly in the
        // final per-machine failure report rather than a generic
        // "Failed to build" wrapper that hides the hint.
        let toplevel = self.build_machine_role(name, "nixos").await?;

        self.nix_copy(&target, &toplevel, &machine.target.ssh_opts)
            .await
            .wrap_err_with(|| format!("Failed to copy NixOS closure to {host}"))?;

        let toplevel_str = toplevel.display().to_string();
        // NixOS activation: swap the system profile to the new toplevel, then
        // run the toplevel's switch-to-configuration. Doing both in a single
        // remote shell reduces round trips and matches what nixos-rebuild
        // switch --target-host does.
        let script = format!(
            "nix-env --profile /nix/var/nix/profiles/system --set {p} && \
             {p}/bin/switch-to-configuration switch",
            p = toplevel_str
        );
        self.ssh_run(&target, &machine.target.ssh_opts, &script)
            .await
            .wrap_err_with(|| format!("NixOS activation failed on {host}"))?;
        Ok(())
    }

    async fn deploy_nix_darwin(&self, name: &str, machine: &MachineMeta) -> Result<()> {
        let host = machine.target.host.as_deref().ok_or_else(|| {
            miette!(
                "machines.{name}.nix-darwin requires target.host to be set — nix-darwin deploys always go over SSH."
            )
        })?;
        let target = SshTarget::parse(host)
            .wrap_err_with(|| format!("Failed to parse machines.{name}.target.host"))?;

        // See `deploy_nixos` for why the build error isn't wrapped.
        let toplevel = self.build_machine_role(name, "nix-darwin").await?;

        self.nix_copy(&target, &toplevel, &machine.target.ssh_opts)
            .await
            .wrap_err_with(|| format!("Failed to copy nix-darwin closure to {host}"))?;

        // nix-darwin activation swaps the system profile and runs the
        // toplevel's `activate` script with the root HOME it expects. Normal
        // administrator targets use passwordless sudo; explicit root targets
        // run the same operations directly.
        let script = nix_darwin_activation_script(&toplevel);
        self.ssh_run(&target, &machine.target.ssh_opts, &script)
            .await
            .wrap_err_with(|| format!("nix-darwin activation failed on {host}"))?;
        Ok(())
    }

    async fn deploy_home_manager(&self, name: &str, machine: &MachineMeta) -> Result<()> {
        // See `deploy_nixos` for why the build error isn't wrapped.
        let activation_package = self.build_machine_role(name, "home-manager").await?;
        let activate = activation_package.join("activate");
        let activate_str = activate.display().to_string();

        match machine.target.host.as_deref() {
            Some(host) => {
                let target = SshTarget::parse(host)
                    .wrap_err_with(|| format!("Failed to parse machines.{name}.target.host"))?;
                self.nix_copy(&target, &activation_package, &machine.target.ssh_opts)
                    .await
                    .wrap_err_with(|| format!("Failed to copy home-manager closure to {host}"))?;
                self.ssh_run(&target, &machine.target.ssh_opts, &activate_str)
                    .await
                    .wrap_err_with(|| format!("home-manager activation failed on {host}"))?;
            }
            None => {
                // Local activation: run $activationPackage/activate in-process.
                let status = process::Command::new(&activate)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to spawn {activate_str}"))?;
                if !status.success() {
                    bail!("home-manager activation {activate_str} exited with {status}");
                }
            }
        }
        Ok(())
    }

    /// Build a single machine role using the existing `devenv build` path.
    async fn build_machine_role(&self, name: &str, role: &str) -> Result<PathBuf> {
        let attr = format!("machines.{name}.build.{role}");
        let mut results = self.build(std::slice::from_ref(&attr)).await?;
        let (_built_attr, path) = results.pop().ok_or_else(|| {
            miette!("devenv build {attr} produced no output paths (internal error)")
        })?;
        Ok(path)
    }

    async fn nix_copy(
        &self,
        target: &SshTarget,
        store_path: &Path,
        user_ssh_opts: &[String],
    ) -> Result<()> {
        let uri = target.nix_copy_uri();
        let mut cmd = process::Command::new("nix");
        cmd.arg("copy")
            .arg("--to")
            .arg(&uri)
            .arg(store_path)
            .env("NIX_SSHOPTS", nix_ssh_opts_env(user_ssh_opts))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd
            .status()
            .await
            .into_diagnostic()
            .wrap_err("Failed to spawn nix copy")?;
        if !status.success() {
            bail!(
                "nix copy --to {uri} {} exited with {status}",
                store_path.display()
            );
        }
        Ok(())
    }

    async fn ssh_run(
        &self,
        target: &SshTarget,
        user_ssh_opts: &[String],
        script: &str,
    ) -> Result<()> {
        let mut cmd = process::Command::new("ssh");
        for opt in ssh_opts_argv(user_ssh_opts) {
            cmd.arg(opt);
        }
        if let Some(port) = target.port() {
            cmd.arg("-p").arg(port.to_string());
        }
        cmd.arg(target.ssh_destination()).arg(script);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd
            .status()
            .await
            .into_diagnostic()
            .wrap_err("Failed to spawn ssh")?;
        if !status.success() {
            bail!(
                "ssh {} '{}' exited with {status}",
                target.ssh_destination(),
                script
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_host() {
        let t = SshTarget::parse("root@laptop.local").unwrap();
        assert_eq!(t.user.as_deref(), Some("root"));
        assert_eq!(t.host, "laptop.local");
        assert_eq!(t.port, None);
        assert_eq!(t.ssh_destination(), "root@laptop.local");
        assert_eq!(t.nix_copy_uri(), "ssh://root@laptop.local");
    }

    #[test]
    fn parse_user_host_port() {
        let t = SshTarget::parse("admin@host.example.com:2222").unwrap();
        assert_eq!(t.user.as_deref(), Some("admin"));
        assert_eq!(t.host, "host.example.com");
        assert_eq!(t.port, Some(2222));
        assert_eq!(t.ssh_destination(), "admin@host.example.com");
        assert_eq!(t.nix_copy_uri(), "ssh://admin@host.example.com:2222");
    }

    #[test]
    fn parse_ssh_uri() {
        let t = SshTarget::parse("ssh://root@192.0.2.10:22").unwrap();
        assert_eq!(t.user.as_deref(), Some("root"));
        assert_eq!(t.host, "192.0.2.10");
        assert_eq!(t.port, Some(22));
    }

    #[test]
    fn parse_no_user() {
        let t = SshTarget::parse("host.example.com").unwrap();
        assert_eq!(t.user, None);
        assert_eq!(t.host, "host.example.com");
        assert_eq!(t.ssh_destination(), "host.example.com");
        assert_eq!(t.nix_copy_uri(), "ssh://host.example.com");
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(SshTarget::parse("").is_err());
        assert!(SshTarget::parse("   ").is_err());
    }

    #[test]
    fn parse_rejects_bad_port() {
        assert!(SshTarget::parse("root@host:notaport").is_err());
        assert!(SshTarget::parse("root@host:999999").is_err());
    }

    #[test]
    fn parse_rejects_empty_host() {
        assert!(SshTarget::parse("root@").is_err());
        assert!(SshTarget::parse("root@:22").is_err());
    }

    #[test]
    fn ssh_opts_argv_places_configured_values_before_defaults() {
        let user = vec!["-o".to_string(), "IdentitiesOnly=yes".to_string()];
        let argv = ssh_opts_argv(&user);
        assert_eq!(argv[0], "-o");
        assert_eq!(argv[1], "IdentitiesOnly=yes");
        assert_eq!(argv[2], "-o");
        assert_eq!(argv[3], "StrictHostKeyChecking=accept-new");
        assert_eq!(argv[4], "-o");
        assert_eq!(argv[5], "ConnectTimeout=10");
    }

    #[test]
    fn sensitive_install_ssh_policy_cannot_be_weakened_by_machine_options() {
        let user = vec!["-o".to_string(), "StrictHostKeyChecking=no".to_string()];
        let configured = sensitive_install_ssh_opts(&user);
        let argv = ssh_opts_argv(&configured);

        assert_eq!(&argv[0..2], ["-o", "StrictHostKeyChecking=yes"]);
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["-o", "ClearAllForwardings=yes"])
        );
        let insecure_position = argv
            .iter()
            .position(|arg| arg == "StrictHostKeyChecking=no")
            .unwrap();
        assert!(insecure_position > 1);
    }

    #[test]
    fn every_selected_local_file_phase_enables_sensitive_ssh_policy() {
        let disko = HashSet::from([InstallPhase::Disko]);
        let install = HashSet::from([InstallPhase::Install]);

        assert!(install_transmits_local_files(&disko, true, false, false));
        assert!(install_transmits_local_files(&install, false, true, false));
        assert!(install_transmits_local_files(&install, false, false, true));
        assert!(!install_transmits_local_files(&disko, false, true, true));
        assert!(!install_transmits_local_files(&install, true, false, false));
    }

    #[test]
    fn shell_quote_escapes_values() {
        assert_eq!(shell_quote("/tmp/luks.key"), "'/tmp/luks.key'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("ada's key"), r"'ada'\''s key'");
        assert_eq!(shell_quote("/tmp/k; rm -rf /mnt"), "'/tmp/k; rm -rf /mnt'");
        assert_eq!(shell_quote("$(whoami)"), "'$(whoami)'");
    }

    #[test]
    fn nix_darwin_activation_escalates_non_root_targets() {
        let script = nix_darwin_activation_script(Path::new("/nix/store/test-darwin-system"));
        assert!(script.contains("if [ \"$(id -u)\" -eq 0 ]"));
        assert!(script.contains("sudo -H -- \"$nix_env\""));
        assert!(script.contains("sudo -H -- /usr/bin/env HOME=/var/root"));
        assert!(script.contains("/nix/var/nix/profiles/system"));
        assert!(script.contains("$p/activate"));
    }

    #[test]
    fn target_file_path_rejects_escape_and_relative_paths() {
        assert!(validate_target_file_path("/var/lib/app/key").is_ok());
        assert!(validate_target_file_path("relative/key").is_err());
        assert!(validate_target_file_path("/").is_err());
        assert!(validate_target_file_path("/var/lib/../../etc/shadow").is_err());
        assert!(validate_target_file_path("/var/lib/key\nname").is_err());
    }

    #[test]
    fn bootstrap_file_metadata_is_restricted() {
        assert!(validate_file_owner("0:0").is_ok());
        assert!(validate_file_owner("1000:100").is_ok());
        assert!(validate_file_owner("root:root").is_err());
        assert!(validate_file_owner("--reference=/etc/passwd").is_err());
        assert!(validate_file_mode("600").is_ok());
        assert!(validate_file_mode("0600").is_ok());
        assert!(validate_file_mode("u=rw").is_err());
        assert!(validate_file_mode("0888").is_err());
        assert!(validate_secret_file_mode("0400").is_ok());
        assert!(validate_secret_file_mode("0600").is_ok());
        assert!(validate_secret_file_mode("0640").is_ok());
        assert!(validate_secret_file_mode("0660").is_err());
        assert!(validate_secret_file_mode("0644").is_err());
        assert!(validate_secret_file_mode("0700").is_err());
        assert!(validate_secret_file_mode("4600").is_err());
    }

    #[test]
    fn nix_ssh_opts_env_joins_with_spaces() {
        let user = vec!["-o".to_string(), "IdentitiesOnly=yes".to_string()];
        let env = nix_ssh_opts_env(&user);
        assert_eq!(
            env,
            "-o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn install_file_receiver_supports_root_and_replaces_only_after_complete_payload() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use tokio::io::AsyncWriteExt;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("var/lib/app/key");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old-secret").unwrap();
        let root_metadata = std::fs::metadata(root.path()).unwrap();
        let owner = format!("{}:{}", root_metadata.uid(), root_metadata.gid());
        let script = install_file_receiver_script("/", target.to_str().unwrap(), "0600", &owner);

        let payload = b"complete\0binary\nsecret";
        let mut child = process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(format!("{}\n", payload.len()).as_bytes())
            .await
            .unwrap();
        stdin.write_all(payload).await.unwrap();
        stdin.shutdown().await.unwrap();
        drop(stdin);
        let output = child.wait_with_output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&target).unwrap(), payload);
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn install_file_receiver_rejects_truncation_and_preserves_old_value() {
        use std::os::unix::fs::MetadataExt;
        use tokio::io::AsyncWriteExt;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("var/lib/app/key");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old-secret").unwrap();
        let root_metadata = std::fs::metadata(root.path()).unwrap();
        let owner = format!("{}:{}", root_metadata.uid(), root_metadata.gid());
        let script = install_file_receiver_script(
            root.path().to_str().unwrap(),
            target.to_str().unwrap(),
            "0600",
            &owner,
        );

        let payload = b"partial";
        let mut child = process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(format!("{}\n", payload.len() + 5).as_bytes())
            .await
            .unwrap();
        stdin.write_all(payload).await.unwrap();
        stdin.shutdown().await.unwrap();
        drop(stdin);
        let status = child.wait().await.unwrap();

        assert!(!status.success());
        assert_eq!(std::fs::read(&target).unwrap(), b"old-secret");
        assert!(
            std::fs::read_dir(target.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".devenv-secret."))
        );
    }

    #[test]
    fn target_manifest_flattens_profiles_without_resolving_values() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("secretspec.toml");
        std::fs::write(
            &path,
            r#"
[project]
name = "machine-test"
revision = "1.0"
require_reason = false

[profiles.default]
BOOTSTRAP_KEY = { description = "bootstrap key", providers = ["remote"] }
UNRELATED = { description = "kept for composition semantics", required = false }

[profiles.production]
BOOTSTRAP_KEY = { description = "production bootstrap key" }

[profiles.development]
DEV_ONLY = { description = "development only" }

[providers]
remote = "env"
"#,
        )
        .unwrap();

        let manifest =
            target_secretspec_manifest(&path, "production", ["BOOTSTRAP_KEY".to_string()]).unwrap();
        let manifest = String::from_utf8(manifest).unwrap();
        let parsed: secretspec::Config = manifest.parse().unwrap();

        assert!(parsed.project.extends.is_none());
        assert!(parsed.scopes.is_none());
        assert!(parsed.profiles.contains_key("default"));
        assert!(parsed.profiles.contains_key("production"));
        assert!(!parsed.profiles.contains_key("development"));
        assert_eq!(
            parsed.profiles["default"].secrets["BOOTSTRAP_KEY"].as_path,
            Some(true)
        );
        assert_eq!(
            parsed.profiles["production"].secrets["BOOTSTRAP_KEY"].as_path,
            Some(true)
        );
        assert!(parsed.profiles["default"].secrets.contains_key("UNRELATED"));
    }

    #[test]
    fn target_manifest_rejects_undeclared_reference() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("secretspec.toml");
        std::fs::write(
            &path,
            r#"
[project]
name = "machine-test"
revision = "1.0"
require_reason = false

[profiles.default]
DECLARED = { description = "declared" }
"#,
        )
        .unwrap();

        let error = target_secretspec_manifest(&path, "default", ["MISSING".to_string()])
            .expect_err("an undeclared target secret must fail before installation");
        assert!(
            error
                .to_string()
                .contains("does not declare bootstrap secret")
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn target_resolver_installs_exact_bytes_and_cleans_temporary_files() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use tokio::io::AsyncWriteExt;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("var/lib/app/key");
        let resolver = root.path().join("fake-secretspec");
        std::fs::write(
            &resolver,
            "#!/bin/sh\nset -eu\nout=$(mktemp)\nprintf 'target\\000resolved\\nsecret' >\"$out\"\nprintf '%s\\n' \"$out\"\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&resolver).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&resolver, permissions).unwrap();
        let root_metadata = std::fs::metadata(root.path()).unwrap();
        let owner = format!("{}:{}", root_metadata.uid(), root_metadata.gid());
        let secrets = vec![BootstrapSecret {
            target: "/var/lib/app/key".to_string(),
            secret: "BOOTSTRAP_KEY".to_string(),
            owner,
            mode: "0600".to_string(),
        }];
        let script = target_secretspec_installer_script(
            root.path().to_str().unwrap(),
            &resolver,
            "server",
            "production",
            Some("env"),
            &secrets,
        );
        assert!(script.contains("--profile 'production'"));
        assert!(script.contains("--provider 'env'"));

        let manifest = b"non-secret manifest";
        let mut child = process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(format!("{}\n", manifest.len()).as_bytes())
            .await
            .unwrap();
        stdin.write_all(manifest).await.unwrap();
        stdin.shutdown().await.unwrap();
        drop(stdin);
        let output = child.wait_with_output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"target\0resolved\nsecret");
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn kexec_url_x86() {
        let url = kexec_url("x86_64-linux").unwrap();
        assert!(url.contains("x86_64-linux"));
        assert!(url.starts_with("https://"));
        assert!(url.contains("nixos-images"));
    }

    #[test]
    fn kexec_url_aarch64() {
        let url = kexec_url("aarch64-linux").unwrap();
        assert!(url.contains("aarch64-linux"));
    }

    #[test]
    fn kexec_url_unsupported() {
        assert!(kexec_url("aarch64-darwin").is_err());
        assert!(kexec_url("x86_64-darwin").is_err());
    }

    #[test]
    fn validate_machine_names() {
        assert!(validate_machine_name("web1").is_ok());
        assert!(validate_machine_name("db-primary").is_ok());
        assert!(validate_machine_name("_scratch").is_ok());
        assert!(validate_machine_name("").is_err());
        assert!(validate_machine_name("foo.bar").is_err());
        assert!(validate_machine_name("foo;rm").is_err());
        assert!(validate_machine_name("1foo").is_err());
    }

    #[test]
    fn validate_kexec_urls() {
        assert!(validate_kexec_url("https://example.com/image.tar.gz").is_ok());
        assert!(validate_kexec_url("http://example.com/a/b.tar.gz").is_ok());
        assert!(validate_kexec_url("file:///tmp/foo").is_err());
        assert!(validate_kexec_url("https://example.com/$(whoami)").is_err());
        assert!(validate_kexec_url("https://example.com/x;rm").is_err());
        assert!(validate_kexec_url("https://example.com/x y").is_err());
    }

    #[test]
    fn host_facts_parse_root() {
        let output = "user=0\nhas_tar=1\nhas_curl=1\n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, Some(0));
        assert!(facts.has_tar);
        assert!(facts.has_curl);
    }

    #[test]
    fn host_facts_parse_non_root() {
        let output = "user=1000\nhas_tar=1\nhas_curl=0\n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, Some(1000));
        assert!(facts.has_tar);
        assert!(!facts.has_curl);
    }

    #[test]
    fn host_facts_parse_empty_fails_closed() {
        let facts = HostFacts::parse("");
        assert_eq!(facts.uid, None);
        assert!(!facts.has_tar);
        assert!(!facts.has_curl);
    }

    #[test]
    fn host_facts_parse_malformed_uid() {
        let facts = HostFacts::parse("user=abc\nhas_tar=1\nhas_curl=1\n");
        assert_eq!(facts.uid, None);
    }

    #[test]
    fn host_facts_parse_extra_whitespace() {
        let output = "  user=0  \n  has_tar=1  \n  has_curl=1  \n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, Some(0));
        assert!(facts.has_tar);
        assert!(facts.has_curl);
    }

    #[test]
    fn resolve_builders_config_empty() {
        let meta = BTreeMap::new();
        assert!(resolve_builders_config(&meta).is_none());
    }

    #[test]
    fn resolve_builders_config_with_targets() {
        let mut meta = BTreeMap::new();
        meta.insert(
            "server".to_string(),
            MachineMeta {
                system: "x86_64-linux".to_string(),
                target: MachineTarget {
                    host: Some("root@server.example.com".to_string()),
                    ssh_opts: vec![],
                },
                has_nixos: true,
                has_nix_darwin: false,
                has_home_manager: false,
                kexec_image: None,
                kexec_post_ssh_port: None,
                copy_host_keys: false,
                secretspec: MachineSecretspec::default(),
                bootstrap_secrets: Vec::new(),
                extra_files: BTreeMap::new(),
                encryption_keys: BTreeMap::new(),
            },
        );
        meta.insert(
            "local".to_string(),
            MachineMeta {
                system: "x86_64-linux".to_string(),
                target: MachineTarget {
                    host: None,
                    ssh_opts: vec![],
                },
                has_nixos: false,
                has_nix_darwin: false,
                has_home_manager: true,
                kexec_image: None,
                kexec_post_ssh_port: None,
                copy_host_keys: false,
                secretspec: MachineSecretspec::default(),
                bootstrap_secrets: Vec::new(),
                extra_files: BTreeMap::new(),
                encryption_keys: BTreeMap::new(),
            },
        );
        let config = resolve_builders_config(&meta).unwrap();
        // Only the machine with target.host is included
        assert!(config.contains("ssh://root@server.example.com"));
        assert!(config.contains("x86_64-linux"));
        assert!(!config.contains("local"));
    }
}
