# Executed by the NixOS test driver outside a build sandbox. Uses the actual
# CLI under test and real OpenSSH. Faults are injected on the server after
# commands execute, never by replacing the controller's ssh or nix binaries.
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess

machine.start()
machine.wait_for_unit("sshd.service")
project = Path(os.environ["DEVENV_SSH_TEST_PROJECT"])
cli = os.environ["DEVENV_SSH_TEST_CLI"]
executor_package = os.environ["DEVENV_SSH_TEST_EXECUTOR"]
confirmation_executor_package = os.environ["DEVENV_SSH_TEST_CONFIRMATION_EXECUTOR"]
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
machine.forward_port(port, 22)
original = machine.succeed("readlink -f /run/current-system").strip()
# Establish the system profile used by deployment and rollback.
machine.succeed("nix-env --profile /nix/var/nix/profiles/system --set " + original)
updated = machine.succeed(
    "readlink -f /run/current-system/specialisation/updated"
).strip()
opts = [
    "-i",
    str(project / ".ssh-test-key"),
    "-o",
    "IdentitiesOnly=yes",
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "UserKnownHostsFile=" + str(project / "known-hosts"),
]
# Only machine metadata is needed to apply pinned outputs. Making its roles
# throw proves that the real transport test never rebuilds the reviewed system.
(project / "devenv.local.nix").write_text(
    """{ lib, ... }: {
  machines.server = {
    target.host = %s;
    target.sshOpts = %s;
    hardware.facter = null;
    nixos = { ... }: { forbidden = throw "apply evaluated a system role"; };
    build.nixos = lib.mkForce (throw "apply rebuilt a system");
    build.deployer = lib.mkForce (throw "apply rebuilt an executor");
  };
}
"""
    % (
        json.dumps(f"root@127.0.0.1:{port}"),
        "[ " + " ".join(json.dumps(v) for v in opts) + " ]",
    )
)
# Each scenario reuses the same system closures. The target observes its own
# state through the executor, and the controller independently checks over SSH.
facts = json.loads(Path(original, "etc/devenv/machine-facts.json").read_text())
requested = json.loads(Path(updated, "etc/devenv/machine-facts.json").read_text())
assert facts == requested
# Both generations have homedir key lookup, which must be reported as unknown.
findings = [
    {
        "code": "access-analysis-incomplete",
        "severity": "warning",
        "message": "Dynamic key sources or custom SSH/firewall configuration require manual review; declared facts do not prove reachability",
        "before": None,
        "after": None,
    }
]
closure = lambda path: set(
    subprocess.check_output(["nix-store", "-qR", path], text=True).splitlines()
)
old_closure, new_closure = closure(original), closure(updated)
plan = {
    "version": 4,
    "scope": "fleet",
    "machines": {
        "server": {
            "target": f"root@127.0.0.1:{port}",
            "sshOpts": opts,
            "system": "x86_64-linux",
            "nixos": {
                "currentSystem": original,
                "currentProfile": original,
                "requestedSystem": updated,
                "executor": executor_package,
                "systemChanged": True,
                "profileChanged": True,
                "addedStorePaths": sorted(new_closure - old_closure),
                "removedStorePaths": sorted(old_closure - new_closure),
                "currentFacts": facts,
                "requestedFacts": requested,
                "findings": findings,
            },
            "nixDarwin": None,
            "homeManager": None,
        }
    },
}
plan_file = project / "ssh-plan.json"
plan_file.write_text(json.dumps(plan))


def start_cli(*args):
    log = (project / "cli.log").open("w+")
    proc = subprocess.Popen(
        [cli, "machines", *args], cwd=project, stdout=log, stderr=subprocess.STDOUT
    )
    return proc, log


def finish(proc, log, success):
    code = proc.wait(timeout=120)
    log.seek(0)
    output = log.read()
    log.close()
    assert (code == 0) == success, output
    return output


def state():
    return json.loads(machine.succeed("devenv-machine-deploy status"))


def wait_outcome(outcome):
    machine.wait_until_succeeds(
        "devenv-machine-deploy status | jq -e '.outcome == "
        + json.dumps(outcome)
        + "'",
        timeout=120,
    )
    assert state()["outcome"] == outcome


def assert_generation(path):
    assert machine.succeed("readlink -f /run/current-system").strip() == path
    assert machine.succeed("readlink -f /nix/var/nix/profiles/system").strip() == path


def check_controller_status(outcome):
    result = subprocess.run(
        [cli, "machines", "status", "server"],
        cwd=project,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["server"]["outcome"] == outcome


def logs():
    """Count submissions and confirmation requests that reached the server."""
    return (
        int(machine.succeed("wc -l < /run/devenv-test-starts").strip()),
        int(machine.succeed("wc -l < /run/devenv-test-confirms").strip()),
    )


def begin_fault(mode):
    machine.succeed(
        "echo %s > /run/devenv-ssh-fault; : > /run/devenv-test-starts; "
        ": > /run/devenv-test-confirms; rm -f /run/devenv-test-status-fault "
        "/run/devenv-test-fault-phase"
        % shlex.quote(mode)
    )


def end_fault():
    machine.succeed("rm -f /run/devenv-ssh-fault")


def assert_recovery(outcome, counts, generation):
    """Drop the injected fault, then assert the target and controller agree."""
    end_fault()
    wait_outcome(outcome)
    assert logs() == counts, logs()
    assert_generation(generation)
    check_controller_status(outcome)


# Connection refusal before submission must never create a checkpoint.
# This VM uses the service, not socket activation. Including the absent socket
# in the same stop command can prevent systemd from stopping the service.
machine.succeed("systemctl stop sshd.service")
machine.wait_for_closed_port(22)
machine.succeed(": > /run/devenv-test-starts")
machine.succeed(": > /run/devenv-test-confirms")
proc, log = start_cli("apply", str(plan_file))
output = finish(proc, log, False)
assert "deployed" not in output, output
machine.succeed("test ! -e /var/lib/devenv-machines/current.json")
assert logs() == (0, 0), logs()
machine.succeed("systemctl start sshd.service")
machine.wait_for_unit("sshd.service")

# A submission whose response is lost after the command ran may have started.
begin_fault("lost-start")
proc, log = start_cli("apply", str(plan_file))
output = finish(proc, log, False)
assert "may have started" in output, output
assert_recovery("rolled-back", (1, 0), original)

# A target that never answers a status poll again is not progress: the
# controller must stop observing at the gap bound, confirm nothing, and leave
# recovery to the armed watchdog. The fault latches on the first post-switch
# observation; every later poll fails immediately instead of blocking on a
# phase that has already passed.
begin_fault("status-persistent")
proc, log = start_cli("apply", str(plan_file))
output = finish(proc, log, False)
assert "Lost observation" in output, output
assert "for 90s" in output, output
assert "could not be observed" in output, output
machine.succeed("test -e /run/devenv-test-status-fault")
assert machine.succeed("cat /run/devenv-test-fault-phase").strip() == "switching"
assert_recovery("rolled-back", (1, 0), original)

# A controller that dies while waiting for confirmation must not keep the
# target from rolling back when nobody confirms. Allow the same activation
# budget as the following scenarios that need to reach confirmation.
plan["machines"]["server"]["nixos"]["executor"] = confirmation_executor_package
plan_file.write_text(json.dumps(plan))
begin_fault("controller-exit")
proc, log = start_cli("apply", str(plan_file))
machine.wait_until_succeeds("test -e /run/devenv-test-confirm-waiting", timeout=120)
proc.terminate()
finish(proc, log, False)
assert_recovery("rolled-back", (1, 1), original)

# One dropped status round trip is a gap, not a decision: the controller keeps
# watching the same transaction, confirms it, and starts nothing twice. The
# fault fires once while the target is switching; later polls pass through.
begin_fault("status-transient")
proc, log = start_cli("apply", str(plan_file))
output = finish(proc, log, True)
assert "server: deployed" in output, output
assert "Lost observation" not in output, output
machine.succeed("test -e /run/devenv-test-status-fault")
assert machine.succeed("cat /run/devenv-test-fault-phase").strip() == "switching"
assert_recovery("succeeded", (1, 1), updated)

# A lost confirmation response no longer aborts a confirmed deployment: the
# executor persisted the result before replying, and the controller reads that
# record back. The target is already running the requested generation, so this
# redeploys the same generation and still proves the lost-response path.
plan["machines"]["server"]["nixos"].update(
    {
        "currentSystem": updated,
        "currentProfile": updated,
        "systemChanged": False,
        "profileChanged": False,
        "addedStorePaths": [],
        "removedStorePaths": [],
        "currentFacts": requested,
        "requestedFacts": requested,
    }
)
plan_file.write_text(json.dumps(plan))
begin_fault("lost-confirm")
proc, log = start_cli("apply", str(plan_file))
output = finish(proc, log, True)
assert "server: deployed" in output, output
assert "Could not acknowledge confirmation" not in output, output
assert_recovery("succeeded", (1, 1), updated)

# A confirmed generation must not later be reverted by the watchdog.
machine.sleep(92)
assert_generation(updated)
assert state()["outcome"] == "succeeded"
