# Based on a modified version of docker.nix from NixOS/nix, licensed under LGPL-3.0
# https://github.com/NixOS/nix/blob/5b15544bdd13c31688d6f243c80d1751bd8f0de2/docker.nix
{
  # Core dependencies
  pkgs ? import <nixpkgs> { }
, lib ? pkgs.lib
, dockerTools ? pkgs.dockerTools
, runCommand ? pkgs.runCommand
, buildPackages ? pkgs.buildPackages
, # Image configuration
  name ? "devenv"
, tag ? "latest"
, bundleNixpkgs ? true
, channelName ? "nixpkgs"
, channelURL ? "https://nixos.org/channels/nixpkgs-unstable"
, extraPkgs ? [ ]
, maxLayers ? 70
, nixConf ? { }
, flake-registry ? null
, uid ? 0
, gid ? 0
, uname ? "root"
, gname ? "root"
, Labels ? { }
, Cmd ? [ (lib.getExe bashInteractive) ]
, # Default Packages
  nix ? pkgs.nix
, bashInteractive ? pkgs.bashInteractive
, coreutils-full ? pkgs.coreutils-full
, cacert ? pkgs.cacert
, iana-etc ? pkgs.iana-etc
, gitMinimal ? pkgs.gitMinimal
, # Devcontainer support
  enableSudo ? false        # Enable passwordless sudo for non-root user
, enableLocale ? false      # Enable locale configuration
, locale ? "en_US.UTF-8"    # Locale to configure
, extraEnv ? [ ]            # Additional environment variables
, enableZsh ? false         # Enable zsh with Oh My Zsh
, zshTheme ? "devcontainers" # Oh My Zsh theme name
, # Shell configuration scripts (paths to files)
  rcSnippet ? null          # Path to rc_snippet.sh
, bashThemeSnippet ? null   # Path to bash_theme_snippet.sh
, zshThemeFile ? null       # Path to custom zsh theme file
, # Direnv configuration
  enableDirenv ? false      # Enable direnv with shell hooks
, direnvWhitelist ? [ ]     # Paths to whitelist in direnv config
, # Other dependencies
  shadow ? pkgs.shadow
,
}:
let
  # Custom zsh theme derivation
  customZshTheme = runCommand "zsh-theme-${zshTheme}" {} ''
    mkdir -p $out/share/oh-my-zsh/custom/themes
    cp ${zshThemeFile} $out/share/oh-my-zsh/custom/themes/${zshTheme}.zsh-theme
  '';

  # Combine oh-my-zsh with custom theme if provided
  ohMyZsh =
    if zshThemeFile != null
    then pkgs.symlinkJoin {
      name = "oh-my-zsh-with-theme";
      paths = [ pkgs.oh-my-zsh customZshTheme ];
    }
    else pkgs.oh-my-zsh;

  defaultPkgs = [
    nix
    bashInteractive
    coreutils-full
    cacert.out
    iana-etc
    gitMinimal
  ]
  ++ lib.optionals enableSudo [ pkgs.sudo ]
  ++ lib.optionals enableLocale [ pkgs.glibcLocales ]
  ++ lib.optionals enableZsh [ pkgs.zsh ohMyZsh ]
  ++ lib.optionals enableDirenv [ pkgs.direnv ]
  ++ extraPkgs;

  users = {

    root = {
      uid = 0;
      shell = lib.getExe bashInteractive;
      home = "/root";
      gid = 0;
      groups = [ "root" ];
      description = "System administrator";
    };

    nobody = {
      uid = 65534;
      shell = lib.getExe' shadow "nologin";
      home = "/var/empty";
      gid = 65534;
      groups = [ "nobody" ];
      description = "Unprivileged account (don't use!)";
    };

  }
  // lib.optionalAttrs (uid != 0) {
    "${uname}" = {
      inherit uid;
      shell = if enableZsh then lib.getExe pkgs.zsh else lib.getExe bashInteractive;
      home = "/home/${uname}";
      inherit gid;
      groups = [ "${gname}" ] ++ lib.optionals enableSudo [ "sudo" ];
      description = "Nix user";
    };
  }
  // lib.listToAttrs (
    map
      (n: {
        name = "nixbld${toString n}";
        value = {
          uid = 30000 + n;
          gid = 30000;
          groups = [ "nixbld" ];
          description = "Nix build user ${toString n}";
        };
      })
      (lib.lists.range 1 32)
  );

  groups = {
    root.gid = 0;
    nixbld.gid = 30000;
    nobody.gid = 65534;
  }
  // lib.optionalAttrs (gid != 0) {
    "${gname}".gid = gid;
  }
  // lib.optionalAttrs enableSudo {
    sudo.gid = 27;
  };

  userToPasswd =
    k:
    { uid
    , gid ? 65534
    , home ? "/var/empty"
    , description ? ""
    , shell ? "/bin/false"
    , groups ? [ ]
    ,
    }:
    "${k}:x:${toString uid}:${toString gid}:${description}:${home}:${shell}";
  passwdContents = lib.concatStringsSep "\n" (lib.attrValues (lib.mapAttrs userToPasswd users));

  userToShadow = k: _: "${k}:!:1::::::";
  shadowContents = lib.concatStringsSep "\n" (lib.attrValues (lib.mapAttrs userToShadow users));

  # Map groups to members
  # {
  #   group = [ "user1" "user2" ];
  # }
  groupMemberMap =
    let
      # Create a flat list of user/group mappings
      mappings = builtins.foldl'
        (
          acc: user:
            let
              groups = users.${user}.groups or [ ];
            in
            acc
            ++ map
              (group: {
                inherit user group;
              })
              groups
        ) [ ]
        (lib.attrNames users);
    in
    builtins.foldl'
      (
        acc: v:
          acc
          // {
            ${v.group} = acc.${v.group} or [ ] ++ [ v.user ];
          }
      )
      { }
      mappings;

  groupToGroup =
    k:
    { gid }:
    let
      members = groupMemberMap.${k} or [ ];
    in
    "${k}:x:${toString gid}:${lib.concatStringsSep "," members}";
  groupContents = lib.concatStringsSep "\n" (lib.attrValues (lib.mapAttrs groupToGroup groups));

  toConf =
    with pkgs.lib.generators;
    toKeyValue {
      mkKeyValue = mkKeyValueDefault
        {
          mkValueString = v: if lib.isList v then lib.concatStringsSep " " v else mkValueStringDefault { } v;
        } " = ";
    };

  nixConfContents = toConf (
    {
      build-users-group = "nixbld";
      experimental-features = [
        "nix-command"
        "flakes"
      ];
      # `filter-syscalls` controls a security feature that prevents builders from creating setuid binaries.
      # On multi-user systems, this would allow for root privilege escalation.
      # For our container use-case it's not much of a concern.
      # The feature is disabled by default because not all container hosts support seccomp emulation.
      filter-syscalls = false;
      max-jobs = "auto";
      sandbox = false;
      trusted-public-keys = [ "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=" ];
    }
    // nixConf
  );

  userHome = if uid == 0 then "/root" else "/home/${uname}";

  baseSystem =
    let
      nixpkgs = pkgs.path;
      channel = runCommand "channel-nixos" { inherit bundleNixpkgs; } ''
        mkdir $out
        if [ "$bundleNixpkgs" ]; then
          ln -s ${
            builtins.path {
              path = nixpkgs;
              name = "source";
            }
          } $out/nixpkgs
          echo "[]" > $out/manifest.nix
        fi
      '';
      # doc/manual/source/command-ref/files/manifest.nix.md
      manifest = buildPackages.runCommand "manifest.nix" { } ''
        cat > $out <<EOF
        [
        ${lib.concatStringsSep "\n" (
          builtins.map (
            drv:
            let
              outputs = drv.outputsToInstall or [ "out" ];
            in
            ''
              {
                ${lib.concatStringsSep "\n" (
                  builtins.map (output: ''
                    ${output} = { outPath = "${lib.getOutput output drv}"; };
                  '') outputs
                )}
                outputs = [ ${lib.concatStringsSep " " (builtins.map (x: "\"${x}\"") outputs)} ];
                name = "${drv.name}";
                outPath = "${drv}";
                system = "${drv.system}";
                type = "derivation";
                meta = { };
              }
            ''
          ) defaultPkgs
        )}
        ]
        EOF
      '';
      profile = buildPackages.buildEnv {
        name = "root-profile-env";
        paths = defaultPkgs;

        postBuild = ''
          mv $out/manifest $out/manifest.nix
        '';
        inherit manifest;
      };
      flake-registry-path =
        if (flake-registry == null) then
          null
        else if (builtins.readFileType (toString flake-registry)) == "directory" then
          "${flake-registry}/flake-registry.json"
        else
          flake-registry;
    in
    runCommand "base-system"
      {
        inherit
          passwdContents
          groupContents
          shadowContents
          nixConfContents
          ;
        passAsFile = [
          "passwdContents"
          "groupContents"
          "shadowContents"
          "nixConfContents"
        ];
        allowSubstitutes = false;
        preferLocalBuild = true;
      }
      (
        ''
          env
          set -x
          mkdir -p $out/etc

          # may get replaced by pkgs.dockerTools.caCertificates
          mkdir -p $out/etc/ssl/certs
          # Old NixOS compatibility.
          ln -s /nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt $out/etc/ssl/certs
          # NixOS canonical location
          ln -s /nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt $out/etc/ssl/certs/ca-certificates.crt

          cat $passwdContentsPath > $out/etc/passwd
          echo "" >> $out/etc/passwd

          cat $groupContentsPath > $out/etc/group
          echo "" >> $out/etc/group

          cat $shadowContentsPath > $out/etc/shadow
          echo "" >> $out/etc/shadow

          mkdir -p $out/usr
          ln -s /nix/var/nix/profiles/share $out/usr/

          mkdir -p $out/nix/var/nix/gcroots

          mkdir $out/tmp

          mkdir -p $out/var/tmp

          mkdir -p $out/etc/nix
          cat $nixConfContentsPath > $out/etc/nix/nix.conf

          mkdir -p $out${userHome}
          mkdir -p $out/nix/var/nix/profiles/per-user/${uname}

          # see doc/manual/source/command-ref/files/profiles.md
          ln -s ${profile} $out/nix/var/nix/profiles/default-1-link
          ln -s /nix/var/nix/profiles/default-1-link $out/nix/var/nix/profiles/default
          ln -s /nix/var/nix/profiles/default $out${userHome}/.nix-profile

          # see doc/manual/source/command-ref/files/channels.md
          ln -s ${channel} $out/nix/var/nix/profiles/per-user/${uname}/channels-1-link
          ln -s /nix/var/nix/profiles/per-user/${uname}/channels-1-link $out/nix/var/nix/profiles/per-user/${uname}/channels

          # see doc/manual/source/command-ref/files/default-nix-expression.md
          mkdir -p $out${userHome}/.nix-defexpr
          ln -s /nix/var/nix/profiles/per-user/${uname}/channels $out${userHome}/.nix-defexpr/channels
          echo "${channelURL} ${channelName}" > $out${userHome}/.nix-channels

          # may get replaced by pkgs.dockerTools.binSh & pkgs.dockerTools.usrBinEnv
          mkdir -p $out/bin $out/usr/bin
          ln -s ${lib.getExe' coreutils-full "env"} $out/usr/bin/env
          ln -s ${lib.getExe bashInteractive} $out/bin/sh

        ''
        + lib.optionalString enableSudo ''
          # Configure passwordless sudo for the user
          mkdir -p $out/etc/sudoers.d
          echo "${uname} ALL=(ALL) NOPASSWD:ALL" > $out/etc/sudoers.d/${uname}
        ''
        + lib.optionalString (rcSnippet != null || bashThemeSnippet != null) ''
          # Generate .bashrc
          cat > $out${userHome}/.bashrc <<'BASHRC'
          # ~/.bashrc: executed by bash for non-login shells.
          [ -z "$PS1" ] && return

          # Source global definitions
          if [ -f /etc/bashrc ]; then
              . /etc/bashrc
          fi

          BASHRC
          ${lib.optionalString (rcSnippet != null) ''
          cat ${rcSnippet} >> $out${userHome}/.bashrc
          ''}
          ${lib.optionalString (bashThemeSnippet != null) ''
          cat ${bashThemeSnippet} >> $out${userHome}/.bashrc
          ''}
        ''
        + lib.optionalString enableZsh ''
          # Generate .zshrc with Oh My Zsh configuration
          cat > $out${userHome}/.zshrc <<'ZSHRC'
          export ZSH="/nix/var/nix/profiles/default/share/oh-my-zsh"
          ZSH_THEME="${zshTheme}"
          plugins=(git)
          source $ZSH/oh-my-zsh.sh
          zstyle ':omz:update' mode disabled
          ZSHRC
          ${lib.optionalString (rcSnippet != null) ''
          cat ${rcSnippet} >> $out${userHome}/.zshrc
          ''}
        ''
        + lib.optionalString enableDirenv ''
          # Configure direnv
          mkdir -p $out${userHome}/.config/direnv
          cat > $out${userHome}/.config/direnv/config.toml <<'DIRENV_CONFIG'
          ${lib.optionalString (direnvWhitelist != []) ''
          [whitelist]
          prefix = [ ${lib.concatMapStringsSep ", " (p: "\"${p}\"") direnvWhitelist} ]
          ''}
          DIRENV_CONFIG

          # Add direnv hook to .bashrc
          echo 'eval "$(direnv hook bash)"' >> $out${userHome}/.bashrc

          ${lib.optionalString enableZsh ''
          # Add direnv hook to .zshrc
          echo 'eval "$(direnv hook zsh)"' >> $out${userHome}/.zshrc
          ''}
        ''
        + (lib.optionalString (flake-registry-path != null) ''
          nixCacheDir="${userHome}/.cache/nix"
          mkdir -p $out$nixCacheDir
          globalFlakeRegistryPath="$nixCacheDir/flake-registry.json"
          ln -s ${flake-registry-path} $out$globalFlakeRegistryPath
          mkdir -p $out/nix/var/nix/gcroots/auto
          rootName=$(${lib.getExe' nix "nix"} --extra-experimental-features nix-command hash file --type sha1 --base32 <(echo -n $globalFlakeRegistryPath))
          ln -s $globalFlakeRegistryPath $out/nix/var/nix/gcroots/auto/$rootName
        '')
      );

in
dockerTools.buildLayeredImageWithNixDb {

  inherit
    name
    tag
    maxLayers
    uid
    gid
    uname
    gname
    ;

  contents = [ baseSystem ];

  extraCommands = ''
    rm -rf nix-support
    ln -s /nix/var/nix/profiles nix/var/nix/gcroots/profiles
  '';
  fakeRootCommands = ''
    chmod 1777 tmp
    chmod 1777 var/tmp
    chown -R ${toString uid}:${toString gid} .${userHome}
    chown -R ${toString uid}:${toString gid} nix
  ''
  + lib.optionalString enableSudo ''
    chmod 440 etc/sudoers.d/${uname}
  '';

  config = {
    inherit Cmd Labels;
    User = "${toString uid}:${toString gid}";
    Env = [
      "USER=${uname}"
      "PATH=${
        lib.concatStringsSep ":" [
          "${userHome}/.nix-profile/bin"
          "/nix/var/nix/profiles/default/bin"
          "/nix/var/nix/profiles/default/sbin"
        ]
      }"
      "MANPATH=${
        lib.concatStringsSep ":" [
          "${userHome}/.nix-profile/share/man"
          "/nix/var/nix/profiles/default/share/man"
        ]
      }"
      "SSL_CERT_FILE=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt"
      "GIT_SSL_CAINFO=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt"
      "NIX_SSL_CERT_FILE=/nix/var/nix/profiles/default/etc/ssl/certs/ca-bundle.crt"
      "NIX_PATH=/nix/var/nix/profiles/per-user/${uname}/channels:${userHome}/.nix-defexpr/channels"
    ]
    ++ lib.optionals enableLocale [
      "LANG=${locale}"
      "LC_ALL=${locale}"
      "LOCALE_ARCHIVE=/nix/var/nix/profiles/default/lib/locale/locale-archive"
    ]
    ++ lib.optionals enableZsh [
      "SHELL=${lib.getExe pkgs.zsh}"
    ]
    ++ extraEnv;
  };

}
