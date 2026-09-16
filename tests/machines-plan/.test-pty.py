"""Exercise the real default-no prompt using a foreground terminal."""

import os
from pathlib import Path
import select
import signal
import time

for response, should_copy in [(b"\n", False), (b"y\n", True)]:
    log = Path(os.environ["PLAN_LOG"])
    log.write_text("")
    pid, terminal = os.forkpty()
    if pid == 0:
        os.execvp("devenv", ["devenv", "machines", "deploy", "server"])
    output = b""
    answered = False
    completed = False
    try:
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if select.select([terminal], [], [], 0.1)[0]:
                try:
                    output += os.read(terminal, 65536)
                except OSError:
                    pass
            if not answered and b"Apply this NixOS plan?" in output:
                # The displayed plan must precede the confirmation.
                assert os.environ["PLAN_NEW"].encode() in output
                assert "copy " not in log.read_text()
                os.write(terminal, response)
                answered = True
            child, status = os.waitpid(pid, os.WNOHANG)
            if child:
                completed = True
                assert os.waitstatus_to_exitcode(status) == 0, output.decode(
                    errors="replace"
                )
                break
        assert completed and answered, output.decode(errors="replace")
        assert ("copy " in log.read_text()) == should_copy
    finally:
        if not completed:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(terminal)
