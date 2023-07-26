{ pkgs, lib, config, ... }:

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
  };

  config = lib.mkIf (domainList != "") {
    process.before = ''
      mkdir -p "${config.env.DEVENV_STATE}/mkcert"

      if [[ ! -f "$DEVENV_STATE/mkcert/rootCA.pem" ]]; then
        PATH="${pkgs.nssTools}/bin:$PATH" ${pkgs.mkcert}/bin/mkcert -install
      fi

      if [[ ! -f "$DEVENV_STATE/mkcert/hash" || "$(cat "$DEVENV_STATE/mkcert/hash")" != "${hash}" ]]; then
        echo "${hash}" > "${config.env.DEVENV_STATE}/mkcert/hash"

        pushd ${config.env.DEVENV_STATE}/mkcert > /dev/null

        PATH="${pkgs.nssTools}/bin:$PATH" ${pkgs.mkcert}/bin/mkcert ${domainList} 2> /dev/null

        popd > /dev/null
      fi
    '';

    env.CAROOT = "${config.env.DEVENV_STATE}/mkcert";
    env.NODE_EXTRA_CA_CERTS = "${config.env.DEVENV_STATE}/mkcert/rootCA.pem";
  };
}
