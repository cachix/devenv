---
title: "Machines"
---

:::caution[Experimental]

Machines are new in devenv 2.4. The interface may change before it is declared stable.
:::

A machine is a NixOS, nix-darwin, or home-manager configuration defined in `devenv.nix`. You can build it from your development environment, then install or deploy it with `devenv machines`.

| What you want to do | Command |
| --- | --- |
| See configured machines | `devenv machines info` |
| Build one without contacting its target | `devenv build machines.server` |
| Check NixOS SSH access changes without building | `devenv machines check server` |
| Install NixOS on a fresh host | `devenv machines install server` |
| Update an existing machine | `devenv machines deploy server` |
| Review now and deploy the same outputs later | `devenv machines plan server`, then `devenv machines apply plan-...` |
| Check or reverse the last NixOS deployment | `devenv machines status server`, `devenv machines rollback server` |

`install` partitions disks and requires a machine name. `deploy` updates an existing system and, with no names, selects all remote machines. [The options reference](/reference/options/#machines) lists every setting.

## Define a machine

A machine needs a name and at least one role. NixOS machines require the disko input, even when you only deploy to an existing host:

```sh
devenv inputs add disko github:nix-community/disko --follows nixpkgs
```

This example updates a host whose NixOS module already includes its hardware configuration:

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@server.example.com";
    hardware.facter = null;
    nixos = import ./nixos/server.nix;
  };
}
```

The imported file is a normal NixOS module. Give it the services, users, bootloader, and hardware configuration that the host needs. You can also define the module inline.

`target.host` is the SSH destination: `user@host`, `user@host:port`, or `ssh://user@host:port`. NixOS deployment and installation require root SSH. A nix-darwin deployment can use an administrator with passwordless `sudo`. A home-manager-only machine can omit `target.host` to activate locally. Setting it to `localhost` still uses SSH.

Names must start with a letter or underscore and contain only letters, digits, underscores, and hyphens. The roles are `nixos`, `nix-darwin`, and `home-manager`. A machine can have a system role and a home-manager role; both use the same target.

### Inspect and build

```sh
devenv machines info
devenv build machines.server
# Or build only one role:
devenv build machines.server.build.nixos
```

`info` reads machine metadata without building or contacting targets. `build` produces the configured role outputs locally without deploying them. If a role needs an input you have not added, devenv gives a `devenv inputs add` hint. The build may still need a matching local architecture or a configured Nix builder. If you use the default hardware report path instead of setting `hardware.facter = null`, add the nixos-facter input and provide that report before building.

### SSH settings

For direct SSH and `nix copy`, devenv accepts new host keys and sets a connection timeout. Preload `known_hosts` if you need stricter verification. To override SSH options, set `target.sshOpts`:

```nix
machines.server.target.sshOpts = [ "-o" "IdentitiesOnly=yes" ];
```

When an install sends local secrets, encryption keys, or extra files, devenv requires a known host key from its first connection and disables forwarding. Add the host keys for both the original host and the kexec installer if they differ. You can select a dedicated file with `[ "-o" "UserKnownHostsFile=/absolute/path" ]`. The stricter settings cannot be overridden with `sshOpts`.

## Install NixOS on a fresh host

`install` connects to a Linux host over root SSH, boots a temporary NixOS installer with kexec, detects hardware, builds the new system, partitions disks with disko, installs NixOS, and reboots. The host does not need to be running NixOS beforehand.

Add the inputs used by the disk layout and hardware detection if they are not already in your project:

```sh
devenv inputs add disko github:nix-community/disko --follows nixpkgs
devenv inputs add nixos-facter-modules github:nix-community/nixos-facter-modules
```

Then define the host, including its disk layout and bootloader. The example below is for a UEFI host with one disk. Replace the disk ID and provide suitable networking and SSH configuration before using it.

### Disk layout with disko

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@server.example.com";
    nixos = {
      disko.devices.disk.main = {
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
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 ..." ];
    };
  };
}
```

Find the disk ID on the target with `ls -l /dev/disk/by-id`. Use a stable `/dev/disk/by-id/` or `/dev/disk/by-path/` path. Names such as `/dev/sda` can change between boots and point at the wrong disk. The example requires UEFI; BIOS hosts need a different bootloader and partition layout. Changing partitions or filesystems later generally requires a backup and reinstall. Test the disko layout in a VM before using real disks. For a non-root ZFS pool, add it to `boot.zfs.extraPools` so it is imported at boot.

### Installing on a fresh host

Before running `install`, verify the SSH destination and disk IDs. The target must allow root SSH, support kexec, and have `tar` and `curl`. Allow roughly 1 GB of free RAM for the temporary installer. If kexec is unavailable, boot a suitable installer yourself and use `--phases facter,disko,install,reboot`.

:::caution[Install wipes disks without a confirmation prompt]

`devenv machines install` partitions and formats the devices in your disko layout. A normal retry repeats those steps. Inspect the target before running it again; there is no dry run or automatic resume.
:::

```sh
devenv machines install server
```

The system builds **before** disko changes disks. A build failure stops before partitioning, but the target may already be running the temporary installer. `install` requires explicit names, including for a fleet:

```sh
devenv machines install server1 server2 --max-concurrent 1
```

Use `--max-concurrent N` to limit simultaneous installs. For an interrupted install, inspect the target before selecting phases with `--phases`. The phases run in this order: `kexec`, `facter`, `disko`, `install`, `reboot`. `--disko-mode mount` can mount an existing layout without repartitioning; `format` creates missing storage structures without the destroy phase; the default `disko` mode is destructive. `--stop-after-disko` and `--no-reboot` are available for controlled installs. Phase selection does not check whether omitted steps succeeded.

### Hardware detection with nixos-facter

During the first install, devenv runs nixos-facter in the temporary installer and saves `.machines/server/facter.json`. Commit that report so teammates and CI can build the same machine without contacting it. Reports contain host-specific details such as disk IDs and MAC addresses, so keep one per host.

To use a different report path, set `machines.server.hardware.facter = ./hardware/server.json;`. Set it to `null` if you supply hardware configuration yourself. devenv imports the nixos-facter module automatically when a NixOS machine has a report.

## Updating an existing host

For a configured host that is already running NixOS:

```sh
devenv machines check server
devenv machines deploy server
```

`check` compares declared SSH access with facts read from the target, without building or changing it. It blocks reviewed deployments that would disable SSH or root login, and warns about changes such as SSH ports and administrator keys. It cannot verify external firewalls, dynamic keys, or that you possess a working key. `check --json server` gives structured results for automation.

`deploy` builds every role on the machine, shows a plan, and asks for confirmation. After confirmation, it copies the reviewed outputs and activates them. Pass `--yes` for automation. NixOS requires root SSH and systemd. It does not repartition disks or reboot after a kernel change; reboot separately when needed.

### Review now, apply later

```sh
devenv machines plan server
# Review the summary and note the printed plan ID.
devenv machines apply plan-...
```

`plan` builds outputs and records the NixOS system, access facts, and store closure changes. It does not copy or activate anything. `apply` uses those exact outputs without rebuilding, checks that the machine still matches the plan, copies all outputs, then activates them. A changed NixOS generation or target definition makes the plan stale. For automation, `devenv machines plan --json server` exports a plan that `apply` can also read.

Saved plans live under `.devenv/machine-plans/<id>/` and retain their outputs. Remove an old plan directory when you no longer need it. Treat a plan as a trusted deployment input because it selects executable store paths. Closure changes describe which paths are present, not download size or which services restart.

### NixOS rollback and health checks

NixOS deployment runs in a systemd service on the target. A watchdog restores the previous system if activation or a health check fails, or if the controller cannot confirm success before the deadline. The default deadline is 300 seconds; the default health check only verifies system paths. Add an application check when that is insufficient:

```nix title="devenv.nix"
{ ... }: {
  machines.server.deploy = {
    rollbackTimeout = 300; # 30 to 600 seconds
    healthCheck = ''
      /run/current-system/sw/bin/systemctl is-active --quiet my-app.service
    '';
  };
}
```

The check runs as root. Use absolute paths for commands and allow enough time for service startup and SSH reconnection. If your connection drops, **check the result before retrying**:

```sh
devenv machines status server
devenv machines rollback server
```

`status` reports the latest operation without building. `pending` means activation or recovery is still running; `rolled-back` and `rollback-failed` are failures; `unknown` means the last operation has no observable final result. `rollback` restores the previous recorded NixOS system after its service stops. A new deployment is blocked while the previous outcome is unknown.

The target also attempts recovery after reboot if a deployment was left unconfirmed and the new system reaches userspace. Recovery cannot fix an early boot failure, reverse application data changes, or undo side effects of activation scripts. A kernel change still needs a reboot. The last operation and retained system paths live under `/var/lib/devenv-machines` and `/nix/var/nix/gcroots/devenv-machines/` on the target. Older history can be removed after confirming no operation needs it; keep the latest operation and its executor.

## nix-darwin and home-manager

A nix-darwin machine uses a Darwin `system` and a `nix-darwin` module:

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

Run `devenv machines deploy mac` to switch it. There is no `machines install` for macOS. A non-root SSH user needs passwordless `sudo` because activation is noninteractive. nix-darwin deployment has no automatic rollback.

A home-manager machine can activate locally or over SSH:

```nix title="devenv.nix"
{ ... }: {
  machines.me = {
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      programs.git.enable = true;
    };
  };

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

`devenv machines deploy me` activates locally. `devenv machines deploy workstation` activates over SSH. home-manager also has no automatic rollback.

### Combine system and user roles

Add `home-manager` to the same machine to deploy the user's configuration after NixOS or nix-darwin succeeds. Both roles use the same `target.host`:

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@server.example.com";
    nixos = import ./nixos/server.nix;
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      programs.git.enable = true;
    };
  };
}
```

`install` provisions only NixOS. After the first boot, `deploy` activates both roles. The system role runs first, then home-manager as `home.username`. Make sure that user exists and its home directory matches the system configuration. Pin the user's UID and group GID on a new NixOS host so later changes do not break file ownership.

The roles are separate activations. If home-manager fails after NixOS succeeds, the NixOS deployment remains applied. NixOS rollback does not revert home-manager files.

## Deploy several machines

With no names, `devenv machines deploy` selects all machines with `target.host`. Local home-manager machines must be named explicitly. To limit the deployment, pass names:

```sh
devenv machines deploy
devenv machines deploy server1 server2
devenv machines deploy me
```

The command builds and reviews every selected role, checks every target, and copies every remote output before activating any machine. By default, machines activate one at a time in name order. `--max-concurrent N` activates batches of up to N. If a batch fails, active machines finish but later batches do not start. Successful activations remain applied; the fleet is not one transaction.

There are no machine tags or CLI group selectors. Use explicit names or declare different sets of machines in [profiles](/profiles/) and run, for example, `devenv --profile staging machines deploy`.

### Build for another platform

By default, devenv builds on the machine running the command. For a mixed fleet, `--use-machines-as-builders` lets C-Nix use declared remote machines as builders for their matching `system`:

```sh
devenv machines deploy --use-machines-as-builders
```

The flag also works with `install`, but a fresh target cannot build its own system. Another declared machine with the matching architecture must be available. This flag currently requires the C-Nix backend. Plain `devenv build` does not accept it.

Remote builders cannot use `target.sshOpts`. Put builder authentication and routing in an SSH host alias instead, configured for the user running the nix-daemon. The target must accept the copied paths: make the invoking user trusted with `nix.settings.trusted-users`, or sign the paths with a key the target trusts. Otherwise the copy can fail with a missing trusted signature. Devenv enables substituters for these builders.

## Bootstrap files and secrets for NixOS installs

Use sops-nix or agenix in your NixOS or home-manager modules for ongoing secret management. Never put a secret literal in a Nix module: it would be copied into the readable Nix store. `install` can place the initial credentials needed on first boot.

| Option | When it runs | Purpose |
| --- | --- | --- |
| `install.encryptionKeys` | Before disko | Send local key files used by the disk layout, such as a LUKS key |
| `install.extraFiles` | After nixos-install | Copy local files into the installed system |
| `install.secrets` | After extra files | Write named SecretSpec values into the installed system |
| `install.copyHostKeys` | Before reboot | Preserve the installer's SSH host keys |

Use **strings** for local file paths, such as `"secrets/server.key"`, rather than Nix path literals such as `./secrets/server.key`. Nix path literals copy file contents into the Nix store. File owners use numeric `uid:gid` because the installer may not know the users in the new system. For `install.secrets`, modes such as `0400` and `0600` are accepted. All local file payloads require pre-pinned SSH host keys; see [SSH settings](#ssh-settings).

### Bootstrapping from SecretSpec

For example, you can provide the age key that sops-nix needs on first boot. Declare the secret in `secretspec.toml`:

```toml title="secretspec.toml"
[project]
name = "infrastructure"
revision = "1.0"

[profiles.production]
SERVER_AGE_KEY = { description = "sops age identity for server" }
```

Then add this mapping to your existing machine declaration:

```nix title="devenv.nix"
{ ... }: {
  machines.server.install.secrets."/var/lib/sops-nix/key.txt" = {
    secret = "SERVER_AGE_KEY";
    owner = "0:0";
    mode = "0600";
  };
}
```

In the NixOS module, configure sops-nix and set `sops.age.keyFile = "/var/lib/sops-nix/key.txt";`. By default, `execution = "local"`: devenv resolves the value on your workstation and streams it over SSH. Enable SecretSpec in `devenv.yaml`, then configure its provider and profile there or with `--secretspec-provider` and `--secretspec-profile`. The value is not put in the Nix store. Bootstrap files are written only by `install`, not refreshed by `deploy`.

To resolve the secret in the temporary installer instead, set:

```nix
machines.server.install.secretspec = {
  execution = "target";
  profile = "production";
};
```

This sends the committed SecretSpec declaration without fetching the value on the workstation. The installer must already have access to the provider, for example through instance identity. Target execution does not require `secretspec.enable` in `devenv.yaml`; global SecretSpec CLI flags apply only to local execution. You can add provider helper programs with `install.secretspec.extraPackages = pkgs: [ pkgs.sops ];`. Target execution keeps provider credentials off the workstation, but the workstation still controls the system it installs, so review that configuration before trusting it with secrets.

For other bootstrap files, use `install.extraFiles` with a string source path. For disk encryption, point disko's `passwordFile` or `settings.keyFile` at an installer path and map it to a local string path with `install.encryptionKeys`. `install.copyHostKeys = true` preserves SSH identity across first boot.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| Install hangs after kexec | DHCP may have given the temporary installer another IP. Check its console or lease table; use a fixed address or reservation. |
| `Host key verification failed` | Installs sending local files require known keys for both the original host and temporary installer. |
| `Too many authentication failures` | Set `target.sshOpts = [ "-o" "IdentitiesOnly=yes" ];`. For remote builders, configure the nix-daemon user's SSH settings. |
| Copy rejects a path with no trusted signature | Trust the invoking user on the target or sign the path with a trusted key. |
| Connection drops during deployment | Run `devenv machines status server` before retrying; the target may still be activating or rolling back. |

## Reference

The old `configurations` option still evaluates through a compatibility alias, but use `machines` in new files. See the [machines options reference](/reference/options/#machines) for the full schema.
