{ pkgs, ... }:

{
  languages.c.enable = true;
  languages.c.gcc.enable = true;

  packages = with pkgs; [ SDL2.dev meson ninja ];
}
