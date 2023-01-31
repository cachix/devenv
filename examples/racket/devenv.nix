{ pkgs, ... }:

{
  packages = with pkgs; [
    bash-completion
  ];

  languages = {
    racket = {
      enable = true;
      # Use package with bundled packages (Racket Full distribution )
      # Shell completion files are only available in Racket Full distribution
      package = pkgs.racket;
    };
  };

  enterShell = ''
    # Check if everything works as expected
    racket --version
    # Enable bash completion in devenv shell for `raco`
    source ${pkgs.racket}/share/racket/pkgs/shell-completion/racket-completion.bash
  '';
}
