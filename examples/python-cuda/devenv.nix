{ pkgs, lib, ... }:

{
  languages.python.enable = true;
  languages.python.venv.enable = true;
  languages.python.poetry.enable = true;
  toolkits.cuda.enable = true;
}
