{ pkgs, lib, config, ... }:

let
  cfg = config.services.vault;

  types = lib.types;

  configFile = pkgs.writeText "config.hcl" ''
    storage "file" {
      path = "${config.env.DEVENV_STATE}/vault-data"
    }

    listener "tcp" {
      address     = "${cfg.address}"
      tls_disable = "true"
    }

    disable_mlock      = ${if cfg.disableMlock then "true" else "false"}
    disable_clustering = ${if cfg.disableClustering then "true" else "false"}
    ui                 = ${if cfg.ui then "true" else "false"}
  '';

  configureScript = pkgs.writeShellScriptBin "configure-vault" ''
    set -euo pipefail

    # Wait for the vault server to start up
    response=""
    while [ -z "$response" ]; do
      response=$(${pkgs.curl}/bin/curl -s --max-time 5 "${config.env.VAULT_API_ADDR}/v1/sys/init" | ${pkgs.jq}/bin/jq '.initialized' || true)
      if [ -z "$response" ]; then
        echo "Waiting for vault server to respond..."
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

    echo "Vault Unseal key is $UNSEAL_KEY"
    echo "Vault Root token is $VAULT_TOKEN"

    # Unseal the vault
    is_sealed=$(${pkgs.curl}/bin/curl -s "${config.env.VAULT_API_ADDR}/v1/sys/seal-status" | ${pkgs.jq}/bin/jq '.sealed' || true)
    if [ "$is_sealed" == "true" ]; then
      echo "Vault is sealed. Attempting to unsealing automatically..."
      response=$(${pkgs.curl}/bin/curl -s --request POST --data "{\"key\": \"$UNSEAL_KEY\"}" "${config.env.VAULT_API_ADDR}/v1/sys/unseal")
      if ${pkgs.jq}/bin/jq -e '.errors' <<< "$response" > /dev/null; then
        echo "Failed to unseal the vault: $response"
      fi
    fi

    while true
    do
      sleep 1
    done
  '';
in
{
  options.services.vault = {
    enable = lib.mkEnableOption "vault process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of Vault to use.";
      default = pkgs.vault-bin;
      defaultText = lib.literalExpression "pkgs.vault-bin";
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

    disableMlock = lib.mkOption {
      type = types.bool;
      default = true;
      description = ''
        Disables the server from executing the mlock syscall
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
    env.VAULT_API_ADDR = "http://${cfg.address}";
    env.VAULT_ADDR = "http://${cfg.address}";
    scripts.vault.exec = "exec ${cfg.package}/bin/vault $@";
    processes.vault.exec = "${cfg.package}/bin/vault server -config=${configFile}";
    processes.vault-configure.exec = "${configureScript}/bin/configure-vault";
  };
}
