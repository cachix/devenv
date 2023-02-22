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
  envValSerializer = if implementation == "process-compose" then toString else builtins.toJSON;
  envList =
    lib.mapAttrsToList
      (name: value: "${name}=${envValSerializer value}")
      (if config.devenv.flakesIntegration then
      # avoid infinite recursion in the scenario the `config` parameter is
      # used in a `processes` declaration inside a devenv module.
        builtins.removeAttrs config.env [ "DEVENV_PROFILE" ]
      else
        config.env);

  procfileScripts = {
    honcho = ''
      ${pkgs.honcho}/bin/honcho start -f ${config.procfile} --env ${config.procfileEnv} & 
    '';

    overmind = ''
      OVERMIND_ENV=${config.procfileEnv} ${pkgs.overmind}/bin/overmind start --root ${config.env.DEVENV_ROOT} --procfile ${config.procfile} &
    '';

    process-compose = ''
      ${pkgs.process-compose}/bin/process-compose --config ${config.procfile} \
         --port ''${PC_HTTP_PORT:-${toString config.process.process-compose.port}} \
         --tui=''${PC_TUI_ENABLED:-${toString config.process.process-compose.tui}} &
    '';

    hivemind = ''
      ${pkgs.hivemind}/bin/hivemind --print-timestamps ${config.procfile} &
    '';
  };
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

      before = lib.mkOption {
        type = types.lines;
        description = "Bash code to execute before starting processes.";
        default = "";
      };

      after = lib.mkOption {
        type = types.lines;
        description = "Bash code to execute after stopping processes.";
        default = "";
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

    procfileScript = pkgs.writeShellScript "devenv-up" ''
      ${config.process.before}

      ${procfileScripts.${implementation}}

      if [[ ! -d "$DEVENV_STATE" ]]; then
        mkdir -p "$DEVENV_STATE"
      fi

      stop_up() {
        echo "Stopping processes..."
        kill -TERM $(cat "$DEVENV_STATE/devenv.pid")
        rm "$DEVENV_STATE/devenv.pid"
        wait
        ${config.process.after}
        echo "Processes stopped."
      }

      trap stop_up SIGINT SIGTERM

      echo $! > "$DEVENV_STATE/devenv.pid"

      wait
    '';

    ci = [ config.procfileScript ];

    infoSections."processes" = lib.mapAttrsToList (name: process: "${name}: ${process.exec}") config.processes;

    env =
      if implementation == "process-compose" then {
        PC_HTTP_PORT = implementation-options.port;
        PC_TUI_ENABLED = lib.boolToString implementation-options.tui;
      } else
        { };

  };
}
