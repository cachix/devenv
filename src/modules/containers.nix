{ pkgs, config, lib, inputs, self, ... }:

let
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
  mkEntrypoint = cfg: pkgs.writeScript "entrypoint" ''
    #!${pkgs.bash}/bin/bash

    export PATH=/bin

    source ${shell.envScript}

    exec "$@"
  '';
  mkDerivation = cfg: nix2container.nix2container.buildImage {
    name = cfg.name;
    tag = cfg.version;
    maxLayers = cfg.maxLayers;
    copyToRoot = [
      (pkgs.runCommand "create-paths" { } ''
        mkdir -p $out/tmp
        mkdir -p $out/usr/bin
        ln -s ${pkgs.coreutils-full}/bin/env $out/usr/bin/env
      '')
      (pkgs.buildEnv {
        name = "root";
        paths = [
          pkgs.coreutils-full
          pkgs.dockerTools.caCertificates
          pkgs.bash
        ] ++ lib.optionals (cfg.copyToRoot != null)
          (if builtins.typeOf cfg.copyToRoot == "list"
          then cfg.copyToRoot
          else [ cfg.copyToRoot ]);
        pathsToLink = [ "/" "/lib" ];
      })
    ];
    config = {
      Env = lib.mapAttrsToList (name: value: "${name}=${lib.escapeShellArg (toString value)}") config.env;
      Cmd = [ cfg.startupCommand ];
      Entrypoint = cfg.entrypoint;
    };
  };

  # <container> <registry> <args>
  mkCopyScript = cfg: pkgs.writeScript "copy-container" ''
    container=$1
    shift


    if [[ "$1" == "" ]]; then
      registry=${cfg.registry}
    else
      registry="$1"
    fi
    shift

    dest="''${registry}${cfg.name}:${cfg.version}"

    if [[ $# == 0 ]]; then
      args=(${toString cfg.defaultCopyArgs})
    else
      args=("$@")
    fi

    echo
    echo "Copying container $container to $dest"
    echo

    ${nix2container.skopeo-nix2container}/bin/skopeo --insecure-policy copy "nix:$container" "$dest" "''${args[@]}"
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

      copyToRoot = lib.mkOption {
        type = types.nullOr (types.either types.path (types.listOf types.path));
        description = "Add a path to the container. Defaults to the whole git repo.";
        default = self;
        defaultText = "self";
      };

      startupCommand = lib.mkOption {
        type = types.nullOr (types.either types.str types.package);
        description = "Command to run in the container.";
        default = null;
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

          docker run -it ${config.name}:${config.version} "$@"
        '';
      };

      maxLayers = lib.mkOption {
        type = types.int;
        description = "the maximum number of layers to create.";
        defaultText = lib.literalExpression "1";
        default = 1;
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
        name = lib.mkDefault "shell";
        startupCommand = lib.mkDefault "bash";
      };

      containers.processes = {
        name = lib.mkDefault "processes";
        startupCommand = lib.mkDefault config.procfileScript;
      };
    }
    (if envContainerName == "" then { } else {
      containers.${envContainerName}.isBuilding = true;
    })
    (lib.mkIf config.container.isBuilding {
      devenv.root = lib.mkForce "/";
    })
  ];
}
