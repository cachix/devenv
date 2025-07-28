{ pkgs, ... }:

{
  languages.python = {
    enable = true;
    venv.enable = true;
    uv.enable = true;
    libraries = [ pkgs.file ];
  };
}
