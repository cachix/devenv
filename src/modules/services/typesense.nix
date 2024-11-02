{ pkgs, lib, config, ... }:

let
  cfg = config.services.typesense;
  types = lib.types;
in
{
  options.services.typesense = {
    enable = lib.mkEnableOption "typesense process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of typesense to use";
      default = pkgs.typesense;
      defaultText = lib.literalExpression "pkgs.typesense";
    };

    host = lib.mkOption {
      type = types.str;
      default = "127.0.0.1";
      description = ''
        The HTTP host to accept connections.
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = 8108;
      description = ''
        The HTTP port to accept connections.
      '';
    };

    apiKey = lib.mkOption {
      type = types.str;
      description = "API Key.";
      default = "example";
    };

    searchOnlyKey = lib.mkOption {
      type = types.nullOr types.str;
      description = "Search Only Key.";
      default = null;
    };

    additionalArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ ];
      example = [ ];
      description = ''
        Additional arguments passed to `typesense`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.typesense.exec = ''
      mkdir -p "$DEVENV_STATE/typesense"
      exec "${cfg.package}/bin/typesense-server" \
        --data-dir "$DEVENV_STATE/typesense" \
        --api-key ${lib.escapeShellArg cfg.apiKey} \
        --api-host ${cfg.host} \
        --api-port ${toString cfg.port} \
        ${lib.optionalString (cfg.searchOnlyKey != null) "--search-only-api-key ${lib.escapeShellArg cfg.searchOnlyKey}"} \
        ${lib.escapeShellArgs cfg.additionalArgs}
    '';
  };
}
