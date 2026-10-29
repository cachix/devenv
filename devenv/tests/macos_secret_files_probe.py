#!/usr/bin/env python3
"""Native macOS RAM-disk lifecycle experiment, using only public fixture bytes.

Run without sudo. Never format or detach a device that this invocation did not
allocate. This is a feasibility probe, not the production SecretSpec backend.
"""

import concurrent.futures
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import select
import subprocess
import sys
import tempfile
import time


def run(*args):
    print("+", *map(str, args), file=sys.stderr, flush=True)
    result = subprocess.run(args, capture_output=True, timeout=45)
    if result.returncode:
        raise RuntimeError(
            f"{args[0]} exited {result.returncode}: "
            f"{result.stdout.decode(errors='replace')}"
            f"{result.stderr.decode(errors='replace')}"
        )
    return result.stdout


def detach(device):
    assert re.fullmatch(r"/dev/disk[0-9]+", device), device
    for attempt in range(3):
        try:
            run("/usr/bin/hdiutil", "detach", "-force", device)
            return
        except RuntimeError:
            if attempt == 2:
                raise
            time.sleep(0.2)


class Volume:
    def __init__(self, root, label):
        self.mount = root / label
        self.mount.mkdir(mode=0o700)
        self.device = None
        self.file = self.mount / "key"
        self.ram_url = f"ram://{32768 + secrets.randbelow(32768)}"

    def create(self, contents):
        # ram:// creates a blank, memory-backed device, not an image file.
        info = plistlib.loads(run(
            "/usr/bin/hdiutil", "attach", "-nomount", "-plist", self.ram_url
        ))
        devices = [entry["dev-entry"] for entry in info["system-entities"]
                   if re.fullmatch(r"/dev/disk[0-9]+", entry.get("dev-entry", ""))]
        assert len(devices) == 1, info
        self.device = devices[0]
        run("/sbin/newfs_hfs", "-U", str(os.geteuid()), "-G", str(os.getegid()),
            "-M", "700", "-v", "devenv-secret-probe", self.device)
        run("/sbin/mount", "-t", "hfs", "-o", "owners,nobrowse,nodev,nosuid,noexec",
            self.device, str(self.mount))
        # Refuse to write even fixture contents until the private directory has
        # actually become a separate mounted filesystem with ownership enabled.
        assert self.mount.stat().st_dev != self.mount.parent.stat().st_dev
        info = plistlib.loads(run("/usr/sbin/diskutil", "info", "-plist", str(self.mount)))
        print("mounted volume:", json.dumps(info, default=str), flush=True)
        assert info["DeviceIdentifier"] == self.device.removeprefix("/dev/")
        assert Path(info["MountPoint"]).resolve() == self.mount.resolve()
        assert info["GlobalPermissionsEnabled"] is True, info
        assert self.mount.stat().st_uid == os.geteuid()
        assert self.mount.stat().st_mode & 0o777 == 0o700
        fd = os.open(self.file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as output:
            output.write(contents)
            os.fchmod(output.fileno(), 0o400)
        assert self.file.stat().st_mode & 0o777 == 0o400

    def cleanup(self):
        if self.device is not None:
            # Check the image association before touching a device identifier
            # that could have been reused after successful supervisor cleanup.
            images = plistlib.loads(run("/usr/bin/hdiutil", "info", "-plist"))["images"]
            ours = [image for image in images
                    if image.get("image-path") == self.ram_url
                    and any(entry.get("dev-entry") == self.device
                            for entry in image.get("system-entities", []))]
            if ours:
                detach(self.device)
            else:
                assert self.mount.stat().st_dev == self.mount.parent.stat().st_dev
        self.mount.rmdir()


def supervise(owner, mount, device):
    with select.kqueue() as queue:
        event = select.kevent(owner, filter=select.KQ_FILTER_PROC,
                             flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                             fflags=select.KQ_NOTE_EXIT)
        queue.control([event], 0, 0)
        print("ready", flush=True)
        events = queue.control(None, 1, 60)
        assert events and events[0].fflags & select.KQ_NOTE_EXIT
        # Unlink before detaching, so a busy volume never retains a pathname.
        (Path(mount) / "key").unlink()
        detach(device)
        print("cleaned", flush=True)


def start_supervisor(owner, volume):
    child = subprocess.Popen(
        [sys.executable, __file__, "--supervise", str(owner.pid),
         str(volume.mount), volume.device],
        stdout=subprocess.PIPE, text=True,
    )
    assert select.select([child.stdout], [], [], 30)[0], "supervisor startup timed out"
    assert child.stdout.readline().strip() == "ready"
    return child


def start_owner():
    return subprocess.Popen([sys.executable, "-c", "input()"], stdin=subprocess.PIPE)


def check_reads(path, expected):
    with path.open("rb") as source:
        assert source.read() == expected
        source.seek(0)
        assert source.read() == expected


def probe():
    assert sys.platform == "darwin", "this experiment requires native macOS"
    assert os.geteuid() != 0, "run as an ordinary user, without sudo"
    run("/usr/bin/sw_vers")
    run("/usr/bin/id")
    volumes, owners, supervisors = [], [], []
    with tempfile.TemporaryDirectory(prefix="devenv-secret-probe-") as directory:
        root = Path(directory)
        try:
            for index in range(2):
                volume = Volume(root, f"consumer-{index}")
                volumes.append(volume)
                contents = f"public-test-fixture-{index}".encode()
                volume.create(contents)
                owner = start_owner()
                owners.append(owner)
                supervisors.append(start_supervisor(owner, volume))
                with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
                    list(executor.map(lambda _: check_reads(volume.file, contents), range(12)))
                run(sys.executable, "-c",
                    "import pathlib,sys; assert pathlib.Path(sys.argv[1]).read_bytes() == sys.argv[2].encode()",
                    str(volume.file), contents.decode())
            print("PASS: unprivileged mounts, permissions, seek/reopen, concurrent readers and consumers", flush=True)
            for index, (owner, supervisor, volume) in enumerate(zip(owners, supervisors, volumes)):
                # Keep a descriptor open to exercise force-unmount of busy files.
                with volume.file.open("rb"):
                    if index == 0:
                        owner.stdin.write(b"exit\n")
                        owner.stdin.flush()
                    else:
                        owner.kill()
                    owner.wait(timeout=10)
                    assert supervisor.wait(timeout=30) == 0
                    assert supervisor.stdout.read().strip() == "cleaned"
                    assert not volume.file.exists()
                    assert volume.mount.stat().st_dev == root.stat().st_dev
                print(f"PASS: {'normal exit' if index == 0 else 'SIGKILL'} cleanup with open file", flush=True)
                if index == 0:
                    assert volumes[1].file.read_bytes() == b"public-test-fixture-1"
            # Establish the precise failure guarantee instead of assuming the
            # kernel destroys the disk when the cleanup supervisor is killed.
            volume = Volume(root, "supervisor-killed")
            volumes.append(volume)
            volume.create(b"public-crash-fixture")
            owner = start_owner()
            owners.append(owner)
            supervisor = start_supervisor(owner, volume)
            supervisors.append(supervisor)
            supervisor.kill()
            supervisor.wait(timeout=10)
            owner.kill()
            owner.wait(timeout=10)
            assert volume.file.read_bytes() == b"public-crash-fixture"
            print("CONFIRMED LIMITATION: supervisor SIGKILL leaves the RAM disk attached", flush=True)
        finally:
            for child in supervisors + owners:
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=10)
            errors = []
            for volume in reversed(volumes):
                try:
                    volume.cleanup()
                except Exception as error:
                    errors.append(str(error))
            assert not errors, errors


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--supervise":
        supervise(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    else:
        probe()
