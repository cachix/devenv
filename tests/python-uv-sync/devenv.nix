{ pkgs, config, inputs, ... }:
let
  # Test the uv2nix import functionality
  myapp = config.languages.python.import ./directory { };
in
{
  languages.python = {
    enable = true;
    directory = "./directory";
    venv.enable = true;
    uv = {
      enable = true;
      package = pkgs.uv;
      sync.enable = true;
    };
  };

  # Include the imported package in the environment
  packages = [ myapp ];

  # Expose the package as an output for testing
  outputs = {
    inherit myapp;
  };
}
