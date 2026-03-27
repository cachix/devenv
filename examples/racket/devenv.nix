{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.languages.racket;
in
{
  languages.racket = {
    enable = true;
    # Use package with bundled packages (Racket Full distribution )
    # Shell completion files are only available in Racket Full distribution
    package = pkgs.racket;
  };

  packages = [
    pkgs.bash-completion
  ];

  enterShell = lib.optionalString cfg.enable ''
    # Check if everything works as expected
    racket --version
    # Enable bash completion in devenv shell for `raco`
    source ${cfg.package}/share/racket/pkgs/shell-completion/racket-completion.bash
  '';
}
