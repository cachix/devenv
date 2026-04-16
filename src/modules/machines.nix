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

  # Resolve a user-provided `hardware.facter` value to a path. Users may pass
  # either a Nix path literal (preferred) or a string relative to the project
  # root, so the option accepts both.
  resolveFacterPath = value:
    if value == null then null
    else if builtins.isPath value then value
    else if lib.hasPrefix "/" value then /. + value
    else /. + (outerConfig.devenv.root + "/" + value);

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
          Extra `ssh -o` options appended after devenv's defaults
          (`StrictHostKeyChecking=accept-new`, `IdentitiesOnly=yes` when an `IdentityFile` is configured,
          and a bounded `ConnectTimeout`). The last value wins, so this list overrides the defaults.
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
                      installers. Accepts a local path or an http(s) URL that is fetched
                      on the remote target.
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
                    type = lib.types.path;
                    description = "Local path to the file to copy onto the installed system.";
                  };
                  owner = lib.mkOption {
                    type = lib.types.str;
                    default = "0:0";
                    description = "Owner in `uid:gid` format applied via chown on the target.";
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
                    source = ./secrets/age.key;
                    owner = "0:0";
                    mode = "0600";
                  };
                }
              '';
            };

            encryptionKeys = lib.mkOption {
              type = lib.types.attrsOf lib.types.path;
              default = { };
              description = ''
                Keyfiles dropped onto the installer **before disko runs**, so LUKS
                layouts with `passwordFile = "/tmp/luks.key"` can unlock. Attribute
                names are absolute target paths on the installer; values are local
                source paths on the host running `devenv machines install`.
              '';
              example = lib.literalExpression ''
                {
                  "/tmp/luks.key" = ./secrets/luks.key;
                }
              '';
            };

            copyHostKeys = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = ''
                Preserve `/etc/ssh/ssh_host_*` across a reinstall by copying
                them from the installer into the installed system before reboot.
                This keeps the target's SSH identity stable so `known_hosts` on
                every client keeps working.
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
    };

    config.build = {
      nixos = lib.mkIf (config.nixos != null) (nixosEval config).config.system.build.toplevel;
      diskoScript = lib.mkIf (config.nixos != null) (nixosEval config).config.system.build.diskoScript;
      diskoFormatScript = lib.mkIf (config.nixos != null) (nixosEval config).config.system.build.formatScript;
      diskoMountScript = lib.mkIf (config.nixos != null) (nixosEval config).config.system.build.mountScript;
      nix-darwin = lib.mkIf (config.nix-darwin != null) (buildNixDarwinToplevel config);
      home-manager = lib.mkIf (config.home-manager != null) (buildHomeManagerToplevel config);
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
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          system = lib.mkOption { type = lib.types.str; };
          target = lib.mkOption {
            type = lib.types.submodule {
              options = {
                host = lib.mkOption { type = lib.types.nullOr lib.types.str; };
                sshOpts = lib.mkOption { type = lib.types.listOf lib.types.str; };
              };
            };
          };
          hasNixos = lib.mkOption { type = lib.types.bool; };
          hasNixDarwin = lib.mkOption { type = lib.types.bool; };
          hasHomeManager = lib.mkOption { type = lib.types.bool; };
          kexecImage = lib.mkOption { type = lib.types.nullOr lib.types.str; };
          kexecPostSshPort = lib.mkOption { type = lib.types.nullOr lib.types.port; };
          copyHostKeys = lib.mkOption { type = lib.types.bool; };
          # extraFiles and encryptionKeys contain Nix paths that can't be
          # JSON-serialised to absolute filesystem paths directly. Instead
          # we expose them as attrsets of { source = "/nix/store/..."; ... }
          # which the Rust CLI can read and pipe to the target via SSH.
          extraFiles = lib.mkOption { type = lib.types.attrsOf lib.types.attrs; };
          encryptionKeys = lib.mkOption { type = lib.types.attrsOf lib.types.str; };
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
      extraFiles = lib.mapAttrs
        (_path: f: {
          source = builtins.toString f.source;
          inherit (f) owner mode;
        })
        m.install.extraFiles;
      encryptionKeys = lib.mapAttrs (_path: src: builtins.toString src) m.install.encryptionKeys;
    })
    config.machines;
}
