#!/usr/bin/env bash
set -euo pipefail

# Export the whole metadata object so the JSON walker encounters the literal
# target paths, rather than selecting a dotted attribute path through the CLI.
devenv --no-eval-cache eval machinesMeta > metadata.json
jq -e --arg root "$PWD" '
  .machinesMeta.server.encryptionKeys == {
    "/tmp/luks.key": ($root + "/secrets/luks.key")
  } and
  .machinesMeta.server.extraFiles["/etc/a.conf"].source == ($root + "/files/a.conf")
' metadata.json

# This loads and deserializes machinesMeta without contacting the target.
devenv --no-eval-cache machines info > info.txt
grep -q server info.txt
