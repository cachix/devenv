---
draft: false
date: 2024-09-30
authors:
  - sandydoo
---

# devenv 1.3: Instant developer environments using Nix caching

Inspired by [lorri](https://github.com/nix-community/lorri), we've added SQLite3-backed
cache for Nix evaluation to devenv.

Since Nix evaluation is expensive and can take many seconds to complete,
we're now parsing Nix logs to determine what files Nix needs for evaluation
and if none of them changed, we return the cached response.

This brings down all devenv commands to sub-100ms overhead.

<<show before/after>

## Differences to lorri / direnv / nix-direnv

Compared to `lorri`, devenv doesn't require a separate daemon and works out of the box.

`nix-direnv` and `direnv` don't reload unless you change the main Nix file,
while `devenv` can detect changes like when using `import`, `builtins.readFile`
or even `builtins.readDir`.

Sander
