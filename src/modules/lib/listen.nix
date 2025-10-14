{ lib }:

let
  types = lib.types;
in
types.submodule {
  options = {
    name = lib.mkOption {
      type = types.str;
      description = "Name of the socket (e.g., 'http', 'admin')";
    };
    kind = lib.mkOption {
      type = types.enum [ "tcp" "unix_stream" ];
      description = "Type of socket (tcp or unix_stream)";
    };
    address = lib.mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "TCP address (e.g., '127.0.0.1:8080'). Required for TCP sockets.";
    };
    path = lib.mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Unix socket path. Required for unix_stream sockets.";
    };
    backlog = lib.mkOption {
      type = types.nullOr types.int;
      default = 128;
      description = "Listen backlog size";
    };
    mode = lib.mkOption {
      type = types.nullOr types.int;
      default = null;
      description = "Unix socket file permissions (e.g., 384 for 0o600). Only for unix_stream.";
    };
  };
}
