{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.vhdl;
  compilerPackages = {
    ghdl-gcc = pkgs.ghdl-gcc;
    ghdl-llvm = pkgs.ghdl-llvm;
    ghdl-mcode = pkgs.ghdl-mcode;
    nvc = pkgs.nvc;
  };
  defaultPackageText =
    if pkgs.stdenv.isDarwin
    then "pkgs.ghdl-llvm"
    else "pkgs.ghdl-gcc";
in
{
  options.languages.vhdl = {
    enable = lib.mkEnableOption "tools for VHDL Development";
    compiler = lib.mkOption {
      type = lib.types.enum (builtins.attrNames compilerPackages);
      default =
        if pkgs.stdenv.isDarwin
        then "ghdl-llvm"
        else "ghdl-gcc";
      description = ''
        The VHDL compiler to use.
        - ghdl-gcc: GHDL with GCC backend
        - ghdl-llvm: GHDL with LLVM backend
        - ghdl-mcode: GHDL with built-in mcode backend
        - nvc: VHDL compiler and simulator
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = compilerPackages.${cfg.compiler};
      defaultText = lib.literalExpression defaultPackageText;
      description = "The VHDL package to use (automatically set based on compiler).";
    };

    lsp = {
      enable = lib.mkEnableOption "VHDL Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.vhdl-ls;
        defaultText = lib.literalExpression "pkgs.vhdl-ls";
        description = ''
          The VHDL language server package to use (automatically set based on compiler).

          You can see this example of [vhdl-ls configuration](https://github.com/VHDL-LS/rust_hdl#example-vhdl_lstoml--quickstart) to start.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
