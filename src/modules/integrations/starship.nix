{ pkgs
, config
, lib
, ...
}:

let
  toml = pkgs.formats.toml { };
  cfg = config.starship.config;
in
{
  options.starship = {
    enable = lib.mkEnableOption "the Starship.rs command prompt";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.starship;
      defaultText = lib.literalExpression "pkgs.starship";
      description = "The Starship package to use.";
    };

    config.enable = lib.mkEnableOption "Starship config override";

    config.path = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = lib.literalExpression "\${config.env.DEVENV_ROOT}/starship.toml";
      description = "The Starship configuration file to use.";
    };

    config.settings = lib.mkOption {
      type = toml.type;
      default = { };
      defaultText = lib.literalExpression "{}";
      description = "Starship configuration to use";
    };
  };

  config = lib.mkIf config.starship.enable {

    assertions = [
      {
        assertion = cfg.enable -> (lib.xor (cfg.path == null) (cfg.settings == { }));
        message = ''
          starship.config: one and only one of `path` or `settings` must be set if starship.config.enable == true
          Remove one of the two options.
        '';
      }
    ];

    packages = [
      config.starship.package
    ];

    enterShell = ''
      ${lib.optionalString cfg.enable "export STARSHIP_CONFIG=${
        if builtins.isPath cfg.path then cfg.path else toml.generate cfg.settings
      }"}

      # Identify the user's terminal to call the appropiate 'starship init' command
      eval "$(starship init $(echo $0))"
    '';

    # If the user's shell inherits STARSHIP_SHELL modifications from devenv,
    # when the user's shell loads *its* Starship hooks, as happens with direnv
    # use cases, the user's shell will can the prompt. This is because since
    # enterShell always runs in bash, it will unconditionally set STARSHIP_SHELL
    # to bash, which is sometimes the wrong shell. We unset it here to avoid
    # that problem.
    #
    # Caveat: STARSHIP_SESSION_KEY will still be exported as it is omitted here.
    # If the user's shell's starship process (outside of the devShell, after
    # `direnv export`) encounters errors, it will log to the same file as the
    # starship process run in enterShell.
    unsetEnvVars = [ "STARSHIP_SHELL" ];
  };
}
