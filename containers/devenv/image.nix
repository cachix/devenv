# A non-root container image with devenv and Nix pre-installed.
#
# This container is built upon Nix's `docker.nix` derivation:
# https://github.com/NixOS/nix/blob/master/docker.nix
{
  pkgs,
  nixInput,
  devenv,
}:

let
  docker = "${nixInput}/docker.nix";

  # Borrowed from https://github.com/nix-community/docker-nixpkgs/
  gitExtraMinimal =
    (pkgs.git.override {
      perlSupport = false;
      pythonSupport = false;
      withManual = false;
      withpcre2 = false;
    }).overrideAttrs
      (_: {
        doInstallCheck = false;
      });
in
import docker {
  inherit pkgs;

  name = "devenv";

  Labels = {
    "org.opencontainers.image.title" = "devenv";
    "org.opencontainers.image.source" = "https://github.com/cachix/devenv";
    "org.opencontainers.image.vendor" = "Cachix";
    "org.opencontainers.image.version" = devenv.version;
    "org.opencontainers.image.description" = "devenv container image";
  };

  # Set up non-root user
  uid = 1000;
  gid = 100;
  uname = "devenv";
  gname = "users";

  nixConf = {
    experimental-features = [
      "nix-command"
      "flakes"
    ];
    # Fixes unable to load seccomp BPF
    # https://github.com/NixOS/nix/issues/5258
    # Probably redundant now that we don't run the Nix installer
    filter-syscalls = false;
    max-jobs = "auto";
    substituters = [
      "https://cache.nixos.org/"
      "https://devenv.cachix.org/"
    ];
    trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
    ];
  };

  # Add devenv
  extraPkgs = [ devenv ];

  # Don't bundle Nix to reduce the image size
  bundleNixpkgs = false;

  # Remove unneeded tools or reduce their closure size
  coreutils-full = pkgs.busybox;
  curl = pkgs.emptyDirectory;
  gnutar = pkgs.emptyDirectory;
  gzip = pkgs.emptyDirectory;
  gitMinimal = gitExtraMinimal;
  openssh = pkgs.emptyDirectory;
  wget = pkgs.emptyDirectory;
}
