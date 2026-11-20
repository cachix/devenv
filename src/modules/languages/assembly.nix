{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.assembly;

  hasPlatform = cfg.toolchain.platform != null;
  hasPackage = cfg.toolchain.package != null;

  assemblers = [ "nasm" "fasm" "yasm" ];
  assemblerOptions = lib.genAttrs assemblers (
    name: {
      enable =
        lib.mkEnableOption
          "${lib.toUpper (lib.initial name)}${lib.tail name} assembler."
        // {
          default =
            name
            == "nasm"
            && !cfg.toolchain.fasm.enable
            && !cfg.toolchain.yasm.enable
            && cfg.toolchain.package == null
            && cfg.toolchain.platform == null;
        };
    }
  );

  # Explicit native assembler selections.
  nasmEnabled = cfg.toolchain.nasm.enable;
  fasmEnabled = cfg.toolchain.fasm.enable;
  yasmEnabled = cfg.toolchain.yasm.enable;
  anyNativeAssemblerEnabled = nasmEnabled || fasmEnabled || yasmEnabled;

  selectedNativeAssembler =
    lib.findFirst
      (name: cfg.toolchain.${name}.enable)
      null
      assemblers;

  # Cross stdenv when a platform is selected.
  crossPkgs =
    if hasPlatform
    then pkgs.pkgsCross.${cfg.toolchain.platform}
    else null;

  # Packages that land in the devshell.
  toolchainPackages =
    lib.optionals hasPlatform [ crossPkgs.stdenv.cc ]
    ++ lib.optionals hasPackage [ cfg.toolchain.package ]
    ++ lib.optionals anyNativeAssemblerEnabled [ pkgs.${selectedNativeAssembler} ];

  # Derived compiler path used for diagnostics when the user did not
  # override lsp.config.compiler.
  derivedCompiler =
    if hasPlatform
    then
    # Prefer the target-prefixed cc from the cross stdenv.
      let
        cc = crossPkgs.stdenv.cc;
        prefix = cc.targetPrefix or "";
      in
      "${cc}/bin/${prefix}cc"
    else null;

  # Effective compiler that ends up in the TOML (user override wins).
  effectiveCompiler =
    if cfg.lsp.config.compiler != null
    then cfg.lsp.config.compiler
    else derivedCompiler;

  # Derive assembler name from the selected toolchain.
  derivedAssembler =
    if hasPlatform
    then "gas" # every pkgsCross stdenv ships GAS
    else if nasmEnabled || yasmEnabled
    then "nasm"
    else lib.optionalString fasmEnabled "fasm" ? null;

  # Effective values that go into the TOML (user overrides win).
  effectiveAssembler =
    if cfg.lsp.config.assembler != null
    then cfg.lsp.config.assembler
    else derivedAssembler;

  effectiveInstructionSet = cfg.lsp.config.instructionSet;

  effectiveDiagnostics =
    if cfg.lsp.config.diagnostics != null
    then cfg.lsp.config.diagnostics
    else hasPlatform;

  # compile_flags_txt semantics (proposal §2):
  #   null  → key omitted entirely (asm-lsp falls back to project files)
  #   []    → key present and empty
  #   [...] → key present with the given flags
  #
  # When the user supplies flags but forgot to put the compiler first,
  # we prepend effectiveCompiler automatically (asm-lsp requires it).
  compileFlagsTxt =
    if cfg.lsp.config.compilerFlags != null
    then cfg.lsp.config.compilerFlags
    else null;

  defaultConfig =
    lib.optionalAttrs (effectiveAssembler != null)
      {
        assembler = effectiveAssembler;
      }
    // lib.optionalAttrs (effectiveInstructionSet != null) {
      instruction_set = effectiveInstructionSet;
    }
    // {
      diagnostics = effectiveDiagnostics;
      default_diagnostics = effectiveDiagnostics;
    };

  opts =
    lib.optionalAttrs (effectiveCompiler != null)
      {
        compiler = effectiveCompiler;
      }
    // lib.optionalAttrs (compileFlagsTxt != null) {
      compile_flags_txt = compileFlagsTxt;
    }
    // {
      diagnostics = effectiveDiagnostics;
    };

  asmLspSettings =
    lib.recursiveUpdate
      {
        default_config = defaultConfig // { inherit opts; };
      }
      cfg.lsp.config.extraSettings;
in
{
  options.languages.assembly = {
    enable = lib.mkEnableOption "tools for Assembly Development.";

    toolchain =
      assemblerOptions
      // {
        platform = lib.mkOption {
          type = lib.types.nullOr (lib.types.enum (builtins.attrNames pkgs.pkgsCross));
          default = null;
          description = ''
            Name of a `pkgs.pkgsCross.*` crossStdenv (e.g. `"riscv64-embedded"`,
            `"aarch64-multiplatform"`).  Resolves the full cross toolchain
            (stdenv.cc + bintools) and derives the corresponding asm-lsp
            `assembler = "gas"` / `instruction_set` values.

            Mutually exclusive with `toolchain.package`.
          '';
        };

        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = null;
          description = ''
            Native assembler package outside of pkgsCross (nasm, fasm, yasm, …).
            Added to the devshell; its name is used to derive the asm-lsp
            `assembler` flavour when possible.

            Mutually exclusive with `toolchain.platform`.
          '';
        };
      };

    lsp = {
      enable =
        lib.mkEnableOption "Assembly Language Server."
        // {
          default = cfg.enable;
        };
      package = lib.mkPackageOption pkgs "asm-lsp" {
        extraDescription = "The Assembly Language Server package to use.";
      };

      config = {
        enable =
          lib.mkEnableOption "generation of `.asm-lsp.toml` at the root of your project."
          // {
            default = cfg.lsp.enable;
          };

        compiler = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          defaultText = lib.literalExpression "null";
          description = ''
            Override for `opts.compiler` in `.asm-lsp.toml`.
            Accepts a binary name present in `$PATH` (e.g. `"zig"`, `"gcc"`)
            or an absolute store path.
          '';
        };

        compilerFlags = lib.mkOption {
          type = lib.types.nullOr (lib.types.listOf lib.types.str);
          default = null;
          defaultText = lib.literalExpression "null";
          description = ''
            Value for `opts.compile_flags_txt`.

            * `null` (default) — the key is **omitted**; asm-lsp falls back
              to discovering `compile_flags.txt` / `compile_commands.json`
              in the project tree.
            * `[]` — the key is written as an empty list (no external
              fallback).
            * `[ "flag1" "flag2" ]` — written literally.  If a compiler is
              known and the first element is not already that compiler, the
              module prepends it automatically (asm-lsp requires `cc` as
              the first entry).
          '';
        };

        assembler = lib.mkOption {
          type = lib.types.nullOr (lib.types.enum [ "gas" "go" "masm" "nasm" "ca65" "avr" "fasm" "mars" ]);
          default = null;
          defaultText = lib.literalExpression "null";
          description = ''
            Override the asm-lsp `assembler` flavor.
            Leave `null` to derive it automatically from `toolchain`.
          '';
        };

        instructionSet = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          defaultText = lib.literalExpression "null";
          description = ''
            Define the asm-lsp `instruction_set`.
          '';
        };

        diagnostics = lib.mkOption {
          type = lib.types.nullOr lib.types.bool;
          default = null;
          defaultText = lib.literalMD "null";
          description = ''
            Enables diagnostics via a C compiler (`-x assembler-with-cpp`,
            GAS syntax only). Leave `null` to use the default derived
            from the selected toolchain.
          '';
        };

        extraSettings = lib.mkOption {
          type = lib.types.attrsOf lib.types.anything;
          default = { };
          description = ''
            Extra settings recursively merged into the generated
            `.asm-lsp.toml` (e.g. to declare `[[project]]` overrides for
            sub-directories, or a `[opts] compile_flags_txt`).
          '';
          example = lib.literalExpression ''
            {
              project = [
                {
                  path = "boot";
                  assembler = "nasm";
                  instruction_set = "x86";
                }
              ];
            }
          '';
        };
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [
        {
          assertion = !(hasPlatform && hasPackage);
          message = ''
            languages.assembly: `toolchain.platform` and `toolchain.package`
            are mutually exclusive.  Set exactly one of them.
          '';
        }
        {
          assertion = !(hasPlatform && anyNativeAssemblerEnabled);
          message = ''
            languages.assembly: `toolchain.platform` cannot be combined
            with `toolchain.nasm.enable`, `toolchain.fasm.enable` or
            `toolchain.yasm.enable`.
            Select either the cross toolchain or a native assembler.
          '';
        }
        {
          assertion = !(hasPackage && anyNativeAssemblerEnabled);
          message = ''
            languages.assembly: `toolchain.package` cannot be combined
            with native assembler enable options.
            Use `toolchain.package` for custom assembler packages, or
            enable exactly one native assembler option.
          '';
        }
        {
          assertion =
            builtins.length
              (lib.filter (enabled: enabled) [
                nasmEnabled
                fasmEnabled
                yasmEnabled
              ])
            <= 1;
          message = ''
            languages.assembly: only one native assembler can be enabled
            at a time. Choose between nasm, fasm and yasm.
          '';
        }
      ];
    }
    {
      warnings =
        lib.optional
          (hasPlatform && effectiveInstructionSet == null)
          ''
            languages.assembly: platform "${cfg.toolchain.platform}" has no
            corresponding asm-lsp instruction_set.  The key will be omitted
            from .asm-lsp.toml.  Set lsp.config.instructionSet manually if
            you need a specific value.
          '';
    }
    {
      packages =
        toolchainPackages
        ++ lib.optional cfg.lsp.enable cfg.lsp.package;

      files = lib.mkIf (cfg.lsp.enable && cfg.lsp.config.enable) {
        ".asm-lsp.toml".toml = asmLspSettings;
      };
    }
  ]);
}
