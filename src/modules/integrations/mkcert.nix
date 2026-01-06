{ pkgs
, lib
, config
, ...
}:

let
  domainList = lib.concatStringsSep " " config.certificates;
  hash = builtins.hashString "sha256" domainList;
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
    certFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Custom certificate file name, uses mkcert default if unset";
      example = "mycert.pem";
    };
    keyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Custom key file name, uses mkcert default if unset";
      example = "mykey.pem";
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

        PATH="${pkgs.nssTools}/bin:$PATH" ${pkgs.mkcert}/bin/mkcert \
          ${lib.optionalString (config.keyFile != null) "-key-file ${config.keyFile}"} \
          ${lib.optionalString (config.certFile != null) "-cert-file ${config.certFile}"} \
          ${domainList} 2> /dev/null

        popd > /dev/null
      fi
    '';

    env.CAROOT = "${config.env.DEVENV_STATE}/mkcert";
    env.NODE_EXTRA_CA_CERTS = "${config.env.DEVENV_STATE}/mkcert/rootCA.pem";
  };
}
