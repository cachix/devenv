{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.verilog;
in
{
  options.languages.verilog = {
    enable = lib.mkEnableOption "tools for Verilog/SystemVerilog Development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.verilator;
      defaultText = lib.literalExpression "pkgs.verilator";
      description = ''
        Verilog/SystemVerilog package to use.
        By default, `pkgs.verilator` supports both Verilog and SystemVerilog.
        You can use `pkgs.sv-lang` instead if you only need SystemVerilog support.
      '';
    };

    lsp = {
      enable = lib.mkEnableOption "Verilog/SystemVerilog Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.verible;
        defaultText = lib.literalExpression "pkgs.verible";
        description = ''
          Verilog/SystemVerilog Language Server to use.

          You can use `pkgs.svls` instead if you only need SystemVerilog LSP support.
          See the [svls project](https://github.com/dalance/svls) for more details.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
