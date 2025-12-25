# Verbatim copy from https://github.com/devcontainers/features
# Copyright (c) Microsoft Corporation. All rights reserved.
# Licensed under the MIT License. See https://github.com/devcontainers/features/blob/main/LICENSE

if [ -z "${USER}" ]; then export USER=$(whoami); fi
if [[ "${PATH}" != *"$HOME/.local/bin"* ]]; then export PATH="${PATH}:$HOME/.local/bin"; fi

# Display optional first run image specific notice if configured and terminal is interactive
if [ -t 1 ] && [[ "${TERM_PROGRAM}" = "vscode" || "${TERM_PROGRAM}" = "codespaces" ]] && [ ! -f "$HOME/.config/vscode-dev-containers/first-run-notice-already-displayed" ]; then
    if [ -f "/usr/local/etc/vscode-dev-containers/first-run-notice.txt" ]; then
        cat "/usr/local/etc/vscode-dev-containers/first-run-notice.txt"
    elif [ -f "/workspaces/.codespaces/shared/first-run-notice.txt" ]; then
        cat "/workspaces/.codespaces/shared/first-run-notice.txt"
    fi
    mkdir -p "$HOME/.config/vscode-dev-containers"
    # Mark first run notice as displayed after 10s to avoid problems with fast terminal refreshes hiding it
    ((sleep 10s; touch "$HOME/.config/vscode-dev-containers/first-run-notice-already-displayed") &)
fi

# Set the default git editor if not already set
if [ -z "$(git config --get core.editor)" ] && [ -z "${GIT_EDITOR}" ]; then
    if  [ "${TERM_PROGRAM}" = "vscode" ]; then
        if [[ -n $(command -v code-insiders) &&  -z $(command -v code) ]]; then
            export GIT_EDITOR="code-insiders --wait"
        else
            export GIT_EDITOR="code --wait"
        fi
    fi
fi
