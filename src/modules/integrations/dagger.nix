{ pkgs, config, lib, ... }:

let
  cfg = config.dagger;

  input = config.lib.getInput {
    name = "dagger";
    url = "github:dagger/nix";
    attribute = "dagger.enable";
    follows = [ "nixpkgs" ];
  };
in
{
  options.dagger = {
    enable = lib.mkEnableOption "Dagger";

    dang.enable = lib.mkEnableOption "Dang tooling for Dagger";

    package = lib.mkOption {
      type = lib.types.package;
      default = input.packages.${pkgs.stdenv.hostPlatform.system}.dagger;
      defaultText = lib.literalExpression "dagger.packages.\${pkgs.stdenv.hostPlatform.system}.dagger";
      description = "The Dagger package to use.";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "v0.20.0";
      description = "The Dagger release to use, exported as `DAGGER_X_RELEASE`.";
    };

    engine = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.strMatching "cloud|((image|container)([+](docker|apple|podman|finch|nerdctl))?|tcp|tls|ssh|kube-pod|unix|docker-image|docker-container)://.+"
      );
      default = null;
      example = "cloud";
      description = ''
        The Dagger engine to use, exported as `DAGGER_ENGINE`. Supported patterns:

        - Dagger Cloud: `cloud`
        - OCI image: `image://IMAGE` or `image+RUNTIME://IMAGE`
        - Running container: `container://NAME` or `container+RUNTIME://NAME`
        - Direct connection: `tcp://HOST:PORT` (no authentication),
          `tls://HOST[:PORT]`, `ssh://[USER@]HOST[:PORT]`, `kube-pod://POD`, or `unix://PATH`
        - Legacy Docker: `docker-image://IMAGE` or `docker-container://NAME`

        RUNTIME can be `docker`, `apple`, `podman`, `finch`, or `nerdctl`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    languages.dang.enable = lib.mkIf cfg.dang.enable true;

    packages = [
      cfg.package
    ];

    env = {
      DO_NOT_TRACK = "1";
    }
    // lib.optionalAttrs (cfg.version != null) {
      DAGGER_X_RELEASE = cfg.version;
    }
    // lib.optionalAttrs (cfg.engine != null) {
      DAGGER_ENGINE = cfg.engine;
    };
  };
}
