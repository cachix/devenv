# VS Code devcontainer image with devenv and Nix pre-installed.
{ pkgs
, devenv
,
}:

let
  devcontainer-utils = pkgs.callPackage ./devcontainer-utils.nix {
    containerMetadata = {
      VERSION = devenv.version;
      DEFINITION_ID = "devenv-devcontainer";
      GIT_REPOSITORY = "https://github.com/cachix/devenv";
      CONTENTS_URL = "https://github.com/cachix/devenv/tree/main/containers/devcontainer";
    };
  };
in
import ../docker.nix {
  inherit pkgs;

  name = "devenv-devcontainer";

  Labels = {
    "org.opencontainers.image.title" = "devenv-devcontainer";
    "org.opencontainers.image.source" = "https://github.com/cachix/devenv";
    "org.opencontainers.image.vendor" = "Cachix";
    "org.opencontainers.image.version" = devenv.version;
    "org.opencontainers.image.description" = "devenv devcontainer image for VS Code";
  };

  # VS Code convention: user "vscode" with UID/GID 1000
  uid = 1000;
  gid = 1000;
  uname = "vscode";
  gname = "vscode";

  # Enable devcontainer features
  enableSudo = true;
  enableLocale = true;
  locale = "en_US.UTF-8";
  enableZsh = true;
  zshTheme = "devcontainers";

  # Shell configuration scripts (from devcontainers/features common-utils)
  rcSnippet = ./scripts/rc_snippet.sh;
  bashThemeSnippet = ./scripts/bash_theme_snippet.sh;
  zshThemeFile = ./scripts/devcontainers.zsh-theme;

  # Enable direnv with whitelist for workspaces
  enableDirenv = true;
  direnvWhitelist = [ "/workspaces" ];

  nixConf = {
    substituters = [
      "https://cache.nixos.org/"
      "https://devenv.cachix.org/"
    ];
    trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
    ];
  };

  extraPkgs = [
    devenv
    devcontainer-utils # code, devcontainer-info, systemctl wrappers
    pkgs.procps # ps command for process management
    pkgs.openssh # SSH client for git operations
    pkgs.gnupg # Commit signing
    pkgs.gnutar # Install vscode-server
    pkgs.gzip
    pkgs.xz
    pkgs.getent
    pkgs.gnugrep
    pkgs.gnused
    pkgs.glibc
    pkgs.iconv
    pkgs.which
  ];

  # Keep container running for VS Code to attach
  Cmd = [ "${pkgs.coreutils}/bin/sleep" "infinity" ];

  # Don't bundle nixpkgs to reduce the image size
  bundleNixpkgs = false;

  gitMinimal = pkgs.git;
}
