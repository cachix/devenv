#!/usr/bin/env bash
set -euo pipefail

devenv eval \
  machines.server._nixosEval.config.services.openssh.enable \
  machines.server._nixosEval.config.services.openssh.ports \
  machines.server._nixosEval.config.networking.hostName \
  | jq -e '
      .["machines.server._nixosEval.config.services.openssh.enable"] == true
      and .["machines.server._nixosEval.config.services.openssh.ports"] == [2222]
      and .["machines.server._nixosEval.config.networking.hostName"] == "merged"
    '
