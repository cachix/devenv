{ config, lib, pkgs, ... }:
let
  cfg = config.programs.devenv;
  hookPath = shell: ext: "${cfg.package}/share/devenv/shell-integration/${shell}/hook.${ext}";
in
{
  options.programs.devenv = {
    enable = lib.mkEnableOption "devenv, fast declarative reproducible developer environments";

    package = lib.mkPackageOption pkgs "devenv" { };

    enableBashIntegration = lib.mkEnableOption "Bash integration" // { default = true; };
    enableZshIntegration = lib.mkEnableOption "Zsh integration" // { default = true; };
    enableFishIntegration = lib.mkEnableOption "Fish integration" // { default = true; };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.systemPackages = [ cfg.package ];
    }

    (lib.mkIf cfg.enableBashIntegration {
      programs.bash.interactiveShellInit = ''
        source "${hookPath "bash" "sh"}"
      '';
    })

    (lib.mkIf cfg.enableZshIntegration {
      programs.zsh.interactiveShellInit = ''
        source "${hookPath "zsh" "zsh"}"
      '';
    })

    (lib.mkIf cfg.enableFishIntegration {
      programs.fish.interactiveShellInit = ''
        source "${hookPath "fish" "fish"}"
      '';
    })
  ]);
}
