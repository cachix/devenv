{ pkgs, ... }: {
  languages.c.enable = true;

  packages = [ pkgs.cmake pkgs.ceedling ];

  enterShell = ''
    cmake --version
  '';

  pre-commit.excludes = [ ".devenv" ];
  pre-commit.hooks = {
    clang-format.enable = true;
    clang-tidy.enable = true;
  };
}
