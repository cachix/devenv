{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.rocq;
in
{
  options.languages.rocq = {
    enable = lib.mkEnableOption "tools for Rocq Prover.";
    package = lib.mkPackageOption pkgs "rocq-core" {
      extraDescription = "Rocq Prover package to use.";
    };
    lsp = {
      enable = lib.mkEnableOption "Rocq Prover Language Server." // {
        default = cfg.enable;
      };
      package = lib.mkPackageOption pkgs [ "rocqPackages" "coq-lsp" ] {
        extraDescription = "Rocq Prover LSP package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ]
    ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
