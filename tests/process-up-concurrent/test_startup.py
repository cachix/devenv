"""Queue real CLI callers behind an external lock before releasing them together."""

import fcntl
import json
import os
from pathlib import Path
import signal
import socketserver
import subprocess
import sys
import time
import threading


runtime = Path(sys.argv[1])
runtime.mkdir(parents=True, exist_ok=True)
state = Path(".devenv/state")
pid_file = runtime / "native-manager.pid"
socket = runtime / "native.sock"
waiting_message = "Waiting for native process manager startup lock"


# Observe real CLI cleanup requests without touching the user's shared proxy.
# Proxy-disabled configuration legitimately reconciles once before discovery;
# a losing or cancelled caller must not send another destructive replacement.
class ProxyControl(socketserver.StreamRequestHandler):
    def handle(self):
        request = json.loads(self.rfile.readline())
        assert request["command"] == "replace_owner", request
        self.server.commands.append(request)
        self.wfile.write(b'{"status":"ok","routes":null,"removed":null}\n')


proxy_path = runtime / "test-proxy.sock"
proxy_path.unlink(missing_ok=True)
proxy = socketserver.UnixStreamServer(str(proxy_path), ProxyControl)
proxy.commands = []
proxy_thread = threading.Thread(target=proxy.serve_forever, daemon=True)
proxy_thread.start()
os.environ["DEVENV_PROXY_SOCKET"] = str(proxy_path)


def wait_until(predicate, description, timeout=60):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() >= deadline:
            raise AssertionError(f"Timed out waiting for {description}")
        time.sleep(0.1)


def run(*args):
    return subprocess.run(["devenv", *args], check=True, timeout=60)


def stop_manager(children):
    if not pid_file.exists():
        return
    manager_pid = int(pid_file.read_text())
    owner = next((child for child in children if child.pid == manager_pid), None)
    if owner is not None:
        # Reap a foreground child ourselves; a synchronous `processes down`
        # would otherwise wait for this parent's unreaped zombie.
        owner.send_signal(signal.SIGINT)
        owner.wait(timeout=30)
    else:
        run("processes", "down")


def exercise(first_daemon, second_daemon):
    label = "-".join("detached" if daemon else "foreground"
                     for daemon in (first_daemon, second_daemon))
    children = []
    state.joinpath("ready").unlink(missing_ok=True)
    state.joinpath("launch-pids").unlink(missing_ok=True)

    def start(name, daemon=False):
        trace = Path(f"{label}-{name}.json")
        trace.unlink(missing_ok=True)
        with Path(f"{label}-{name}.log").open("w") as log:
            child = subprocess.Popen(
                ["devenv", "--verbose", "--trace-to", f"json:file:{trace}",
                 "up", "--no-tui", *(["-d"] if daemon else [])],
                stdout=log,
                stderr=subprocess.STDOUT,
                env={**os.environ, "DEVENV_NO_AI_AGENT": "1", "RUST_LOG": "devenv=debug"},
            )
        children.append(child)

        def blocked():
            assert child.poll() is None, f"{name} exited before reaching the startup lock"
            assert not pid_file.exists(), f"{name} published while the startup lock was held"
            assert not socket.exists(), f"{name} bound its socket while the startup lock was held"
            return trace.exists() and waiting_message in trace.read_text()

        wait_until(blocked, f"{name} to wait for the startup lock")
        return child

    try:
        # Both callers must reach discovery while no manager exists. This avoids
        # depending on machine speed to hit the pre-publication race.
        with (runtime / "native-start.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            first = start("first", daemon=first_daemon)
            second = start("second", daemon=second_daemon)
            cancelled = start("cancelled", daemon=second_daemon)
            reconciled = len(proxy.commands)
            assert reconciled >= 3, "Each caller must reconcile before waiting"
            cancelled.send_signal(signal.SIGINT)
            assert cancelled.wait(timeout=30) != 0
            assert len(proxy.commands) == reconciled, "Cancelled waiter cleared shared proxy routes"
            assert not pid_file.exists(), "A waiting caller published a manager"
            assert not socket.exists(), "A waiting caller published a socket"
            fcntl.flock(lock, fcntl.LOCK_UN)

        wait_until(lambda: pid_file.exists() and pid_file.read_text().strip().isdigit(),
                   "manager publication")
        manager_pid = int(pid_file.read_text())
        foreground_owner = any(child.pid == manager_pid and not daemon
                               for child, daemon in ((first, first_daemon), (second, second_daemon)))
        for name, child, daemon in (("first", first, first_daemon),
                                    ("second", second, second_daemon)):
            if child.pid == manager_pid:
                assert not daemon
                assert child.poll() is None
            else:
                status = child.wait(timeout=30)
                log = Path(f"{label}-{name}.log").read_text()
                if daemon and not foreground_owner:
                    assert status == 0, log
                else:
                    assert status != 0, log
                    expected = "foreground" if daemon else "Processes already running"
                    assert expected in log, log

        assert len(proxy.commands) == reconciled, "Losing caller cleared the winner's proxy routes"

        # The winner must release the lock before readiness, including across
        # re-exec into a daemon. Otherwise future startup calls cannot progress.
        with (runtime / "native-start.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(lock, fcntl.LOCK_UN)

        wait_until(lambda: state.joinpath("launch-pids").exists()
                   and state.joinpath("launch-pids").read_text().strip(), "process launch")
        launches = state.joinpath("launch-pids").read_text().splitlines()
        assert len(launches) == 1, f"Multiple managers launched processes: {launches}"
        assert not state.joinpath("ready").exists(), "Readiness gate unexpectedly opened"
        # Exercise shutdown during readiness as well as normal ready shutdown.
        if first_daemon or second_daemon:
            state.joinpath("ready").touch()
            run("processes", "wait")
        stop_manager(children)
        for child in children:
            child.wait(timeout=30)
        assert not pid_file.exists()
        assert not socket.exists()
        assert len(proxy.commands) == reconciled + 1, "Manager shutdown did not clear its proxy routes"
        for launch_pid in map(int, launches):
            def gone():
                try:
                    os.kill(launch_pid, 0)
                    return False
                except ProcessLookupError:
                    return True
            wait_until(gone, f"process {launch_pid} to exit after shutdown", timeout=30)
        print(f"Passed {label}", flush=True)
    finally:
        stop_manager(children)
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                try:
                    child.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=10)


try:
    for first_daemon, second_daemon in ((False, False), (False, True),
                                        (True, False), (True, True)):
        exercise(first_daemon, second_daemon)
except BaseException:
    for log in sorted(Path(".").glob("*-*.log")):
        print(f"==> {log} <==", file=sys.stderr)
        print(log.read_text()[-8000:], file=sys.stderr)
    raise
finally:
    proxy.shutdown()
    proxy.server_close()
    proxy_thread.join(timeout=5)
    proxy_path.unlink(missing_ok=True)
