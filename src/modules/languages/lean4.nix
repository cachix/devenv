{ pkgs, config, lib, ... }:

let cfg = config.languages.lean4;
in {
  options.languages.lean4 = {
    enable = lib.mkEnableOption "tools for lean4 development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.lean4;
      defaultText = lib.literalExpression "pkgs.lean4";
      description = ''
        The lean4 package to use.
      '';
    };

    lsp = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Enable Lean 4 language server support.
          The language server is included with the main Lean 4 package.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
