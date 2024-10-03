---
draft: false
date: 2024-10-03
authors:
  - sandydoo
---

# devenv 1.3: Instant developer environments with Nix caching

Hot on the heels of the [previous release of tasks](/blog/2024/09/24/devenv-12-tasks-for-convergent-configuration-with-nix/),
 we're releasing devenv 1.3! 🎉

This release brings precise caching to Nix evaluation, significantly speeding up developer environments.

Once cached, the results of a Nix eval/build can be recalled in single-digit milliseconds.

If any of the automatically-detected inputs change, the cache is invalidated and the build is performed.

![Caching comparison](/assets/images/caching.gif)

!!!note

     If you run into any issues, run devenv with `--refresh-eval-cache` and report
     [an issue](https://github.com/cachix/devenv/issues/new?assignees=&labels=bug&projects=&template=bug_report.md&title=).

## How does it work?

Behind the scenes, devenv now parses Nix's internal logs to determine which files and directories were accessed during evaluation.

This approach is very much inspired by [lorri](https://github.com/nix-community/lorri) and instead of running a daemon;
the paths, the hash of their contents, and their last-modified timestamp are stored in a SQLite database.

The caching process works as follows:

1. During Nix evaluation, devenv parses Nix logs for which files and directories are accessed.
2. For each accessed path, we store:
   - The full path
   - A hash of the file contents
   - The last modification timestamp

This information is then saved in a SQLite database for quick retrieval.

When you run a devenv command, we:

1. Check the database for all previously accessed paths
2. Compare the current file hashes and timestamps to the stored values
3. If any differences are detected, we invalidate the cache and perform a full re-evaluation
4. If no differences are found, we use the cached results, significantly speeding up the process

This approach allows us to efficiently detect changes in your project, including:

- Direct modifications to Nix files
- Changes to imported files or directories
- Updates to files read using Nix built-ins like `readFile` or `readDir`

## Comparison with Nix's built-in flake evaluation cache

Nix's built-in flake evaluation caches outputs based on the lock of the inputs,
ignoring changes to Nix evaluation that often happen during development workflow.

## Comparison with existing tools

Let's take a closer look at how devenv's new caching system compares to other popular tools in the Nix ecosystem:

### lorri

While lorri pioneered the approach of parsing Nix's internal logs for caching,
devenv builds on this concept, integrating caching as a built-in feature that works automatically without additional setup.

### direnv and nix-direnv

These tools excel at caching evaluated Nix environments, but have limitations in change detection:

- Manual file watching: Users often need to manually specify which files to watch for changes.
- Limited scope: They typically can't detect changes in deeply nested imports or files read by Nix built-ins.

To leverage devenv's caching capabilities with direnv, we've updated the `.envrc` file to utilize devenv's new caching logic.

If you enjoy the convenience of direnv integration into editors and reloading of your development environment make sure to update `.envrc` to:

```
source_url "https://raw.githubusercontent.com/cachix/devenv/82c0147677e510b247d8b9165c54f73d32dfd899/direnvrc" "sha256-7u4iDd1nZpxL4tCzmPG0dQgC5V+/44Ba+tHkPob1v2k="

use devenv
```

## What's next?

`nix develop` currently remains the last bit that's rather slow and uncachable,
we're aiming to rewrite it to bring down overhead of cached results to sub 100ms.

Pop up on [Discord](https://discord.gg/naMgvexb6q) if you have any questions,

Sander
