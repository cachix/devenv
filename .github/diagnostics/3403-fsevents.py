"""Exercise genuine FSEvents overflow against a packaged CLI in a macOS PTY."""

import datetime
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


HINTS = ("rescan: user dropped", "rescan: kernel dropped")
VALUE_COMMAND = b"printf 'PROBE_VALUE=%s\\n' \"$PROBE_VALUE\"\n"


def clock_stamp():
    return datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")


def registered_clients(pid, start):
    predicate = (
        'process == "fseventsd" AND eventMessage CONTAINS "fsevent_add_client" '
        f'AND eventMessage CONTAINS "pid {pid}"'
    )
    result = subprocess.run(
        ["/usr/bin/log", "show", "--start", start, "--style", "compact",
         "--predicate", predicate],
        text=True, capture_output=True, timeout=40, check=False,
    )
    if result.returncode:
        print(f"fseventsd log unavailable: {result.stderr.strip()}", flush=True)
        return set()
    return {line for line in result.stdout.splitlines()
            if re.search(rf"\bpid {pid}\b", line)}


def start_project(root, repo):
    project = root / "project"
    project.mkdir()
    churn = project / "tracked" / "churn"
    churn.mkdir(parents=True)
    value = project / "value"
    value.write_text("v1")
    shutil.copyfile(repo / "devenv.lock", project / "devenv.lock")
    (project / "devenv.yaml").write_text(
        "inputs:\n"
        "  nixpkgs:\n"
        "    url: github:cachix/devenv-nixpkgs/rolling\n"
        "  devenv:\n"
        f"    url: path:{repo / 'src/modules'}\n"
    )
    (project / "devenv.nix").write_text(
        "{ ... }: {\n"
        "  env.PROBE_VALUE = builtins.readFile ./value;\n"
        "  env.PROBE_ENTRIES = toString (builtins.length (builtins.attrNames (builtins.readDir ./tracked)));\n"
        "  enterShell = ''echo GH_FSEVENTS_PROBE_READY'';\n"
        "}\n"
    )
    return project, churn, value


def run_trial(label, binary, repo, root):
    project, churn, value = start_project(root, repo)
    trace = root / "watcher.trace"
    env = os.environ.copy()
    for key in ("CI", "DEVENV_TUI", "DEVENV_TRACE_TO", "DEVENV_ROOT",
                "DEVENV_SHELL_TYPE", "_DEVENV_SHELL_HINT", "_DEVENV_HOOK_DIR"):
        env.pop(key, None)
    env.update(
        TERM="xterm-256color", DEVENV_NO_AI_AGENT="1", TMPDIR=str(root),
        XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
        DEVENV_HOME=str(root / "devenv-home"),
        PATH=f"{Path(binary).parent}:{env['PATH']}",
    )
    start = clock_stamp()
    pid, master = pty.fork()
    if pid == 0:
        os.chdir(project)
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
        os.execve(binary, [binary, "--shell", "bash", "--trace-to",
                          f"full:file:{trace}", "shell"], env)

    terminal = bytearray()

    def pull(seconds, marker=None, ready_file=None):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if marker is not None and marker in terminal:
                return True
            if ready_file is not None and ready_file.is_file():
                return True
            if not select.select([master], [], [], min(0.2, max(0, deadline - time.monotonic())))[0]:
                continue
            try:
                data = os.read(master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if not data:
                raise RuntimeError(f"{label}: interactive shell exited")
            terminal.extend(data)
            del terminal[:-65536]
        return False

    def wait_for_output(command, expected, timeout=30):
        terminal.clear()
        os.write(master, command)
        if not pull(timeout, marker=expected):
            raise RuntimeError(f"{label}: did not see {expected!r} at the prompt")

    def run_overflow(seconds, new_value=None):
        before = trace.stat().st_size
        deadline = time.monotonic() + seconds
        count = 0
        os.kill(pid, signal.SIGSTOP)
        try:
            if new_value is not None:
                value.write_text(new_value)
            while time.monotonic() < deadline and count < 500_000:
                path = churn / f"event-{count:08x}"
                path.write_bytes(b"x")
                path.unlink()
                count += 1
        finally:
            os.kill(pid, signal.SIGCONT)
        pull(6)
        with trace.open() as log:
            log.seek(before)
            lines = log.read().splitlines()
        drops = [line for line in lines if "processed notify event into watchexec event" in line
                 and any(hint in line for hint in HINTS)]
        registrations = [line for line in lines if "Config: pathset" in line]
        print(f"{label}: {count:,} file create/delete pairs, {len(drops)} delivered "
              f"overflow notices, {len(registrations)} watch pathset updates", flush=True)
        for line in drops[:2] + registrations[:2]:
            print(line[:3500], flush=True)
        return len(drops), len(registrations)

    try:
        if not pull(250, marker=b"GH_FSEVENTS_PROBE_READY"):
            raise RuntimeError(f"{label}: shell did not become ready")
        wait_for_output(VALUE_COMMAND, b"PROBE_VALUE=v1")
        pull(2)
        initial_events = registered_clients(pid, start)
        initial_trace = trace.read_text(errors="replace")
        if "Config: pathset" not in initial_trace or str(value.resolve()) not in initial_trace:
            raise RuntimeError(f"{label}: initial watch registration was not visible in the trace")
        print(f"{label}: pid={pid}, {len(initial_events)} startup registrations in fseventsd logs",
              flush=True)

        dropped = 0
        pathset_updates = 0
        for duration in (5, 9, 13):
            drops, updates = run_overflow(duration)
            dropped += drops
            pathset_updates += updates
            if dropped and (label == "patched" or pathset_updates):
                break

        if initial_events:
            subsequent_events = registered_clients(pid, start) - initial_events
            print(f"{label}: {len(subsequent_events)} new fseventsd client registrations during churn",
                  flush=True)
            for line in sorted(subsequent_events)[:6]:
                print(line, flush=True)
        else:
            subsequent_events = None
            print(f"{label}: fseventsd registration count unavailable for this runner", flush=True)

        if label == "baseline":
            return bool(dropped and pathset_updates)
        if pathset_updates or subsequent_events:
            raise RuntimeError("patched shell reconfigured watches during churn")
        if not dropped:
            raise RuntimeError("patched trial saw no delivered overflow")

        reload_drops, _ = run_overflow(13, new_value="v2")
        reload_file = root / f"devenv-reload-{pid}.sh"
        if not pull(120, ready_file=reload_file):
            raise RuntimeError("patched shell did not prepare a reload for the changed file")
        wait_for_output(b"__devenv_reload_apply; " + VALUE_COMMAND, b"PROBE_VALUE=v2")
        print(f"patched: the live Bash session loaded v2 ({reload_drops} overflow notices "
              "in the file-edit cycle)", flush=True)
        if not reload_drops:
            raise RuntimeError("reload worked but the file-edit cycle saw no delivered overflow")
        return True
    except Exception:
        print(terminal[-8000:].decode(errors="replace"), flush=True)
        if trace.is_file():
            relevant = (line for line in trace.read_text(errors="replace").splitlines()
                        if any(needle in line for needle in
                               ("File content changed", "watcher for", "Config: pathset",
                                "rescan: user dropped", "rescan: kernel dropped")))
            for line in list(relevant)[-15:]:
                print(line[:1800], flush=True)
        raise
    finally:
        try:
            os.kill(pid, signal.SIGCONT)
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        os.close(master)
        for _ in range(40):
            if os.waitpid(pid, os.WNOHANG)[0]:
                break
            time.sleep(0.1)
        else:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)


def main():
    label, binary = sys.argv[1:]
    repo = Path.cwd().resolve()
    print(f"{label}: {subprocess.check_output([binary, '--version'], text=True).strip()}",
          flush=True)
    with tempfile.TemporaryDirectory(prefix=f"fsevents-{label}-") as directory:
        confirmed = run_trial(label, binary, repo, Path(directory).resolve())
    if not confirmed:
        raise SystemExit("baseline did not reproduce re-registration after a delivered overflow")


if __name__ == "__main__":
    main()
