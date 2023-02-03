{ pkgs, config, lib, ... }:
with lib;
let
  cfg = config.services.wiremock;
  mappingsFormat = pkgs.formats.json { };
  rootDir = pkgs.linkFarm "wiremock-root" [
    {
      name = "mappings/mappings.json";
      path = mappingsFormat.generate "mappings.json" {
        mappings = cfg.mappings;
      };
    }
  ];
in
{
  options.services.wiremock = {
    enable = mkEnableOption "wiremock";
    package = mkOption {
      type = types.package;
      default = pkgs.wiremock;
      defaultText = lib.literalExpression "pkgs.wiremock";
      description = ''
        Which package of wiremock to use.
      '';
    };
    port = mkOption {
      type = types.int;
      default = 8080;
      description = ''
        The port number for the HTTP server to listen on.
      '';
    };
    disableBanner = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to disable print banner logo
      '';
    };
    verbose = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to log verbosely to stdout
      '';
    };
    mappings = mkOption {
      type = mappingsFormat.type;
      description = ''
        The mappings to mock.
        See the JSON examples on <https://wiremock.org/docs/stubbing/> for more information.
      '';
      default = [ ];
      example = [
        {
          request = {
            method = "GET";
            url = "/body";
          };
          response = {
            status = 200;
            headers."Content-Type" = "text/plain";
            body = "Literal text to put in the body";
          };
        }
        {
          request = {
            method = "GET";
            url = "/json";
          };
          response = {
            status = 200;
            jsonBody = {
              someField = "someValue";
            };
          };
        }
      ];
    };
  };

  config = mkIf cfg.enable {
    processes.wiremock.exec =
      let
        arguments = [
          "--port ${toString cfg.port}"
          "--root-dir ${rootDir}"
        ]
        ++ lib.optional cfg.disableBanner "--disable-banner"
        ++ lib.optional cfg.verbose "--verbose";
      in
      ''
        ${cfg.package}/bin/wiremock ${lib.concatStringsSep " " arguments} "$@"
      '';
  };
}
