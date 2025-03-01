FROM nixpkgs/nix:latest@sha256:79397901292ed23c3502177552e4d261c2d711720185e2b3a5beb715f35f595b

ARG CACHIX_VERSION=v1
ARG DEVENV_VERSION=latest

# Add Nix to PATH
ENV PATH="/nix/var/nix/profiles/default/bin:$PATH"

# https://github.com/NixOS/nix/issues/5258
RUN mkdir -p -m 0755 /etc/nix
RUN echo "filter-syscalls = false" > /etc/nix/nix.conf

RUN nix-env -iA cachix -f https://cachix.org/api/$CACHIX_VERSION/install && cachix use devenv
RUN nix-env -if https://github.com/cachix/devenv/tarball/$DEVENV_VERSION
