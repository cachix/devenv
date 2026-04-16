use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::cli::{DiskoMode, InstallPhase};

use cli_table::{Table, WithTitle};
use devenv_activity::{ActivityInstrument, activity};
use futures::stream::StreamExt;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::Deserialize;
use tokio::process;

use super::Devenv;

/// Default SSH options applied to every connection `devenv machines deploy`
/// opens, ahead of any user-supplied `target.sshOpts`. User options are
/// appended after, so they win on conflicts (doc: "last value wins").
const DEFAULT_SSH_OPTS: &[&str] = &[
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "ConnectTimeout=10",
];

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
struct MachineTarget {
    host: Option<String>,
    #[serde(rename = "sshOpts")]
    ssh_opts: Vec<String>,
}

/// Facts collected from a preflight SSH probe on the install target.
/// Parsed from key=value lines emitted by the probe script.
#[derive(Debug, Default)]
struct HostFacts {
    uid: u32,
    has_tar: bool,
    has_curl: bool,
}

impl HostFacts {
    fn parse(output: &str) -> Self {
        let mut facts = HostFacts::default();
        for line in output.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("user=") {
                facts.uid = val.parse().unwrap_or(u32::MAX);
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
/// Format: `ssh://user@host system - - - - -` (one line per builder),
/// joined by `;` for `NIX_CONFIG`.
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

/// Build the argv fragment for ssh options: defaults first, user opts last.
fn ssh_opts_argv(user_opts: &[String]) -> Vec<String> {
    let mut v: Vec<String> = DEFAULT_SSH_OPTS.iter().map(|s| s.to_string()).collect();
    v.extend(user_opts.iter().cloned());
    v
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
        self.assemble().await?;
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
        self.assemble().await?;

        let meta = self.load_machines_meta().await?;

        // Configure remote builders if requested (same mechanism as deploy).
        if use_machines_as_builders {
            if let Some(builders) = resolve_builders_config(&meta) {
                let existing = std::env::var("NIX_CONFIG").unwrap_or_default();
                let sep = if existing.is_empty() { "" } else { "\n" };
                // SAFETY: set_var is unsafe in Rust 2024 because env mutation
                // is not thread-safe. We are single-threaded at this point
                // (before spawning parallel machine jobs), and no other thread
                // reads NIX_CONFIG concurrently.
                unsafe {
                    std::env::set_var(
                        "NIX_CONFIG",
                        format!(
                            "{existing}{sep}builders = {builders}\nbuilders-use-substitutes = true"
                        ),
                    );
                }
            }
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
        }

        let concurrency = max_concurrent
            .unwrap_or(names.len())
            .max(1)
            .min(names.len());

        let jobs = names.iter().map(|name| {
            let machine = &meta[name];
            let target_label = machine.target.host.as_deref().unwrap_or("(no target)");
            let activity = activity!(
                INFO,
                operation,
                format!("Installing {name} ({target_label})")
            );
            async move {
                let outcome = async {
                    self.install_one_machine(name, machine, phases, disko_mode)
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
    ) -> Result<()> {
        let host = machine
            .target
            .host
            .as_deref()
            .expect("validated in machines_install");
        let pre_kexec_target = SshTarget::parse(host)?;
        let ssh_opts = &machine.target.ssh_opts;

        // Preflight: probe the target before any destructive work. The
        // probe is lightweight (one SSH round-trip) and catches the most
        // common misconfiguration — not running as root — before kexec
        // wipes the boot environment. Only runs when kexec is in the
        // phase set (if the user is resuming from a later phase, the
        // target is already in installer state and the probe is moot).
        if phases.contains(&InstallPhase::Kexec) {
            let act = activity!(INFO, operation, "Preflight check");
            async {
                self.install_preflight(name, &pre_kexec_target, ssh_opts)
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
                self.install_kexec(name, &pre_kexec_target, ssh_opts, machine)
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
            async { self.install_facter(name, &target, ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        // Encryption keys are dropped onto the installer BEFORE disko runs
        // so LUKS layouts with `passwordFile` can reference them.
        if phases.contains(&InstallPhase::Disko) && !machine.encryption_keys.is_empty() {
            let act = activity!(INFO, operation, "Copying encryption keys");
            async {
                self.install_encryption_keys(&target, ssh_opts, &machine.encryption_keys)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // Phase 3: disko
        if phases.contains(&InstallPhase::Disko) {
            let act = activity!(INFO, operation, "Partitioning (disko)");
            async {
                self.install_disko(name, &target, ssh_opts, disko_mode)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        // Phase 4: install
        if phases.contains(&InstallPhase::Install) {
            let act = activity!(INFO, operation, "Installing NixOS");
            async { self.install_nixos(name, &target, ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        // Extra files are copied after nixos-install (system is at /mnt)
        // Extra files and host keys are part of the install phase conceptually
        // (after nixos-install, before reboot).
        if phases.contains(&InstallPhase::Install) && !machine.extra_files.is_empty() {
            let act = activity!(INFO, operation, "Copying extra files");
            async {
                self.install_extra_files(&target, ssh_opts, &machine.extra_files)
                    .await
            }
            .in_activity(&act)
            .await?;
        }

        if phases.contains(&InstallPhase::Install) && machine.copy_host_keys {
            let act = activity!(INFO, operation, "Copying SSH host keys");
            async { self.install_copy_host_keys(&target, ssh_opts).await }
                .in_activity(&act)
                .await?;
        }

        // Phase 5: reboot
        if phases.contains(&InstallPhase::Reboot) {
            let act = activity!(INFO, operation, "Rebooting");
            async { self.install_reboot(&target, ssh_opts).await }
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

        if facts.uid != 0 {
            bail!(
                "machines.{name}: install requires root SSH access, but the \
                 current user on the target has uid {}. Either SSH in as root \
                 or configure root login on the target.",
                facts.uid
            );
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
            Some(custom) => custom.clone(),
            None => kexec_url(&machine.system)?,
        };
        let script = format!(
            "set -euo pipefail && \
             curl --fail -L {url} | tar xzf - -C /root && \
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
    ) -> Result<()> {
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
        Ok(())
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
            let contents = tokio::fs::read(local_source)
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to read encryption key {local_source}"))?;
            // Pipe the key via stdin to avoid it appearing in the process
            // command line or on the remote filesystem before chmod.
            let script = format!("cat > {target_path} && chmod 0600 {target_path}");
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
            let mut child = cmd.spawn().into_diagnostic()?;
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin
                    .write_all(&contents)
                    .await
                    .into_diagnostic()
                    .wrap_err("Failed to pipe encryption key to ssh")?;
                // drop stdin to close the pipe and let the remote `cat` finish
            }
            let status = child.wait().await.into_diagnostic()?;
            if !status.success() {
                bail!("Failed to copy encryption key to {target_path} on target");
            }
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
            let contents = tokio::fs::read(&file.source)
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to read {}", file.source))?;
            // Write to /mnt/<path> since the installed system is mounted there.
            let mnt_path = format!("/mnt{target_path}");
            let script = format!(
                "mkdir -p $(dirname {mnt_path}) && \
                 cat > {mnt_path} && \
                 chmod {mode} {mnt_path} && \
                 chown {owner} {mnt_path}",
                mode = file.mode,
                owner = file.owner,
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
            let mut child = cmd.spawn().into_diagnostic()?;
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(&contents).await.into_diagnostic()?;
            }
            let status = child.wait().await.into_diagnostic()?;
            if !status.success() {
                bail!("Failed to copy extra file to {mnt_path} on target");
            }
        }
        Ok(())
    }

    /// Preserve SSH host keys across a reinstall by copying them from
    /// the installer's `/etc/ssh/` to the installed system at `/mnt/etc/ssh/`.
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
        // reboot exits non-zero because the SSH connection drops. Ignore
        // the exit code; what matters is that the command was sent.
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
            if let Ok(status) = cmd.status().await {
                if status.success() {
                    return Ok(());
                }
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
        // When --use-machines-as-builders is set, configure Nix's remote
        // builder list from the machines metadata BEFORE assemble(), so the
        // FFI backend picks it up during initialization. Uses NIX_CONFIG
        // env var which Nix reads at startup.
        if use_machines_as_builders {
            // We need meta to resolve builders, but meta requires assemble.
            // Chicken-and-egg: assemble first (without builders), load meta,
            // then re-set env for subsequent build calls.
            self.assemble().await?;
            let meta = self.load_machines_meta().await?;
            if let Some(builders) = resolve_builders_config(&meta) {
                let existing = std::env::var("NIX_CONFIG").unwrap_or_default();
                let sep = if existing.is_empty() { "" } else { "\n" };
                // SAFETY: set_var is unsafe in Rust 2024 because env mutation
                // is not thread-safe. We are single-threaded at this point
                // (before spawning parallel machine jobs), and no other thread
                // reads NIX_CONFIG concurrently.
                unsafe {
                    std::env::set_var(
                        "NIX_CONFIG",
                        format!(
                            "{existing}{sep}builders = {builders}\nbuilders-use-substitutes = true"
                        ),
                    );
                }
            }
            // Continue with the existing meta
            let meta = meta; // rebind to make the borrow checker happy
            return self
                .machines_deploy_inner(names, max_concurrent, &meta)
                .await;
        }

        self.assemble().await?;
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
        // the outcome for each one. Pass `-j N` to cap how many machines
        // run at once; `-j 1` runs them strictly one at a time."
        //
        // Default cap = working_set.len() (every machine runs concurrently);
        // an explicit `--max-concurrent N` clamps it. `-j 1` gives
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
    async fn load_machines_meta(&self) -> Result<BTreeMap<String, MachineMeta>> {
        let json = self
            .nix
            .eval(&["devenv.config.machinesMeta"])
            .await
            .wrap_err("Failed to load machinesMeta from devenv.nix")?;
        serde_json::from_str(&json)
            .into_diagnostic()
            .wrap_err("Failed to parse machinesMeta JSON")
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

        let toplevel_str = toplevel.display().to_string();
        // nix-darwin activation: swap the system profile, then run the
        // toplevel's `activate` script. `HOME=/var/root` is required because
        // the darwin activation script calls `defaults` and launchd which
        // resolve paths relative to $HOME — without a stable value they
        // misbehave in subtle ways. Users still need to arrange root SSH or
        // passwordless sudo themselves; doc covers the TouchID caveat.
        let script = format!(
            "nix-env --profile /nix/var/nix/profiles/system --set {p} && \
             HOME=/var/root {p}/activate",
            p = toplevel_str
        );
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
        let mut results = self.build(&[attr.clone()]).await?;
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
    fn ssh_opts_argv_prepends_defaults() {
        let user = vec!["-o".to_string(), "IdentitiesOnly=yes".to_string()];
        let argv = ssh_opts_argv(&user);
        assert_eq!(argv[0], "-o");
        assert_eq!(argv[1], "StrictHostKeyChecking=accept-new");
        assert_eq!(argv[2], "-o");
        assert_eq!(argv[3], "ConnectTimeout=10");
        // User opts are appended after so "last value wins" semantics on
        // conflicts matches the SSH option parser's behaviour.
        assert_eq!(argv[4], "-o");
        assert_eq!(argv[5], "IdentitiesOnly=yes");
    }

    #[test]
    fn nix_ssh_opts_env_joins_with_spaces() {
        let user = vec!["-o".to_string(), "IdentitiesOnly=yes".to_string()];
        let env = nix_ssh_opts_env(&user);
        assert_eq!(
            env,
            "-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -o IdentitiesOnly=yes"
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
    fn host_facts_parse_root() {
        let output = "user=0\nhas_tar=1\nhas_curl=1\n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, 0);
        assert!(facts.has_tar);
        assert!(facts.has_curl);
    }

    #[test]
    fn host_facts_parse_non_root() {
        let output = "user=1000\nhas_tar=1\nhas_curl=0\n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, 1000);
        assert!(facts.has_tar);
        assert!(!facts.has_curl);
    }

    #[test]
    fn host_facts_parse_empty() {
        let facts = HostFacts::parse("");
        // Defaults: uid=0 (u32 default), has_tar=false, has_curl=false
        assert_eq!(facts.uid, 0);
        assert!(!facts.has_tar);
        assert!(!facts.has_curl);
    }

    #[test]
    fn host_facts_parse_extra_whitespace() {
        let output = "  user=0  \n  has_tar=1  \n  has_curl=1  \n";
        let facts = HostFacts::parse(output);
        assert_eq!(facts.uid, 0);
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
