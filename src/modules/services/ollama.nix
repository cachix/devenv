{ pkgs, lib, config, ... }:

let
  cfg = config.services.ollama;
  types = lib.types;

  ollama = lib.getExe cfg.package;

  supportsWantedBy = config.devenv.cli.version == null
    || lib.versionAtLeast config.devenv.cli.version "2.3.2";

  # Port allocation
  basePort = cfg.port;
  allocatedPort = config.processes.ollama.ports.http.value;

  # Wildcard addresses are fine to bind to, but not to connect to.
  connectHost =
    if builtins.elem cfg.host [ "0.0.0.0" "::" "[::]" ]
    then "127.0.0.1"
    else cfg.host;

  connectAddress = "${connectHost}:${toString allocatedPort}";
in
{
  options.services.ollama = {
    enable = lib.mkEnableOption "Ollama server for local large language models";

    package = lib.mkOption {
      type = types.package;
      description = ''
        Which package of Ollama to use.

        Hardware acceleration depends on the package: use `pkgs.ollama-cuda`,
        `pkgs.ollama-rocm` or `pkgs.ollama-vulkan` for GPU support on Linux.
        On macOS, `pkgs.ollama` uses Metal.
      '';
      default = pkgs.ollama;
      defaultText = lib.literalExpression "pkgs.ollama";
      example = lib.literalExpression "pkgs.ollama-cuda";
    };

    host = lib.mkOption {
      type = types.str;
      default = "127.0.0.1";
      example = "[::]";
      description = ''
        The host address the Ollama HTTP server listens on.
        Wrap IPv6 addresses in brackets.
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = 11434;
      description = ''
        The base port the Ollama HTTP server listens on.
        The actual port is allocated by devenv and may differ;
        `OLLAMA_HOST` and `OLLAMA_BASE_URL` point to it.
      '';
    };

    modelsDir = lib.mkOption {
      type = types.str;
      default = "${config.devenv.state}/ollama/models";
      defaultText = lib.literalExpression ''"''${config.devenv.state}/ollama/models"'';
      example = "/home/user/.ollama/models";
      description = ''
        The directory Ollama reads models from and downloads new models to.

        Point this at a shared location (such as `~/.ollama/models`, as an absolute path)
        to reuse models across projects instead of downloading them per project.
      '';
    };

    loadModels = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "gemma3" "qwen3:0.6b" ];
      description = ''
        Models to download with `ollama pull` once the server is ready.
        Models that are already present are not downloaded again.

        Search for models at <https://ollama.com/library>.
      '';
    };

    environmentVariables = lib.mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        OLLAMA_CONTEXT_LENGTH = "8192";
        OLLAMA_KEEP_ALIVE = "10m";
      };
      description = ''
        Additional environment variables for the Ollama server process.
        See `ollama serve --help` for available options.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env = {
      # Point the ollama CLI (and other clients) at the managed server.
      OLLAMA_HOST = connectAddress;

      # OpenAI-compatible endpoint, read by clients such as Pydantic AI.
      OLLAMA_BASE_URL = "http://${connectAddress}/v1";
    };

    processes.ollama = {
      exec = "exec ${ollama} serve";

      env = cfg.environmentVariables // {
        OLLAMA_HOST = "${cfg.host}:${toString allocatedPort}";
        OLLAMA_MODELS = cfg.modelsDir;
      };

      ports.http.allocate = basePort;

      ready.http.get = {
        host = connectHost;
        port = allocatedPort;
        path = "/api/version";
      };
    };

    tasks."devenv:ollama:setup" = {
      exec = "mkdir -p ${lib.escapeShellArg cfg.modelsDir}";
      before = [ "devenv:processes:ollama" ];
    };

    tasks."devenv:ollama:load-models" = lib.mkIf (cfg.loadModels != [ ]) {
      description = "Download Ollama models";
      exec = ''
        set -euo pipefail

        for model in ${lib.escapeShellArgs cfg.loadModels}; do
          if ${ollama} show "$model" >/dev/null 2>&1; then
            echo "$model is already present"
          else
            echo "Pulling $model"
            ${ollama} pull "$model"
          fi
        done
      '';
      env.OLLAMA_HOST = connectAddress;
      after = [ "devenv:processes:ollama@ready" ];
      wantedBy = lib.mkIf supportsWantedBy [ "devenv:processes:ollama" ];
    };
  };
}
