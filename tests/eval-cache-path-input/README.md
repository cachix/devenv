# eval-cache-path-input

Verifies that editing files inside a local `path:` input invalidates the
evaluation cache. Previously the input was copied into the Nix store and the
cache key never changed, so edits were invisible until `.devenv` was deleted.
