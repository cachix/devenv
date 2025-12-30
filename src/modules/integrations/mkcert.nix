{
  pkgs,
  lib,
  config,
  ...
}:

let
  domainList = lib.concatStringsSep " " config.certificates;
  hash = builtins.hashString "sha256" domainList;
  flags = lib.cli.toCommandLine (optionName: {
    option = "-${optionName}";
    sep = " ";
  }) config.certificateOptions;
in
{
  options = {
    certificates = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "List of domains to generate certificates for.";
      example = [
        "example.com"
        "*.example.com"
      ];
    };
    certificateOptions = lib.mkOption {
      type = lib.types.submodule {
        options = {
          cert-file = lib.mikOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "Custom certificate file name, uses mkcert default if unset";
            example = "mycert.pem";
          };
          key-file = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "Custom key file name, uses mkcert default if unset";
            example = "mykey.pem";
          };
        };
        description = "Additional configuration options for mkcert";
        default = { };
      };
    };
  };

  config = lib.mkIf (domainList != "") {
    process.manager.before = ''
      mkdir -p "${config.env.DEVENV_STATE}/mkcert"

      if [[ ! -f "$DEVENV_STATE/mkcert/rootCA.pem" ]]; then
        PATH="${pkgs.nssTools}/bin:$PATH" ${pkgs.mkcert}/bin/mkcert -install
      fi

      if [[ ! -f "$DEVENV_STATE/mkcert/hash" || "$(cat "$DEVENV_STATE/mkcert/hash")" != "${hash}" ]]; then
        echo "${hash}" > "${config.env.DEVENV_STATE}/mkcert/hash"

        pushd ${config.env.DEVENV_STATE}/mkcert > /dev/null

        PATH="${pkgs.nssTools}/bin:$PATH" ${pkgs.mkcert}/bin/mkcert ${flags} ${domainList} 2> /dev/null

        popd > /dev/null
      fi
    '';

    env.CAROOT = "${config.env.DEVENV_STATE}/mkcert";
    env.NODE_EXTRA_CA_CERTS = "${config.env.DEVENV_STATE}/mkcert/rootCA.pem";
  };
}
