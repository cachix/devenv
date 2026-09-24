---
title: "devenv 2.4: Machines"
date: 2026-09-24
draft: false
authors:
  - name: "Domen Kožar"
    picture: "https://github.com/domenkozar.png"
    url: "https://github.com/domenkozar"
---

devenv 2.4 introduces [Machines](https://devenv.sh/machines). 

Define machine configurations alongside your development environment, then build and deploy them with `devenv machines`. 

It supports:

- [NixOS](https://nixos.org/)
- [nix-darwin](https://github.com/nix-darwin/nix-darwin)
- [home-manager](https://github.com/nix-community/home-manager)

You can contribute support for another OS to Machines.

[SecretSpec](https://secretspec.dev/) can provide the credentials a new NixOS host needs on its first boot.

Machines are experimental, and we'd like feedback from people using it on real hosts.

## From a development environment to a machine

For a NixOS server, add the disk and hardware detection inputs:

```sh
devenv inputs add disko github:nix-community/disko --follows nixpkgs
devenv inputs add nixos-facter-modules github:nix-community/nixos-facter-modules
```

Then declare the machine in `devenv.nix`:

```nix title="devenv.nix"
{ ... }: {
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@server.example.com";
    nixos = import ./nixos/server.nix;
  };
}
```

The imported NixOS module can define services, users, a bootloader, and a disk layout. devenv wires in disko and nixos-facter. On first install, it saves the host's hardware report to `.machines/server/facter.json`. Commit that file so others and CI can build the configuration. See the [disk layout example](/machines/#disk-layout-with-disko) before installing.

Inspect or build a machine without contacting its target:

```sh
devenv machines info
devenv build machines.server
```

`info` lists systems, targets, and roles. `build` realizes all roles for `server` so you can check or cache them before deploying.

## Install a new NixOS host

Run:

```sh
devenv machines install server
```

devenv connects over SSH, enters a NixOS installer with kexec, collects hardware facts, and builds the system **before** changing disks. If the build succeeds, it runs disko, installs the system, and reboots.

You must name each machine. Installation partitions and formats disks without prompting, so check the SSH target and disko disk paths first. Use stable `/dev/disk/by-id/` paths instead of names such as `/dev/sda`. You can install multiple hosts and limit concurrency with `--max-concurrent`.

See [installation options and preflight requirements](/machines/#installing-on-a-fresh-host) for encryption keys, extra files, and SSH host key preservation.

## Bootstrap secrets with SecretSpec

A new host may need a secret before sops-nix can start, such as an age identity. Declare it in `secretspec.toml` and map it to a file in the installed system:

```toml title="secretspec.toml"
[project]
name = "infrastructure"
revision = "1.0"

[profiles.production]
SERVER_AGE_KEY = { description = "sops age identity for server" }
```

```nix title="devenv.nix"
{
  machines.server.install.secrets."/var/lib/sops-nix/key.txt" = {
    secret = "SERVER_AGE_KEY";
    owner = "0:0";
    mode = "0600";
  };

  machines.server.install.secretspec = {
    execution = "target";
    profile = "production";
  };
}
```

In `./nixos/server.nix`, set `sops.age.keyFile = "/var/lib/sops-nix/key.txt";` to use the file on first boot.

With `execution = "target"`, the live installer resolves the secret through its own SecretSpec provider and writes the file after `nixos-install`, before reboot. The workstation sends the declaration, not the secret or provider credentials. The provider must work in the live installer, for example through instance identity. This mode does not require `secretspec.enable` in `devenv.yaml`.

The default `execution = "local"` resolves secrets on your workstation and sends them over SSH. This requires trusting the target's SSH host key in advance. Neither mode puts secret values in the Nix store. Bootstrap files are written only during `machines install`; use sops-nix or agenix for later rotation. See [bootstrapping from SecretSpec](/machines/#bootstrapping-from-secretspec) for provider setup and transfer details.

## Review a deployment before it changes anything

Use `check` to review SSH access changes without building, or `deploy` to build, review, and apply a NixOS system:

```sh
devenv machines check server
devenv machines deploy server
```

`deploy` compares the build with the running generation and shows closure and access changes. It blocks configurations that disable SSH or root login and warns about changed ports or administrator keys. External firewalls still need your review.

To review now and deploy later, save a plan:

```sh
devenv machines plan server
# Review the summary, then use the printed plan ID.
devenv machines apply plan-...
```

`apply` uses the planned outputs without rebuilding and rejects a stale plan if the target or NixOS generation has changed. For multiple targets, it prepares all of them before activating any. Both `deploy` and `apply` require confirmation unless you pass `--yes`.

## Recovery runs on the NixOS target

NixOS activation runs in a systemd service on the target, under a deployment lock. A watchdog restores the previous system if activation or a health check fails, or if devenv cannot confirm success before the deadline. It can also recover an unconfirmed deployment after reboot, once NixOS reaches userspace.

Configure an application health check in `devenv.nix`:

```nix title="devenv.nix"
{
  machines.server.deploy = {
    healthCheck = ''
      /run/current-system/sw/bin/systemctl is-active --quiet my-app.service
    '';
  };
}
```

The default deadline is 300 seconds. Without a custom health check, devenv checks only the system paths. If your connection drops, check the outcome before retrying. Use `rollback` if you need to switch to the previous recorded system:

```sh
devenv machines status server
devenv machines rollback server
```

Recovery requires the target to reach userspace with its previous store paths and state intact. It cannot fix early boot failures or undo application data changes.

## One plan for a mixed fleet

One machine can combine NixOS or nix-darwin with home-manager. With no machine names, `deploy` selects every SSH target and reviews the fleet together:

```sh
devenv machines deploy
devenv machines deploy --max-concurrent 2
```

Every role is built, and remote outputs are copied, before activation begins. Machines activate one at a time in name order by default. `--max-concurrent` activates batches; after a failure, no new batches start, but successful machines stay deployed.

Within a machine, home-manager activates after the system role. NixOS has automatic rollback; nix-darwin and home-manager do not. A failed home-manager activation leaves an already confirmed NixOS deployment in place.

With the C-Nix backend, `--use-machines-as-builders` makes declared SSH targets available as remote builders for cross-platform deployments.

## Other fixes since 2.3

devenv 2.3.1 restored public signing key fetching for `cachix.pull` caches and removed duplicate cache entries. This release also fixes quoted `devenv shell` commands, a crash when its terminal closes, and `devenv lsp` startup aborts. See the [changelog](https://github.com/cachix/devenv/blob/main/CHANGELOG.md) for the full list.

See the [Machines guide](/machines/) for configuration and operational details.

If you try Machines, tell us how it goes in a [GitHub issue](https://github.com/cachix/devenv/issues) or on [Discord](https://discord.gg/naMgvexb6q).

Domen
