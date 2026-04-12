{ lib }:
lib.types.submodule {
  options = {
    scheme = lib.mkOption {
      type = lib.types.str;
      default = "http";
      description = "URL scheme.";
      example = "https";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "URL host.";
      example = "localhost";
    };

    port = lib.mkOption {
      type = lib.types.port;
      description = "URL port.";
      example = 8080;
    };

    path = lib.mkOption {
      type = lib.types.str;
      default = "/";
      description = "URL path.";
      example = "/admin";
    };
  };
}
