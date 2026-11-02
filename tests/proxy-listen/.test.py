"""Exercise project listener configuration through the real CLI and daemon."""

import errno
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

ROOT = Path.cwd()
ENV = dict(os.environ, DEVENV_NO_AI_AGENT="1")
for variable in ("DEVENV_PROXY_LISTEN", "DEVENV_PROXY_HTTPS_LISTEN"):
    ENV.pop(variable, None)
ENV["DEVENV_PROXY_SOCKET"] = str(ROOT / "proxy.sock")
ENV["DEVENV_RUNTIME"] = str(ROOT / ".runtime")
DAEMONS = set()
PROJECTS = {ROOT}


def cli(*args, project=ROOT, env=None, succeeds=True):
    result = subprocess.run(
        ["devenv", "--no-tui", *args], cwd=project, env=env or ENV,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=120,
    )
    assert (result.returncode == 0) == succeeds, result.stdout
    return result.stdout


def settings(project=ROOT, **values):
    values.setdefault("name", "proxy-listen-test")
    (project / "listeners.nix").write_text(
        "{\n" + "".join(f"  {key} = {json.dumps(value)};\n" for key, value in values.items()) + "}\n"
    )
    (project / "marker").write_text(values["name"])


def free_port(host="127.0.0.1"):
    family = socket.AF_INET6 if ":" in host else socket.AF_INET
    with socket.socket(family) as listener:
        listener.bind((host, 0))
        return listener.getsockname()[1]


def control(command="status"):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5)
        stream.connect(ENV["DEVENV_PROXY_SOCKET"])
        stream.sendall(json.dumps({"command": command}).encode() + b"\n")
        return json.loads(stream.makefile("rb").readline())


def wait_for(check):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            if check():
                return
        except (OSError, http.client.HTTPException):
            pass
        time.sleep(0.05)
    raise AssertionError("timed out waiting for proxy request")


def request(host, port, name="proxy-listen-test"):
    connection = http.client.HTTPConnection(host, port, timeout=2)
    try:
        connection.request("GET", "/marker", headers={"Host": f"web.{name}.localhost"})
        response = connection.getresponse()
        return response.status == 200 and response.read().decode() == name
    finally:
        connection.close()


def stop():
    for project in PROJECTS:
        cli("down", project=project)
    if Path(ENV["DEVENV_PROXY_SOCKET"]).exists():
        try:
            pid = control()["pid"]
        except (OSError, KeyError):
            return
        assert pid in DAEMONS, "refusing to stop a daemon not started by this test"
        os.kill(pid, signal.SIGTERM)
        # The control socket can remain on disk after exit.
        def stopped():
            try:
                control()
                return False
            except OSError:
                return True
        wait_for(stopped)


def start(**kwargs):
    output = cli("up", "-d", **kwargs)
    status = control()
    DAEMONS.add(status["pid"])
    return output, status


try:
    for host in ("127.0.0.1", "::1"):
        try:
            port = free_port(host)
        except OSError as error:
            if host == "::1" and error.errno in (errno.EAFNOSUPPORT, errno.EADDRNOTAVAIL, errno.EPROTONOSUPPORT):
                print(f"skipping IPv6: {error}")
                continue
            raise
        address = f"[{host}]:{port}" if ":" in host else f"{host}:{port}"
        settings(http=address, https="invalid-unused-https")
        output, status = start(env=dict(ENV, DEVENV_PROXY_HTTPS_LISTEN="invalid-unused-override"))
        assert status["listen"] == address, status
        assert status["https_listen"] is None, status
        assert f"http://web.proxy-listen-test.localhost:{port}" in output, output
        wait_for(lambda: request(host, port))
        stop()
finally:
    stop()
