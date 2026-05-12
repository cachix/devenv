{ config, lib, pkgs, ... }:
let
  cfg = config.programs.devenv;
  hookPath = shell: ext: "${cfg.package}/share/devenv/shell-integration/${shell}/hook.${ext}";
in
{
  options.programs.devenv = {
    enable = lib.mkEnableOption "devenv, fast declarative reproducible developer environments";

    package = lib.mkPackageOption pkgs "devenv" { };

    enableBashIntegration = lib.hm.shell.mkBashIntegrationOption { inherit config; };
    enableZshIntegration = lib.hm.shell.mkZshIntegrationOption { inherit config; };
    enableFishIntegration = lib.hm.shell.mkFishIntegrationOption { inherit config; };
    enableNushellIntegration = lib.hm.shell.mkNushellIntegrationOption { inherit config; };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      home.packages = [ cfg.package ];
    }

    (lib.mkIf cfg.enableBashIntegration {
      programs.bash.initExtra = ''
        source "${hookPath "bash" "sh"}"
      '';
    })

    (lib.mkIf cfg.enableZshIntegration {
      programs.zsh.initContent = ''
        source "${hookPath "zsh" "zsh"}"
      '';
    })

    (lib.mkIf cfg.enableFishIntegration {
      programs.fish.interactiveShellInit = ''
        source "${hookPath "fish" "fish"}"
      '';
    })

    (lib.mkIf cfg.enableNushellIntegration {
      programs.nushell.extraConfig = ''
        source "${hookPath "nu" "nu"}"
      '';
    })
  ]);
}
