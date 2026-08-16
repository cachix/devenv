#!/usr/bin/env bash
set -ex
env | grep FOO=1
env | grep BAR=1
env | grep CHAZ=4
env | grep BAZ=5
env | grep TASK_GENERATED=yes
test "$QUOTED" = "two words"
test "$JSON" = '{"enabled":true}'
test "$INLINE_COMMENT" = "value"
test "$MULTILINE" = $'first\nsecond'
test "$EXPANDED" = "0+1"
test "$FROM_HOST" = "$USER"
test "$LITERAL_DOLLAR" = '$2a$10$hash'
test -z "${DISABLED+x}"
test "$MUTATED_BY_TASK" = "after"
test -z "${REMOVED_BY_TASK+x}"
test "$SHELL" != "/dotenv/shell"
test "$DEVENV_CMDLINE" != "dotenv"

# Direnv consumes this list to reload when a dotenv file changes.
grep -Fx "$PWD/.env" .devenv/input-paths.txt
grep -Fx "$PWD/config/.env.bar" .devenv/input-paths.txt

# A new CLI invocation re-reads the file even when the Nix evaluation is cached.
echo 'FRESH=first' >> config/.env.bar
devenv shell bash -- -c 'test "$FRESH" = first'
sed -i 's/FRESH=first/FRESH=second/' config/.env.bar
devenv shell bash -- -c 'test "$FRESH" = second'

# The first environment capture sees these old values; the enter-shell task
# mutates/removes them before the command starts.
printf '%s\n' 'MUTATED_BY_TASK=before' 'REMOVED_BY_TASK=present' >> .env
devenv shell bash -- -c \
  'test "$MUTATED_BY_TASK" = after && test -z "${REMOVED_BY_TASK+x}"'

# Dotenv must not replace the caller PATH before Nix activation snapshots it.
mkdir -p host-bin
mkdir -p host-share
PATH="$PWD/host-bin:$PATH" XDG_DATA_DIRS="$PWD/host-share" devenv shell bash -- -c \
  'case ":$PATH:" in *":$PWD/host-bin:"*) ;; *) exit 1;; esac
   case ":$PATH:" in *":/dotenv/bin:"*) ;; *) exit 1;; esac
   case "$PATH" in *"/dotenv/bin"*"/dotenv/bin"*) exit 1;; esac
   case ":$XDG_DATA_DIRS:" in *":$PWD/host-share:"*) ;; *) exit 1;; esac
   case ":$XDG_DATA_DIRS:" in *":/dotenv/share:"*) ;; *) exit 1;; esac
   case "$XDG_DATA_DIRS" in *"/dotenv/share"*"/dotenv/share"*) exit 1;; esac'

# The sourceable activation output follows the same ordering guarantees.
PATH="$PWD/host-bin:$PATH" XDG_DATA_DIRS="$PWD/host-share" bash -c '
  eval "$(devenv print-dev-env)"
  case ":$PATH:" in *":$PWD/host-bin:"*) ;; *) exit 1;; esac
  case ":$PATH:" in *":/dotenv/bin:"*) ;; *) exit 1;; esac
  case "$PATH" in *"/dotenv/bin"*"/dotenv/bin"*) exit 1;; esac
  case ":$XDG_DATA_DIRS:" in *":$PWD/host-share:"*) ;; *) exit 1;; esac
  case ":$XDG_DATA_DIRS:" in *":/dotenv/share:"*) ;; *) exit 1;; esac
  case "$XDG_DATA_DIRS" in *"/dotenv/share"*"/dotenv/share"*) exit 1;; esac
'
