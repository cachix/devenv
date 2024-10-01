---
draft: false
date: 2024-09-30
authors:
  - sandydoo
---

# devenv 1.3: Instant developer environments using Nix caching

We're excited to announce the release of devenv 1.3, which brings

Inspired by [lorri](https://github.com/nix-community/lorri), we've added an SQLite-backed
cache to devenv, which allows us to cache the results of our (many) Nix evaluations.

Once cached, the results of a Nix command can be recalled in single-digit milliseconds.

<!-- Since Nix evaluation is expensive and can take many seconds to complete, -->

<<show before/after>

## How does it work?

Behind the scenes, devenv now parses Nix's internal logs to determine which files were accessed during evaluation.
The files, the hash of their contents, and their last modified timestamp are stored in a SQLite database.

When a Nix command is about to be run, devenv first checks the cache to see if the result is already stored.

## Why not use the built-in flake evaluation cache?

TODO: why it was disabled in devenv
TODO: CLI-level, similar to this
TODO: control, visibility, and integration with other tools (direnv)

## Differences to lorri / direnv / nix-direnv

Unlike `lorri`, devenv doesn't require a separate daemon running in the background.
Re-evaluation never happens in the background and can easily be aborted.

`direnv`, and its sister project `nix-direnv`, are great for caching the evaluated Nix environment, but are limited in their ability to detect changes.

On the other hand, `devenv` can detect changes when using `import`, `builtins.readFile` and even `builtins.readDir`.

## What's next?

We aim to aggressively bring down the time it takes to launch a developer environment.
`nix develop` currently remains a (slow and uncachable) pain point.

One of the challenges we face in adapting Nix to real-world developer environments is in how it treats file paths.
All paths are first copied to the store and then replaced with a store path.
For our use-case, this can be slow, unnecessary, and, in some cases, even a security risk.
Files that impact the instantiation of the shell environment but dont affect the Nix evaluation are a common occurance — think configurations files, or package manager lock files.
There's no need to copy them to the store, but we'd like to know they're there to reload the environment when they change.
We're working on a solution that leverages our new caching infrastructure to provide a transparent way to track these files.

Sander
