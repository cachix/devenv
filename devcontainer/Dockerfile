FROM mcr.microsoft.com/devcontainers/base:ubuntu-22.04
ARG VERSION=latest
ARG DEVENV_INSTALL_URI=github:cachix/devenv/$VERSION
# To pin a Nix version: https://releases.nixos.org/nix/nix-2.24.9/install
ARG NIX_INSTALL_SCRIPT=https://nixos.org/nix/install

COPY --chmod=0755 nix-entrypoint.sh /nix-entrypoint.sh
COPY nix.conf /tmp/nix.conf

# Update and install dependencies
RUN apt-get update && \
    apt-get install -y bash curl git sudo xz-utils

# Install Nix
# NOTE: The extra conf file does not apply to the install script itself.
# We need to disable the syscall filter beforehand.
RUN mkdir -p -m 0755 /etc/nix
RUN echo "filter-syscalls = false" > /etc/nix/nix.conf
RUN curl -L ${NIX_INSTALL_SCRIPT} | \
    sh -s -- \
      --daemon \
      --no-channel-add \
      --nix-extra-conf-file /tmp/nix.conf

# Add Nix to PATH
ENV PATH="/nix/var/nix/profiles/default/bin:${PATH}"

# Start nix-daemon
RUN /nix-entrypoint.sh sleep 5

# Install devenv and dependencies
RUN nix profile install nixpkgs#direnv nixpkgs#cachix
RUN USER=root cachix use devenv
RUN nix profile install $DEVENV_INSTALL_URI --accept-flake-config

# Clean up
RUN rm -rf ~/.cache/nix && \
    rm /tmp/nix.conf && \
    sudo -i rm -rf /var/cache/apk/* && \
    sudo -i nix-collect-garbage --delete-old

# Set default user
USER vscode

# Configure direnv
RUN mkdir -p ~/.config/direnv
RUN <<EOF > ~/.config/direnv/config.toml
[whitelist]
prefix = [ "/workspaces" ]
EOF
RUN direnv hook bash >> ~/.bashrc

# Start the nix-daemon when the container starts
ENTRYPOINT [ "/nix-entrypoint.sh", "sleep", "infinity" ]
