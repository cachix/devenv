---
draft: false
date: 2024-09-24
authors:
  - domenkozar
---

# devenv 1.2: Tasks for convergent configuration with Nix

For devenv, our mission is to make Nix the ultimate tool for managing developer environments. Nix
excels at [congruent configuration](https://constructolution.wordpress.com/2012/07/08/divergent-convergent-and-congruent-infrastructures/),
where the system state is fully described by declarative code.

However, the real world often throws curveballs. Side-effects like database migrations, one-off
tasks such as data imports, or external API calls don't always fit neatly into this paradigm.
In these cases, we often resort to [convergent configuration](https://constructolution.wordpress.com/2012/07/08/divergent-convergent-and-congruent-infrastructures/),
where we define the desired end-state and let the system figure out how to get there.

To bridge this gap and make Nix more versatile, we're introducing tasks. These allow you to
handle those pesky real-world scenarios while still leveraging Nix's powerful ecosystem.

![Tasks interactive example](/assets/images/tasks.gif)

## Usage

For example if you'd like to execute python code after virtualenv has been created:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }: {
  languages.python.enable = true;
  languages.python.venv.enable = true;

  tasks = {
    "python:setup" = {
      exec = "python ${pkgs.writeText "setup.py" ''
          print("hello world")
      ''}";
      after = [ "devenv:python:virtualenv" ];
    };
    "devenv:enterShell".after = [ "python:setup" ];
  };
}
```

`python:setup` task executes before `devenv:enterShell` but after `python:virtualenv` task:

For all supported use cases see [tasks documentation](/tasks/).


## Task Server Protocol for SDKs

We've talked to many teams that **dropped Nix** after a while and they usually fit into two categories:

* 1) Maintaining **Nix was too complex** and the team didn't fully onboard, **creating friction inside the teams**.
* 2) Went **all-in Nix** and it took **a big toll on the team productivity**.

While devenv already addresses (1), bridging **the gap between Nix provided developer environments
and existing devops tooling written in your favorite language is still an unsolved problem until now**.
<br>

We've designed [Task Server Protocol](https://github.com/cachix/devenv/issues/1457) so that you can write tasks
using your existing automation by providing an executable that exposes the tasks to devenv:
<br>

```nix title="devenv.nix"
{ pkgs, ... }:
let
  myexecutable = pkgs.rustPlatform.buildRustPackage rec {
    pname = "foo-bar";
    version = "0.1";
    cargoLock.lockFile = ./myexecutable/Cargo.lock;
    src = pkgs.lib.cleanSource ./myexecutable;
  }
in {
  task.serverProtocol = [ "${myexecutable}/bin/myexecutable" ];
}
```

In a few weeks we're planning to provide [Rust TSP SDK](https://github.com/cachix/devenv/issues/1457)
with a **full test suite** so you can implement your own abstraction in your language of choice.
<br>

You can now use your **preferred language for automation**, running tasks with a simple `devenv tasks run <names>` command. This
**flexibility** allows for more **intuitive and maintainable scripts**, tailored to your team's familiarity.

For devenv itself, we'll slowly **transition from bash to Rust for
internal glue code**, enhancing performance and reliability. This change will make devenv more
**robust and easier to extend**, ultimately providing you with a **smoother development experience**.

## Upgrading

If you run `devenv update` on your existing repository you should already be using tasks,
without needing to upgrade to devenv 1.2.

Domen
