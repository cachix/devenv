{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.openbao;

  types = lib.types;

  configFile = pkgs.writeText "config.hcl" ''
    storage "file" {
      path = "${config.env.DEVENV_STATE}/openbao-data"
    }

    listener "tcp" {
      address     = "${cfg.address}"
      tls_disable = "true"
    }

    disable_clustering = ${if cfg.disableClustering then "true" else "false"}
    ui                 = ${if cfg.ui then "true" else "false"}
  '';

  configureScript = pkgs.writeShellScriptBin "configure-openbao" ''
    set -euo pipefail

    # Wait for the vault server to start up
    response=""
    while [ -z "$response" ]; do
      response=$(${pkgs.curl}/bin/curl -s --max-time 5 "${config.env.VAULT_API_ADDR}/v1/sys/init" | ${pkgs.jq}/bin/jq '.initialized' || true)
      if [ -z "$response" ]; then
        echo "Waiting for openbao server to respond..."
        sleep 1
      fi
    done

    if [ -f "${config.env.DEVENV_STATE}/env_file" ]; then
      source "${config.env.DEVENV_STATE}/env_file"
    fi

    # Initialize it if needed
    if [ "$response" == "false" ]; then
      echo "Performing initialization"
      response=$(${pkgs.curl}/bin/curl -s --request POST --data '{"secret_shares": 1, "secret_threshold": 1}' "${config.env.VAULT_API_ADDR}/v1/sys/init")

      root_token=$(echo "$response" | ${pkgs.jq}/bin/jq -r '.root_token')
      first_key_base64=$(echo "$response" | ${pkgs.jq}/bin/jq -r '.keys_base64[0]')

      export VAULT_TOKEN="$root_token"
      export UNSEAL_KEY="$first_key_base64"

      echo "export VAULT_TOKEN=$VAULT_TOKEN" > "${config.env.DEVENV_STATE}/env_file"
      echo "export UNSEAL_KEY=$UNSEAL_KEY" >> "${config.env.DEVENV_STATE}/env_file"
    fi

    echo "OpenBao Unseal key is $UNSEAL_KEY"
    echo "OpenBao Root token is $VAULT_TOKEN"

    # Unseal the vault
    is_sealed=$(${pkgs.curl}/bin/curl -s "${config.env.VAULT_API_ADDR}/v1/sys/seal-status" | ${pkgs.jq}/bin/jq '.sealed' || true)
    if [ "$is_sealed" == "true" ]; then
      echo "OpenBao is sealed. Attempting to unsealing automatically..."
      response=$(${pkgs.curl}/bin/curl -s --request POST --data "{\"key\": \"$UNSEAL_KEY\"}" "${config.env.VAULT_API_ADDR}/v1/sys/unseal")
      if ${pkgs.jq}/bin/jq -e '.errors' <<< "$response" > /dev/null; then
        echo "Failed to unseal OpenBao: $response"
      fi
    fi

    while true
    do
      sleep 1
    done
  '';
in
{
  options.services.openbao = {
    enable = lib.mkEnableOption "openbao process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of OpenBao to use.";
      default = pkgs.openbao;
      defaultText = lib.literalExpression "pkgs.openbao";
    };

    address = lib.mkOption {
      type = types.str;
      default = "127.0.0.1:8200";
      description = ''
        Specifies the address to bind to for listening
      '';
    };

    disableClustering = lib.mkOption {
      type = types.bool;
      default = true;
      description = ''
        Specifies whether clustering features such as request forwarding are enabled
      '';
    };

    ui = lib.mkOption {
      type = types.bool;
      default = true;
      description = ''
        Enables the built-in web UI
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];
    env.VAULT_API_ADDR = "http://${cfg.address}";
    env.VAULT_ADDR = "http://${cfg.address}";
    scripts.openbao.exec = "exec ${cfg.package}/bin/bao $@";
    processes.openbao.exec = "${cfg.package}/bin/bao server -config=${configFile}";
    processes.openbao-configure.exec = "${configureScript}/bin/configure-openbao";
  };
}
