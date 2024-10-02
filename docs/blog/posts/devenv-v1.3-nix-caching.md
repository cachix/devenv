---
draft: true
date: 2024-09-30
authors:
  - sandydoo
---

# devenv 1.3: Instant-er developer environments with caching

Hot on the heels of the previous release, we're releasing devenv 1.3, which brings caching to Nix commands run inside of devenv.

We've hooked up an SQLite-backed cache to automatically detect

Once cached, the results of a Nix command can be recalled in single-digit milliseconds.
And if any of the automatically-detected inputs change, the cache is invalidated and the command is re-run.

<<show before/after>>

You can toggle the cache off with `--no-eval-cache`.
If you run into any issues, you can refresh the cache with `--refresh-eval-cache`.

## How does it work?

Behind the scenes, devenv now parses Nix's internal logs to determine which files and directories were accessed during evaluation.
This approach is very much inspired by [lorri](https://github.com/nix-community/lorri) and ...
The paths, the hash of their contents, and their last-modified timestamp are stored in a SQLite database.

## Why not use the built-in flake evaluation cache?

Since v2.4, Nix has had a built-in evaluation cache for flakes attributes, which is enabled by default.
It's worth pointing out that it's implemented at the command-line level; it is not a true expression cache.
This makes it quite limited in terms of what it can cache.
Calls to `getFlake` are not cached, and neither are any intermediate evaluation results.
Nonetheless, it has it's uses and it does significantly speed up repeated calls to `nix build`.

Coming back to devenv, we made the choice to disable the built-in cache by default since v1.0.
We found that it had a tendency to aggresively cache evaluation errors, leading to a lot of frustration amongst our users.

Running our own cache gives us more control and visibility over the caching process, and allows us to improve our integration with other tools, like direnv.

The way devenv works, you can think of it as a composite command made of a series of `nix` commands.
Each individual call to `nix` is relatively expensive, but we can cache and reuse each step between devenv commands.
This makes our use of caching far more effective.
We can also predictably cache the outputs of a wider range of commands, like `nix eval` and `nix print-dev-env`.

## What makes this different from lorri, or direnv and nix-direnv?

Unlike `lorri`, devenv doesn't require a separate daemon running in the background.
Re-evaluation never happens in the background and can easily be aborted.

`direnv`, and its sister project `nix-direnv`, are great for caching the evaluated Nix environment, but are limited in their ability to detect changes.
The best we could previously do was manually watch the files we reasonably expected to affect evaluation.

On the other hand, `devenv`'s new caching layer can automatically detect dependencies from path types, `import`s, and even `builtins.readFile` and `builtins.readDir`.

## What's next?

### More caching

`nix develop` currently remains a rather slow and uncachable pain point.

### Better options for tracking files

One of the challenges we face in adapting Nix to real-world developer environments is in how it treats file paths.
All paths are first copied to the store and then replaced with a store path.
For our use-case, this can be slow, unnecessary, and, in some cases, even a security risk.
Files that impact the instantiation of the shell environment but dont affect the Nix evaluation are a common occurance — think configurations files, or package manager lock files.
There's no need to copy them to the store, but we'd like to know they're there to reload the environment when they change.
We're working on a solution that leverages our new caching infrastructure to provide a transparent way to track these files.

Sander
