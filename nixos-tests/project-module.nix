{ config, lib, pkgs, ... }:

let
  inherit (lib) types mkOption mkIf;
  cfg = config.devenvTest.project;
in
{
  options.devenvTest.project = {
    enable = mkOption {
      type = types.bool;
      default = false;
    };

    path = mkOption {
      type = types.str;
      default = "/home/dev/project";
    };

    devenvYaml = mkOption {
      type = types.path;
      description = "Path to devenv.yaml file copied into the project dir.";
    };

    devenvNix = mkOption {
      type = types.path;
      description = "Path to devenv.nix file copied into the project dir.";
    };

    devenvLock = mkOption {
      type = types.nullOr types.path;
      default = null;
    };

    flakeLock = mkOption {
      type = types.nullOr types.path;
      default = null;
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [ pkgs.git ];

    # devenv resolves inputs from github + substituters at runtime.
    # `runNixOSTest` strips substituters by default to keep tests hermetic;
    # re-enable explicitly so devenv can fetch closures.
    nix.settings = {
      experimental-features = [ "nix-command" "flakes" ];
      substituters = lib.mkForce [
        "https://cache.nixos.org"
        "https://devenv.cachix.org"
      ];
      trusted-public-keys = lib.mkForce [
        "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
        "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
      ];
    };

    networking.firewall.enable = false;

    system.activationScripts.devenvProjectSeed = ''
      install -d -o ${config.devenvTest.user} -g users -m 0755 ${cfg.path}
      install -m 0644 -o ${config.devenvTest.user} -g users \
        ${cfg.devenvYaml} ${cfg.path}/devenv.yaml
      install -m 0644 -o ${config.devenvTest.user} -g users \
        ${cfg.devenvNix} ${cfg.path}/devenv.nix
      ${lib.optionalString (cfg.devenvLock != null) ''
        install -m 0644 -o ${config.devenvTest.user} -g users \
          ${cfg.devenvLock} ${cfg.path}/devenv.lock
      ''}
      ${lib.optionalString (cfg.flakeLock != null) ''
        install -m 0644 -o ${config.devenvTest.user} -g users \
          ${cfg.flakeLock} ${cfg.path}/flake.lock
      ''}

      ${pkgs.su}/bin/su - ${config.devenvTest.user} -c '
        set -e
        cd ${cfg.path}
        ${pkgs.git}/bin/git init -q
        ${pkgs.git}/bin/git config user.email test@devenv.local
        ${pkgs.git}/bin/git config user.name "devenv test"
        ${pkgs.git}/bin/git add .
        ${pkgs.git}/bin/git commit -q -m "init"
      '
    '';
  };
}
