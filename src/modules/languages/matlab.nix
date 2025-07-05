{ pkgs, config, lib, ... }:

let
  cfg = config.languages.matlab;
in
{
  options.languages.matlab = {
    enable = lib.mkEnableOption "tools for MATLAB development";

    interpreter = lib.mkOption {
      type = lib.types.enum [ "octave" "scilab" ];
      default = "octave";
      description = "MATLAB-compatible interpreter to use.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = if cfg.interpreter == "octave" then pkgs.octave else pkgs.scilab-bin;
      defaultText = lib.literalExpression "pkgs.octave or pkgs.scilab-bin";
      description = "The MATLAB-compatible interpreter package to use.";
    };

    octavePackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = "List of Octave packages to install when using Octave interpreter.";
      example = lib.literalExpression ''
        with pkgs.octavePackages; [
          statistics
          signal
          control
          symbolic
          image
        ]
      '';
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable MATLAB development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable matlab-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.matlab-language-server;
          defaultText = lib.literalExpression "pkgs.matlab-language-server";
          description = "The MATLAB language server package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = ''
            Enable matlab formatter.
          '';
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable matlab linter.";
        };
        missHit = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable miss-hit linter.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.python3Packages.miss-hit;
            defaultText = lib.literalExpression "pkgs.python3Packages.miss-hit";
            description = "The miss-hit package to use.";
          };
        };
      };

      jupyter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable Jupyter kernel support for MATLAB/Octave.";
        };
        octaveKernel = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable Octave Jupyter kernel when using Octave interpreter.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.octave-kernel;
            defaultText = lib.literalExpression "pkgs.octave-kernel";
            description = "The Octave Jupyter kernel package to use.";
          };
        };
      };

      fileFormat = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable MATLAB file format support tools.";
        };
        matio = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable matio library for reading/writing MATLAB MAT files.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.matio;
            defaultText = lib.literalExpression "pkgs.matio";
            description = "The matio package to use.";
          };
        };
      };

      ide = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable IDE support for scientific computing.";
        };
        spyder = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable Spyder IDE (MATLAB-like IDE for Python/scientific computing).";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.spyder;
            defaultText = lib.literalExpression "pkgs.spyder";
            description = "The Spyder IDE package to use.";
          };
        };
      };
    };

    alternatives = {
      sage = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Include SageMath as an alternative mathematical computing environment.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.sage;
          defaultText = lib.literalExpression "pkgs.sage";
          description = "The SageMath package to use.";
        };
      };

      maxima = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Include Maxima computer algebra system.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.maxima;
          defaultText = lib.literalExpression "pkgs.maxima";
          description = "The Maxima package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ (lib.optionals (cfg.interpreter == "octave") cfg.octavePackages)
    ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optionals cfg.dev.linter.enable (
          lib.optional cfg.dev.linter.missHit.enable cfg.dev.linter.missHit.package
        ) ++
        lib.optionals cfg.dev.jupyter.enable (
          lib.optional (cfg.interpreter == "octave" && cfg.dev.jupyter.octaveKernel.enable) cfg.dev.jupyter.octaveKernel.package
        ) ++
        lib.optionals cfg.dev.fileFormat.enable (
          lib.optional cfg.dev.fileFormat.matio.enable cfg.dev.fileFormat.matio.package
        ) ++
        lib.optionals cfg.dev.ide.enable (
          lib.optional cfg.dev.ide.spyder.enable cfg.dev.ide.spyder.package
        )
    ) ++ lib.optionals cfg.alternatives.sage.enable [ cfg.alternatives.sage.package ]
    ++ lib.optionals cfg.alternatives.maxima.enable [ cfg.alternatives.maxima.package ];

    env = {
      # Set MATLAB-compatible environment variables for Octave
      OCTAVE_HISTFILE = lib.optionalString (cfg.interpreter == "octave")
        "\${DEVENV_STATE}/octave_hist";

      # Set library path for matio if enabled
      MATIO_LIBRARY_PATH = lib.optionalString (cfg.dev.fileFormat.enable && cfg.dev.fileFormat.matio.enable)
        "${cfg.dev.fileFormat.matio.package}/lib";
    };

    enterShell = ''
      echo "MATLAB-compatible development environment activated!"
      echo "Primary interpreter: ${cfg.interpreter}"
      ${if cfg.interpreter == "octave" then ''
        echo "Octave: $(command -v octave 2>/dev/null && octave --version | head -1 || echo 'octave not found')"
        ${lib.optionalString (cfg.octavePackages != [ ]) ''
          echo "Octave packages: ${toString (lib.length cfg.octavePackages)} packages installed"
        ''}
      '' else ''
        echo "Scilab: $(command -v scilab 2>/dev/null && echo 'scilab available' || echo 'scilab not found')"
      ''}
      
      ${lib.optionalString cfg.dev.enable ''
        echo "Development tools:"
        ${lib.optionalString cfg.dev.lsp.enable ''
          echo "  - MATLAB Language Server: $(command -v matlab-language-server 2>/dev/null && echo 'available' || echo 'not found')"
        ''}
        ${lib.optionalString (cfg.dev.linter.enable && cfg.dev.linter.missHit.enable) ''
          echo "  - MISS_HIT analyzer: $(command -v mh_style 2>/dev/null && echo 'available' || echo 'not found')"
        ''}
        ${lib.optionalString (cfg.dev.jupyter.enable && cfg.interpreter == "octave" && cfg.dev.jupyter.octaveKernel.enable) ''
          echo "  - Octave Jupyter kernel: $(command -v octave_kernel 2>/dev/null && echo 'available' || echo 'check jupyter kernelspec list')"
        ''}
        ${lib.optionalString (cfg.dev.fileFormat.enable && cfg.dev.fileFormat.matio.enable) ''
          echo "  - MATLAB file I/O: matio library available"
        ''}
        ${lib.optionalString (cfg.dev.ide.enable && cfg.dev.ide.spyder.enable) ''
          echo "  - Spyder IDE: $(command -v spyder 2>/dev/null && echo 'available' || echo 'not found')"
        ''}
      ''}
      
      ${lib.optionalString (cfg.alternatives.sage.enable || cfg.alternatives.maxima.enable) ''
        echo "Alternative mathematical tools:"
        ${lib.optionalString cfg.alternatives.sage.enable ''
          echo "  - SageMath: $(command -v sage 2>/dev/null && echo 'available' || echo 'not found')"
        ''}
        ${lib.optionalString cfg.alternatives.maxima.enable ''
          echo "  - Maxima: $(command -v maxima 2>/dev/null && echo 'available' || echo 'not found')"
        ''}
      ''}
      
      echo ""
      echo "Quick start:"
      ${if cfg.interpreter == "octave" then ''
        echo "  Start Octave: octave"
        echo "  Run script: octave script.m"
        echo "  Interactive: octave --no-gui"
      '' else ''
        echo "  Start Scilab: scilab"
        echo "  Run script: scilab -f script.sce"
      ''}
      ${lib.optionalString (cfg.dev.linter.enable && cfg.dev.linter.missHit.enable) ''
        echo "  Check style: mh_style *.m"
        echo "  Check bugs: mh_bug *.m"
      ''}
      ${lib.optionalString (cfg.dev.jupyter.enable && cfg.interpreter == "octave") ''
        echo "  Jupyter: jupyter notebook (Octave kernel available)"
      ''}
    '';
  };
}
