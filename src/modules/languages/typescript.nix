{ pkgs, config, lib, ... }:

let
  cfg = config.languages.typescript;
in
{
  options.languages.typescript = {
    enable = lib.mkEnableOption "tools for TypeScript development";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.typescript
    ];
  };
}
