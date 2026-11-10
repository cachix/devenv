#!/usr/bin/env bash
set -euo pipefail

devenv print-dev-env --json > captured.json
python3 - <<'PY'
import json
values = json.load(open("captured.json"))["variables"]
for name, expected in {"CC": "zig cc", "TZ": "Etc/GMT+3", "SSL_CERT_FILE": "/configured/ca.pem"}.items():
    assert values[name]["value"] == expected, (name, values[name])
PY

devenv print-dev-env > activation.sh
bash --noprofile --norc -c '
  source ./activation.sh
  test "$CC" = "zig cc"
  test "$TZ" = Etc/GMT+3
  test "$SSL_CERT_FILE" = /configured/ca.pem
  test -z "${_devenv_env_final_values+x}"
'

devenv info > info.txt
grep -F 'CC: zig cc' info.txt
grep -F 'ENV_CHILD: /configured/child' info.txt
