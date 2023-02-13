{ pkgs, lib, config, ... }:

let
  domainList = lib.concatStringsSep " " config.mkcert.domains;
  hash = builtins.hashString "sha256" domainList;
in
{
  options.mkcert = {
    domains = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "List of domains to generate certificates for.";
      example = [
        "example.com"
        "127.0.0.1"
      ];
    };
  };

  config = lib.mkIf (domainList != "") {
    process.before = ''
      mkdir -p "${config.env.DEVENV_STATE}/mkcert"

      if [[ ! -f "$DEVENV_STATE/mkcert/rootCA.pem" ]]; then
        mkcert -install
      fi

      if [[ ! -f "$DEVENV_STATE/mkcert/hash" || "$(cat "$DEVENV_STATE/mkcert/hash")" != "${hash}" ]]; then
        echo "${hash}" > "${config.env.DEVENV_STATE}/mkcert/hash"

        cd ${config.env.DEVENV_STATE}/mkcert
        PATH="${pkgs.nss}/bin/certutil:$PATH" ${pkgs.mkcert}/bin/mkcert ${domainList}
      fi
    '';

    env.CAROOT = "${config.env.DEVENV_STATE}/mkcert";
    env.NODE_EXTRA_CA_CERTS = "${config.env.DEVENV_STATE}/mkcert/rootCA.pem";
  };
}
