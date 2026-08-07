#!/usr/bin/env bash
set -euo pipefail

build_json=$(devenv build machines.combined.build.home-manager)
generation=$(jq -er '."machines.combined.build.home-manager"' <<<"$build_json")
driver=$(readlink -f "$generation/activate")
bin_driver=$(readlink -f "$generation/bin/home-manager-generation")

if [[ "$driver" != "$bin_driver" ]]; then
  echo "home-manager activation entry points do not share the user-switching driver"
  exit 1
fi
grep -q "jdoe" "$driver"
grep -Fq 'runuser -u "$user"' "$driver"
grep -Fq 'sudo -H -u "$user"' "$driver"
grep -Fq 'USER="$user" LOGNAME="$user" HOME="$home"' "$driver"

if [[ ! -e "$generation/home-files" || ! -e "$generation/home-path" ]]; then
  echo "wrapped home-manager generation lost activation-package contents"
  exit 1
fi

echo "built user-switching home-manager generation $generation"
