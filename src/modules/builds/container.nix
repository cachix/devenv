{ pkgs, config, inputs, self, lib, options, ... }:

let
  setup = ''
    inputs:
      nix2container:
        url: github:nlewo/nix2container
        inputs:
          nixpkgs:
            follows: nixpkgs
      mk-shell-bin:
        url: github:rrbutani/nix-mk-shell-bin
  '';
  types = lib.types;
  nix2containerInput = inputs.nix2container or (throw "To build the container, you need to add the following to your devenv.yaml:\n\n${setup}");
  nix2container = nix2containerInput.packages.${pkgs.system};
  cfg = config.builds.container;
  mk-shell-bin = inputs.mk-shell-bin or (throw "To build the container, you need to add the following to your devenv.yaml:\n\n${setup}");
  shell = mk-shell-bin.lib.mkShellBin { drv = config.shell; nixpkgs = pkgs; };
  # set devenv root to be at /
  containerEnv = config.env // { DEVENV_ROOT = ""; };
  entrypoint = pkgs.writeScript "entrypoint" ''
    #!${pkgs.bash}/bin/bash

    export PATH=/bin
    
    source ${shell.envScript}

    exec ${toString cfg.command}
  '';
in
{
  options.builds.container = lib.mkOption {
    type = types.submodule {
      imports = [ ../build-options.nix ];

      options = {
        version = lib.mkOption {
          type = types.nullOr types.str;
          description = "Version/tag of the container.";
          default = null;
        };

        copySource = lib.mkOption {
          type = types.bool;
          description = "Add the source of the project to the container.";
          default = true;
        };

        command = lib.mkOption {
          type = types.nullOr types.str;
          description = "Command to run in the container.";
          default = null;
        };

        entrypoint = lib.mkOption {
          type = types.listOf types.str;
          description = "Entrypoint of the container.";
          defaultText = lib.literalExpression "[ ${entrypoint} ]";
        };
      };
    };
  };

  config = {
    builds.container.entrypoint = lib.mkDefault [ "${entrypoint}" ];
    builds.container.derivation = nix2container.nix2container.buildImage {
      name =
        if config.name == null
        then throw ''You need to set `name = "myproject";` to be able to generate a container.''
        else config.name;
      tag = cfg.version;
      copyToRoot = [
        (pkgs.runCommand "create-paths" { } ''
          mkdir -p $out/tmp
        '')
        (pkgs.buildEnv {
          name = "root";
          paths = [
            pkgs.coreutils-full
            pkgs.bash
          ] ++ lib.optionals cfg.copySource [ self ];
          pathsToLink = "/";
        })
      ];
      config = {
        Env = lib.mapAttrsToList (name: value: "${name}=${lib.escapeShellArg (toString value)}") containerEnv;
        Cmd = cfg.entrypoint;
      };
    };

    builds.copy-container.derivation = pkgs.writeScript "copy-container" ''
      if [ $# -eq 0 ]; then
        echo "Usage: $0 <destination> [SKOPEO-ARGS]"
        echo 
        echo "Examples:"
        echo
        echo "  $0" 'docker-daemon:myproject:latest'
        echo "  $0" 'docker://myproject:latest'
        echo "  $0" 'docker://registry.fly.io/myproject:latest --dest-creds x:"$(flyctl auth token)"'
        exit 1
      fi

      echo "Copying container ${config.builds.container.derivation} to $1"
      echo

      ${nix2container.skopeo-nix2container}/bin/skopeo --insecure-policy copy nix:${config.builds.container.derivation} "$@"
    '';
  };
}
