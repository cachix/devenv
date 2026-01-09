{ config, pkgs, lib, ... }:

let
  pythonPackages = config.languages.python.package.pkgs;
in
{
  languages.python.enable = true;

  packages = [ pythonPackages.flask ]
    ++ lib.optionals (!config.container.isBuilding) [ pkgs.flyctl ];

  processes.serve.exec = "exec flask --app hello run";

  containers.processes = {
    name = "simple-python-app";
    registry = "docker://registry.fly.io/";
    defaultCopyArgs = [
      "--dest-creds"
      ''x:"$(${lib.getExe pkgs.flyctl}l auth token)"''
    ];
  };
}
