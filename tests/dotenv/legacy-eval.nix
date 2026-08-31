{
  modulePath,
  nixpkgsPath,
  root,
  self,
}:

let
  pkgs = import nixpkgsPath { };
  inherit (pkgs) lib;

  evaluated = lib.evalModules {
    specialArgs = {
      devenvPrimops = { };
      inherit self;
    };
    modules = [
      modulePath
      (
        { lib, ... }:
        {
          options = {
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.unspecified;
              default = [ ];
            };
            env = lib.mkOption {
              type = lib.types.attrsOf (lib.types.nullOr lib.types.str);
              default = { };
            };
            devenv = {
              flakesIntegration = lib.mkOption { type = lib.types.bool; };
              root = lib.mkOption { type = lib.types.str; };
              cli.version = lib.mkOption { type = lib.types.nullOr lib.types.str; };
            };
          };

          config = {
            devenv = {
              flakesIntegration = false;
              inherit root;
              cli.version = "2.2.1";
            };
            dotenv = {
              enable = true;
              filename = ".legacy.env";
            };
            env.BAR = "nix-owned";
          };
        }
      )
    ];
  };
in
assert evaluated.config.env.FOO == "legacy";
assert evaluated.config.env.BAR == "nix-owned";
assert evaluated.config.env.BAZ == "legacy-export";
assert !(evaluated.config.env ? SHELL);
true
