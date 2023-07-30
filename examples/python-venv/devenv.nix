{ pkgs, ... }:

{
  languages.python.enable = true;
  languages.python.venv.enable = true;
  languages.python.venv.pythonPackages = ps: [ ps.requests ];
}
