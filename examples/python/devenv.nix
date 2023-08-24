{ pkgs, ... }:

{
  languages.python = {
    enable = true;
    version = "3.11.3";

    venv.enable = true;
    venv.requirements = ./requirements.txt;
  };
}
