#!/usr/bin/env python3
"""Run a command in a fresh PTY, relaying stdin to it and capturing output.

Portable stand-in for util-linux `script -qefc CMD FILE`, which BSD/macOS
`script` cannot express (no -f/-c). Stdin — typically a scripted pipe of
keystrokes — is forwarded to the PTY as it arrives, PTY output is written
to FILE, and the exit status mirrors the child's, with 128+N for death by
signal N (so a SIGINT-terminated session reports 130, like `script -e`).

Usage: pty-run.py FILE CMD
"""

import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time

out_path, command = sys.argv[1], sys.argv[2]

pid, master = pty.fork()
if pid == 0:
    os.execvp("/bin/sh", ["/bin/sh", "-c", command])

# The TUI assertions assume a 40x120 terminal.
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))


def _kill_session(signum, _frame):
    # Sessions are bounded by timeout(1), which signals us, not the child.
    # A hung child may ignore SIGTERM, so escalate to SIGKILL on its
    # process group rather than leaving an orphan behind.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(pid, sig)
        except (ProcessLookupError, PermissionError):
            break
        time.sleep(0.5)
    os._exit(128 + signum)


signal.signal(signal.SIGTERM, _kill_session)
signal.signal(signal.SIGINT, _kill_session)
signal.signal(signal.SIGHUP, _kill_session)

stdin_fd = sys.stdin.fileno()
stdin_open = True
with open(out_path, "wb") as out:
    while True:
        fds = [master]
        if stdin_open:
            fds.append(stdin_fd)
        try:
            ready, _, _ = select.select(fds, [], [])
        except InterruptedError:
            continue
        if stdin_open and stdin_fd in ready:
            chunk = os.read(stdin_fd, 4096)
            if chunk:
                os.write(master, chunk)
            else:
                stdin_open = False
        if master in ready:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                # Linux raises EIO once the slave side is closed.
                chunk = b""
            if not chunk:
                break
            out.write(chunk)

_, status = os.waitpid(pid, 0)
code = os.waitstatus_to_exitcode(status)
sys.exit(code if code >= 0 else 128 - code)
