{
  pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.languages.vhdl;
in {
  options.languages.vhdl = {
    enable = lib.mkEnableOption "tools for VHDL Development";
    backend = lib.mkOption {
      type = lib.types.enum ["ghdl-gcc" "ghdl-llvm" "ghdl-mcode" "nvc"];
      default = "ghdl-gcc";
      description = ''
        The VHDL compiler backend to use.
        - ghdl-gcc: GHDL with GCC backend
        - ghdl-llvm: GHDL with LLVM backend
        - ghdl-mcode: GHDL with built-in mcode backend
        - nvc: VHDL compiler and simulator
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = let
        packages = {
          "ghdl-gcc" = pkgs.ghdl-gcc;
          "ghdl-llvm" = pkgs.ghdl-llvm;
          "ghdl-mcode" = pkgs.ghdl-mcode;
          "nvc" = pkgs.nvc;
        };
      in
        packages.${config.languages.vhdl.backend} or pkgs.ghdl-gcc;
      description = "The VHDL package to use (automatically set based on backend).";
    };

    lsp = {
      enable = lib.mkEnableOption "VHDL Language Server" // {default = true;};
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.vhdl-ls;
        defaultText = lib.literalExpression "pkgs.vhdl_ls";
        description = "The VHDL language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [cfg.package] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
