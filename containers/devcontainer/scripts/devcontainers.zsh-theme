# Verbatim copy from https://github.com/devcontainers/features
# Copyright (c) Microsoft Corporation. All rights reserved.
# Licensed under the MIT License. See https://github.com/devcontainers/features/blob/main/LICENSE

# Oh My Zsh! theme - partly inspired by https://github.com/ohmyzsh/ohmyzsh/blob/master/themes/robbyrussell.zsh-theme
__zsh_prompt() {
    local prompt_username
    if [ ! -z "${GITHUB_USER}" ]; then
        prompt_username="@${GITHUB_USER}"
    else
        prompt_username="%n"
    fi
    PROMPT="%{$fg[green]%}${prompt_username} %(?:%{$reset_color%}➜ :%{$fg_bold[red]%}➜ )" # User/exit code arrow
    PROMPT+='%{$fg_bold[blue]%}%(5~|%-1~/…/%3~|%4~)%{$reset_color%} ' # cwd
    PROMPT+='`\
        if [ "$(git config --get devcontainers-theme.hide-status 2>/dev/null)" != 1 ] && [ "$(git config --get codespaces-theme.hide-status 2>/dev/null)" != 1 ]; then \
            export BRANCH=$(git --no-optional-locks symbolic-ref --short HEAD 2>/dev/null || git --no-optional-locks rev-parse --short HEAD 2>/dev/null); \
            if [ "${BRANCH}" != "" ]; then \
                echo -n "%{$fg_bold[cyan]%}(%{$fg_bold[red]%}${BRANCH}" \
                && if [ "$(git config --get devcontainers-theme.show-dirty 2>/dev/null)" = 1 ] && \
                    git --no-optional-locks ls-files --error-unmatch -m --directory --no-empty-directory -o --exclude-standard ":/*" > /dev/null 2>&1; then \
                        echo -n " %{$fg_bold[yellow]%}✗"; \
                fi \
                && echo -n "%{$fg_bold[cyan]%})%{$reset_color%} "; \
            fi; \
        fi`'
    PROMPT+='%{$fg[white]%}$ %{$reset_color%}'
    unset -f __zsh_prompt
}
__zsh_prompt

# Check if the terminal is xterm
if [[ "$TERM" == "xterm" ]]; then
    # Function to set the terminal title to the current command
    preexec() {
        local cmd=${1}
        echo -ne "\033]0;${USER}@${HOSTNAME}: ${cmd}\007"
    }

    # Function to reset the terminal title to the shell type after the command is executed
    precmd() {
        echo -ne "\033]0;${USER}@${HOSTNAME}: ${SHELL}\007"
    }

    # Add the preexec and precmd functions to the corresponding hooks
    autoload -Uz add-zsh-hook
    add-zsh-hook preexec preexec
    add-zsh-hook precmd precmd
fi
