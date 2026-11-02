{ config, lib, pkgs, ... }:
let
  nixosConfig = config;
  secrets = config.devenv.secrets;
  names = lib.attrNames secrets;

  store = "/var/lib/devenv-machines/secrets";
  runtime = "/run/devenv-secrets";
  generations = "/run/devenv-secrets.d";

  modeIsPrivate = mode:
    let
      value = lib.foldl (acc: digit: acc * 8 + lib.toInt digit) 0 (lib.stringToCharacters mode);
      bit = mask: lib.mod (value / mask) 2 == 1;
    in
    builtins.match "[0-7]{3,4}" mode != null
    && value < 512
    && !(bit 64 || bit 16 || bit 8 || bit 4 || bit 2 || bit 1);

  isNumeric = id: builtins.match "[0-9]+" id != null;

  stager = import ./secret-stager.nix {
    inherit lib pkgs;
    systemd = config.systemd.package;
    secrets = lib.mapAttrs (_: secret: secret.secret) secrets;
  };

  installSecret = name:
    let
      secret = secrets.${name};
      install = "install_secret ${lib.escapeShellArgs [ name secret.owner secret.group secret.mode ]}";
      schedule = list: map (unit: "${list}+=(${lib.escapeShellArg unit})");
      actions = schedule "restart_units" secret.restartUnits
        ++ schedule "reload_units" secret.reloadUnits;
    in
    if actions == [ ] then
      "${install}\n"
    else
      ''
        ${install}
        if [ ! -e ${runtime}/${lib.escapeShellArg name} ] || ! cmp -s -- "$generation"/${lib.escapeShellArg name} ${runtime}/${lib.escapeShellArg name}; then
          ${lib.concatStringsSep "\n  " actions}
        fi
      '';

  activation = ''
    (
      set -euo pipefail
      export PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.diffutils config.systemd.package ]}:$PATH
      umask 077
      generation=
      cleanup() {
        if [ -n "$generation" ]; then rm -rf -- "$generation"; fi
      }
      trap cleanup EXIT
      mkdir -p /run/nixos
      mkdir -p -m 0751 ${generations}
      generation=$(mktemp -d ${generations}/XXXXXX)
      chmod 0751 "$generation"
      current=$(readlink -e ${store}/current || true)
      restart_units=()
      reload_units=()

      install_secret() {
        if [ -z "$current" ] || ! systemd-creds decrypt --name="$1" "$current/$1.cred" "$generation/$1"; then
          echo "devenv-machines: secret $1 has not been delivered to this machine" >&2
          rm -f -- "$generation/$1"
          exit 1
        fi
        chown -- "$2:$3" "$generation/$1"
        chmod -- "$4" "$generation/$1"
      }

      ${lib.concatMapStrings installSecret names}

      ln -sfnT "$generation" ${runtime}.tmp
      mv -T ${runtime}.tmp ${runtime}
      published=$generation
      generation=
      if [ -n "''${NIXOS_ACTION:-}" ] && [ "''${#restart_units[@]}" -gt 0 ]; then
        printf '%s\n' "''${restart_units[@]}" >> /run/nixos/activation-restart-list
      fi
      if [ -n "''${NIXOS_ACTION:-}" ] && [ "''${#reload_units[@]}" -gt 0 ]; then
        printf '%s\n' "''${reload_units[@]}" >> /run/nixos/activation-reload-list
      fi
      for old in ${generations}/*; do
        if [ "$old" != "$published" ]; then rm -rf -- "$old"; fi
      done
    )
  '';
in
{
  key = "devenv-machines-secrets";

  options.devenv.secrets = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule ({ name, config, ... }: {
      options = {
        secret = lib.mkOption {
          type = lib.types.str;
          default = name;
          defaultText = lib.literalExpression "<name>";
          description = "Name of the secret in `secretspec.toml`.";
        };
        owner = lib.mkOption {
          type = lib.types.str;
          default = "root";
          description = "User that owns the decrypted file.";
        };
        group = lib.mkOption {
          type = lib.types.str;
          default = nixosConfig.users.users.${config.owner}.group or "root";
          defaultText = lib.literalExpression ''config.users.users.''${owner}.group or "root"'';
          description = "Group that owns the decrypted file.";
        };
        mode = lib.mkOption {
          type = lib.types.str;
          default = "0400";
          description = ''
            Permissions of the decrypted file. Special and execute bits, group
            write, and every permission for other users are rejected.
          '';
        };
        restartUnits = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          example = [ "app.service" ];
          description = "Units restarted when a deployment changes this value.";
        };
        reloadUnits = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          example = [ "nginx.service" ];
          description = "Units reloaded when a deployment changes this value.";
        };
        path = lib.mkOption {
          type = lib.types.str;
          readOnly = true;
          default = "${runtime}/${name}";
          defaultText = lib.literalExpression ''"${runtime}/<name>"'';
          description = "Path of the decrypted file.";
        };
      };
    }));
    default = { };
    example = lib.literalExpression ''
      {
        DATABASE_PASSWORD = {
          owner = "app";
          restartUnits = [ "app.service" ];
        };
      }
    '';
    description = ''
      Secrets that `devenv machines install` and `devenv machines deploy`
      resolve with SecretSpec and deliver to this machine. Each deployment
      stores its values encrypted with the machine's credential key, and
      activation decrypts them into `/run/devenv-secrets/<name>`. Rolling back
      a deployment restores the previous values.
    '';
  };

  config = {
    assertions = [
      {
        assertion =
          lib.all (secret: isNumeric secret.owner && isNumeric secret.group) (lib.attrValues secrets)
            || !(config.services.userborn.enable or false || config.systemd.sysusers.enable or false);
        message = "devenv.secrets owners and groups must be numeric IDs with userborn or systemd-sysusers, because their users do not exist yet during activation.";
      }
    ] ++ lib.concatMap
      (name: [
        {
          assertion = builtins.match "[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}" name != null;
          message = "devenv.secrets.${name}: names may contain only letters, digits, '_', '.', and '-'.";
        }
        {
          assertion = modeIsPrivate secrets.${name}.mode;
          message = "devenv.secrets.${name}.mode must be an octal mode without special or execute bits, group write, or permissions for other users.";
        }
      ])
      names;

    environment.etc = lib.mkIf (secrets != { }) {
      "devenv/secrets.json".text = builtins.toJSON {
        version = 1;
        stager = "${stager}/bin/devenv-machines-secrets";
        secrets = lib.mapAttrs (_: secret: secret.secret) secrets;
      };
    };

    system.activationScripts.devenvSecrets = {
      deps = [ "specialfs" "users" "groups" ];
      text =
        if secrets == { } then
          "rm -rf -- ${runtime} ${generations}"
        else
          activation;
    };
  };
}
