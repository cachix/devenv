{ pkgs, config, lib, ... }:

{
  options.starship = {
    enable = lib.mkEnableOption "the Starship command prompt";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.starship;
      defaultText = lib.literalExpression "pkgs.starship";
      description = "The Starship package to use.";
    };

    config.enable = lib.mkEnableOption "Starship config override";

    config.path = lib.mkOption {
      type = lib.types.path;
      default = config.env.DEVENV_ROOT + "/starship.toml";
      defaultText = lib.literalExpression "\${config.env.DEVENV_ROOT}/starship.toml";
      description = "The Starship configuration file to use.";
    };
  };

  config = lib.mkIf config.starship.enable {
    packages = [
      config.starship.package
    ];

    enterShell = ''
      ${lib.optionalString config.starship.config.enable
        "export STARSHIP_CONFIG=${config.starship.config.path}"
      }

      # Identify the user's terminal to call the appropiate 'starship init' command
      eval "$(starship init $(echo $0))"
    '';
  };
}
