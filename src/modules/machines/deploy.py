"""Root-only NixOS deployment executor, invoked over SSH.

The systemd service owns activation. SSH owns only submission and observation.
Checkpoints precede effects; an interrupted activation is never retried implicitly.
"""

import argparse
import contextlib
import fcntl
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


TERMINAL = {"succeeded", "failed", "rolled-back", "rollback-failed"}
STORE_PATH = re.compile(r"/nix/store/[0-9a-z]{32}-[^/\s]+")
REQUEST_ID = re.compile(r"[A-Za-z0-9-]{1,100}")


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=".state-")
    try:
        with os.fdopen(fd, "w") as stream:
            json.dump(value, stream)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        sync_directory(path.parent)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class Executor:
    def __init__(
        self,
        directory=Path("/var/lib/devenv-machines"),
        roots=Path("/nix/var/nix/gcroots/devenv-machines"),
        profile=Path("/nix/var/nix/profiles/system"),
        running=Path("/run/current-system"),
        boot=Path("/proc/sys/kernel/random/boot_id"),
        run=subprocess.run,
        health_check=None,
        rollback_timeout=300,
    ):
        self.directory = directory
        self.roots = roots
        self.profile = profile
        self.running = running
        self.boot = boot
        self.run = run
        self.health_check = health_check
        self.rollback_timeout = rollback_timeout
        self.state_file = directory / "current.json"

    @contextlib.contextmanager
    def lock(self, wait=False):
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        sync_directory(self.directory.parent)
        with (self.directory / "lock").open("a") as stream:
            try:
                fcntl.flock(stream, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
            except BlockingIOError:
                raise RuntimeError(
                    "another deployment operation owns the target"
                ) from None
            yield

    def read(self):
        try:
            state = json.loads(self.state_file.read_text())
        except FileNotFoundError:
            return None
        # Corrupt/unsupported state must not authorize a new activation.
        if (
            not isinstance(state, dict)
            or state.get("version") != 1
            or not isinstance(state.get("id"), str)
            or not REQUEST_ID.fullmatch(state["id"])
            or not isinstance(state.get("phase"), str)
            or state.get("phase")
            not in {
                "queued",
                "switching",
                "checking",
                "awaiting-confirmation",
                "awaiting-rollback",
                "rolling-back",
                *TERMINAL,
            }
            or not isinstance(state.get("operation"), str)
            or state["operation"] not in {"deploy", "rollback"}
            or not isinstance(state.get("bootId"), str)
            or not isinstance(state.get("previousSystem"), str)
            or not STORE_PATH.fullmatch(state["previousSystem"])
            or not isinstance(state.get("requestedSystem"), str)
            or not STORE_PATH.fullmatch(state["requestedSystem"])
            or (("expectedSystem" in state) != ("expectedProfile" in state))
            or any(
                key in state
                and (
                    not isinstance(state[key], str)
                    or not STORE_PATH.fullmatch(state[key])
                    or state["operation"] != "deploy"
                )
                for key in ("expectedSystem", "expectedProfile")
            )
            or (
                "recoveryBootId" in state
                and (
                    not isinstance(state["recoveryBootId"], str)
                    or not state["recoveryBootId"]
                    or "deadline" not in state
                )
            )
            or (
                "deadline" in state
                and (
                    type(state["deadline"]) not in {int, float}
                    or not math.isfinite(state["deadline"])
                    or state["deadline"] <= 0
                    or state["operation"] != "deploy"
                )
            )
            or (
                state["phase"]
                in {
                    "checking",
                    "awaiting-confirmation",
                    "awaiting-rollback",
                    "rolling-back",
                    "rolled-back",
                    "rollback-failed",
                }
                and "deadline" not in state
            )
        ):
            raise RuntimeError(
                "invalid deployment checkpoint; inspect target state before recovery"
            )
        return state

    def unit(self, state):
        return "devenv-machine-" + state["id"] + ".service"

    def active(self, state):
        if state["bootId"] != self.boot.read_text().strip():
            return False
        return self.unit_active(self.unit(state))

    def unit_active(self, unit):
        result = self.run(
            [
                "systemctl",
                "show",
                unit,
                "--property=LoadState,ActiveState,SubState,Job",
            ],
            capture_output=True,
            text=True,
        )
        fields = dict(
            line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
        )
        if fields.get("LoadState") == "not-found":
            return False
        if (
            result.returncode != 0
            or not {"ActiveState", "SubState", "Job"} <= fields.keys()
        ):
            raise RuntimeError("cannot observe activation service; outcome is unknown")
        if fields["Job"].split(" ", 1)[0] not in {"", "0"}:
            return True
        if fields["ActiveState"] in {"inactive", "failed"}:
            return False
        # RemainAfterExit keeps a successful unit observable without running work.
        return not (
            fields["ActiveState"] == "active" and fields["SubState"] == "exited"
        )

    def status(self):
        state = self.read()
        if state is None:
            return {"version": 1, "phase": "uninitialized"}
        state = dict(state)
        state["unit"] = self.unit(state)
        if "recoveryBootId" in state:
            state["recoveryUnit"] = "devenv-machine-boot-" + state["id"] + ".service"
        if state["phase"] not in TERMINAL:
            try:
                active = self.active(state)
                if state.get("recoveryBootId") == self.boot.read_text().strip():
                    active = active or self.unit_active(state["recoveryUnit"])
                if (
                    not active
                    and "deadline" in state
                    and state["bootId"] == self.boot.read_text().strip()
                ):
                    active = self.unit_active(
                        "devenv-machine-watchdog-" + state["id"] + ".timer"
                    ) or self.unit_active(
                        "devenv-machine-watchdog-" + state["id"] + ".service"
                    )
                state["outcome"] = "pending" if active else "unknown"
            except RuntimeError as error:
                state["outcome"] = "unknown"
                state["observationError"] = str(error)
        else:
            state["outcome"] = state["phase"]
        return state

    def validate_system(self, path):
        if (
            not STORE_PATH.fullmatch(str(path))
            or not (Path(path) / "bin/switch-to-configuration").is_file()
        ):
            raise RuntimeError("expected an existing NixOS system store path")
        return str(path)

    def retain(self, request_id, previous, requested, executable):
        # Retain every operation's recovery closures. No implicit GC of rollback
        # history: roots may be removed by an operator after recovery is complete.
        root = self.roots / request_id
        root.mkdir(mode=0o700, parents=True, exist_ok=False)
        package = Path(executable).parent.parent
        for name, target in (
            ("previous", previous),
            ("requested", requested),
            ("executor", package),
        ):
            (root / name).symlink_to(target)
        sync_directory(root)
        sync_directory(self.roots)
        sync_directory(self.roots.parent)
        # Stable, rooted entry point for status/rollback without local evaluation.
        temporary = self.roots / (".executor-" + request_id)
        temporary.symlink_to(package)
        os.replace(temporary, self.roots / "executor")
        sync_directory(self.roots)

    def start(
        self,
        request_id,
        requested,
        executable,
        rollback=False,
        expected_system=None,
        expected_profile=None,
    ):
        if not REQUEST_ID.fullmatch(request_id):
            raise RuntimeError("invalid deployment ID")
        if (expected_system is None) != (expected_profile is None) or (
            rollback and expected_system is not None
        ):
            raise RuntimeError("both generation preconditions are required for deploy")
        with self.lock():
            prior = self.read()
            operation = "rollback" if rollback else "deploy"
            if prior and prior["id"] == request_id:
                if prior["operation"] != operation or (
                    not rollback
                    and (
                        prior["requestedSystem"] != requested
                        or prior.get("expectedSystem") != expected_system
                        or prior.get("expectedProfile") != expected_profile
                    )
                ):
                    raise RuntimeError(
                        "deployment ID already belongs to a different request"
                    )
                return prior
            if prior:
                if self.active(prior):
                    raise RuntimeError("another activation service is still running")
                if prior["phase"] not in TERMINAL and not rollback:
                    raise RuntimeError(
                        "previous activation outcome is unknown; inspect status and explicitly roll back"
                    )
            if rollback:
                if prior is None:
                    raise RuntimeError("no recorded deployment to roll back")
                requested = prior["previousSystem"]
            requested = self.validate_system(requested)
            previous = self.validate_system(str(self.running.resolve(strict=True)))
            if expected_system is not None and (
                previous != expected_system
                or str(self.profile.resolve(strict=True)) != expected_profile
            ):
                raise RuntimeError("stale deployment plan: target generations changed")
            self.retain(request_id, previous, requested, executable)
            state = {
                "version": 1,
                "id": request_id,
                "operation": operation,
                "phase": "queued",
                "previousSystem": previous,
                "requestedSystem": requested,
                "bootId": self.boot.read_text().strip(),
            }
            if not rollback:
                state["deadline"] = time.monotonic() + self.rollback_timeout
            if expected_system is not None:
                state["expectedSystem"] = expected_system
                state["expectedProfile"] = expected_profile
            atomic_json(self.state_file, state)
            if not rollback:
                # Arm recovery before allowing any activation effect. This unit
                # is independent of both SSH and the activation cgroup.
                self.run(
                    [
                        "systemd-run",
                        "--quiet",
                        "--no-block",
                        "--unit=devenv-machine-watchdog-" + request_id,
                        "--on-active=" + str(self.rollback_timeout) + "s",
                        "--timer-property=AccuracySec=1s",
                        "--property=Type=exec",
                        "--property=RuntimeMaxSec=300",
                        "--property=TimeoutStopSec=5s",
                        "--property=KillMode=control-group",
                        executable,
                        "watchdog",
                        request_id,
                    ],
                    check=True,
                )
            # A lost response or launch error leaves the checkpoint intact.
            # Observers must reconcile the service, never assume it did not start.
            self.run(
                [
                    "systemd-run",
                    "--quiet",
                    "--no-block",
                    "--unit=" + self.unit(state),
                    "--property=Type=exec",
                    "--property=RemainAfterExit=yes",
                    "--property=KillMode=control-group",
                    "--property=UMask=0077",
                    "--property=TimeoutStopSec=5s",
                    "--property=RuntimeMaxSec="
                    + str(self.rollback_timeout + 5 if not rollback else 300),
                    executable,
                    "worker",
                    request_id,
                ],
                check=True,
            )
            return state

    def worker(self, request_id):
        # Submission holds the same lock until systemd-run has acknowledged.
        # Wait for that short handoff; all other public mutations are nonblocking.
        with (self.directory / "lock").open("a") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            state = self.read()
            if not state or state["id"] != request_id or state["phase"] != "queued":
                raise RuntimeError("activation request is no longer queued")
            if state["bootId"] != self.boot.read_text().strip():
                raise RuntimeError(
                    "queued activation belongs to an earlier boot; inspect status before recovery"
                )
            if "expectedSystem" in state and (
                str(self.running.resolve()) != state["expectedSystem"]
                or str(self.profile.resolve()) != state["expectedProfile"]
            ):
                # No activation has occurred. A terminal result also prevents
                # the watchdog from reverting the external generation change.
                state["phase"] = "failed"
                state[
                    "error"
                ] = "stale deployment plan: target generations changed before activation"
                atomic_json(self.state_file, state)
                return
            if "deadline" in state and time.monotonic() >= state["deadline"]:
                state["phase"] = "awaiting-rollback"
                state["error"] = "activation did not start before the rollback deadline"
                atomic_json(self.state_file, state)
                return
            if "deadline" in state and not self.unit_active(
                "devenv-machine-watchdog-" + request_id + ".timer"
            ):
                raise RuntimeError(
                    "rollback watchdog is not active; refusing activation"
                )
            requested = self.validate_system(state["requestedSystem"])
            state["phase"] = "switching"
            atomic_json(self.state_file, state)
            try:
                self.run(
                    ["nix-env", "--profile", str(self.profile), "--set", requested],
                    check=True,
                )
                self.run(
                    [requested + "/bin/switch-to-configuration", "switch"], check=True
                )
                if (
                    str(self.profile.resolve(strict=True)) != requested
                    or str(self.running.resolve(strict=True)) != requested
                ):
                    raise RuntimeError(
                        "activation returned successfully but the requested system is not active"
                    )
                if state["operation"] == "deploy" and self.health_check:
                    state["phase"] = "checking"
                    atomic_json(self.state_file, state)
                    self.run([self.health_check], check=True)
            except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
                state["phase"] = (
                    "awaiting-rollback" if "deadline" in state else "failed"
                )
                state["error"] = str(error)[:8192]
                atomic_json(self.state_file, state)
                raise
            state["phase"] = (
                "awaiting-confirmation" if "deadline" in state else "succeeded"
            )
            atomic_json(self.state_file, state)

    def confirm(self, request_id):
        with self.lock():
            state = self.read()
            if not state or state["id"] != request_id:
                raise RuntimeError("confirmation does not match the current deployment")
            if state["phase"] == "succeeded":
                return state
            if (
                state["phase"] != "awaiting-confirmation"
                or state["bootId"] != self.boot.read_text().strip()
                or time.monotonic() >= state["deadline"]
            ):
                raise RuntimeError("deployment is not eligible for confirmation")
            requested = state["requestedSystem"]
            if (
                str(self.running.resolve()) != requested
                or str(self.profile.resolve()) != requested
            ):
                raise RuntimeError("requested system is no longer active")
            state["phase"] = "succeeded"
            atomic_json(self.state_file, state)
            return state

    def watchdog(self, request_id):
        state = self.read()
        if not state or state["id"] != request_id or state["phase"] in TERMINAL:
            return
        if state["bootId"] != self.boot.read_text().strip():
            raise RuntimeError("watchdog belongs to an earlier boot")
        if "deadline" not in state or time.monotonic() < state["deadline"]:
            raise RuntimeError("rollback deadline has not elapsed")
        # Stop the exact activation cgroup before acquiring its lock. A hung
        # health check or activation and all its children must finish first.
        stopped = self.run(["systemctl", "stop", self.unit(state)])
        if stopped.returncode != 0 and self.active(state):
            raise RuntimeError("cannot stop activation service before rollback")
        with self.lock(wait=True):
            state = self.read()
            if not state or state["id"] != request_id or state["phase"] in TERMINAL:
                return
            state.setdefault(
                "error", "controller did not confirm before the rollback deadline"
            )
            self.restore(state)

    def recover(self, executable):
        # The persistent service also starts when first installed by a live
        # switch. That switch owns the lock and already has its own watchdog.
        initial = self.read()
        if (
            not initial
            or initial["bootId"] == self.boot.read_text().strip()
            or initial["phase"] in TERMINAL
        ):
            return
        with self.lock():
            state = self.read()
            boot_id = self.boot.read_text().strip()
            if (
                not state
                or state["phase"] in TERMINAL
                or "deadline" not in state
                or state["bootId"] == boot_id
            ):
                return
            # Never repeat an interrupted rollback or an ambiguous launch.
            # Explicit rollback remains available after inspecting the target.
            if state["phase"] == "rolling-back" or "recoveryBootId" in state:
                return
            state["recoveryBootId"] = boot_id
            state.setdefault("error", "machine rebooted before deployment confirmation")
            atomic_json(self.state_file, state)
            self.run(
                [
                    "systemd-run",
                    "--quiet",
                    "--no-block",
                    "--unit=devenv-machine-boot-" + state["id"] + ".service",
                    "--property=Type=exec",
                    "--property=After=multi-user.target",
                    "--property=RuntimeMaxSec=300",
                    "--property=TimeoutStopSec=5s",
                    "--property=KillMode=control-group",
                    "--property=UMask=0077",
                    executable,
                    "boot-worker",
                    state["id"],
                ],
                check=True,
            )

    def boot_worker(self, request_id):
        with self.lock(wait=True):
            state = self.read()
            boot_id = self.boot.read_text().strip()
            if (
                not state
                or state["id"] != request_id
                or state["phase"] in TERMINAL
                or state["phase"] == "rolling-back"
                or state.get("recoveryBootId") != boot_id
                or state["bootId"] == boot_id
            ):
                raise RuntimeError("boot recovery no longer belongs to this deployment")
            self.restore(state)

    def restore(self, state):
        # All callers hold the target lock and have stopped competing work.
        state["phase"] = "rolling-back"
        atomic_json(self.state_file, state)
        try:
            previous = self.validate_system(state["previousSystem"])
            self.run(
                ["nix-env", "--profile", str(self.profile), "--set", previous],
                check=True,
            )
            self.run([previous + "/bin/switch-to-configuration", "switch"], check=True)
            if (
                str(self.running.resolve()) != previous
                or str(self.profile.resolve()) != previous
            ):
                raise RuntimeError("previous system is not active after rollback")
        except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
            state["phase"] = "rollback-failed"
            state["rollbackError"] = str(error)[:8192]
            atomic_json(self.state_file, state)
            raise
        state["phase"] = "rolled-back"
        atomic_json(self.state_file, state)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    start = commands.add_parser("start")
    start.add_argument("id")
    start.add_argument("system")
    start.add_argument("--expected-system")
    start.add_argument("--expected-profile")
    rollback = commands.add_parser("rollback")
    rollback.add_argument("id")
    worker = commands.add_parser("worker")
    worker.add_argument("id")
    commands.add_parser("confirm").add_argument("id")
    commands.add_parser("watchdog").add_argument("id")
    commands.add_parser("recover")
    commands.add_parser("boot-worker").add_argument("id")
    commands.add_parser("status")
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error("the machine executor requires root")
    os.umask(0o077)
    executor = Executor(
        health_check=os.environ.get("DEVENV_MACHINE_HEALTH_CHECK"),
        rollback_timeout=int(os.environ.get("DEVENV_MACHINE_ROLLBACK_TIMEOUT", "300")),
    )
    executable = str(Path(sys.argv[0]).resolve().parent / "devenv-machine-deploy")
    try:
        if args.command == "worker":
            executor.worker(args.id)
            return
        if args.command == "watchdog":
            executor.watchdog(args.id)
            return
        if args.command == "recover":
            executor.recover(executable)
            return
        if args.command == "boot-worker":
            executor.boot_worker(args.id)
            return
        if args.command == "status":
            result = executor.status()
        elif args.command == "confirm":
            result = executor.confirm(args.id)
        else:
            result = executor.start(
                args.id,
                getattr(args, "system", None),
                executable,
                rollback=args.command == "rollback",
                expected_system=getattr(args, "expected_system", None),
                expected_profile=getattr(args, "expected_profile", None),
            )
        print(json.dumps(result))
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
