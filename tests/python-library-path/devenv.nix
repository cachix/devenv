{ pkgs, ... }:

{
  languages.python = {
    enable = true;
    venv.enable = true;
    libraries = [ pkgs.file ];
  };
}
