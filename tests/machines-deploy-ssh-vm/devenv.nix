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
        if test "$mode" = during-switch; then
          while ! ${pkgs.jq}/bin/jq -e '.phase == "switching"' /var/lib/devenv-machines/current.json >/dev/null; do sleep 0.05; done
          touch /run/devenv-test-switch-disconnect
          kill -KILL "$PPID"
          exit 255
        fi
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
