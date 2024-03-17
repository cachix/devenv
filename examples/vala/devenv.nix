{ pkgs, ... }:

{
  packages = with pkgs; [
    # Check Vala code files for code-style errors
    # vala-lint
  ];

  languages = {
    vala = {
      enable = pkgs.stdenv.isLinux;
      # This is the default package for Vala for the configured channel (see nixpkgs input in devenv.yaml)
      # It can be configured to use a specific version
      # Take a look [here](https://search.nixos.org/packages?channel=unstable&from=0&size=50&sort=relevance&type=packages&query=vala) to find out which versions are available
      package = pkgs.vala;
    };
  };

  enterShell = ''
    echo "This development environment uses $(vala --version)."
  '';
}
