# TODO: build using Nix: https://github.com/microsoft/vscode-dev-containers/blob/main/containers/alpine/.devcontainer/base.Dockerfile
FROM mcr.microsoft.com/devcontainers/base:jammy
ARG VERSION=latest

ADD nix.conf /nix.conf
ADD nix-entrypoint.sh /nix-entrypoint.sh

RUN sudo apt-get update && sudo apt install -y xz-utils acl

RUN wget -O install.sh https://nixos.org/nix/install \
  && chmod +x install.sh \
  && addgroup --gid 30000 --system nixbld \
  && for i in $(seq 1 30); do adduser --system --no-create-home --home /var/empty --uid $((30000 + i)) nixbld$i && adduser nixbld$i nixbld ; done \
  && USER=root sh ./install.sh --daemon --no-channel-add --nix-extra-conf-file /nix.conf \
  && sudo setfacl -k /tmp \
  && rm -rf /var/cache/apk/* install.sh /nix.conf 

ENV \
    ENV=/etc/profile \
    USER=root \
    PATH=/nix/var/nix/profiles/default/bin:/nix/var/nix/profiles/default/sbin:/bin:/sbin:/usr/bin:/usr/sbin \
    GIT_SSL_CAINFO=/etc/ssl/certs/ca-certificates.crt \
    NIX_SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

RUN nix profile install nixpkgs#direnv nixpkgs#cachix \
 && cachix use devenv \
 && nix profile install github:cachix/devenv/$VERSION \
 && rm -rf ~/.cache/nix \
 && chmod +x /nix-entrypoint.sh \
 && sudo /nix/var/nix/profiles/default/bin/nix-collect-garbage --delete-old

ENTRYPOINT ["/nix-entrypoint.sh", "sleep", "infinity"]