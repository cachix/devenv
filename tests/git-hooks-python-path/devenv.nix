# Test that pre-commit hooks don't leak Python PATH entries,
# which would shadow the user's venv python and break tools like pyright.
# See: https://github.com/cachix/devenv/issues/1678
{ pkgs, lib, ... }:
{
  languages.python = {
    enable = true;
    venv = {
      enable = true;
      requirements = ''
        mashumaro
      '';
    };
  };

  git-hooks.hooks.pyright.enable = true;
  git-hooks.package = pkgs.pre-commit;

  enterTest = ''
    pre-commit run pyright -a
  '';
}
