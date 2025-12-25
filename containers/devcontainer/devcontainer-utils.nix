# Devcontainer utility scripts and binaries
# Based on https://github.com/devcontainers/features/tree/main/src/common-utils
# Copyright (c) Microsoft Corporation. Licensed under MIT License.
{ lib
, writeShellScriptBin
, writeTextFile
, symlinkJoin
, containerMetadata ? { }
,
}:

let
  # Container metadata file for devcontainer-info
  metaEnv = writeTextFile {
    name = "meta.env";
    text = lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "${k}=\"${v}\"") containerMetadata);
  };

  # Helper binary: code - wrapper for VS Code
  code = writeShellScriptBin "code" ''
    get_in_path_except_current() {
        which -a "$1" | grep -A1 "$0" | grep -v "$0"
    }

    code="$(get_in_path_except_current code)"

    if [ -n "$code" ]; then
        exec "$code" "$@"
    elif [ "$(command -v code-insiders)" ]; then
        exec code-insiders "$@"
    else
        echo "code or code-insiders is not installed" >&2
        exit 127
    fi
  '';

  # Helper binary: devcontainer-info - display container metadata
  devcontainer-info = writeShellScriptBin "devcontainer-info" ''
    . ${metaEnv}

    # Minimal output
    if [ "$1" = "version" ] || [ "$1" = "image-version" ]; then
        echo "''${VERSION}"
        exit 0
    elif [ "$1" = "release" ]; then
        echo "''${GIT_REPOSITORY_RELEASE}"
        exit 0
    elif [ "$1" = "content" ] || [ "$1" = "content-url" ] || [ "$1" = "contents" ] || [ "$1" = "contents-url" ]; then
        echo "''${CONTENTS_URL}"
        exit 0
    fi

    # Full output
    echo
    echo "Development container image information"
    echo
    if [ -n "''${VERSION}" ]; then echo "- Image version: ''${VERSION}"; fi
    if [ -n "''${DEFINITION_ID}" ]; then echo "- Definition ID: ''${DEFINITION_ID}"; fi
    if [ -n "''${VARIANT}" ]; then echo "- Variant: ''${VARIANT}"; fi
    if [ -n "''${GIT_REPOSITORY}" ]; then echo "- Source code repository: ''${GIT_REPOSITORY}"; fi
    if [ -n "''${GIT_REPOSITORY_RELEASE}" ]; then echo "- Source code release/branch: ''${GIT_REPOSITORY_RELEASE}"; fi
    if [ -n "''${GIT_REPOSITORY_REVISION}" ]; then echo "- Source code revision: ''${GIT_REPOSITORY_REVISION}"; fi
    if [ -n "''${BUILD_TIMESTAMP}" ]; then echo "- Timestamp: ''${BUILD_TIMESTAMP}"; fi
    if [ -n "''${CONTENTS_URL}" ]; then echo && echo "More info: ''${CONTENTS_URL}"; fi
    echo
  '';

  # Helper binary: systemctl - inform user that systemd is not available
  systemctl = writeShellScriptBin "systemctl" ''
    echo '"systemd" is not available in this container.'
    echo 'Use the "service" command to start services instead, e.g.:'
    echo
    echo '  service --status-all'
  '';

in
symlinkJoin {
  name = "devcontainer-utils";
  paths = [
    code
    devcontainer-info
    systemctl
  ];
}
