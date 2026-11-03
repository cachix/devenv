import json
import os
from pathlib import Path
import socket
import subprocess

machine.start()
machine.wait_for_unit("sshd.service")
project = Path(os.environ["DEVENV_SSH_TEST_PROJECT"])
cli = os.environ["DEVENV_SSH_TEST_CLI"]
executor_package = os.environ["DEVENV_SSH_TEST_EXECUTOR"]
failing_executor_package = os.environ["DEVENV_SSH_TEST_FAILING_EXECUTOR"]
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
machine.forward_port(port, 22)
system = machine.succeed("readlink -f /run/current-system").strip()
machine.succeed("nix-env --profile /nix/var/nix/profiles/system --set " + system)
machine.fail("test -e /run/devenv-secrets/APP_TOKEN")

known_hosts = project / "known-hosts"
known_hosts.touch()
opts = [
    "-i",
    str(project / ".ssh-test-key"),
    "-o",
    "IdentitiesOnly=yes",
    "-o",
    "UserKnownHostsFile=" + str(known_hosts),
]
(project / "devenv.local.nix").write_text(
    """{ lib, ... }: {
  machines.server = {
    target.host = %s;
    target.sshOpts = %s;
    hardware.facter = null;
    nixos = { ... }: { forbidden = throw "apply evaluated a system role"; };
    build.nixos = lib.mkForce (throw "apply rebuilt a system");
    build.deployer = lib.mkForce (throw "apply rebuilt an executor");
    install.secrets."/var/lib/sops-nix/key.txt".secret = "AGE_KEY";
  };
}"""
    % (
        json.dumps(f"root@127.0.0.1:{port}"),
        "[ " + " ".join(json.dumps(v) for v in opts) + " ]",
    )
)
plan_file = project / "secrets-plan.json"


def write_plan(executor, requested=system):
    running = machine.succeed("readlink -f /run/current-system").strip()
    profile = machine.succeed("readlink -f /nix/var/nix/profiles/system").strip()
    manifest_path = Path(requested, "etc/devenv/secrets.json")
    secrets = (
        sorted(json.loads(manifest_path.read_text())["secrets"])
        if manifest_path.exists()
        else []
    )
    plan = {
        "version": 4,
        "scope": "fleet",
        "machines": {
            "server": {
                "target": f"root@127.0.0.1:{port}",
                "sshOpts": opts,
                "system": "x86_64-linux",
                "nixos": {
                    "currentSystem": running,
                    "currentProfile": profile,
                    "requestedSystem": requested,
                    "executor": executor,
                    "systemChanged": running != requested,
                    "profileChanged": profile != requested,
                    "addedStorePaths": [],
                    "removedStorePaths": [],
                    "currentFacts": json.loads(
                        Path(running, "etc/devenv/machine-facts.json").read_text()
                    ),
                    "requestedFacts": json.loads(
                        Path(requested, "etc/devenv/machine-facts.json").read_text()
                    ),
                    "findings": [
                        {
                            "code": "access-analysis-incomplete",
                            "severity": "warning",
                            "message": "Dynamic key sources or custom SSH/firewall configuration require manual review; declared facts do not prove reachability",
                            "before": None,
                            "after": None,
                        }
                    ],
                    "secrets": secrets,
                    "secretspec": {
                        "execution": "local",
                        "provider": None,
                        "profile": "default",
                    }
                    if secrets
                    else None,
                },
                "nixDarwin": None,
                "homeManager": None,
            }
        },
    }
    plan_file.write_text(json.dumps(plan))


def apply(value, success, executor=executor_package, requested=system):
    (project / ".env").write_text(f"APP_TOKEN={value}\nEXTRA_TOKEN=extra-{value}\n")
    write_plan(executor, requested)
    result = subprocess.run(
        [cli, "machines", "apply", str(plan_file)],
        cwd=project,
        capture_output=True,
        text=True,
        timeout=300,
    )
    assert (result.returncode == 0) == success, result.stdout + result.stderr
    return result.stderr


def delivered():
    return machine.succeed("cat /run/devenv-secrets/APP_TOKEN")


def placed():
    assert machine.succeed("ls -A /var/lib/sops-nix").strip() == "key.txt"
    assert (
        machine.succeed("stat -c '%u:%g %a' /var/lib/sops-nix/key.txt").strip()
        == "0:0 600"
    )
    return machine.succeed("cat /var/lib/sops-nix/key.txt")


def invocation():
    return machine.succeed("systemctl show -p InvocationID --value app.service").strip()


write_plan(executor_package)
original_plan = json.loads(plan_file.read_text())
for field, value in (
    ("execution", "target"),
    ("provider", "dotenv"),
    ("profile", "production"),
):
    changed = json.loads(json.dumps(original_plan))
    changed["machines"]["server"]["nixos"]["secretspec"][field] = value
    plan_file.write_text(json.dumps(changed))
    rejected = subprocess.run(
        [cli, "machines", "apply", str(plan_file)],
        cwd=project,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert rejected.returncode != 0
    assert (
        "no longer matches the planned secret resolution settings" in rejected.stderr
    ), rejected.stderr
    machine.succeed("test ! -e /var/lib/devenv-machines/current.json")
    machine.succeed("test ! -e /var/lib/devenv-machines/secrets")

del original_plan["machines"]["server"]["nixos"]["secretspec"]
plan_file.write_text(json.dumps(original_plan))
rejected = subprocess.run(
    [cli, "machines", "apply", str(plan_file)],
    cwd=project,
    capture_output=True,
    text=True,
    timeout=30,
)
assert rejected.returncode != 0
assert "Inconsistent deployment plan" in rejected.stderr, rejected.stderr

output = apply("token:one!", False)
assert "exited with exit status: 255" in output, output
machine.succeed("test ! -e /var/lib/devenv-machines/current.json")
machine.succeed("test ! -e /var/lib/devenv-machines/secrets")
known_hosts.write_text(
    subprocess.check_output(["ssh-keyscan", "-p", str(port), "127.0.0.1"], text=True)
)

local_config = project / "devenv.local.nix"
apply_config = local_config.read_text()
local_config.write_text(
    apply_config.replace(
        'lib.mkForce (throw "apply rebuilt a system")',
        "lib.mkForce { outPath = builtins.storePath " + json.dumps(system) + "; }",
    ).replace(
        'lib.mkForce (throw "apply rebuilt an executor")',
        "lib.mkForce { outPath = builtins.storePath "
        + json.dumps(executor_package)
        + "; }",
    )
)
generated = subprocess.run(
    [cli, "machines", "plan", "--json", "server"],
    cwd=project,
    capture_output=True,
    text=True,
    timeout=300,
)
assert generated.returncode == 0, generated.stdout + generated.stderr
planned_secrets = json.loads(generated.stdout)["machines"]["server"]["nixos"]
assert planned_secrets["secrets"] == ["APP_TOKEN"]
assert planned_secrets["secretspec"] == {
    "execution": "local",
    "provider": None,
    "profile": "default",
}
local_config.write_text(apply_config)

machine.succeed(
    "mkdir -p /var/lib/sops-nix && printf bootstrap-key > /var/lib/sops-nix/key.txt && chmod 600 /var/lib/sops-nix/key.txt"
)
before = invocation()
apply("token:one!", True)
assert delivered() == "token:one!"
assert (
    machine.succeed("stat -c '%U:%G %a' /run/devenv-secrets/APP_TOKEN").strip()
    == "app:app 400"
)
assert invocation() != before
assert placed() == "bootstrap-key"

generation = machine.succeed("readlink /run/devenv-secrets").strip()
machine.succeed(
    "cp /var/lib/devenv-machines/secrets/current/APP_TOKEN.cred /run/test-original-token.cred"
)
machine.succeed(
    "printf 'token:pending!' | systemd-creds encrypt --with-key=host --name=APP_TOKEN - /run/test-pending-token.cred"
)
machine.succeed(
    "cp /run/test-pending-token.cred /var/lib/devenv-machines/secrets/current/APP_TOKEN.cred"
)
for failure in ("missing-owner", "missing-group", "invalid-mode", "missing-secret"):
    machine.succeed(
        "rm -f /run/nixos/activation-restart-list /run/nixos/activation-reload-list"
    )
    machine.fail("NIXOS_ACTION=switch devenv-test-" + failure)
    assert machine.succeed("readlink /run/devenv-secrets").strip() == generation
    assert delivered() == "token:one!"
    assert (
        machine.succeed("stat -c '%U:%G %a' /run/devenv-secrets/APP_TOKEN").strip()
        == "app:app 400"
    )
    assert (
        machine.succeed(
            "find /run/devenv-secrets.d -mindepth 1 -maxdepth 1 | wc -l"
        ).strip()
        == "1"
    )
    assert placed() == "bootstrap-key"
    machine.fail("test -e /run/nixos/activation-restart-list")
    machine.fail("test -e /run/nixos/activation-reload-list")
machine.succeed(
    "mv /run/test-original-token.cred /var/lib/devenv-machines/secrets/current/APP_TOKEN.cred"
)
machine.succeed("rm /run/test-pending-token.cred")

machine.succeed(
    "mv /var/lib/devenv-machines/secrets/current/APP_TOKEN.cred /run/test-token.cred"
)
machine.fail("devenv-test-activate-secrets")
assert machine.succeed("readlink /run/devenv-secrets").strip() == generation
assert delivered() == "token:one!"
machine.succeed(
    "mv /run/test-token.cred /var/lib/devenv-machines/secrets/current/APP_TOKEN.cred"
)

before = invocation()

apply("token:two!", True)
assert delivered() == "token:two!"
assert placed() == "bootstrap-key"
rotated = invocation()
assert rotated != before

apply("token:two!", True)
assert invocation() == rotated

apply("token:three!", False, failing_executor_package)
machine.wait_until_succeeds(
    "devenv-machine-deploy status | jq -e '.outcome == \"rolled-back\"'", timeout=90
)
assert delivered() == "token:two!"
assert placed() == "bootstrap-key"

machine.fail("grep -rF 'token:' /var/lib/devenv-machines")
assert machine.succeed("ls /var/lib/devenv-machines/secrets | wc -l").strip() == "3"

bootstrap = json.loads(machine.succeed("cat /etc/devenv/bootstrap-secrets.json"))
assert len(bootstrap["secrets"]) == 2
assert set(bootstrap["secrets"].values()) == {"AGE_KEY"}
stager = bootstrap["stager"]
payload = "".join(name + "\\n9\\nage-fresh" for name in sorted(bootstrap["secrets"]))
for root in ("/tmp/root", "/tmp/escape"):
    machine.succeed(
        f"mkdir -p {root}/nix/store && mount --bind /nix/store {root}/nix/store"
    )
    machine.succeed(f"printf '{payload}' | {stager} {root} install values")
machine.succeed(f"{stager} /tmp/root install commit")
for path in ("/var/lib/sops-nix/key.txt", "/var/lib/other/key.txt"):
    assert machine.succeed("cat /tmp/root" + path) == "age-fresh"
    assert machine.succeed("stat -c '%u:%g %a' /tmp/root" + path).strip() == "0:0 600"
machine.fail("test -e /tmp/root/var/lib/devenv-machines/secrets/current")
machine.fail("test -e /tmp/root/var/lib/devenv-machines/bootstrap-secrets/install")
machine.fail("grep -rF fresh /tmp/root/var/lib/devenv-machines /tmp/root/run")
machine.succeed(
    "mkdir -p /tmp/outside /tmp/escape/var/lib && ln -s /tmp/outside /tmp/escape/var/lib/sops-nix"
)
machine.fail(f"{stager} /tmp/escape install commit")
assert machine.succeed("ls -A /tmp/outside").strip() == ""

machine.shutdown()
machine.start()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("sshd.service")
machine.forward_port(port, 22)
assert delivered() == "token:two!"
machine.fail("test -e /run/nixos/activation-restart-list")
assert placed() == "bootstrap-key"
assert (
    machine.succeed("stat -c '%U:%G %a' /run/devenv-secrets/APP_TOKEN").strip()
    == "app:app 400"
)

extra = str(Path(system, "specialisation/extra").resolve())
empty = str(Path(system, "specialisation/empty").resolve())
apply("token:extra!", False, failing_executor_package, extra)
machine.wait_until_succeeds(
    f"""devenv-machine-deploy status | jq -e '.outcome == "rolled-back" and .requestedSystem == "{extra}"'""",
    timeout=90,
)
assert delivered() == "token:two!"
machine.fail("test -e /run/devenv-secrets/EXTRA_TOKEN")
assert placed() == "bootstrap-key"

apply("token:extra!", True, requested=extra)
assert machine.succeed("cat /run/devenv-secrets/EXTRA_TOKEN") == "extra-token:extra!"
apply("token:removed!", False, failing_executor_package, empty)
machine.wait_until_succeeds(
    f"""devenv-machine-deploy status | jq -e '.outcome == "rolled-back" and .requestedSystem == "{empty}"'""",
    timeout=90,
)
assert delivered() == "token:extra!"
assert machine.succeed("cat /run/devenv-secrets/EXTRA_TOKEN") == "extra-token:extra!"

apply("token:empty!", True, requested=empty)
machine.fail("test -e /run/devenv-secrets")
machine.fail("test -e /run/devenv-secrets.d")
assert placed() == "bootstrap-key"

rollback = subprocess.run(
    [cli, "machines", "rollback", "server"],
    cwd=project,
    capture_output=True,
    text=True,
    timeout=300,
)
assert rollback.returncode == 0, rollback.stdout + rollback.stderr
assert delivered() == "token:extra!"
assert machine.succeed("cat /run/devenv-secrets/EXTRA_TOKEN") == "extra-token:extra!"
assert placed() == "bootstrap-key"
