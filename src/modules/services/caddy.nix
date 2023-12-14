{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.caddy;

  vhostToConfig = vhostName: vhostAttrs: ''
    ${vhostName} ${builtins.concatStringsSep " " vhostAttrs.serverAliases} {
      ${vhostAttrs.extraConfig}
    }
  '';
  configFile = pkgs.writeText "Caddyfile" (builtins.concatStringsSep "\n"
    ([ cfg.config ] ++ (mapAttrsToList vhostToConfig cfg.virtualHosts)));

  formattedConfig = pkgs.runCommand "formattedCaddyFile" { } ''
    cp --no-preserve=mode,ownership ${configFile} $out
    ${cfg.package}/bin/${cfg.package.meta.mainProgram} fmt --overwrite $out
  '';

  tlsConfig = {
    apps.tls.automation.policies = [{
      issuers = [{
        inherit (cfg) ca email;
        module = "acme";
      }];
    }];
  };

  adaptedConfig = pkgs.runCommand "caddy-config-adapted.json" { } ''
    ${cfg.package}/bin/${cfg.package.meta.mainProgram} adapt \
      --config ${formattedConfig} --adapter ${cfg.adapter} > $out
  '';
  tlsJSON = pkgs.writeText "tls.json" (builtins.toJSON tlsConfig);

  # merge the TLS config options we expose with the ones originating in the Caddyfile
  configJSON =
    if cfg.ca != null then
      let
        tlsConfigMerge = ''
          {"apps":
            {"tls":
              {"automation":
                {"policies":
                  (if .[0].apps.tls.automation.policies == .[1]?.apps.tls.automation.policies
                   then .[0].apps.tls.automation.policies
                   else (.[0].apps.tls.automation.policies + .[1]?.apps.tls.automation.policies)
                   end)
                }
              }
            }
          }'';
      in
      pkgs.runCommand "caddy-config.json" { } ''
        ${pkgs.jq}/bin/jq -s '.[0] * ${tlsConfigMerge}' ${adaptedConfig} ${tlsJSON} > $out
      ''
    else
      adaptedConfig;

  vhostOptions = {
    options = {
      serverAliases = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "www.example.org" "example.org" ];
        description = ''
          Additional names of virtual hosts served by this virtual host configuration.
        '';
      };

      extraConfig = mkOption {
        type = types.lines;
        default = "";
        description = ''
          These lines go into the vhost verbatim.
        '';
      };
    };
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "caddy" "enable" ] [ "services" "caddy" "enable" ])
  ];

  options.services.caddy = {
    enable = mkEnableOption "Caddy web server";

    config = mkOption {
      default = "";
      example = ''
        example.com {
          encode gzip
          log
          root /srv/http
        }
      '';
      type = types.lines;
      description = ''
        Verbatim Caddyfile to use.
        Caddy v2 supports multiple config formats via adapters (see [`services.caddy.adapter`](#servicescaddyconfig)).
      '';
    };

    virtualHosts = mkOption {
      type = types.attrsOf (types.submodule vhostOptions);
      default = { };
      example = literalExpression ''
        {
          "hydra.example.com" = {
            serverAliases = [ "www.hydra.example.com" ];
            extraConfig = ''''''
              encode gzip
              log
              root /srv/http
            '''''';
          };
        };
      '';
      description = "Declarative vhost config.";
    };

    adapter = mkOption {
      default = "caddyfile";
      example = "nginx";
      type = types.str;
      description = ''
        Name of the config adapter to use.
        See <https://caddyserver.com/docs/config-adapters> for the full list.
      '';
    };

    resume = mkOption {
      default = false;
      type = types.bool;
      description = ''
        Use saved config, if any (and prefer over configuration passed with [`caddy.config`](#caddyconfig)).
      '';
    };

    ca = mkOption {
      default = "https://acme-v02.api.letsencrypt.org/directory";
      example = "https://acme-staging-v02.api.letsencrypt.org/directory";
      type = types.nullOr types.str;
      description = ''
        Certificate authority ACME server. The default (Let's Encrypt
        production server) should be fine for most people. Set it to null if
        you don't want to include any authority (or if you want to write a more
        fine-graned configuration manually).
      '';
    };

    email = mkOption {
      default = "";
      type = types.str;
      description = "Email address (for Let's Encrypt certificate).";
    };

    dataDir = mkOption {
      default = "${config.env.DEVENV_STATE}/caddy";
      type = types.path;
      description = ''
        The data directory, for storing certificates. Before 17.09, this
        would create a .caddy directory. With 17.09 the contents of the
        .caddy directory are in the specified data directory instead.
        Caddy v2 replaced CADDYPATH with XDG directories.
        See <https://caddyserver.com/docs/conventions#file-locations>.
      '';
    };

    package = mkOption {
      default = pkgs.caddy;
      defaultText = literalExpression "pkgs.caddy";
      type = types.package;
      description = ''
        Caddy package to use.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.caddy.exec = "XDG_DATA_HOME=${cfg.dataDir}/data XDG_CONFIG_HOME=${cfg.dataDir}/config ${cfg.package}/bin/${cfg.package.meta.mainProgram} run ${optionalString cfg.resume "--resume"} --config ${configJSON}";
  };
}
