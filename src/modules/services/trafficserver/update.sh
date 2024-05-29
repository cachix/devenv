#!/usr/bin/env bash

# Fetch some of the default configuration files for Traffic Server

set -euxo pipefail

FILES=(ip_allow logging)

yq() {
  nix run 'nixpkgs#yq' -- "$@"
}

copy() {
  local dir="$1"
  local files=("${@:2}")
  for f in "${files[@]}"; do
    yq . "$dir/$f.yaml" > "$f.json"
    chmod 644 -- "$f.json"
  done
}

main() {
  local -a outs
  while IFS= read -r line; do
    outs+=("$line")
  done < <(nix build --no-link --print-out-paths 'nixpkgs#trafficserver')

  local d out
  for out in "${outs[@]}"; do
    d="$out/etc/trafficserver"
    if [[ -d "$d" ]]; then
      copy "$d" "${FILES[@]}"
      break
    fi
  done
}

main "$@"
