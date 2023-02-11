{ config, pkgs, ... }:

{
  name = "simple-python-app";

  languages.python.enable = true;

  packages = [ config.languages.python.package.pkgs.flask ];

  processes.serve.exec = "flask --app hello run";

  # run the equivalent of `devenv up` in the container
  builds.container.command = toString config.procfileScript;
}
