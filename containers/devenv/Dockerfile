FROM nixos/nix:latest@sha256:8c144c6c37184fe03fd7fed740c6eeb2ecfd801d6e34a6aba4e38f1c8d10de3e

ARG DEVENV_VERSION=latest

# https://github.com/NixOS/nix/issues/5258
RUN echo "filter-syscalls = false" >> /etc/nix/nix.conf

RUN nix profile install -j1 github:cachix/devenv/$DEVENV_VERSION --extra-experimental-features 'flakes nix-command' --accept-flake-config && nix-collect-garbage -d
