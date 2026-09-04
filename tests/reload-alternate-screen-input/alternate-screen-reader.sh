#!/usr/bin/env bash
set -eu

stty raw -echo
printf '\033[?1049hREADY'
bytes=$(dd bs=2 count=1 2>/dev/null | od -An -tx1)
stty sane
printf '\033[?1049lRECEIVED:%s\n' "$bytes"
