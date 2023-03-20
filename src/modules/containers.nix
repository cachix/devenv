{ pkgs, config, lib, inputs, self, ... }:

let
  inherit (pkgs) docopts;
  projectName = name:
    if config.name == null
    then throw ''You need to set `name = "myproject";` or `containers.${name}.name = "mycontainer"; to be able to generate a container.''
    else config.name;
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
  envContainerName = builtins.getEnv "DEVENV_CONTAINER";
  nix2containerInput = inputs.nix2container or (throw "To build the container, you need to add the following to your devenv.yaml:\n\n${setup}");
  nix2container = nix2containerInput.packages.${pkgs.stdenv.system};
  mk-shell-bin = inputs.mk-shell-bin or (throw "To build the container, you need to add the following to your devenv.yaml:\n\n${setup}");
  shell = mk-shell-bin.lib.mkShellBin { drv = config.shell; nixpkgs = pkgs; };
  # set devenv root to be at /
  containerEnv = config.env // { DEVENV_ROOT = ""; };
  mkEntrypoint = cfg: pkgs.writeScript "entrypoint" ''
    #!${pkgs.bash}/bin/bash

    export PATH=/bin

    source ${shell.envScript}

    exec "$@"
  '';
  mkDerivation = cfg: nix2container.nix2container.buildImage (
    lib.attrsets.recursiveUpdate
      {
        name = cfg.name;
        tag = cfg.version;
        copyToRoot = [
          (pkgs.runCommand "create-paths" { } ''
            mkdir -p $out/tmp $out/usr
            ln -sfT /bin $out/usr/bin
          '')
          (pkgs.buildEnv {
            name = "root";
            paths = [
              pkgs.coreutils-full
              pkgs.bash
              pkgs.dockerTools.caCertificates
              (pkgs.writeShellApplication {
                name = "devenv-entrypoint";
                text = ''exec ${lib.escapeShellArgs cfg.entrypoint} "$@"'';
              })
            ] ++ lib.optionals (cfg.copyToRoot != null) [ cfg.copyToRoot ];
            pathsToLink = "/";
          })
        ];
        config = {
          Env = lib.mapAttrsToList (name: value: "${name}=${lib.escapeShellArg (toString value)}") containerEnv;
          Entrypoint = cfg.entrypoint;
          Cmd = cfg.startupCommand;
        } // (cfg.rawBuildConfig.config or { });
      }
      cfg.rawBuildConfig
  );

  # <registry> <args>
  mkCopyScript = cfg: pkgs.writeScript "copy-container" ''
    source "$(command -v ${docopts}/bin/docopts).sh"
    eval "$(${docopts}/bin/docopts -A args -h '
    Usage: copy-container <spec-path> [options] [<skopeo-args>...]

    Options:
      -r <registry>, --registry <registry>  Registry to copy the container to, eg: docker://ghcr.io/
                                            Available shortcuts: config, local-docker, local [default: config]
      -i <name>, --image <name>             Image name and tag to use [default: ${cfg.name}:${cfg.version}]
    ' : "$@")"
    spec="''${args['<spec-path>']}"

    case "''${args['--registry']}" in
      false|config)
        registry="${cfg.registry}"
      ;;
      local-docker)
        registry="docker-daemon:"
      ;;
      local|local-podman|local-containers|local-buildah)
        registry="containers-storage:"
      ;;
      *) registry="$1" ;;
    esac

    dest="''${registry}''${args['--image']}"

    if [[ ''${args['<skopeo-args>,#']} == 0 ]]; then
      argv=(${toString cfg.defaultCopyArgs})
    else
      eval "$(docopt_get_eval_array args '<skopeo-args>' argv)"
    fi

    echo
    echo "Copying container $spec to $dest"
    echo

    ${nix2container.skopeo-nix2container}/bin/skopeo --insecure-policy copy "nix:$spec" "$dest" "''${argv[@]}"
  '';
  containerOptions = types.submodule ({ name, config, ... }: {
    options = {
      name = lib.mkOption {
        type = types.nullOr types.str;
        description = "Name of the container.";
        defaultText = "top-level name or containers.mycontainer.name";
        default = "${projectName name}-${name}";
      };

      version = lib.mkOption {
        type = types.nullOr types.str;
        description = "Version/tag of the container.";
        default = "latest";
      };

      rawBuildConfig = lib.mkOption {
        type = types.attrsOf types.anything;
        description = ''
          Raw argument overrides to be passed down to nix2container.buildImage.

          see https://github.com/nlewo/nix2container#nix2containerbuildimage
        '';
        default = { };
      };

      copyToRoot = lib.mkOption {
        type = types.nullOr types.path;
        description = "Add a path to the container. Defaults to the whole git repo.";
        default = self;
        defaultText = "self";
      };

      startupCommand = lib.mkOption {
        type = types.nullOr (types.oneOf [ types.str types.package (types.listOf types.anything) ]);
        description = "Command to run in the container.";
        default = null;
        apply = input:
          let type = builtins.typeOf input; in
          if type == "null" then [ ]
          else if type == "string" then [ input ]
          else if type == "list" then builtins.map builtins.toString input
          else [ (builtins.toString input) ];
      };

      entrypoint = lib.mkOption {
        type = types.listOf types.anything;
        description = "Entrypoint of the container.";
        default = [ (mkEntrypoint config) ];
        defaultText = lib.literalExpression "[ entrypoint ]";
      };

      defaultCopyArgs = lib.mkOption {
        type = types.listOf types.str;
        description =
          ''
            Default arguments to pass to `skopeo copy`.
            You can override them by passing arguments to the script.
          '';
        default = [ ];
      };

      registry = lib.mkOption {
        type = types.nullOr types.str;
        description = "Registry to push the container to.";
        default = "docker://";
      };

      isBuilding = lib.mkOption {
        type = types.bool;
        default = false;
        description = "Set to true when the environment is building this container.";
      };

      derivation = lib.mkOption {
        type = types.package;
        internal = true;
        default = mkDerivation config;
      };

      copyScript = lib.mkOption {
        type = types.package;
        internal = true;
        default = mkCopyScript config;
      };

      dockerRun = lib.mkOption {
        type = types.package;
        internal = true;
        default = pkgs.writeScript "docker-run" ''
          #!${pkgs.bash}/bin/bash
          set -eEuo pipefail

          container_args=()
          runtime_args=()

          for arg in "$@" ; do
            if [ "$arg" == "--" ] ; then
              runtime_args=("''${container_args[@]}")
              container_args=()
            else
              container_args+=("$arg")
            fi
          done

          docker run -it "''${runtime_args[@]}" '${config.name}:${config.version}' "''${container_args[@]}"
        '';
      };

      podmanRun = lib.mkOption {
        type = types.package;
        internal = true;
        default = pkgs.writeScript "podman-run" ''
          #!${pkgs.bash}/bin/bash
          set -eEuo pipefail

          container_args=()
          runtime_args=()

          for arg in "$@" ; do
            if [ "$arg" == "--" ] ; then
              runtime_args=("''${container_args[@]}")
              container_args=()
            else
              container_args+=("$arg")
            fi
          done

          podman run -it "''${runtime_args[@]}" '${config.name}:${config.version}' "''${container_args[@]}"
        '';
      };
    };
  });
in
{
  options = {
    containers = lib.mkOption {
      type = types.attrsOf containerOptions;
      default = { };
      description = "Container specifications that can be built, copied and ran using `devenv container`.";
    };

    container = {
      isBuilding = lib.mkOption {
        type = types.bool;
        default = false;
        description = "Set to true when the environment is building a container.";
      };
    };
  };

  config = lib.mkMerge [
    {
      container.isBuilding = envContainerName != "";

      containers.shell = {
        name = "shell";
        startupCommand = "bash";
      };

      containers.processes = {
        name = "processes";
        startupCommand = config.procfileScript;
      };
    }
    (if envContainerName == "" then { } else {
      containers.${envContainerName}.isBuilding = true;
    })
  ];
}
