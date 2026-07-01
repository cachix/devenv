{ pkgs, lib, config, inputs, ... }:
{
  # Uses the git-hooks input without declaring it in devenv.yaml.
  # https://github.com/cachix/devenv/issues/2820
  git-hooks.hooks.shellcheck.enable = true;
}
