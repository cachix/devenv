{ pkgs, config, lib, ... }:

{
  options.starship = {
    enable = lib.mkEnableOption "Enable the Starship command prompt.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.starship;
      defaultText = "pkgs.starship";
      description = "The Starship package to use.";
    };

    config.enable = lib.mkEnableOption "Enable Starship config override.";

    config.path = lib.mkOption {
      type = lib.types.path;
      default = config.env.DEVENV_ROOT + "/starship.toml";
      defaultText = "\${config.env.DEVENV_ROOT}/starship.toml";
      description = "The starship configuration file to use.";
    };
  };

  config = lib.mkIf config.starship.enable {
    packages = [
      config.starship.package
    ];

    enterShell =
      let
        starshipConfExport =
          if config.starship.config.enable
          then ''export STARSHIP_CONFIG=${config.starship.config.path}''
          else '''';
      in
      starshipConfExport + ''
        # Identify user's terminal to call the appropiate 'starship init' command
        eval "$(starship init $(echo $0))"
      '';
  };
}
