#!/usr/bin/env bash
set -eu

# GNU stty reports an error restoring canonical mode on Darwin PTYs.
if [ "$(uname)" = Darwin ]; then
  stty() { /bin/stty "$@"; }
fi

tty_state=$(stty -g)
trap 'stty "$tty_state"' EXIT
stty raw -echo
printf '\033[?1049hREADY'
bytes=$(dd bs=2 count=1 2>/dev/null | od -An -tx1)
stty "$tty_state"
printf '\033[?1049lRECEIVED:%s\n' "$bytes"
