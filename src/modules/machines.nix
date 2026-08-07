{ pkgs, config, lib, self ? null, inputs ? { }, ... }:

let
  outerConfig = config;
  outputType = outerConfig.lib.types.output;

  # Input metadata used to build helpful errors when a machine references a
  # role or feature whose backing input is missing from devenv.yaml.
  diskoInputArgs = {
    name = "disko";
    url = "github:nix-community/disko";
    attribute = "machines.<name>.nixos";
    follows = [ "nixpkgs" ];
  };

  facterInputArgs = {
    name = "nixos-facter-modules";
    url = "github:nix-community/nixos-facter-modules";
    attribute = "machines.<name>.hardware.facter";
    follows = [ ];
  };

  nixDarwinInputArgs = {
    name = "nix-darwin";
    url = "github:LnL7/nix-darwin";
    attribute = "machines.<name>.nix-darwin";
    follows = [ "nixpkgs" ];
  };

  homeManagerInputArgs = {
    name = "home-manager";
    url = "github:nix-community/home-manager";
    attribute = "machines.<name>.home-manager";
    follows = [ "nixpkgs" ];
  };

  # Lazy input lookups. Each evaluates to `null` when the user has not added
  # the input, which we turn into a targeted error only when a machine
  # actually references the feature that needs it.
  disko = inputs.disko or null;
  facter = inputs.nixos-facter-modules or null;
  nixDarwin = inputs.nix-darwin or null;
  homeManager = inputs.home-manager or null;

  # The CLI and SecretSpec are released together. Use the devenv package from
  # the same source as these modules so target-side resolution cannot silently
  # pick an unrelated (and potentially incompatible) nixpkgs SecretSpec.
  #
  # Flake consumers already provide the full devenv flake. The CLI deliberately
  # uses the lightweight `src/modules` flake, whose sourceInfo still points at
  # the repository root; in that case, load the root flake using its locked NAR
  # hash. This remains lazy unless a NixOS machine enables target resolution.
  devenvPackageFor = system:
    let
      devenvInput = inputs.devenv or (throw "The devenv input is required for target-side SecretSpec resolution");
      fullDevenvInput =
        if devenvInput ? packages then
          devenvInput
        else if devenvInput ? sourceInfo && devenvInput.sourceInfo ? outPath then
          let
            sourceInfo =
              if devenvInput.sourceInfo ? narHash then
                devenvInput.sourceInfo
              else
                builtins.fetchTree {
                  type = "path";
                  path = devenvInput.sourceInfo.outPath;
                };
          in
          builtins.getFlake (builtins.unsafeDiscardStringContext
            "path:${sourceInfo.outPath}?narHash=${sourceInfo.narHash}")
        else
          throw "The devenv input does not expose packages or locked source metadata";
    in
      fullDevenvInput.packages.${system}.devenv or
        (throw "The devenv input does not provide a devenv package for ${system}");

  # Resolve a user-provided `hardware.facter` value to a path. Users may pass
  # either a Nix path literal (preferred) or a string relative to the project
  # root, so the option accepts both.
  resolveFacterPath = value:
    if value == null then null
    else if builtins.isPath value then value
    else if lib.hasPrefix "/" value then /. + value
    else /. + (outerConfig.devenv.root + "/" + value);

  # Resolve a user-provided `install.{extraFiles.source,encryptionKeys}` path
  # string to an absolute filesystem path *without* coercing through a Nix
  # path literal. Using strings throughout is load-bearing: coercing the
  # value to a Nix path would `/nix/store`-import the secret, which is
  # exactly what callers need to avoid for LUKS keys and age identities.
  resolveInstallPath = value:
    if lib.hasPrefix "/" value then value
    else "${outerConfig.devenv.root}/${value}";

  # Build the NixOS toplevel closure for a machine's `nixos` module.
  #
  # Requires `inputs.disko` (always, because disko is the declarative
  # partitioning layer that `devenv machines install` relies on) and, when
  # `hardware.facter` is non-null, `inputs.nixos-facter-modules`. Both errors
  # are lazy: evaluation of a home-manager-only machine does not force these.
  # Shared NixOS evaluation for a machine. Returns the full evaluated NixOS
  # system so callers can extract both `.config.system.build.toplevel` and
  # `.config.system.build.diskoScript` (and any future outputs) without
  # evaluating twice — Nix's lazy evaluation handles deduplication.
  nixosEval = machine:
    if disko == null then
      throw (outerConfig.lib._mkInputError diskoInputArgs)
    else
      let
        facterModules =
          if machine.hardware.facter != null then
            if facter == null then
              throw (outerConfig.lib._mkInputError facterInputArgs)
            else [
              facter.nixosModules.facter
              { facter.reportPath = resolveFacterPath machine.hardware.facter; }
            ]
          else [ ];
      in
      inputs.nixpkgs.lib.nixosSystem {
        specialArgs = { inherit inputs self; };
        modules = [
          { nixpkgs.hostPlatform = machine.system; }
          disko.nixosModules.disko
          machine.nixos
          ({ pkgs, ... }:
            let
              targetDevenv = devenvPackageFor machine.system;
              # Copy only the bundled resolver out of the target-architecture
              # devenv package. The installed system therefore does not retain
              # devenv's larger runtime closure just to run SecretSpec once.
              targetSecretspec = pkgs.runCommand "devenv-bundled-secretspec" { } ''
                install -Dm755 ${targetDevenv}/bin/secretspec $out/bin/secretspec
              '';
              targetSecretspecPackages = machine.install.secretspec.extraPackages pkgs;
              targetSecretspecRuntime = pkgs.buildEnv {
                name = "devenv-machines-secretspec-runtime";
                paths = [ targetSecretspec ] ++ targetSecretspecPackages;
                pathsToLink = [ "/bin" ];
                ignoreCollisions = true;
              };
              targetSecretspecLauncher = pkgs.writeShellScriptBin "devenv-machines-secretspec" ''
                export PATH=${targetSecretspecRuntime}/bin:$PATH
                exec ${targetSecretspec}/bin/secretspec "$@"
              '';
            in
            lib.mkIf
              (machine.install.secrets != { } && machine.install.secretspec.execution == "target")
              {
                # Target-side bootstrap resolution runs after nixos-install but
                # before reboot. The private launcher calls devenv's bundled
                # resolver by exact store path and exposes helpers through its
                # own PATH. Only that launcher enters the system profile:
                # another package cannot replace the resolver, and helpers do
                # not pollute the installed machine's global command namespace.
                environment.systemPackages = [ targetSecretspecLauncher ];
              })
        ] ++ facterModules;
      };

  # Build the nix-darwin toplevel closure for a machine's `nix-darwin` module.
  buildNixDarwinToplevel = machine:
    if nixDarwin == null then
      throw (outerConfig.lib._mkInputError nixDarwinInputArgs)
    else
      (nixDarwin.lib.darwinSystem {
        inherit (machine) system;
        specialArgs = { inherit inputs self; };
        modules = [ machine.nix-darwin ];
      }).config.system.build.toplevel;

  # Build the home-manager activation package for a machine's `home-manager` module.
  buildHomeManagerToplevel = machine:
    if homeManager == null then
      throw (outerConfig.lib._mkInputError homeManagerInputArgs)
    else
      let
        machinePkgs = import inputs.nixpkgs { inherit (machine) system; };
      in
      (homeManager.lib.homeManagerConfiguration {
        pkgs = machinePkgs;
        extraSpecialArgs = { inherit inputs self; };
        modules = [ machine.home-manager ];
      }).activationPackage;

  targetOptions = lib.types.submodule {
    options = {
      host = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          SSH destination used by `devenv machines install` and `devenv machines deploy`.
          Accepts `user@host`, `user@host:port`, or a full `ssh://user@host:port` URI.
          Leave unset to activate in process on the current host; this is only valid for `home-manager`.
          Setting it to `"localhost"` is not the same as omitting it: `"localhost"` still routes through SSH.
        '';
        example = "root@laptop.local";
      };

      sshOpts = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = ''
          Extra `ssh -o` options applied before devenv's defaults
          (`StrictHostKeyChecking=accept-new` and a bounded `ConnectTimeout`).
          OpenSSH keeps the first value for most settings, so this list
          overrides the defaults. Installs that transmit SecretSpec bootstrap
          values, encryption keys, or extra files force stricter
          non-overridable SSH settings.
        '';
        example = lib.literalExpression ''[ "-o" "IdentitiesOnly=yes" "-o" "ConnectTimeout=10" ]'';
      };
    };
  };

  machineOptions = lib.types.submodule ({ name, config, ... }: {
    options = {
      system = lib.mkOption {
        type = lib.types.str;
        description = "System architecture for the machine.";
        default = pkgs.stdenv.system;
        defaultText = lib.literalExpression "pkgs.stdenv.system";
        example = "x86_64-linux";
      };

      target = lib.mkOption {
        type = targetOptions;
        default = { };
        description = "SSH destination for install and deploy. See the `target.host` and `target.sshOpts` suboptions.";
      };

      nixos = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "NixOS configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          {
            fileSystems."/".device = "/dev/sda1";
            boot.loader.systemd-boot.enable = true;
            services.openssh.enable = true;
          }
        '';
      };

      hardware = lib.mkOption {
        type = lib.types.submodule {
          options = {
            facter = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = ".machines/${name}/facter.json";
              defaultText = lib.literalExpression ''".machines/''${name}/facter.json"'';
              description = ''
                Path to the [nixos-facter](https://github.com/nix-community/nixos-facter) hardware report for this machine.

                Accepts either a Nix path literal or a string path relative to the devenv project root.
                Defaults to `.machines/<name>/facter.json`, which `devenv machines install` generates
                on first run by probing the target over SSH. The report must be committed to git so that
                teammates and CI can build the machine closure without reaching the target.

                Set to `null` to opt this machine out of facter, for example when it carries a hand written
                `hardware-configuration.nix` instead. Only applies to machines with a `nixos` module set;
                `nix-darwin` and `home-manager` entries ignore this option.
              '';
              example = lib.literalExpression "./hardware/web1.json";
            };
          };
        };
        default = { };
        description = "Hardware detection settings for this machine.";
      };

      install = lib.mkOption {
        type = lib.types.submodule {
          options = {
            kexec = lib.mkOption {
              type = lib.types.submodule {
                options = {
                  image = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                    description = ''
                      Override the default nixos-images kexec tarball URL. Set this for
                      custom installer images, non-standard architectures, or VPN-enabled
                      installers. Must be an HTTP(S) URL fetched on the remote target.
                    '';
                    example = "https://example.com/custom-kexec-aarch64.tar.gz";
                  };

                  postSshPort = lib.mkOption {
                    type = lib.types.nullOr lib.types.port;
                    default = null;
                    description = ''
                      SSH port to reconnect to after kexec lands in the installer. Set
                      this when the live sshd on the target listens on a non-22 port. All
                      subsequent install phases (facter, disko, nixos-install) use this
                      port.
                    '';
                    example = 2222;
                  };
                };
              };
              default = { };
              description = "Kexec override settings for `devenv machines install`.";
            };

            extraFiles = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options = {
                  source = lib.mkOption {
                    type = lib.types.str;
                    description = ''
                      Local path to the file to copy onto the installed system.
                      Accepts an absolute path or a path relative to the devenv project root.
                      Must be a string (not a Nix path literal) so secrets stay on the host
                      running `devenv machines install` and never enter `/nix/store`.
                    '';
                  };
                  owner = lib.mkOption {
                    type = lib.types.str;
                    default = "0:0";
                    description = "Numeric owner in `uid:gid` format applied via chown on the target.";
                  };
                  mode = lib.mkOption {
                    type = lib.types.str;
                    default = "0644";
                    description = "File mode applied via chmod on the target.";
                  };
                };
              });
              default = { };
              description = ''
                Files to copy onto the installed system after `nixos-install` but
                before reboot. Attribute names are absolute target paths (e.g.
                `"/var/lib/secret-age-key"`). Use this for age/sops master keys,
                SSH host keys, or `/var/lib/*` seeds that need to exist on first boot.
                Matches nixos-anywhere's `--extra-files` and `--chown`.
              '';
              example = lib.literalExpression ''
                {
                  "/var/lib/secret-age-key" = {
                    source = "secrets/age.key";
                    owner = "0:0";
                    mode = "0600";
                  };
                }
              '';
            };

            secretspec = lib.mkOption {
              type = lib.types.submodule {
                options = {
                  execution = lib.mkOption {
                    type = lib.types.enum [ "local" "target" ];
                    default = "local";
                    description = ''
                      Where SecretSpec resolves bootstrap values. `local`
                      resolves on the workstation and streams values over SSH.
                      `target` sends only a self-contained SecretSpec manifest and
                      resolves through the provider on the installer machine.
                    '';
                  };
                  provider = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                    description = ''
                      Explicit target-side provider override. When unset, devenv
                      does not pass `--provider`; SecretSpec selects providers
                      from the manifest, target environment, and target-global
                      configuration. The workstation provider is never inherited.
                      Provider credentials must be available independently on the
                      installer and are never forwarded by devenv. This option is
                      non-secret metadata and must not contain credentials; use a
                      provider alias or target-global configuration instead.
                    '';
                  };
                  profile = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                    description = ''
                      Explicit target-side profile override. When unset, devenv
                      does not pass `--profile`; SecretSpec selects the profile
                      from the target environment or target-global configuration,
                      falling back to `default`. The workstation profile is never
                      inherited.
                    '';
                  };
                  extraPackages = lib.mkOption {
                    type = lib.types.functionTo (lib.types.listOf lib.types.package);
                    default = _targetPkgs: [ ];
                    defaultText = lib.literalExpression "_targetPkgs: [ ]";
                    example = lib.literalExpression "targetPkgs: [ targetPkgs.sops targetPkgs.pass ]";
                    description = ''
                      Function selecting additional target-architecture packages
                      required by the chosen SecretSpec providers, such as `sops`
                      or `pass`. These packages are included in a private resolver
                      runtime for target execution, not the system-wide command
                      namespace.
                    '';
                  };
                };
              };
              default = { };
              description = "How install-time SecretSpec bootstrap values are resolved.";
            };

            secrets = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options = {
                  secret = lib.mkOption {
                    type = lib.types.str;
                    description = ''
                      Name of the secret in the active SecretSpec profile. The
                      value is resolved at install time according to
                      `install.secretspec.execution` and is never included in
                      machine metadata or a Nix store path.
                    '';
                    example = "WEB1_AGE_KEY";
                  };
                  owner = lib.mkOption {
                    type = lib.types.str;
                    default = "0:0";
                    description = "Numeric owner in `uid:gid` format applied via chown on the target.";
                  };
                  mode = lib.mkOption {
                    type = lib.types.str;
                    default = "0600";
                    description = ''
                      File mode applied via chmod on the target. Special and
                      execute bits, group write, and every permission for
                      other users are rejected. Modes such as 0400, 0600, and
                      0640 are accepted.
                    '';
                  };
                };
              });
              default = { };
              description = ''
                Bootstrap files populated from the active SecretSpec profile
                after `nixos-install` and before reboot. Attribute names are
                absolute paths in the installed system. Local execution streams
                values over strictly authenticated SSH; target execution sends
                only a self-contained declaration manifest and fetches values
                on the installer.
                Both modes atomically install files without placing values in
                `/nix/store`.

                Local execution requires SecretSpec to be enabled in
                `devenv.yaml`; use the global `--secretspec-provider` and
                `--secretspec-profile` flags to select its source. Target
                execution only requires `secretspec.toml` and leaves provider
                and profile selection to the target unless explicitly overridden
                by `install.secretspec`.
              '';
              example = lib.literalExpression ''
                {
                  "/var/lib/sops-nix/key.txt" = {
                    secret = "WEB1_AGE_KEY";
                    owner = "0:0";
                    mode = "0600";
                  };
                }
              '';
            };

            encryptionKeys = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = { };
              description = ''
                Keyfiles dropped onto the installer **before disko runs**, so LUKS
                layouts with `passwordFile = "/tmp/luks.key"` can unlock. Attribute
                names are absolute target paths on the installer; values are local
                source paths (string, absolute or relative to the devenv project root)
                on the host running `devenv machines install`. Strings — not Nix path
                literals — so LUKS keys never enter `/nix/store`.
              '';
              example = lib.literalExpression ''
                {
                  "/tmp/luks.key" = "secrets/luks.key";
                }
              '';
            };

            copyHostKeys = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = ''
                Copy `/etc/ssh/ssh_host_*` from the live installer into the
                installed system before reboot. This keeps the SSH identity
                stable between the post-kexec installer and the first boot, so
                that identity's `known_hosts` entry keeps working.
              '';
            };
          };
        };
        default = { };
        description = "Install-time settings for `devenv machines install`.";
      };

      home-manager = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "Home Manager configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          {
            home.username = "jdoe";
            home.homeDirectory = "/home/jdoe";
            programs.git.enable = true;
          }
        '';
      };

      nix-darwin = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "nix-darwin configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          { pkgs, ... }: {
            environment.systemPackages = [
              pkgs.vim
            ];
            services.nix-daemon.enable = true;
          }
        '';
      };

      # Computed build outputs. These are picked up by the `build` walker in
      # `devenv-nix-backend/bootstrap/bootstrapLib.nix` because the walker
      # recurses into `attrsOf submodule` options and collects output-typed
      # sub-options. The result is that
      #     devenv build machines.<name>
      # builds every defined role for that machine (flattening to paths like
      # `machines.<name>.build.nixos`), and
      #     devenv build machines.<name>.build.<role>
      # builds a single role.
      build.nixos = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Built NixOS system toplevel for this machine, or null if no `nixos` module is set.";
      };

      build.nix-darwin = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Built nix-darwin system toplevel for this machine, or null if no `nix-darwin` module is set.";
      };

      build.home-manager = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Built home-manager activation package for this machine, or null if no `home-manager` module is set.";
      };

      build.diskoScript = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Disko partitioning script (destroy + create + mount) for this machine.";
      };

      build.diskoFormatScript = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Disko format script (create without destroy) for this machine.";
      };

      build.diskoMountScript = lib.mkOption {
        type = outputType;
        default = null;
        internal = true;
        description = "Disko mount script (mount only, no partitioning) for this machine.";
      };

      # Shared lazy NixOS evaluation, set once per machine when `nixos != null`.
      # Every `build.*` and `installCheck` consumer reads through this option
      # so `nixosSystem { … }` runs exactly once, instead of once per output.
      _nixosEval = lib.mkOption {
        type = lib.types.unspecified;
        internal = true;
        visible = false;
        default = null;
        description = "Shared internal NixOS evaluation result for this machine.";
      };

      installCheck = lib.mkOption {
        visible = false;
        type = lib.types.submodule {
          options = {
            hasRootAuth = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Whether the evaluated NixOS root account has a password or authorized SSH key.";
            };
          };
        };
        internal = true;
        default = { };
      };
    };

    # Evaluate NixOS once per machine. The thunk stays unforced while
    # `config.nixos == null` because every `build.*` and `installCheck`
    # consumer guards access behind `lib.mkIf (config.nixos != null) …`.
    config._nixosEval = lib.mkIf (config.nixos != null) (nixosEval config);

    config.build = {
      nixos = lib.mkIf (config.nixos != null) config._nixosEval.config.system.build.toplevel;
      diskoScript = lib.mkIf (config.nixos != null) config._nixosEval.config.system.build.diskoScript;
      diskoFormatScript = lib.mkIf (config.nixos != null) config._nixosEval.config.system.build.formatScript;
      diskoMountScript = lib.mkIf (config.nixos != null) config._nixosEval.config.system.build.mountScript;
      nix-darwin = lib.mkIf (config.nix-darwin != null) (buildNixDarwinToplevel config);
      home-manager = lib.mkIf (config.home-manager != null) (buildHomeManagerToplevel config);
    };

    # Lazy: only forced when the CLI reads `installCheck.hasRootAuth`
    # (i.e. during `devenv machines install`), never during `info`/`deploy`.
    # For home-manager-only machines the default `hasRootAuth = false` stays.
    config.installCheck = lib.mkIf (config.nixos != null) {
      hasRootAuth =
        let
          root = config._nixosEval.config.users.users.root or { };
          keys = root.openssh.authorizedKeys.keys or [ ];
          keyFiles = root.openssh.authorizedKeys.keyFiles or [ ];
          hasPassword =
            (root.hashedPassword or null) != null
            || (root.initialHashedPassword or null) != null
            || (root.hashedPasswordFile or null) != null
            || (root.password or null) != null
            || (root.initialPassword or null) != null;
        in
        keys != [ ] || keyFiles != [ ] || hasPassword;
    };
  });
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "configurations" ] [ "machines" ])
  ];

  options = {
    machines = lib.mkOption {
      type = lib.types.attrsOf machineOptions;
      default = { };
      description = "Machines for NixOS, home-manager, and nix-darwin.";
    };

    # Internal metadata surface consumed by `devenv machines deploy` in the
    # Rust CLI. Separate from `machines` so that we can eval it cheaply in
    # one call without forcing `build.*` (which would trigger disko/facter
    # input-missing errors on unrelated machines) and without trying to
    # serialise user-supplied NixOS module functions through `devenv eval`.
    machinesMeta = lib.mkOption {
      visible = false;
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          system = lib.mkOption {
            type = lib.types.str;
            description = "Machine system copied into CLI metadata.";
          };
          target = lib.mkOption {
            description = "Machine SSH target copied into CLI metadata.";
            type = lib.types.submodule {
              options = {
                host = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  description = "SSH host copied into CLI metadata.";
                };
                sshOpts = lib.mkOption {
                  type = lib.types.listOf lib.types.str;
                  description = "SSH options copied into CLI metadata.";
                };
              };
            };
          };
          hasNixos = lib.mkOption {
            type = lib.types.bool;
            description = "Whether the machine defines a NixOS role.";
          };
          hasNixDarwin = lib.mkOption {
            type = lib.types.bool;
            description = "Whether the machine defines a nix-darwin role.";
          };
          hasHomeManager = lib.mkOption {
            type = lib.types.bool;
            description = "Whether the machine defines a home-manager role.";
          };
          kexecImage = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            description = "Kexec image override copied into CLI metadata.";
          };
          kexecPostSshPort = lib.mkOption {
            type = lib.types.nullOr lib.types.port;
            description = "Post-kexec SSH port copied into CLI metadata.";
          };
          copyHostKeys = lib.mkOption {
            type = lib.types.bool;
            description = "Whether install should preserve SSH host keys.";
          };
          secretspec = lib.mkOption {
            description = "SecretSpec bootstrap execution metadata.";
            type = lib.types.submodule {
              options = {
                execution = lib.mkOption {
                  type = lib.types.enum [ "local" "target" ];
                  description = "Where bootstrap values are resolved.";
                };
                provider = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  description = "Target-side SecretSpec provider override.";
                };
                profile = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  description = "Target-side SecretSpec profile override.";
                };
              };
            };
          };
          # Only SecretSpec names and target metadata cross the Nix/Rust
          # boundary. Values stay either in the CLI's resolved state or on
          # the installer, according to the selected execution mode.
          secrets = lib.mkOption {
            type = lib.types.listOf lib.types.attrs;
            description = "Normalized SecretSpec bootstrap references for CLI consumption.";
          };
          # extraFiles.source and encryptionKeys values are resolved to
          # absolute filesystem path strings by `resolveInstallPath`. They
          # never enter `/nix/store` — critical for LUKS keys and age
          # identities.
          extraFiles = lib.mkOption {
            type = lib.types.attrsOf lib.types.attrs;
            description = "Normalized install-time extra files for CLI consumption.";
          };
          encryptionKeys = lib.mkOption {
            type = lib.types.attrsOf lib.types.str;
            description = "Normalized install-time encryption keys for CLI consumption.";
          };
        };
      });
      internal = true;
      default = { };
      description = "Metadata surface for `devenv machines` CLI consumption. Internal.";
    };
  };

  config.machinesMeta = lib.mapAttrs
    (_name: m: {
      inherit (m) system;
      target = { inherit (m.target) host sshOpts; };
      hasNixos = m.nixos != null;
      hasNixDarwin = m.nix-darwin != null;
      hasHomeManager = m.home-manager != null;
      kexecImage = m.install.kexec.image;
      kexecPostSshPort = m.install.kexec.postSshPort;
      copyHostKeys = m.install.copyHostKeys;
      secretspec = {
        inherit (m.install.secretspec) execution;
        provider =
          if m.install.secretspec.execution == "target" then
            m.install.secretspec.provider
          else
            outerConfig.secretspec.provider;
        profile =
          if m.install.secretspec.execution == "target" then
            m.install.secretspec.profile
          else
            outerConfig.secretspec.profile;
      };
      secrets = lib.mapAttrsToList
        (target: secret: secret // { inherit target; })
        m.install.secrets;
      extraFiles = lib.mapAttrs
        (_path: f: {
          source = resolveInstallPath f.source;
          inherit (f) owner mode;
        })
        m.install.extraFiles;
      encryptionKeys = lib.mapAttrs (_path: src: resolveInstallPath src) m.install.encryptionKeys;
    })
    config.machines;
}
