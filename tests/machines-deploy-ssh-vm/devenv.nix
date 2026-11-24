{
  pkgs,
  inputs,
  config,
  ...
}:
let
  executor = import (inputs.devenv.modules + /machines/deploy.nix) {
    inherit pkgs;
    rollbackTimeout = 30;
    healthCheck = "true";
  };
  confirmationExecutor = import (inputs.devenv.modules + /machines/deploy.nix) {
    inherit pkgs;
    # A loaded CI VM can spend most of 30 seconds in NixOS activation before
    # the controller gets a chance to confirm the successful deployment.
    rollbackTimeout = 90;
    healthCheck = "true";
  };
  faultShell = pkgs.writeShellScript "devenv-test-ssh-fault" ''
    set -eu
    mode=$(cat /run/devenv-ssh-fault 2>/dev/null || true)
    case "$SSH_ORIGINAL_COMMAND" in
      *" start "*)
        echo start >> /run/devenv-test-starts
        if test "$mode" = lost-start; then
          ${pkgs.bash}/bin/bash -c "$SSH_ORIGINAL_COMMAND" > /run/devenv-test-response
          kill -KILL "$PPID"
          exit 255
        fi
        ;;
      *" confirm "*)
        echo confirm >> /run/devenv-test-confirms
        if test "$mode" = lost-confirm; then
          ${pkgs.bash}/bin/bash -c "$SSH_ORIGINAL_COMMAND" > /run/devenv-test-response
          kill -KILL "$PPID"
          exit 255
        fi
        if test "$mode" = controller-exit; then
          touch /run/devenv-test-confirm-waiting
          sleep 60
          exit 255
        fi
        ;;
      *" status")
        case "$mode" in
          status-transient|status-persistent)
            if test ! -e /run/devenv-test-status-fault; then
              # Wait until activation owns the target, so the interruption
              # observes a real switch rather than a still-queued request.
              phase=
              n=0
              while test "$n" -lt 400; do
                phase=$(${pkgs.jq}/bin/jq -r '.phase' /var/lib/devenv-machines/current.json 2>/dev/null || true)
                case "$phase" in
                  switching|checking|awaiting-confirmation) break ;;
                esac
                n=$((n + 1))
                sleep 0.05
              done
              printf '%s\n' "''${phase:-unobserved}" > /run/devenv-test-fault-phase
              # Latch the fault. A transient interruption lets later polls
              # through; a persistent one fails every later poll immediately
              # instead of waiting for a phase that has already passed.
              touch /run/devenv-test-status-fault
              kill -KILL "$PPID"
              exit 255
            fi
            if test "$mode" = status-persistent; then
              kill -KILL "$PPID"
              exit 255
            fi
            ;;
        esac
        ;;
    esac
    exec ${pkgs.bash}/bin/bash -c "$SSH_ORIGINAL_COMMAND"
  '';
  test = pkgs.testers.runNixOSTest {
    name = "devenv-machines-real-ssh";
    nodes.machine = { lib, ... }: {
      imports = [
        (inputs.devenv.modules + /machines/recovery.nix)
        (inputs.devenv.modules + /machines/facts.nix)
      ];
      environment.systemPackages = [
        executor
        pkgs.jq
      ];
      services.openssh.enable = true;
      services.openssh.settings.PermitRootLogin = "prohibit-password";
      users.users.root.openssh.authorizedKeys.keys = [
        ''command="${faultShell}" ${
          lib.removeSuffix "\n" (builtins.readFile (config.devenv.root + "/.ssh-test-key.pub"))
        }''
      ];
      environment.etc.devenv-deploy-marker.text = "original";
      specialisation.updated.configuration = {
        environment.etc.devenv-deploy-marker.text = lib.mkForce "updated";
        system.activationScripts.devenv-test-delay.text = "sleep 5";
      };
      virtualisation.memorySize = 2048;
      virtualisation.useBootLoader = true;
    };
    testScript = builtins.readFile ./test.py;
  };
in
{
  outputs.ssh-driver = test.driver;
  outputs.ssh-executor = executor;
  outputs.ssh-confirmation-executor = confirmationExecutor;
}
