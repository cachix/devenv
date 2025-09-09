FROM nixpkgs/nix:latest-x86_64-linux AS amd64
FROM nixpkgs/nix:latest-aarch64-linux AS arm64
FROM ${TARGETARCH} AS final

# The version of devenv to install
ARG DEVENV_VERSION=latest

# The non-root user to create
ENV DEVENV_USER=devenv

# filter-syscalls: https://github.com/NixOS/nix/issues/5258
RUN mkdir /etc/nix && cat > /etc/nix/nix.conf <<EOF
filter-syscalls = false
experimental-features = nix-command flakes
EOF

# Set up a non-root user
# - Create the user
# - Create the per-user directory for the user nix profile
# - Chown /nix to the non-root user
RUN nix shell nixpkgs#shadow -c bash -c "useradd -m $DEVENV_USER" && \
    chown -R $DEVENV_USER:root /nix && \
    chown -R $DEVENV_USER:users /home/$DEVENV_USER && \
    install -d -o $DEVENV_USER -g users -m 755 /nix/var/nix/profiles/per-user/$DEVENV_USER

# Export the non-root user profile
ENV PATH="/home/${DEVENV_USER}/.nix-profile/bin:${PATH}"

# Switch to the non-root user
USER $DEVENV_USER
WORKDIR /home/${DEVENV_USER}

# Install devenv
RUN nix profile install -j1 --accept-flake-config \
   github:cachix/devenv/$DEVENV_VERSION

# Clean up
RUN nix-collect-garbage -d && rm -rf /home/${DEVENV_USER}/.cache
