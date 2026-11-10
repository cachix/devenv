"""Exercise actual shell entry and direnv load/unload, rather than only capture."""

import errno
import fcntl
import json
import os
import pathlib
import pty
import select
import shlex
import signal
import subprocess
import struct
import tempfile
import termios
import time

ROOT = pathlib.Path.cwd()
EXPECTED = {
    "CC": "zig cc",
    "TZ": "Etc/GMT+3",
    "SSL_CERT_FILE": "/configured/ca.pem",
    "ENV_SEARCH_PATH": "first:setup:last",
    "ENV_EMPTY": "",
    "ENV_ENTER_WIN": "enterShell",
    "ENV_TASK_WIN": "task",
}


def checked(command, **kwargs):
    result = subprocess.run(command, capture_output=True, text=True, timeout=180, **kwargs)
    assert result.returncode == 0, (command, result.stdout[-4000:], result.stderr[-4000:])
    return result.stdout


with tempfile.TemporaryDirectory(prefix="devenv-helper-activation-") as directory:
    temporary = pathlib.Path(directory)
    # Use the same assertion program inside every interactive shell. Its success
    # marker is absent from the command sent to the terminal, so echoed input
    # cannot masquerade as a successful assertion.
    assertion = temporary / "assert-environment.py"
    assertion.write_text(
        "import os\n"
        f"expected = {EXPECTED!r}\n"
        "for name, value in expected.items():\n"
        "    assert os.environ.get(name) == value, (name, os.environ.get(name), value)\n"
        "assert not any(name.startswith('_devenv_env_') for name in os.environ)\n"
        "print('HELPER_ACTIVATION_PASSED', flush=True)\n"
    )
    environment = os.environ.copy()
    # Prevent personal interactive configuration from changing the assertions.
    environment["XDG_CONFIG_HOME"] = str(temporary / "config")
    environment["ZDOTDIR"] = str(temporary / "zsh")
    environment["TERM"] = "xterm-256color"
    for shell in ("bash", "zsh", "fish", "nu"):
        pid, terminal = pty.fork()
        if pid == 0:
            os.execvpe(
                "devenv",
                ["devenv", "--no-tui", "--no-reload", "--shell", shell, "shell"],
                environment,
            )
        output = bytearray()
        status = None
        sent_command = False
        queries = b""
        try:
            fcntl.ioctl(terminal, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 120, 0, 0))
            command = f"python3 {shlex.quote(str(assertion))}; exit\n".encode()
            if shell != "nu":
                os.write(terminal, command)
                sent_command = True
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline:
                ready, _, _ = select.select([terminal], [], [], 1)
                if ready:
                    try:
                        chunk = os.read(terminal, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not chunk:
                        break
                    output.extend(chunk)
                    # Reedline asks the terminal about cursor position and
                    # keyboard support before it accepts commands. Answer those
                    # queries, and wait for the prompt before sending input.
                    queries += chunk
                    for query, response in (
                        (b"\x1b[6n", b"\x1b[1;1R"),
                        (b"\x1b[?u", b"\x1b[?0u"),
                        (b"\x1b[c", b"\x1b[?1;2c"),
                    ):
                        while query in queries:
                            os.write(terminal, response)
                            queries = queries.replace(query, b"", 1)
                    queries = queries[-32:]
                    if not sent_command and b"(devenv)" in output:
                        os.write(terminal, command)
                        sent_command = True
                finished, child_status = os.waitpid(pid, os.WNOHANG)
                if finished:
                    status = child_status
                    break
            else:
                raise AssertionError(f"{shell} activation timed out: {output[-6000:]!r}")
            if status is None:
                _, status = os.waitpid(pid, 0)
        finally:
            os.close(terminal)
            if status is None:
                # Also clean up a stalled shell if an assertion above fails.
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                _, status = os.waitpid(pid, 0)
        assert b"HELPER_ACTIVATION_PASSED\r\n" in output, (shell, output[-6000:])
        assert os.waitstatus_to_exitcode(status) == 0, (shell, status, output[-6000:])
        print(f"PASS: actual {shell} activation, composition, cleanup and task precedence", flush=True)

    # direnv must restore both existing values and previously unset variables.
    (ROOT / ".envrc").write_text('eval "$(devenv direnvrc)"\nuse devenv\n')
    direnv_script = r'''
set -euo pipefail
export DIRENV_LOG_FORMAT=''
export CC=outer-cc TZ=outer-tz ENV_SEARCH_PATH=outer-path
export SSL_CERT_FILE="$ENV_TEST_CA_BUNDLE"
unset ENV_EMPTY ENV_ENTER_WIN ENV_TASK_WIN
unset DIRENV_DIFF DIRENV_DIR DIRENV_FILE DIRENV_WATCHES
# The fixture's .envrc must be evaluated by the real direnv executable.
direnv allow .
eval "$(direnv export bash)"
python3 "$1"
test -z "${_devenv_env_final_values+x}"
cd "$2"
eval "$(direnv export bash)"
test "$CC" = outer-cc
test "$TZ" = outer-tz
test "$SSL_CERT_FILE" = "$ENV_TEST_CA_BUNDLE"
test "$ENV_SEARCH_PATH" = outer-path
test -z "${ENV_EMPTY+x}"
test -z "${ENV_ENTER_WIN+x}"
test -z "${ENV_TASK_WIN+x}"
'''
    checked(["bash", "--noprofile", "--norc", "-c", direnv_script,
             "bash", str(assertion), str(temporary)], env=environment)
    print("PASS: direnv helper activation and restoration on exit", flush=True)

    if os.uname().sysname == "Darwin":
        sdk = {
            "DEVELOPER_DIR": "/Applications/DevenvTestXcode.app/Contents/Developer",
            "SDKROOT": "/configured/MacOSX.sdk",
        }
        captured = json.loads(checked(["devenv", "--profile", "sdk", "print-dev-env", "--json"]))
        for name, expected in sdk.items():
            assert captured["variables"][name]["value"] == expected
        sdk_assertion = temporary / "assert-sdk.py"
        sdk_assertion.write_text(
            "import os\n"
            f"expected = {sdk!r}\n"
            "for name, value in expected.items():\n"
            "    assert os.environ.get(name) == value, (name, os.environ.get(name))\n"
        )
        checked(["devenv", "--profile", "sdk", "shell", "--", "python3", str(sdk_assertion)])
        print("PASS: macOS SDK overrides survive setup, capture and CLI activation", flush=True)
