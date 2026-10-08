import errno
import os
from pathlib import Path
import socket
import sys


if sys.argv[1] == "server":
    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        server.listen()
        Path("network-port").write_text(str(server.getsockname()[1]))
        while True:
            connection, _ = server.accept()
            with connection:
                connection.sendall(b"ok")

blocked = sys.argv[1] == "blocked"
address = ("127.0.0.1", int(os.environ["SANDBOX_NETWORK_PORT"]))
for protocol in ("tcp", "udp"):
    try:
        if protocol == "tcp":
            with socket.create_connection(address, timeout=2) as connection:
                assert connection.recv(2) == b"ok"
        else:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
                connection.sendto(b"check", address)
    except OSError as error:
        # A stopped listener or timeout must not masquerade as sandbox enforcement.
        if not blocked or error.errno not in (errno.EACCES, errno.EPERM):
            raise
    else:
        assert not blocked, f"Sandbox allowed {protocol} networking"
