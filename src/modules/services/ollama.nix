{ config
, lib
, pkgs
, ...
}:

with lib;

let
  cfg = config.services.ollama;
  inherit (lib) types;

  ollamaPackage = cfg.package.override { inherit (cfg) acceleration; };

  loadModels = pkgs.writeShellScriptBin "loadModels" ''
    total=${toString (builtins.length cfg.loadModels)}
    failed=0

    for model in ${lib.escapeShellArgs cfg.loadModels}; do
      '${lib.getExe ollamaPackage}' pull "$model" &
    done

    for job in $(jobs -p); do
      set +e
      wait $job
      exit_code=$?
      set -e

      if [ $exit_code != 0 ]; then
        failed=$((failed + 1))
      fi
    done

    if [ $failed != 0 ]; then
      echo "error: $failed out of $total attempted model downloads failed" >&2
      exit 1
    fi
  '';
in
{
  options = {
    services.ollama = {
      enable = mkEnableOption "ollama";
      package = lib.mkPackageOption pkgs "ollama" { };

      address = lib.mkOption {
        type = types.str;
        default = "127.0.0.1";
        example = "[::]";
        description = ''
          The host address which the ollama server HTTP interface listens to.
        '';
      };

      port = lib.mkOption {
        type = types.port;
        default = 11434;
        example = 11111;
        description = ''
          Which port the ollama server listens to.
        '';
      };

      loadModels = lib.mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = ''
          Download these models using `ollama pull` as soon as `ollama.service` has started.

          This creates a systemd unit `ollama-model-loader.service`.

          Search for models of your choice from: https://ollama.com/library
        '';
      };

      acceleration = lib.mkOption {
        type = types.nullOr (
          types.enum [
            false
            "rocm"
            "cuda"
          ]
        );
        default = null;
        example = "rocm";
        description = ''
          What interface to use for hardware acceleration.

          - `null`: default behavior
            - if `nixpkgs.config.rocmSupport` is enabled, uses `"rocm"`
            - if `nixpkgs.config.cudaSupport` is enabled, uses `"cuda"`
            - otherwise defaults to `false`
          - `false`: disable GPU, only use CPU
          - `"rocm"`: supported by most modern AMD GPUs
            - may require overriding gpu type with `services.ollama.rocmOverrideGfx`
              if rocm doesn't detect your AMD gpu
          - `"cuda"`: supported by most modern NVIDIA GPUs
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = cfg.enable;
      message = ''
        To use Ollama, you have to enable it. (services.ollama.enable = true;)
      '';
    }];

    env = {
      OLLAMA_HOST = "${cfg.address}:${toString cfg.port}";
    };

    scripts.loadModels.exec = ''
      exec ${loadModels}/bin/loadModels "$@"
    '';

    processes.ollama = {
      exec = "${lib.getExe ollamaPackage} serve";

      process-compose = {
        readiness_probe = {
          exec.command = "${pkgs.curl}/bin/curl -f -k ${cfg.address}:${toString cfg.port}";
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 2;
          success_threshold = 1;
          failure_threshold = 5;
        };

        availability.restart = "on_failure";
      };
    };
  };
}
