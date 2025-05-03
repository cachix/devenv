{ pkgs, ... }: {
  languages.c.enable = true;

  packages = [ pkgs.cmake pkgs.ceedling ];

  enterShell = ''
    cmake --version
  '';

  git-hooks.excludes = [ ".devenv" ];
  git-hooks.hooks = {
    clang-tidy.enable = true;
  };
}
