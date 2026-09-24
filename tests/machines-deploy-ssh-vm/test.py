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
}"""
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
    code = proc.wait(timeout=90)
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
        timeout=90,
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


# Connection refusal before submission must never create a checkpoint.
machine.succeed("systemctl stop sshd.service sshd.socket || true")
proc, log = start_cli("apply", str(plan_file))
finish(proc, log, False)
machine.succeed("test ! -e /var/lib/devenv-machines/current.json")
machine.succeed("systemctl start sshd.service")
machine.wait_for_unit("sshd.service")

for mode in ["lost-start", "during-switch", "controller-exit", "lost-confirm"]:
    if mode == "lost-confirm":
        # Successful confirmation needs time for a full NixOS activation.
        # Keep the shorter watchdog deadline for the rollback scenarios above.
        plan["machines"]["server"]["nixos"]["executor"] = confirmation_executor_package
        plan_file.write_text(json.dumps(plan))
    machine.succeed(
        "echo "
        + shlex.quote(mode)
        + " > /run/devenv-ssh-fault; : > /run/devenv-test-starts"
    )
    proc, log = start_cli("apply", str(plan_file))
    if mode == "controller-exit":
        machine.wait_until_succeeds(
            "test -e /run/devenv-test-confirm-waiting", timeout=60
        )
        proc.terminate()
    output = finish(proc, log, False)
    if mode == "lost-start":
        assert "may have started" in output, output
    elif mode == "during-switch":
        machine.succeed("test -e /run/devenv-test-switch-disconnect")
        assert "Lost observation" in output, output
    elif mode == "lost-confirm":
        assert "Could not acknowledge confirmation" in output, output
    machine.succeed("rm /run/devenv-ssh-fault")
    outcome = "succeeded" if mode == "lost-confirm" else "rolled-back"
    wait_outcome(outcome)
    assert machine.succeed("wc -l < /run/devenv-test-starts").strip() == "1"
    assert_generation(updated if mode == "lost-confirm" else original)
    check_controller_status(outcome)

# A lost confirmation response must not later cause watchdog rollback.
machine.sleep(92)
assert_generation(updated)
assert state()["outcome"] == "succeeded"
