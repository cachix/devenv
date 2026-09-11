import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest


WRITERS = json.loads(os.environ["FILE_COPY_WRITERS"])
CONTENT = "new content\n"


class CopyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.destination = self.root / "destination"

    def run_writer(self, kind="copy", **kwargs):
        return subprocess.run(
            [WRITERS[kind]], cwd=self.root, capture_output=True, timeout=10,
            **kwargs,
        )

    def assert_clean(self):
        self.assertEqual(list(self.root.glob(".devenv-file.*")), [])

    def pause_writer(self, kind, name):
        ready = self.root / f"{name}.ready"
        release = self.root / f"{name}.release"
        env = dict(os.environ, COPY_READY=str(ready), COPY_RELEASE=str(release))
        # Bash functions let the test pause the real copy before publication,
        # without depending on copy size or racing the scheduler.
        env["BASH_FUNC_cp%%"] = '''() {
            command cp "$@" || return
            touch "$COPY_READY"
            while [ ! -e "$COPY_RELEASE" ]; do sleep 0.01; done
        }'''
        process = subprocess.Popen(
            [WRITERS[kind]], cwd=self.root, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True,
        )

        def cleanup():
            release.touch()
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()

        self.addCleanup(cleanup)
        deadline = time.monotonic() + 10
        while not ready.exists():
            self.assertIsNone(process.poll(), "writer exited before its copy")
            self.assertLess(time.monotonic(), deadline, "copy did not reach barrier")
            time.sleep(0.01)
        return process, release

    def finish_writer(self, process, release):
        release.touch()
        _, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr.decode())

    def test_copy_publishes_complete_file(self):
        self.destination.write_text("old content\n")
        process, release = self.pause_writer("copy", "copy")
        self.assertEqual(self.destination.read_text(), "old content\n")
        self.finish_writer(process, release)
        self.assertEqual(self.destination.read_text(), CONTENT)
        self.assertTrue(self.destination.stat().st_mode & 0o200)
        self.assert_clean()

    def test_concurrent_seed_preserves_winners_edits(self):
        first, release_first = self.pause_writer("seed", "first")
        second, release_second = self.pause_writer("seed", "second")
        self.assertFalse(self.destination.exists())
        self.finish_writer(first, release_first)
        self.assertEqual(self.destination.read_text(), CONTENT)
        self.destination.write_text("user edit\n")
        self.finish_writer(second, release_second)
        self.assertEqual(self.destination.read_text(), "user edit\n")
        self.assert_clean()

    def test_failed_copy_preserves_destination(self):
        self.destination.write_text("old content\n")
        env = dict(os.environ)
        env["BASH_FUNC_cp%%"] = '() { return 1; }'
        result = self.run_writer(env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.destination.read_text(), "old content\n")
        self.assertFalse((self.root / "files.json").exists())
        self.assert_clean()

    def test_copy_replaces_symlinks_without_touching_targets(self):
        for target_kind in ("file", "directory", "missing", "store"):
            with self.subTest(target=target_kind):
                target = self.root / target_kind
                if target_kind == "file":
                    target.write_text("untouched\n")
                elif target_kind == "directory":
                    target.mkdir()
                elif target_kind == "store":
                    target = Path(os.environ["FILE_COPY_SOURCE"])
                self.destination.symlink_to(target)
                result = self.run_writer()
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertFalse(self.destination.is_symlink())
                self.assertEqual(self.destination.read_text(), CONTENT)
                if target_kind == "file":
                    self.assertEqual(target.read_text(), "untouched\n")
                elif target_kind == "directory":
                    self.assertEqual(list(target.iterdir()), [])
                elif target_kind == "missing":
                    self.assertFalse(target.exists())
                self.destination.unlink()
        self.assert_clean()

    def test_seed_keeps_existing_file_and_dangling_symlink(self):
        self.destination.write_text("user edit\n")
        self.assertEqual(self.run_writer("seed").returncode, 0)
        self.assertEqual(self.destination.read_text(), "user edit\n")
        self.destination.unlink()
        self.destination.symlink_to("missing")
        self.assertEqual(self.run_writer("seed").returncode, 0)
        self.assertEqual(os.readlink(self.destination), "missing")
        self.assertFalse((self.root / "missing").exists())

    def test_seed_converts_store_symlink(self):
        self.destination.symlink_to(os.environ["FILE_COPY_SOURCE"])
        self.assertEqual(self.run_writer("seed").returncode, 0)
        self.assertFalse(self.destination.is_symlink())
        self.assertEqual(self.destination.read_text(), CONTENT)
        self.assertTrue(self.destination.stat().st_mode & 0o200)

    def test_executable_mode_is_preserved(self):
        self.assertEqual(self.run_writer("executable").returncode, 0)
        self.assertEqual(self.destination.stat().st_mode & 0o700, 0o700)

    def test_directory_replacement(self):
        self.destination.mkdir()
        (self.destination / "old").touch()
        self.assertEqual(self.run_writer("directory").returncode, 0)
        self.assertEqual([p.name for p in self.destination.iterdir()], ["child"])
        self.assertEqual((self.destination / "child").read_text(), CONTENT)
        self.assertTrue((self.destination / "child").stat().st_mode & 0o200)
        self.assertEqual(self.run_writer().returncode, 0)
        self.assertEqual(self.destination.read_text(), CONTENT)
        self.assert_clean()

    def test_directory_seed_keeps_edits(self):
        self.assertEqual(self.run_writer("seedDirectory").returncode, 0)
        (self.destination / "child").write_text("user edit\n")
        self.assertEqual(self.run_writer("seedDirectory").returncode, 0)
        self.assertEqual((self.destination / "child").read_text(), "user edit\n")


if __name__ == "__main__":
    unittest.main()
