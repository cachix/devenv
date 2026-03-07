{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";

    lsp = {
      enable = lib.mkEnableOption "C Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.ccls;
        defaultText = lib.literalExpression "pkgs.ccls";
        description = "The C language server package to use.";
      };
    };

    debugger = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default =
        if pkgs.stdenv.isDarwin
        then pkgs.lldb
        else if lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb
        then pkgs.gdb
        else null;
      defaultText = lib.literalExpression "pkgs.gdb";
      description = ''
        An optional debugger package to use with c.
        The default is `lldb` on macOS, `gdb` on Linux (if supported), or `null` otherwise.
      '';
    };
  };

  config = lib.mkMerge [
    {
      changelogs = [
        {
          date = "2026-03-07";
          title = "languages.c.debugger defaults to lldb on macOS";
          when = cfg.enable && pkgs.stdenv.isDarwin;
          description = ''
            The default debugger for `languages.c` on macOS has been changed from `gdb` to `lldb`.
          '';
        }
      ];
    }
    (lib.mkIf cfg.enable {
      packages = with pkgs; [
        clang-tools
        stdenv
        gnumake
        pkg-config
      ] ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional (cfg.debugger != null) cfg.debugger
      ++ lib.optional (lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.valgrind && !pkgs.valgrind.meta.broken) pkgs.valgrind;
    })
  ];
}
