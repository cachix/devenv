---
title: "Machines"
---

:::caution[Experimental]

The machines interface is new and may still change before it is declared stable.
:::

The `machines` option lets a single `devenv.nix` declare one or more full system configurations (NixOS hosts, nix-darwin machines, or home-manager users) alongside the dev shell that builds them. Each entry is keyed by name and carries an optional `system`, an optional `target` submodule, and one or more of `nixos`, `nix-darwin`, or `home-manager` modules.

:::note[Current implementation]

The current machines implementation includes:

- **Build targets** work: `devenv build machines.<name>` realises every role a machine declares; `devenv build machines.<name>.build.<role>` builds a single role.
- **`devenv machines info [name...]`** lists every machine with its system, target, and configured roles. Read-only; does not force `build.*`.
- **`devenv machines deploy`** works for all three roles — **NixOS**, **nix-darwin**, and **home-manager**. Each named machine is built locally, copied to `target.host` over SSH via `nix copy`, and activated. The fixed per-machine role order is NixOS → nix-darwin → home-manager. `target.host`-less home-manager machines activate in-process on the current host. nix-darwin activation sets `HOME=/var/root` automatically; the TouchID/sudo caveat under [nix-darwin](#nix-darwin) still applies because that's a target-side concern devenv can't work around.
- **Parallel deploys** are on by default. The working set runs concurrently under one top-level activity, each machine pipelined independently (build → copy → activate). Pass **`--max-concurrent N`** to cap how many machines run at once; `--max-concurrent 1` forces strictly sequential ordering, matching the doc's "watching a single host closely" case. `-j` is not used here because it's already the global Nix `max-jobs` flag. A failure on one machine does not stop the others: every machine finishes its own pipeline, and the run exits non-zero if any failed.
- **Bulk `devenv machines deploy`** (no arguments) enumerates every entry in the attrset and deploys each one that has `target.host` set. Entries without a host — including local-only home-manager — are reported through the activity layer as `skipped`, which is how you opt them out of bulk runs. To still deploy a `target.host`-less home-manager entry, name it explicitly.
- **`devenv machines install <name1> [<name2> ...]`** runs the full install pipeline: preflight probe → kexec into NixOS installer → nixos-facter hardware probe (writes `.machines/<name>/facter.json`) → disko partitioning → nix copy + nixos-install → reboot. Supports `--phases`, `--stop-after-disko`, `--no-reboot`, `--disko-mode disko|format|mount`, and `--max-concurrent N`. Install-time encryption keys (`install.encryptionKeys`) are piped to the target before disko; extra files (`install.extraFiles`), SecretSpec bootstrap files (`install.secrets`), and SSH host key preservation (`install.copyHostKeys`) happen after nixos-install, before reboot. Custom kexec images are supported via `install.kexec.image` and `install.kexec.postSshPort`.
- **`--use-machines-as-builders`** configures the live C-Nix remote builder settings from machines metadata (every machine with `target.host` becomes a candidate builder for its `system`) and enables builder substitutes. It is available on both `deploy` and `install` and currently requires the C-Nix backend.
:::

:::note[Renamed from `configurations`]

Earlier versions exposed this option as `configurations`. It was renamed to `machines`, and the old name still works via `lib.mkRenamedOptionModule`, so existing configs keep evaluating.
:::

## Defining a machine

A minimal NixOS machine:

```nix title="devenv.nix"
{ ... }: {
  machines.laptop = {
    system = "x86_64-linux";
    target.host = "root@laptop.local";
    nixos = {
      networking.hostName = "laptop";
      services.openssh.enable = true;
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 ..." ];
    };
  };
}
```

`target` is a submodule describing the SSH destination used by install and deploy. It exposes two fields:

- `target.host` is an optional string. Set it to an SSH URI in one of these forms: `user@host`, `user@host:port`, or the full `ssh://user@host:port`. Omitting `target.host` means "activate in process on the current host" and is only valid for `home-manager`. Note that setting it to `"localhost"` is not the same as omitting it; `"localhost"` still routes through SSH.
- `target.sshOpts` is an optional list of SSH option tokens. They are applied before devenv's defaults for direct SSH and `nix copy` connections because OpenSSH keeps the first value it obtains for most settings; use them to override the default set below. Nix remote-builder connections are the exception described under [SSH config and the nix-daemon](#ssh-config-and-the-nix-daemon).

One `target` is shared across `nixos`, `nix-darwin`, and `home-manager` on the same machine, so if you declare more than one role on a single entry they all land on the same host.

### SSH defaults

Every direct SSH or `nix copy` connection devenv opens for install and deploy uses these defaults:

- `StrictHostKeyChecking=accept-new`, so parallel runs do not deadlock on interactive host key prompts on first contact. Pre-populate `~/.ssh/known_hosts` if you want stricter checking.
- A bounded `ConnectTimeout`, so an unreachable host in a bulk run fails fast instead of hanging the whole batch.

Override these by adding your own `-o` pair to `target.sshOpts`; the configured value is passed first and therefore wins.

An install that transmits local file payloads through `install.secrets`, `install.encryptionKeys`, or `install.extraFiles` is deliberately stricter from its first SSH connection: it forces `StrictHostKeyChecking=yes` and disables forwarding, agent forwarding, X11 forwarding, local commands, and TTY allocation. These settings cannot be weakened through `target.sshOpts`. Add every host identity used during the run to `known_hosts` before starting—including both the original system and kexec installer identities if they differ. A per-machine file can be selected with `[ "-o" "UserKnownHostsFile=/absolute/path" ]`. Unknown identities fail before preflight or destructive work.

### Machine names

Machine names must start with a letter or underscore and contain only letters, digits, underscores, and hyphens. Dotted names such as `machines."host.example.com"` are not accepted because the CLI uses names in Nix attribute paths.

## NixOS

NixOS deployment is inspired by [nixos-anywhere](https://github.com/nix-community/nixos-anywhere). It uses the same SSH + kexec approach for fresh installs and relies on [disko](https://github.com/nix-community/disko) for declarative partitioning.

### Disk layout with disko

Add the `disko` input to `devenv.yaml`, then declare the disk layout inside `machines.<name>.nixos`. devenv imports `disko.nixosModules.disko` automatically:

```yaml title="devenv.yaml"
inputs:
  disko:
    url: github:nix-community/disko
    inputs:
      nixpkgs:
        follows: nixpkgs
```

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@192.0.2.10";
    nixos = {
      disko.devices.disk.main = {
        # Use a stable path under /dev/disk/by-id; kernel names like /dev/sda
        # reorder across reboots and live USB sessions and have wiped the
        # wrong disk for real users. Look up the id on the target with
        # `ls -l /dev/disk/by-id`.
        device = "/dev/disk/by-id/ata-REPLACE-ME";
        type = "disk";
        content = {
          type = "gpt";
          partitions = {
            ESP = {
              size = "512M";
              type = "EF00";
              content = {
                type = "filesystem";
                format = "vfat";
                mountpoint = "/boot";
              };
            };
            root = {
              size = "100%";
              content = {
                type = "filesystem";
                format = "ext4";
                mountpoint = "/";
              };
            };
          };
        };
      };
      boot.loader.systemd-boot.enable = true;
      services.openssh.enable = true;
    };
  };
}
```

A few things the example bakes in that are worth knowing before you copy it:

- **Use `/dev/disk/by-id/...`, not `/dev/sda`.** Kernel device names reorder across reboots and live USB sessions. This is the footgun that has wiped the wrong disk for real users. Stable paths under `/dev/disk/by-id` or `/dev/disk/by-path` are the only safe choice for `install`.
- **UEFI only.** The example pairs `systemd-boot` with an `EF00` ESP, which silently requires UEFI firmware. BIOS and legacy boot hosts (common on older dedicated servers and some VPS providers) need a GRUB setup with a BIOS boot partition instead.
- **The 512M ESP is intentional.** It is sized to fit several systemd-boot generations without running out of space as kernels accumulate. disko does not support resizing partitions in place, so pick a sensible size at install time.
- **The layout is effectively immutable.** Changing partition shape, filesystem type, or encryption setup after install is not an in place operation; plan to back up, reinstall, and restore.
- **ZFS has an extra step.** If you create a non root ZFS pool in your layout, add it to `boot.zfs.extraPools` in the same `nixos` module. Otherwise the pool is not imported at boot and the target hangs at stage 1 on first reboot.

### Hardware detection with nixos-facter

devenv uses [nixos-facter](https://github.com/nix-community/nixos-facter) to capture each machine's hardware profile. facter produces a `facter.json` report that replaces the traditional `hardware-configuration.nix`: initrd kernel modules, CPU microcode, GPU drivers, and network controller modules are all derived from the report by the facter NixOS module, which devenv imports automatically for every `nixos` entry.

On first `install` devenv probes the target over SSH after kexec and before disko, writes the report to `.machines/<name>/facter.json` in your project root, and stages it with `git add --intent-to-add`. The directory is dotted so it stays out of the way, but the files inside **must be committed to git**. Without a committed report, teammates and CI cannot build the machine closure without first reaching the live target, and every fresh checkout would have to re probe hardware.

Each machine gets its own subdirectory keyed by the machine name. facter reports contain machine specific identifiers (disk UUIDs, MAC addresses, serial numbers), so sharing a report across hosts is almost always wrong; the per machine layout is intentional.

Override the default path per machine with `machines.<name>.hardware.facter`:

```nix title="devenv.nix"
{ ... }: {
  machines.web1 = {
    system = "x86_64-linux";
    target.host = "root@web1.example.com";
    hardware.facter = ./hardware/web1.json;
    nixos = {
      # ...
    };
  };
}
```

Set `hardware.facter = null;` to opt a machine out entirely, for example when you maintain a hand written `hardware-configuration.nix` instead. `nix-darwin` and `home-manager` entries ignore the option; only `nixos` entries carry hardware reports.

### Installing on a fresh host

Install over SSH. The target is read from `machines.<name>.target.host`:

```sh
$ devenv machines install server
```

devenv builds the NixOS toplevel, connects to the target over SSH, kexecs into a minimal installer, runs disko to partition and format, copies the closure, installs the bootloader, and reboots. The remote only needs SSH access and a running Linux kernel. No pre-existing NixOS is required.

Install requires an explicit name. Because it wipes disks, running `devenv machines install` with no arguments is an error rather than "install everything". You can still install more than one host in a single invocation by naming them, and the named hosts are installed in parallel:

```sh
$ devenv machines install server1 server2
```

Pass `--max-concurrent N` to cap how many hosts install at once. `--max-concurrent 1` runs them one at a time, which is useful for a controlled rollout or for watching a single host closely. (`-j` remains the global Nix `max-jobs` flag.)

#### Preflight for install

Before running `install` against a real target, confirm:

- **Root SSH is enabled on the remote installer.** Install logs in as `root` and does not escalate with `sudo`. Many cloud minimal images disable root login by default; either pick an image that allows it or run `passwd root` on the console before invoking install.
- **The target kernel can kexec.** kexec is how devenv pivots into the NixOS installer without requiring pre installed NixOS. Some older ARM boards (early Raspberry Pi revisions) and a few locked down cloud kernels refuse mid kexec. If you hit this, boot the NixOS minimal ISO manually and use `devenv machines deploy` instead.
- **The target has roughly 1 GB of free RAM.** The kexec'd installer holds the next system closure in memory before writing it to disk, and smaller VPS instances (512 MB, 1 GB) have OOMed mid run.
- **TCP 22 is reachable from the host running devenv.** Ordinary installs add the target to `known_hosts` on first contact because `StrictHostKeyChecking=accept-new` is the default. Installs that transmit local files require a pre-pinned identity as described under [SSH defaults](#ssh-defaults).

The disko layout describes the filesystems and you own `boot.loader.*` directly. Everything else the target needs (initrd kernel modules, CPU microcode, GPU drivers, network controller modules) comes from the nixos-facter report that devenv generates on first install; see [Hardware detection with nixos-facter](#hardware-detection-with-nixos-facter).

:::caution[Install wipes disks]

`devenv machines install` partitions and formats the target according to your disko layout. Any data on the listed devices will be destroyed. There is no confirmation prompt, so only name hosts you actually mean to wipe. `devenv machines deploy` does not touch disks.

There is no dry run and no resume. A failed install is recovered by re running `install`, which re wipes the disks. Test disko layouts with `devenv build machines.<name>` or a disko VM test before pointing `install` at real hardware.
:::

If an install appears to hang right after the kexec phase, the most common cause is that DHCP handed the kexec'd installer a different IP than the one you started the run against. Check the console or DHCP lease table for the installer's new address; use a static address or a MAC reservation to avoid the problem on subsequent runs. See [Troubleshooting](#troubleshooting) for more symptoms.

### Updating an existing host

For machines that are already running NixOS, use `deploy`:

```sh
$ devenv machines deploy server
```

This is equivalent to `nixos-rebuild switch --target-host`. It builds the closure locally, copies it over, and activates. Activation is always `switch`; devenv does not expose a `boot` style deferred activation.

## nix-darwin

nix-darwin machines follow the same shape. Set `system` to a Darwin value and provide a `nix-darwin` module:

```nix title="devenv.nix"
{ ... }: {
  machines.mac = {
    system = "aarch64-darwin";
    target.host = "admin@mac.local";
    nix-darwin = { pkgs, ... }: {
      environment.systemPackages = [ pkgs.vim ];
      services.nix-daemon.enable = true;
    };
  };
}
```

Deploy with:

```sh
$ devenv machines deploy mac
```

This is equivalent to `darwin-rebuild switch` over SSH. There is no `install` equivalent for darwin, since Apple ships the OS. Consequently, the `install.*` bootstrap settings—including SecretSpec—are NixOS-only.

Two macOS specific things to know before deploying:

- **sudo over SSH and TouchID.** On macOS, admin user `sudo` can be gated on TouchID, which cannot be satisfied by devenv's non-interactive SSH command. Either SSH in as `root` (which requires enabling root login and is not recommended by Apple), or configure passwordless `sudo` for the deploy user in the nix-darwin module (`security.sudo.extraConfig` or the equivalent). devenv invokes `sudo -H` automatically for non-root targets.
- **Activation expects `HOME=/var/root`.** darwin activation uses launchd and `defaults` which resolve paths relative to `$HOME`, and without an explicit `HOME` set the activation misbehaves in subtle ways. devenv sets `HOME=/var/root` for the remote activation step so you do not need to; this is documented here so you are not surprised if you look at the remote command line.

## home-manager

home-manager is the one case where `target.host` is genuinely optional. Leave it unset for a local activation, or set it for remote:

```nix title="devenv.nix"
{ ... }: {
  # Local activation: no target.host, runs on the current machine.
  machines.me = {
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      programs.git.enable = true;
    };
  };

  # Remote activation: target.host set, runs over SSH.
  machines.workstation = {
    target.host = "jdoe@workstation.lan";
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      programs.git.enable = true;
    };
  };
}
```

Activation runs `home-manager switch`:

```sh
$ devenv machines deploy me
$ devenv machines deploy workstation
```

## Combining roles on one machine

A single entry can carry more than one role. Both modules apply to the same host, using the same `target`:

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@192.0.2.10";

    nixos = {
      services.openssh.enable = true;
      users.users.jdoe = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
      };
    };

    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      programs.git.enable = true;
    };
  };
}
```

`devenv machines install server` provisions only the NixOS role. After the target reboots, `devenv machines deploy server` activates the declared roles in order, including the home-manager configuration for `jdoe`.

During `deploy`, roles activate in a fixed order: NixOS (or nix-darwin) first, then home-manager. home-manager depends on the user existing on the target, so running it after the system switch is the only order that works for a fresh entry.

When the shared SSH target logs in as `root` or as a different administrator, devenv runs the home-manager activation as `home.username` with `HOME` set to `home.homeDirectory`. It uses `runuser` for root sessions and falls back to passwordless `sudo` for administrator sessions.

The two roles are not transactional: a partial success is possible and is reported as a failure. If NixOS activation fails, home-manager is not attempted and the entry is reported as failed. If NixOS succeeds but home-manager fails, the NixOS switch stays applied, the entry is still reported as failed, and bulk deploys exit nonzero. Re running `deploy` is safe: NixOS activation is idempotent, so only home-manager will do work on the retry.

A couple of footguns to know about when combining roles on one entry:

- **Pin `users.users.<name>.uid` and `users.groups.<name>.gid` explicitly.** On a fresh install the user is created from the NixOS configuration, and if the uid is not pinned, a later NixOS change that renumbers it will silently break ownership of every file home-manager wrote under that user. Pinning both values up front avoids this class of bug entirely.
- **Make sure `home.homeDirectory` matches the path NixOS actually creates.** A `/Users/jdoe` value copy pasted from a nix-darwin example will activate cleanly on a Linux target and write files to the wrong place, because home-manager does not cross check the NixOS user's `home` attribute. Keep the two in sync, or factor them through a shared `let` binding.

## Secrets

Use [sops-nix](https://github.com/Mic92/sops-nix) or [agenix](https://github.com/ryantm/agenix) inside your `nixos` or `home-manager` module for steady-state secret management. Both integrate cleanly with the standard NixOS activation that `deploy` runs.

:::caution[Do not inline secrets into Nix modules]

Any literal string you embed directly into a module, for example `environment.etc."foo".text = "hunter2";` or a password baked into a `services.*` option, lands in the world readable `/nix/store`. Every user on the target can read it, and the value is also copied to every substituter the target trusts. Always route runtime secrets through sops-nix or agenix (or equivalent) rather than embedding them.
:::

### Bootstrapping from SecretSpec

Runtime secret stores still need an initial credential—for example the age identity that lets sops-nix decrypt secrets on first boot. `install.secrets` writes those bootstrap files from the active [SecretSpec](/integrations/secretspec/) profile after `nixos-install` and before reboot:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: keyring
  profile: production
```

```toml title="secretspec.toml"
[project]
name = "infrastructure"
revision = "1.0"
require_reason = false

[profiles.production]
WEB1_AGE_KEY = { description = "sops age identity for web1" }
```

```nix title="devenv.nix"
machines.web1 = {
  target.host = "root@web1.example.com";

  install.secrets."/var/lib/sops-nix/key.txt" = {
    secret = "WEB1_AGE_KEY";
    owner = "0:0";
    mode = "0600";
  };

  nixos = {
    sops.age.keyFile = "/var/lib/sops-nix/key.txt";
    # ...
  };
};
```

The attribute name is an absolute path in the installed system. `secret` is a name from the selected SecretSpec profile. The default execution mode is `local`: values are resolved and materialized on the workstation before any selected machine starts preflight or destructive work, then streamed to SSH on stdin. They do not appear in machine metadata, process arguments, remote scripts, or Nix store paths. During `machines install`, devenv also withholds resolved SecretSpec values from Nix evaluation—`config.secretspec.secrets` is empty for that invocation—so imported modules cannot accidentally interpolate a bootstrap credential into a derivation. Profile and provider metadata remain available.

All local install file transfers—SecretSpec values, encryption keys, and extra files—use a byte-counted frame. The receiver writes under `umask 077` to a temporary file in the destination directory, rejects truncation, applies ownership and permissions, syncs it, and atomically renames it into place. A failed transfer leaves an existing destination unchanged and removes the temporary file. Payload copies use zeroize-on-drop memory, and core dumps are disabled for the install process and its children.

File ownership must use numeric `uid:gid`, because the live installer does not know users declared only in the new NixOS system. Secret modes cannot contain special or execute bits, group write, or permissions for other users; `0400`, `0600`, and `0640` are accepted.

SecretSpec entries with `as_path = true` are supported: devenv reads the retained temporary file and sends its contents, not its local filename. When installing several machines, every entry can select different secret names, or several entries can reference one shared secret. The provider and profile are global to the invocation; override them with `--secretspec-provider` and `--secretspec-profile`.

#### Resolving on the target

To keep provider credentials and resolved values off the workstation, select target execution for a machine:

```nix title="devenv.nix"
machines.web1 = {
  target.host = "root@web1.example.com";

  install.secretspec = {
    execution = "target";
    profile = "production";
  };

  install.secrets."/var/lib/sops-nix/key.txt" = {
    secret = "WEB1_AGE_KEY";
    owner = "0:0";
    mode = "0600";
  };

  nixos = {
    sops.age.keyFile = "/var/lib/sops-nix/key.txt";
    # ...
  };
};
```

In this mode, devenv parses `secretspec.toml` but never asks the workstation's provider for this machine's values. It flattens manifest inheritance, forces only the requested entries to file output, and sends the self-contained declaration manifest—without any fetched provider values—to the installer over SSH. Profiles, scopes, provider aliases, composed declarations, and validation policy are retained so the target runs normal SecretSpec resolution rather than a devenv-specific provider implementation. Committed SecretSpec defaults remain part of that manifest, so do not use defaults for values the workstation must not know. The target-architecture `secretspec` executable is included in the NixOS system closure; after `nixos-install`, the live installer runs it directly from that closure and asks only for each `install.secrets` reference. Resolved bytes stay in a private target-side temporary directory and are atomically installed beneath `/mnt`.

Target execution deliberately does not inherit the workstation's SecretSpec provider or profile. When `provider` is `null`, devenv omits `--provider`, preserving manifest provider references, target environment selection, and target-global SecretSpec configuration. An explicit value is an opt-in override and, like SecretSpec's own `--provider`, applies to the complete resolution including references. Treat this option as a non-secret provider selector: it is machine metadata and becomes part of the remote command, so credentials belong in target-side provider configuration rather than the override. `profile` behaves the same way: when omitted, SecretSpec selects it from the target environment or target-global configuration and ultimately falls back to `default`. Set it explicitly when the machine declaration should enforce a profile. Devenv also removes workstation-side `SECRETSPEC_*` selector variables from the `ssh` child, preventing an OpenSSH `SendEnv` rule from silently restoring the workstation selection.

Target-only bootstrapping does not require `secretspec.enable` in `devenv.yaml`; only the committed `secretspec.toml` declaration is required on the workstation. Global `--secretspec-provider` and `--secretspec-profile` flags affect local execution, not target execution.

Devenv never copies or explicitly forwards provider credentials. They must already be available to the **live installer** through workload identity, instance metadata, its SecretSpec global configuration, or another provider-native mechanism. Credentials that become available only after boot are not usable by this installer-time mode. A short-lived, machine-scoped bootstrap credential can also be provisioned independently, but forwarding the workstation's long-lived credential defeats the isolation this mode provides. OpenSSH can independently forward arbitrary environment variables through user or system `SendEnv`/`SetEnv` configuration; audit those settings if provider credentials exist in the workstation environment.

The resolver is the SecretSpec executable bundled with the same devenv release as the machines module, built for the target architecture. It does not use `pkgs.secretspec`, so an older or independently versioned nixpkgs package cannot drift from the manifest implementation in devenv. Providers that require helper commands can add target-architecture packages:

```nix
install.secretspec = {
  execution = "target";
  extraPackages = targetPkgs: [ targetPkgs.sops targetPkgs.pass ];
};
```

The dedicated launcher references the bundled resolver's exact store path, so an unrelated system package cannot win a `bin/secretspec` collision. `extraPackages` enter a private resolver `PATH`, making provider helper commands available from the live installer without adding them to the installed system's global command namespace.

Target execution protects secrets from an honest workstation process, but a workstation that controls a malicious NixOS configuration could still install software that exfiltrates values after the target fetches them. Removing that deeper trust requires independently verified or signed system closures and, where appropriate, Secure or Measured Boot with provider-side attestation.

`install.secrets` is intentionally bootstrap-only. It runs as part of the `install` phase, after `install.extraFiles` (so a SecretSpec file wins if both target the same path) and before `install.copyHostKeys` and reboot. It is not reapplied by `devenv machines deploy`; use sops-nix, agenix, or another runtime mechanism for rotation.

LUKS root unlock keys are a separate case: they are consumed by disko at install time, not at runtime. Point disko's `passwordFile` or `settings.keyFile` at an installer path and map that path to a local source with `install.encryptionKeys`. devenv reads the source on the workstation and streams it to the live installer before disko runs, without putting it in the Nix store.

## Deploying multiple machines

`devenv machines deploy` without arguments deploys every machine in the attrset that has `target.host` set. Machines without a host are skipped with an informational line, which is how you opt a local-only home-manager entry out of bulk deploys:

```sh
$ devenv machines deploy
server       (root@192.0.2.10)    deploying... ok
mac          (admin@mac.local)    deploying... ok
me           (no target)          skipped
```

You can also pass several names explicitly:

```sh
$ devenv machines deploy server1 server2 workstation
```

Bulk behavior applies to `deploy` only. `install` always requires explicit names, since it wipes disks.

### Filtering machines

There are no CLI tags, groups, or label selectors on `machines` itself. If you want to roll out a subset (for example staging before production), the supported options are:

- Pass the names explicitly on the command line, as above.
- Declare machines inside [profiles](/profiles/) and activate the profile for the run:

    ```nix title="devenv.nix"
    {
      profiles = {
        staging.module = {
          machines.web1 = { system = "x86_64-linux"; target.host = "root@staging-web1"; nixos = { ... }; };
          machines.web2 = { system = "x86_64-linux"; target.host = "root@staging-web2"; nixos = { ... }; };
        };

        production.module = {
          machines.web1 = { system = "x86_64-linux"; target.host = "root@prod-web1"; nixos = { ... }; };
        };
      };
    }
    ```

    Then scope a run to one tier with `devenv --profile staging machines deploy`. Profiles compose with hostname and user profiles, so the same mechanism lets you gate machines on the operator or the workstation running the deploy.

Keep this at the Nix layer rather than wrapping `devenv machines deploy` in a shell loop: the bulk run reports a single summary and a single exit code, which is the point of running it as one command.

### Parallelism and failure handling

Machines are deployed in parallel by default. Each machine runs its own build, copy, and activation pipeline independently, and the summary printed at the end shows the outcome for each one. Pass `--max-concurrent N` to cap how many machines run at once; `--max-concurrent 1` runs them strictly one at a time, which helps when you want predictable ordering or when debugging a specific host. (`-j` is not used because it's already the global Nix `max-jobs` flag.) The same flag also caps concurrent `install` jobs.

Build and activation are interleaved: each machine builds its own closure immediately before copying and activating, not all closures up front. A failure on one machine does not stop the others. Machines that already activated stay applied, machines still running finish their own pipelines, and every outcome lands in the final summary. `devenv machines deploy` exits nonzero if any machine in the run failed. If you want every closure built before any deploy runs, build each machine explicitly with `devenv build machines.<name>` first and only then invoke `devenv machines deploy`.

If a host in a bulk run is unreachable, devenv still builds its closure locally and only discovers the problem at the copy step; that machine is marked failed in the summary and the other machines continue. There is no precheck that probes every target before building.

Interrupting a parallel run with Ctrl-C leaves in flight machines in whatever state they happened to reach: a machine mid copy stops mid copy, a machine mid activation may be half switched. devenv does not roll those back. Re running `deploy` is the recovery path and is idempotent for the machines that finished cleanly.

## Progress and logs

`install` and `deploy` report progress through devenv's activity tracing system. In TUI mode each machine shows up as its own tracked operation, with distinct phases for build, copy, and activation so you can see which step a stuck run is waiting on. When the TUI is disabled (for example, when tracing is routed to stderr), the same events are emitted as log lines, keeping CI output readable.

For bulk deploys, a summary with one line per machine is printed at the end, matching the format shown above.

## Cross-platform deploys

A single `devenv.nix` can declare machines for different systems, and `devenv machines deploy` handles each one independently. The interesting question is where the closure for a machine gets built when its `system` does not match the host running devenv.

By default, devenv builds locally. If the current host can't realize a derivation for the target's `system`, the build fails loudly rather than silently falling back to building somewhere else.

Pass `--use-machines-as-builders` to change that. With the flag set, devenv adds every entry with `target.host` to Nix's remote-builder list, tagged with that entry's `system`. Nix can then route a build to a machine whose system matches the derivation—for example, an x86_64-linux builder can build for an x86_64-linux target while devenv itself runs on aarch64-darwin. The flag applies to builds performed by that `machines deploy` or `machines install` invocation; plain `devenv build` does not expose this flag.

```sh
$ devenv machines deploy --use-machines-as-builders
```

`install` is more constrained than `deploy`: the freshly kexec'd target has no usable Nix yet, so it can never be its own builder. Cross architecture installs therefore need either a local host whose `system` matches, or `--use-machines-as-builders` with another machine in the attrset that matches. Without one of those, run `devenv machines install` from a host whose architecture matches the fresh target. This is the same constraint nixos-anywhere operates under.

### Builder trust

The orchestrator (the host running `devenv machines deploy`) copies the closure built on the peer builder into the target. For that copy to succeed, the target's nix-daemon has to accept paths that were not signed by a locally trusted key. You have two ways to satisfy that requirement:

- Add the invoking user to `nix.settings.trusted-users` on the target, for example `nix.settings.trusted-users = [ "root" "@wheel" ];`. Trusted users are allowed to push unsigned paths. This is the easier option and the one most users pick.
- Sign the store paths before pushing them, by configuring matching `nix.settings.secret-key-files` on the builder and `nix.settings.trusted-public-keys` on the target.

Without one of these, the copy step fails with `error: cannot add path '/nix/store/...' because it lacks a valid signature by a trusted key`. This is the number one failure mode for first time cross arch deploys.

### Substituters on the builder

devenv sets `builders-use-substitutes = true` on the ephemeral builder configuration it generates for `--use-machines-as-builders`. This lets the remote builder pull dependencies from the same substituters the orchestrator uses, instead of forcing the orchestrator to download every dependency locally and re upload it to the builder. For large closures the difference is significant.

### SSH config and the nix-daemon

The nix-daemon runs as `root` (or a dedicated build user), not as your interactive user, and it reads SSH configuration from that user's home directory. If you have custom host aliases, jump hosts, or identity files in your own `~/.ssh/config`, the daemon will not see them when it opens the builder connection. The generated C-Nix `builders` setting carries the SSH destination and system, but not arbitrary per-machine `target.sshOpts`; configure builder authentication and routing in the daemon user's SSH configuration. `target.sshOpts` still applies to devenv's direct SSH and `nix copy` connections.

## Listing machines

`devenv machines info` prints a table of every machine declared in `devenv.nix` with the metadata devenv uses to build, deploy, and install them:

```sh
$ devenv machines info
+--------+--------------+-----------------+--------------+
| Name   | System       | Target          | Roles        |
+--------+--------------+-----------------+--------------+
| server | x86_64-linux | root@192.0.2.10 | nixos        |
+--------+--------------+-----------------+--------------+
| me     | x86_64-linux | (no target)     | home-manager |
+--------+--------------+-----------------+--------------+
```

Pass one or more names to restrict the listing, or no names to print every machine. Unknown names produce the same "Unknown machine(s)" error shape as `devenv machines deploy`. The command is strictly read-only: it evaluates `machinesMeta` and formats it, without forcing any `build.*` closures or touching any target.

## Building without deploying

Every machine is also exposed as a build target, so you can inspect or cache the closure without touching a remote. `devenv build machines.<name>` realises every role the machine declares (`nixos`, `nix-darwin`, `home-manager`) by flattening through the per-role build paths at `machines.<name>.build.<role>`:

```sh
$ devenv build machines.server
/nix/store/...-nixos-system-server-24.11
```

Ask for a single role by naming it directly:

```sh
$ devenv build machines.server.build.nixos
/nix/store/...-nixos-system-server-24.11

$ devenv build machines.workstation.build.home-manager
/nix/store/...-home-manager-generation
```

This mirrors the `devenv build outputs.<name>` pattern from [Outputs](/outputs/): the build walker picks up output-typed sub-options (`machines.<name>.build.nixos`, `.nix-darwin`, `.home-manager`) and exposes them under `build.*`. The user-facing `machines.<name>.nixos` / `.nix-darwin` / `.home-manager` options still hold the module definitions you wrote, unchanged, so they remain readable via `devenv eval`.

Under the hood, each role resolves to its standard closure:

- `nixos` → `nixpkgs.lib.nixosSystem` `config.system.build.toplevel`, with `disko.nixosModules.disko` auto-imported and, when `hardware.facter` is non-null, `nixos-facter-modules.nixosModules.facter` auto-imported too.
- `nix-darwin` → `nix-darwin.lib.darwinSystem` `config.system.build.toplevel`.
- `home-manager` → `home-manager.lib.homeManagerConfiguration` `activationPackage`.

The supporting inputs (`disko`, `nixos-facter-modules`, `nix-darwin`, `home-manager`) are resolved lazily: a machine that only sets `home-manager` never forces `disko`, so you do not need to add every input unless the corresponding role is actually used. When a role is set without its backing input, `devenv build` fails with a targeted `devenv inputs add` hint pointing at exactly the input that is missing.

## Known limitations

A few behaviors are explicit non features, called out so you do not infer them from the tools devenv machines is compared to:

- **No rollback on activation failure.** If a switch fails halfway, the target stays in whatever state it reached. `deploy` is idempotent; re running it is the recovery path. devenv does not implement magic rollback style watchdogs.
- **Only `switch` activation.** There is no `boot`, `test`, or `dry-activate` mode, and no `--reboot` flag. If you need a reboot after a kernel update, follow `deploy` with a manual `ssh <host> systemctl reboot`.
- **No built in healthchecks.** The summary reports whatever the activation script returned. If you need a post deploy probe, run it yourself after the command exits.
- **Stateless.** The only source of truth is `devenv.nix` plus the target's running system. Unlike NixOps, there is no state file to back up, synchronize, or recover from.
- **Resume is phase-based, not stateful.** Use `--phases` to restart at a known boundary after inspecting the target. devenv does not record completed phases or automatically infer where to resume.
- **Multi role entries are not transactional.** NixOS plus home-manager on one entry can partially succeed. See [Combining roles on one machine](#combining-roles-on-one-machine) for the exact semantics.
- **No CLI tags, groups, or label filters.** Filter at the Nix layer with `lib.filterAttrs`, or pass the names you want explicitly. See [Filtering machines](#filtering-machines).

## Troubleshooting

Common failure symptoms and what they usually mean:

- **`Too many authentication failures` when connecting.** Your `ssh-agent` has more keys loaded than the remote `MaxAuthTries` allows, and the remote disconnects before the right key is offered. For direct deploy/install connections, pass `[ "-o" "IdentitiesOnly=yes" ]` via `target.sshOpts`; for Nix remote-builder connections, set it in the nix-daemon user's SSH configuration.
- **Install hangs right after kexec.** The kexec'd installer most likely came up on a different IP than the one you started against, because DHCP handed it a new lease. Check the console or the DHCP server's lease table for the installer's new address, and use a static address or a MAC reservation for next time.
- **`cannot add path '/nix/store/...' because it lacks a valid signature by a trusted key`.** A cross arch deploy is pushing an unsigned closure and the target's nix-daemon is refusing it. Add the invoking user to `nix.settings.trusted-users` on the target, or sign the paths. See [Builder trust](#builder-trust).
- **`Host key verification failed` on first contact.** devenv normally uses `StrictHostKeyChecking=accept-new`, but installs that transmit `install.secrets`, `install.encryptionKeys`, or `install.extraFiles` force `yes` from the first connection. Pre-populate `~/.ssh/known_hosts` (or the file selected with `UserKnownHostsFile`) with every pre- and post-kexec target identity before starting the install.

## Roadmap

The machines work is organized into independently useful slices. Status labels below distinguish the current implementation from proposed follow-up interfaces.

### Slice 1: hardware detection via nixos-facter

**Status:** implemented.

Adds the `machines.<name>.hardware.facter` option and the `.machines/<name>/facter.json` convention (see [Hardware detection with nixos-facter](#hardware-detection-with-nixos-facter)). During install, the facter phase SSHes into the installer, runs `nixos-facter`, writes the report to `.machines/<name>/facter.json`, and runs `git add --intent-to-add`. The module imports `nixos-facter-modules.nixosModules.facter` and wires `facter.reportPath` whenever `hardware.facter` is non-null.

### Slice 2: install phases and disko modes

**Status:** implemented.

Adds a phase aware install command that can be stopped after any phase and resumed later:

```sh
$ devenv machines install web1 --phases kexec,facter,disko,install,reboot
$ devenv machines install web1 --stop-after-disko
$ devenv machines install web1 --no-reboot
```

Phase order is `kexec -> facter -> disko -> install -> reboot`. Any non empty subset is valid, which unlocks resume after a mid copy network drop (`--phases install,reboot`), reformat without re kexec (`--phases disko`), and controlled rollouts where each phase is inspected before the next runs.

Also adds `--disko-mode disko|format|mount` for non destructive and recovery flows:

- `disko` (default) is the current destructive path: destroy partitions, create, mount.
- `format` creates partitions without destroying existing ones, useful when initializing a second disk alongside an existing layout.
- `mount` mounts an existing layout without touching partitions, which is the recovery path when the rootfs is gone but data partitions survived.

### Slice 3: bootstrap files and install time secrets

**Status:** implemented.

Runtime secret stores (sops-nix, agenix) solve the steady state problem but leave the bootstrap problem: the key needed to decrypt the first secret has to arrive on the target somehow. Slice 3 adds four install time options per machine:

```nix title="devenv.nix"
machines.web1 = {
  install = {
    extraFiles = {
      "/var/lib/secret-age-key" = {
        source = "secrets/web1-age.key";
        owner = "0:0";
        mode = "0600";
      };
    };

    secrets = {
      "/var/lib/sops-nix/key.txt" = {
        secret = "WEB1_AGE_KEY";
        owner = "0:0";
        mode = "0600";
      };
    };

    encryptionKeys = {
      "/tmp/luks.key" = "secrets/web1-luks.key";
    };

    copyHostKeys = true;
  };
};
```

Paths are given as strings (absolute or relative to the devenv project root), **not** as Nix path literals (`./secrets/…`). This is load-bearing: a Nix path literal would copy the secret into `/nix/store`, which is world-readable. Strings are read by the CLI at install time and never leave the host.

- `install.extraFiles` copies files onto `/` of the new system after install, before reboot. Use it for age/sops master keys, SSH host keys, `/var/lib/*` seeds. Matches nixos-anywhere's `--extra-files` and `--chown`.
- `install.secrets` resolves named values from the active SecretSpec profile after `nixos-install`. The default `install.secretspec.execution = "local"` streams them from the workstation; `"target"` resolves them directly on the installer without exposing values or provider credentials to the workstation.
- `install.encryptionKeys` drops keyfiles into the installer **before disko runs**, so LUKS layouts with `passwordFile = "/tmp/luks.key"` can unlock. The keys live on the host running `devenv machines install`, not in the store.
- `install.copyHostKeys` copies `/etc/ssh/ssh_host_*` from the live installer into the installed system, keeping the post-kexec SSH identity stable across the first boot.

### Slice 4: build placement

**Status:** proposed.

A future interface could replace `--use-machines-as-builders` with richer `--build-on auto|local|remote` vocabulary:

- `auto` (default) builds locally when the current host can realize the target's `system`, otherwise routes to a matching machine in the attrset.
- `local` forces local builds and fails loudly if the current host cannot produce the target's `system`.
- `remote` forces building on the target itself. Not useful for `install`, since a freshly kexec'd target has no usable Nix, but valid for `deploy`.

The current implementation exposes only `--use-machines-as-builders`.

### Slice 5: kexec override

**Status:** implemented.

Adds a per machine escape hatch for hosts where the default `nixos-images` kexec tarball does not fit:

```nix title="devenv.nix"
machines.armbox = {
  system = "aarch64-linux";
  target.host = "root@armbox.lan";
  install.kexec = {
    image = "https://example.com/custom-kexec-aarch64.tar.gz";
    postSshPort = 2222;
  };
};
```

- `install.kexec.image` points at an alternate HTTP(S) kexec tarball fetched on the remote. Needed for VPN-enabled installers, non-standard architectures, or locked-down cloud kernels.
- `install.kexec.postSshPort` sets the SSH port to reconnect to after kexec lands in the installer, for targets whose live sshd listens on a non 22 port.

### Slice 6: host fact probing

**Status:** implemented.

Internal only, with no user-facing option. When the kexec phase is selected, install probes the target before changing it and verifies that SSH is running as root and that `tar` and `curl` are available. Resume runs that omit kexec skip this probe because the target is already expected to be in installer state.

## Reference

See the [options reference](/reference/options/#machines) for the full schema.
