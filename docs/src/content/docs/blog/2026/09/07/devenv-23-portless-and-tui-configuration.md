---
title: "devenv 2.3: portless and TUI configuration"
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

## And more

**SecretSpec 0.20.** Git and Docker credential helpers, inline secret specifications, and five new providers: Azure App Configuration, Kubernetes, EJSON, Fly.io, and Cloudflare Secrets Store. See the [release announcement](https://secretspec.dev/blog/secretspec-0-20-git-docker-inline-specs-and-five-new-providers/) for details.

**Better dotenv support.** The new parser handles quotes, multiline values, comments, `export`, and optional variable substitution. Dotenv changes participate in evaluation caching and shell hot reloads, while explicit `env` definitions retain precedence.

**Arguments for auto-activated shells.** Forward shell arguments through the native hook, for example `devenv hook fish -- --no-tui` ([devenv#3128](https://github.com/cachix/devenv/issues/3128)). Bash, zsh, fish, and nushell are supported.

**Configurable process shutdown.** `processes.<name>.shutdown.signal` and `.grace` control how the native manager and process-compose stop and restart a process. PostgreSQL now uses SIGINT for fast shutdown.

**Faster garbage collection.** With a Nix daemon running 2.35 or newer, `devenv gc` removes old environments in a single batch and shows progress. The bundled Nix is now 2.35.2.

**More reliable cleanup.** Processes started as task dependencies are stopped when `devenv tasks run` exits. A second Ctrl+C no longer abandons processes during shutdown, and temporary shell capture scripts no longer accumulate in `.devenv`.

See the full [changelog](https://github.com/cachix/devenv/blob/main/CHANGELOG.md) for the rest.

## Final words

[Open an issue](https://github.com/cachix/devenv/issues) or join the [Discord](https://discord.gg/naMgvexb6q) with feedback.

Domen
