{ pkgs, ... }:

{
  languages.python = {
    enable = true;
    version = "3.7.16";

    venv.enable = true;
    venv.requirements = ''
      requests
    '';
  };
}
