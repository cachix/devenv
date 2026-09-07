---
title: "devenv 2.3: Portless and TUI configuration"
date: 2026-09-07
authors:
  - name: "Domen Kožar"
    picture: "https://github.com/domenkozar.png"
    url: "https://github.com/domenkozar"
---

[devenv 2.0](/blog/2026/03/05/devenv-20-a-fresh-interface-to-nix/) introduced automatic port allocation. Your dev server starts even when another project is already using port 3000. But now it's on 3001, your browser still points to 3000, and you're looking at the wrong app.

[devenv 2.3](https://github.com/cachix/devenv/releases/tag/v2.3) gives your processes stable `<process>.<project>.localhost` URLs and makes the terminal interface configurable, from statusline placement and colors to keybindings and log behavior.

## Portless

Enable the localhost proxy and declare a port for your web server ([devenv#3141](https://github.com/cachix/devenv/pull/3141)):

```nix title="devenv.nix"
{ pkgs, config, ... }:

{
  process.proxy.enable = true;

  processes.web = {
    exec = "${pkgs.python3}/bin/python -m http.server $PORT";
    ports.http.allocate = 8000;
    env.PORT = builtins.toString config.processes.web.ports.http.value;
  };
}
```

Run `devenv up` and open the URL shown in the TUI: `http://web.myapp.localhost` for a project named `myapp`. The URL stays the same even when devenv chooses a different port.

The shared proxy is built on [Pingora](https://github.com/cloudflare/pingora), Cloudflare's Rust framework for building proxies and network services. devenv manages the routes as part of its native process manager.

On Linux, devenv asks for sudo authentication to let the proxy listen on port 80.

### Choose your own hostnames

Override a process's hostname with a full `.localhost` name:

```nix title="devenv.nix"
{
  processes.web.proxy.hostname = "app.localhost";
}
```

Processes with multiple named ports get routes such as `http://admin.app.localhost`. Each port can also have its own hostname:

```nix title="devenv.nix"
{
  processes.web.ports.http.proxy.hostname = "public.localhost";
  processes.web.ports.admin.proxy.hostname = "control.localhost";
}
```

### HTTPS when you need it

Enable HTTPS per process:

```nix title="devenv.nix"
{
  process.proxy.enable = true;
  processes.web.proxy.https.enable = true;
}
```

devenv generates local certificates with mkcert and shows the HTTPS URL in the TUI. Your application keeps serving HTTP; the proxy handles HTTPS for you.

The first setup may ask you to trust the local certificate authority. Restart an already running proxy when first enabling HTTPS.

See [friendly localhost URLs](/processes/#friendly-localhost-urls) for the full configuration.

### Linux capabilities

For services that need to bind a privileged port themselves, the native process manager can grant Linux capabilities while the service keeps running as your user ([devenv#3151](https://github.com/cachix/devenv/pull/3151)):

```nix title="devenv.nix"
{
  processes.web = {
    exec = "caddy run";
    linux.capabilities = [ "net_bind_service" ];
  };
}
```

devenv shows the requested capabilities and authenticates with `sudo` once.

## Your terminal, your configuration

Two common requests since 2.0: let me keep my own shell prompt, and let me hide the statusline. In 2.3, you can do both ([devenv#3117](https://github.com/cachix/devenv/pull/3117)):

```yaml title="~/.config/devenv/config.yaml"
version: 1
shell:
  prompt_prefix: false
tui:
  statusline:
    enabled: false
```

`shell.prompt_prefix: false` removes the `(devenv)` prefix from your shell prompt. `tui.statusline.enabled: false` hides the statusline, including the persistent bar in `devenv shell`. Set either one or both.

These are personal preferences that apply across projects. Save the file at `~/.config/devenv/config.yaml`, or `$XDG_CONFIG_HOME/devenv/config.yaml` if you've set it, and start a new shell.

You can also change statusline placement and colors, remap shortcuts, and adjust log behavior. See [TUI customization](/tui-customization/) for the full configuration.

Run `devenv user-config validate` to check the file, including key conflicts and statusline formats. Add `# yaml-language-server: $schema=https://devenv.sh/devenv.user.schema.json` at the top for editor completion.

The log viewer also gained fullscreen search with highlighted matches, vim style scrolling, and a copied line counter.

## Pin many package versions at once

devenv 2.2 introduced [nixpkgs-multiverse](https://github.com/fzakaria/nixpkgs-multiverse) pins such as `multiverse.cmake."3.16.5"`. Each pin resolved on its own, so five pins could mean five nixpkgs revisions to fetch and evaluate. `multiverse.pins` resolves the whole set through the fewest revisions that can serve every requested version:

```nix title="devenv.nix"
{ multiverse, ... }:

{
  packages = multiverse.pins {
    cmake = "3.26.4";
    bun = "0.7.0";
  };
}
```

You get exactly those versions, and Farid Zakaria's [write up](https://fzakaria.com/2026/08/17/nixpkgs-multiverse-the-fewest-nixpkgs) explains why the selection is minimal. See [pinning](/pinning/#pinning-an-individual-package-version) for details.

## Skills for Claude Code

The [Claude Code integration](/integrations/claude-code/) can now generate skills, the on demand knowledge folders Claude loads when a task matches their description:

```nix title="devenv.nix"
{
  claude.code.skills.database-migrations = {
    description = "How to write and run migrations in this project.";
    allowedTools = [ "Read" "Grep" "Bash" ];
    resources."references/schema.md" = ./docs/schema.md;
    content = ''
      Migrations live in `migrations/` and are applied with `diesel migration run`.
      One logical change per migration; never edit an applied migration.
    '';
  };
}
```

Skills support model and effort overrides, forked subagent contexts, and bundled resource files. Agents gained an `effort` setting, and `claude.code.agent` selects the primary agent for the main conversation. `devenv info` reports both.

## And more

**SecretSpec 0.20.** Git and Docker credential helpers, inline secret specifications, and five new providers: Azure App Configuration, Kubernetes, EJSON, Fly.io, and Cloudflare Secrets Store. See the [release announcement](https://secretspec.dev/blog/secretspec-0-20-git-docker-inline-specs-and-five-new-providers/) for details.

**Better dotenv support.** The new [dotenv-ng](https://github.com/cachix/dotenv-ng) parser runs in the devenv CLI and handles quotes, multiline values, comments, `export`, and optional variable substitution. It supports ordered loading of several files, files in subdirectories, and files generated by tasks. Dotenv changes participate in evaluation caching and shell hot reloads, while explicit `env` definitions retain precedence. Older CLIs fall back to the legacy parser when using newer modules.

**Arguments for auto-activated shells.** Forward shell arguments through the native hook, for example `devenv hook fish -- --no-tui` ([devenv#3128](https://github.com/cachix/devenv/issues/3128)). Bash, zsh, fish, and nushell are supported.

**Configurable process shutdown.** `processes.<name>.shutdown.signal` and `.grace` control how the native manager and process-compose stop and restart a process. PostgreSQL now uses SIGINT for fast shutdown.

**Faster garbage collection.** With a Nix daemon running 2.35 or newer, `devenv gc` removes old environments in a single batch and shows progress. The bundled Nix is now 2.35.2.

**More reliable cleanup.** Processes started as task dependencies are stopped when `devenv tasks run` exits. A second Ctrl+C no longer abandons processes during shutdown, and temporary shell capture scripts no longer accumulate in `.devenv`.

**Recovery after crashes.** A guardian cleans abandoned service sessions, and the next manager reconciles them before starting the same process again. Detached manager state is also kept in `.devenv`, so `devenv processes down` works after logging back in.

**External process managers.** Detached mode is supported by process-compose, Honcho, Hivemind, and Overmind. Unsupported operations fail before launch, and `devenv down` gracefully stops Overmind and waits for it to exit.

**Smaller closure and faster shells.** The devenv closure shrank from 528 MB to 376 MB by removing duplicate dependencies. Git hook installation is skipped when installed hooks already match, file watching uses fewer allocations, and GC roots survive moving a project directory.

**More useful traces.** Process ports, readiness probes, exits, and restarts now appear as structured trace data. OTLP exports Nix evaluator heap and garbage collection metrics, and trace serialization uses fewer allocations.

**Better diagnostics.** Module errors point at the file that defined the offending option, and `devenv tasks list` prints a tree with inline descriptions. The test harness reports runtime and shell closure size and can enforce a `max_closure_size` limit.

See the full [changelog](https://github.com/cachix/devenv/blob/main/CHANGELOG.md) for the rest.

## Final words

[Open an issue](https://github.com/cachix/devenv/issues) or join the [Discord](https://discord.gg/naMgvexb6q) with feedback.

Domen
