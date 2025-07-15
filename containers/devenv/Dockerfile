FROM nixos/nix:latest@sha256:3bb728719e2c4e478df4c50b80f93adbe27d5c561d1417c3a2306eb914d910da

ARG DEVENV_VERSION=latest

# https://github.com/NixOS/nix/issues/5258
RUN echo "filter-syscalls = false" >> /etc/nix/nix.conf

RUN nix-env -if https://github.com/cachix/devenv/tarball/$DEVENV_VERSION
