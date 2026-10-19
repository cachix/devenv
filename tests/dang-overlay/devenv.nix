{ config, pkgs, ... }:

{
  overlays = [
    (final: _prev: {
      dang = final.writeShellScriptBin "dang" ''
        echo "dang from overlay"
      '';
    })
  ];

  languages.dang.enable = true;

  assertions = [
    {
      assertion = pkgs ? dang;
      message = "The overlay did not add pkgs.dang.";
    }
    {
      assertion = config.languages.dang.package == pkgs.dang;
      message = "languages.dang.package should default to pkgs.dang.";
    }
    {
      assertion = builtins.elem pkgs.dang config.packages;
      message = "languages.dang.enable should add pkgs.dang to packages.";
    }
  ];

  enterTest = ''
    test "$(dang)" = "dang from overlay"
  '';
}
