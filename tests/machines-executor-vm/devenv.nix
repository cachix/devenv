{ pkgs, inputs, ... }:
let
  executor = import (inputs.devenv.modules + /machines/deploy.nix) {
    inherit pkgs;
    rollbackTimeout = 30;
    healthCheck = ''
      if test -e /run/devenv-fail-health; then exit 38; fi
      if test -e /run/devenv-hang-health; then sleep 600; fi
    '';
  };
in
{
  outputs.executor-vm = pkgs.testers.runNixOSTest {
    name = "devenv-machines-executor";
    nodes.machine = { lib, ... }: {
      imports = [ (inputs.devenv.modules + /machines/recovery.nix) ];
      # Boot the selected generation from disk, including after a real reboot.
      virtualisation.useBootLoader = true;
      environment.systemPackages = [
        executor
        pkgs.jq
      ];
      environment.etc.devenv-deploy-marker.text = "original";
      specialisation.updated.configuration = {
        environment.etc.devenv-deploy-marker.text = lib.mkForce "updated";
        system.activationScripts.devenv-test-delay.text = "sleep 5";
      };
      specialisation.broken.configuration = {
        system.activationScripts.devenv-test-fail.text = "exit 37";
      };
      virtualisation.memorySize = 2048;
    };
    testScript = ''
      import json

      machine.start(allow_reboot=True)
      machine.wait_for_unit("multi-user.target")
      executor = "devenv-machine-deploy"
      original = machine.succeed("readlink -f /run/current-system").strip()
      updated = original + "/specialisation/updated"
      broken = original + "/specialisation/broken"
      updated = machine.succeed("readlink -f " + updated).strip()
      broken = machine.succeed("readlink -f " + broken).strip()

      def wait_outcome(outcome):
          machine.wait_until_succeeds(
              executor + " status | jq -e '.outcome == \"" + outcome + "\"'"
          )

      # Submission returns while systemd owns the switch. Another operation
      # cannot enter even though the submitting command has already exited.
      preconditions = " --expected-system " + original + " --expected-profile " + original
      machine.fail(executor + " start stale-running " + updated + " --expected-system " + updated + " --expected-profile " + original)
      machine.succeed("nix-env --profile /nix/var/nix/profiles/system --set " + updated)
      machine.fail(executor + " start stale-profile " + updated + preconditions)
      machine.succeed("test ! -e /var/lib/devenv-machines/current.json")
      assert machine.succeed("readlink -f /run/current-system").strip() == original
      machine.succeed("nix-env --profile /nix/var/nix/profiles/system --set " + original)
      machine.succeed(executor + " start first " + updated + preconditions)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"switching\"'")
      machine.fail(executor + " start conflicting " + original)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"awaiting-confirmation\"'")
      machine.succeed(executor + " confirm first")
      machine.succeed(executor + " confirm first")
      wait_outcome("succeeded")
      machine.succeed("grep -qx updated /etc/devenv-deploy-marker")
      assert machine.succeed("readlink -f /run/current-system").strip() == updated
      machine.reboot()
      machine.wait_for_unit("multi-user.target")
      wait_outcome("succeeded")
      assert machine.succeed("readlink -f /run/current-system").strip() == updated

      # Return to the original system, then reboot an unconfirmed generation.
      machine.succeed(executor + " rollback before-reboot")
      wait_outcome("succeeded")
      machine.succeed(executor + " start interrupted " + updated)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"awaiting-confirmation\"'")
      old_boot = machine.succeed("cat /proc/sys/kernel/random/boot_id").strip()
      machine.reboot()
      machine.wait_for_unit("multi-user.target")
      wait_outcome("rolled-back")
      state = json.loads(machine.succeed(executor + " status"))
      assert state["bootId"] == old_boot
      assert state["recoveryBootId"] != old_boot
      assert machine.succeed("readlink -f /run/current-system").strip() == original
      assert machine.succeed("readlink -f /nix/var/nix/profiles/system").strip() == original
      machine.fail(executor + " confirm interrupted")

      # Recovery must update the bootloader too, and remain terminal on reboot.
      machine.reboot()
      machine.wait_for_unit("multi-user.target")
      wait_outcome("rolled-back")
      assert machine.succeed("readlink -f /run/current-system").strip() == original

      # Abrupt power loss during activation must preserve the recovery checkpoint.
      machine.succeed(executor + " start powerloss " + updated)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"switching\"'")
      machine.crash()
      machine.start(allow_reboot=True)
      machine.wait_for_unit("multi-user.target")
      wait_outcome("rolled-back")
      assert machine.succeed("readlink -f /run/current-system").strip() == original
      assert machine.succeed("readlink -f /nix/var/nix/profiles/system").strip() == original

      machine.succeed(executor + " rollback restore")
      wait_outcome("succeeded")
      machine.succeed("grep -qx original /etc/devenv-deploy-marker")
      assert machine.succeed("readlink -f /run/current-system").strip() == original

      # A real activation failure recovers automatically without losing the old root.
      machine.succeed(executor + " start broken " + broken)
      wait_outcome("rolled-back")
      state = json.loads(machine.succeed(executor + " status"))
      assert state["previousSystem"] == original
      machine.succeed("test -e /nix/var/nix/gcroots/devenv-machines/broken/previous/bin/switch-to-configuration")
      assert machine.succeed("readlink -f /run/current-system").strip() == original
      machine.succeed(executor + " rollback recover")
      wait_outcome("succeeded")
      assert machine.succeed("readlink -f /nix/var/nix/profiles/system").strip() == original

      # Losing the controller after a successful switch must restore the old system.
      machine.succeed(executor + " start disconnected " + updated)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"awaiting-confirmation\"'")
      wait_outcome("rolled-back")
      machine.fail(executor + " confirm disconnected")
      assert machine.succeed("readlink -f /run/current-system").strip() == original

      # Failed and hanging application checks cannot commit a deployment.
      for marker in ("fail", "hang"):
          machine.succeed("touch /run/devenv-" + marker + "-health")
          machine.succeed(executor + " start health-" + marker + " " + updated)
          wait_outcome("rolled-back")
          machine.succeed("rm /run/devenv-" + marker + "-health")
          assert machine.succeed("readlink -f /run/current-system").strip() == original
          assert machine.succeed("readlink -f /nix/var/nix/profiles/system").strip() == original

      # A stale watchdog must not affect a later confirmed generation.
      machine.succeed(executor + " start final " + updated)
      machine.wait_until_succeeds(executor + " status | jq -e '.phase == \"awaiting-confirmation\"'")
      machine.succeed(executor + " confirm final")
      machine.succeed(executor + " watchdog disconnected")
      machine.sleep(32)
      wait_outcome("succeeded")
      assert machine.succeed("readlink -f /run/current-system").strip() == updated
    '';
  };
}
