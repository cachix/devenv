{ config, lib, pkgs, ... }:
let
  types = lib.types;
  processType = types.submodule ({ config, ... }: {
    options = {
      exec = lib.mkOption {
        type = types.str;
        description = "Bash code to run the process.";
      };
      process-compose = lib.mkOption {
        type = types.attrs; # TODO: type this explicitly?
        default = { };
        description = ''
          process-compose.yaml specific process attributes.

          Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`

          Only used when using ``process.implementation = "process-compose";``
        '';
        example = {
          environment = [ "ENVVAR_FOR_THIS_PROCESS_ONLY=foobar" ];
          availability = {
            restart = "on_failure";
            backoff_seconds = 2;
            max_restarts = 5; # default: 0 (unlimited)
          };
          depends_on.some-other-process.condition =
            "process_completed_successfully";
        };
      };
    };
  });

  implementation = config.process.implementation;
  implementation-options = config.process.${implementation};
  envList =
    (lib.mapAttrsToList (name: value: "${name}=${toString value}") config.env);
in
{
  options = {
    processes = lib.mkOption {
      type = types.attrsOf processType;
      default = { };
      description =
        "Processes can be started with ``devenv up`` and run in foreground mode.";
    };

    process = {
      implementation = lib.mkOption {
        type = types.enum [ "honcho" "overmind" "process-compose" "hivemind" ];
        description = "The implementation used when performing ``devenv up``.";
        default = "honcho";
        example = "overmind";
      };

      process-compose = lib.mkOption {
        # NOTE: https://github.com/F1bonacc1/process-compose/blob/1c706e7c300df2455de7a9b259dd35dea845dcf3/src/app/config.go#L11-L16
        type = types.attrs;
        description = ''
          Top-level process-compose.yaml options when that implementation is used.
        '';
        default = {
          version = "0.5";
          port = 9999;
          tui = true;
        };
        example = {
          version = "0.5";
          log_location = "/path/to/combined/output/logfile.log";
          log_level = "fatal";
        };
      };
    };

    # INTERNAL
    procfile = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileEnv = lib.mkOption {
      type = types.package;
      internal = true;
    };

    procfileScript = lib.mkOption {
      type = types.package;
      internal = true;
      default = pkgs.writeShellScript "no-processes" "";
    };
  };

  config = lib.mkIf (config.processes != { }) {
    packages = [ pkgs.${implementation} ];

    procfile =
      if implementation == "process-compose" then
        (pkgs.formats.yaml { }).generate "process-compose.yaml"
          ((builtins.removeAttrs implementation-options [ "port" "tui" ]) // {
            environment = envList;
            processes = lib.mapAttrs
              (name: value: { command = value.exec; } // value.process-compose)
              config.processes;
          })
      else # procfile based implementations
        pkgs.writeText "procfile" (lib.concatStringsSep "\n"
          (lib.mapAttrsToList (name: process: "${name}: ${process.exec}")
            config.processes));

    procfileEnv =
      pkgs.writeText "procfile-env" (lib.concatStringsSep "\n" envList);

    procfileScript = {
      honcho = pkgs.writeShellScript "honcho-up" ''
        echo "Starting processes ..." 1>&2
        echo "" 1>&2
        ${pkgs.honcho}/bin/honcho start -f ${config.procfile} --env ${config.procfileEnv}
      '';

      overmind = pkgs.writeShellScript "overmind-up" ''
        OVERMIND_ENV=${config.procfileEnv} ${pkgs.overmind}/bin/overmind start --procfile ${config.procfile}
      '';

      process-compose = pkgs.writeShellScript "process-compose-up" ''
        ${pkgs.process-compose}/bin/process-compose --config ${config.procfile} \
           --port $PC_HTTP_PORT \
           --tui=$PC_TUI_ENABLED
      '';

      hivemind = pkgs.writeShellScript "hivemind-up" ''
        ${pkgs.hivemind}/bin/hivemind --print-timestamps ${config.procfile}
      '';
    }.${implementation};

    env =
      if implementation == "process-compose" then {
        PC_HTTP_PORT = implementation-options.port;
        PC_TUI_ENABLED = lib.boolToString implementation-options.tui;
      } else
        { };

  };
}
