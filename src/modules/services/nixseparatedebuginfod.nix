{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.nixseparatedebuginfod;
  listen_address = "${cfg.host}:${toString cfg.port}";
in
{
  options.services.nixseparatedebuginfod = {
    enable = lib.mkEnableOption "nixseparatedebuginfod";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nixseparatedebuginfod2;
      defaultText = lib.literalExpression "pkgs.nixseparatedebuginfod2";
      description = "nixseparatedebuginfod package to use.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "IP address for nixseparatedebuginfod to listen on.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 1949;
      description = "Port for nixseparatedebuginfod to listen on.";
    };

    substituters = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "local:"
        "https://cache.nixos.org"
      ];
      description = ''
        Substituters to fetch debuginfo from.
      '';
    };

    cache = {
      directory = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Override the directory where files downloaded from the substituter are stored.

          Default is `$XDG_CACHE_DIR/nixseparatedebuginfod2`.
        '';
      };

      expiration = lib.mkOption {
        type = lib.types.str;
        default = "1d";
        description = ''
          How long to keep cache entries.
          A number followed by a unit.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    processes.nixseparatedebuginfod.exec =
      let
        args = [
          "--expiration"
          cfg.cache.expiration
          "--listen-address"
          listen_address
        ]
        ++ lib.optionals (cfg.cache.directory != null) [
          "--cache-dir"
          cfg.cache.directory
        ]
        ++ (lib.lists.concatMap
          (s: [
            "--substituter"
            s
          ])
          cfg.substituters);
      in
      ''
        exec ${lib.getExe cfg.package} ${lib.escapeShellArgs args}
      '';

    enterShell = ''
      export DEBUGINFOD_URLS="http://${listen_address}''${DEBUGINFOD_URLS:+ $DEBUGINFOD_URLS}"
    '';
  };
}
