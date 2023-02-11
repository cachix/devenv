{ config, pkgs, lib, ... }:

let
  pythonPackages = config.languages.python.package.pkgs;
in
{
  languages.python.enable = true;

  packages = [ pythonPackages.flask ]
    ++ lib.optionals (!config.container.isBuilding) [ pkgs.flyctl ];

  processes.serve.exec = "flask --app hello run";

  containers.processes.name = "simple-python-app";
  containers.processes.registry = "docker://registry.fly.io/";
  containers.processes.defaultCopyArgs = [
    "--dest-creds"
    "x:\"$(${pkgs.flyctl}/bin/flyctl auth token)\""
  ];
}
