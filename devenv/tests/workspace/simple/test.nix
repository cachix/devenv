{ pkgs, config, ... }: {
  env.GREET = "determinism";
  packages = [
    pkgs.ncdu
  ];
  enterShell = ''
    echo hello ${config.env.GREET}
    ncdu --version
  '';
}
