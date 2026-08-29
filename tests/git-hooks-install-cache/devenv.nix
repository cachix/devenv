{
  pkgs,
  lib,
  config,
  ...
}:

let
  countingGit = pkgs.writeShellApplication {
    name = "git";
    text = ''
      if [ -n "''${DEVENV_GIT_HOOKS_COUNT_FILE:-}" ]; then
        count="$(cat "$DEVENV_GIT_HOOKS_COUNT_FILE" 2>/dev/null || echo 0)"
        printf '%s\n' "$((count + 1))" > "$DEVENV_GIT_HOOKS_COUNT_FILE"
      fi
      exec ${lib.getExe pkgs.gitMinimal} "$@"
    '';
  };
  countingPrek =
    (pkgs.writeShellApplication {
      name = "prek";
      text = ''
        if [ "''${1:-}" = install ]; then
          count_file="$DEVENV_STATE/git-hooks-test-install-count"
          count="$(cat "$count_file" 2>/dev/null || echo 0)"
          printf '%s\n' "$((count + 1))" > "$count_file"
        fi

        exec ${lib.getExe pkgs.prek} "$@"
      '';
    }).overrideAttrs
      {
        pname = "counting-prek";
        inherit (pkgs.prek) version;
      };
in
{
  assertions = [
    {
      assertion = builtins.hasAttr ".pre-commit-config.yaml" config.files;
      message = "git-hooks must manage its generated config through files.nix";
    }
    {
      assertion = !(lib.hasInfix "--version" config.tasks."devenv:git-hooks:install".exec);
      message = "git-hooks install must use immutable executable paths instead of runtime version probes";
    }
  ];

  packages = [ pkgs.jq ];

  git-hooks = {
    package = countingPrek;
    gitPackage = countingGit;
    hooks.no-op = {
      enable = true;
      name = "No Op";
      pass_filenames = false;
      raw.always_run = true;
      entry = "true";
    };
  };
}
