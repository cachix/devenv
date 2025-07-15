FROM nixpkgs/nix:latest

ARG DEVENV_VERSION=latest

# https://github.com/NixOS/nix/issues/5258
RUN mkdir /etc/nix && echo "filter-syscalls = false" >> /etc/nix/nix.conf

RUN nix profile install -j1 github:cachix/devenv/$DEVENV_VERSION nixpkgs#gnugrep --extra-experimental-features 'flakes nix-command' --accept-flake-config && nix-collect-garbage -d && rm -rf /root/.cache
