"""Exercise durable state and process locks without modifying the host system.

Nix/systemd effects are fake here. The separate VM test covers actual switching.
"""

import importlib.util
import multiprocessing
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("executor", sys.argv.pop(1))
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class ExecutorTests(unittest.TestCase):
    def test_planned_start_checks_both_generations_before_effects(self):
        for running, profile in [(self.new, self.old), (self.old, self.new)]:
            with self.subTest(running=running, profile=profile):
                with self.assertRaisesRegex(RuntimeError, "stale deployment plan"):
                    self.executor.start(
                        "planned",
                        self.new,
                        self.executable,
                        expected_system=running,
                        expected_profile=profile,
                    )
                self.assertIsNone(self.executor.read())
                self.assertEqual(self.calls, [])
                self.assertFalse(self.executor.roots.exists())

    def test_planned_start_and_confirmation(self):
        state = self.executor.start(
            "planned",
            self.new,
            self.executable,
            expected_system=self.old,
            expected_profile=self.old,
        )
        self.assertEqual(state["expectedSystem"], self.old)
        self.executor.worker("planned")
        self.assertEqual(self.executor.confirm("planned")["phase"], "succeeded")
        # Repeated delivery preserves identity, even after generations changed.
        self.assertEqual(
            self.executor.start(
                "planned",
                self.new,
                self.executable,
                expected_system=self.old,
                expected_profile=self.old,
            )["phase"],
            "succeeded",
        )
        with self.assertRaisesRegex(RuntimeError, "different request"):
            self.executor.start(
                "planned",
                self.new,
                self.executable,
                expected_system=self.new,
                expected_profile=self.old,
            )

    def test_planned_worker_rejects_drift_without_watchdog_reverting_it(self):
        for link in (self.running, self.profile):
            with self.subTest(link=link):
                # Each subcase uses a new transaction after a terminal rejection.
                self.active = False
                self.executor.start(
                    link.name,
                    self.new,
                    self.executable,
                    expected_system=self.old,
                    expected_profile=self.old,
                )
                link.unlink()
                link.symlink_to(self.new)
                self.calls.clear()
                self.executor.worker(link.name)
                self.assertEqual(self.executor.read()["phase"], "failed")
                self.executor.watchdog(link.name)
                self.assertEqual(self.calls, [])
                self.assertEqual(str(link.resolve()), self.new)
                link.unlink()
                link.symlink_to(self.old)

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.old = self.system("old")
        self.new = self.system("new")
        # Only relocate the store boundary. All path validation, filesystem
        # operations, checkpoints, and flock calls run through production code.
        relocated = patch.object(
            deploy, "STORE_PATH", re.compile(re.escape(str(self.root)) + r"/[a-z]+")
        )
        relocated.start()
        self.addCleanup(relocated.stop)
        self.profile = self.root / "profile"
        self.running = self.root / "running"
        self.profile.symlink_to(self.old)
        self.running.symlink_to(self.old)
        self.boot = self.root / "boot"
        self.boot.write_text("boot-one")
        self.calls = []
        self.active = False
        self.timer_active = False
        self.fail_switch = False
        self.fail_launch = False
        self.fail_observation = False
        self.interrupt_switch = False
        self.executor = deploy.Executor(
            self.root / "state",
            self.root / "roots",
            self.profile,
            self.running,
            self.boot,
            self.run_command,
        )
        self.executable = str(self.root / "executor/bin/devenv-machine-deploy")

    def system(self, name):
        path = self.root / name
        (path / "bin").mkdir(parents=True)
        (path / "bin/switch-to-configuration").write_text("fixture")
        return str(path)

    def run_command(self, args, **kwargs):
        self.calls.append(args)
        if args[0] == "systemd-run":
            if any(arg.startswith("--on-active=") for arg in args):
                self.timer_active = True
            else:
                self.active = True
            if self.fail_launch:
                raise subprocess.CalledProcessError(1, args)
        elif args[0] == "systemctl":
            if self.fail_observation:
                return subprocess.CompletedProcess(args, 1, stdout="")
            active = self.timer_active if args[-2].endswith(".timer") else self.active
            state = "active" if active else "inactive"
            return subprocess.CompletedProcess(
                args,
                0,
                stdout=f"LoadState=loaded\nActiveState={state}\nSubState=running\nJob=0\n",
            )
        elif args[0] == "nix-env":
            self.profile.unlink()
            self.profile.symlink_to(args[-1])
        else:
            self.assertIn(self.executor.read()["phase"], {"switching", "rolling-back"})
            if self.interrupt_switch:
                (self.root / "switch-started").touch()
                time.sleep(60)
            if self.fail_switch:
                raise subprocess.CalledProcessError(17, args)
            self.running.unlink()
            self.running.symlink_to(Path(args[0]).parent.parent)
        return subprocess.CompletedProcess(args, 0)

    def start(self, request_id="first", rollback=False):
        return self.executor.start(
            request_id, None if rollback else self.new, self.executable, rollback
        )

    def finish(self, request_id="first", confirm=True):
        self.executor.worker(request_id)
        self.active = False
        if confirm and self.executor.read()["phase"] == "awaiting-confirmation":
            self.executor.confirm(request_id)

    def expire(self, request_id="first"):
        with patch.object(
            deploy.time, "monotonic", return_value=self.executor.read()["deadline"] + 1
        ):
            self.executor.watchdog(request_id)

    def test_success_persists_and_reopens(self):
        self.start()
        self.assertEqual(self.running.resolve(), Path(self.old))
        self.assertEqual(self.executor.read()["phase"], "queued")
        self.finish()
        self.assertEqual(self.executor.status()["outcome"], "succeeded")
        self.assertEqual(self.running.resolve(), Path(self.new))
        self.assertEqual((self.root / "roots/first/previous").resolve(), Path(self.old))
        self.assertEqual(
            (self.root / "roots/first/requested").resolve(), Path(self.new)
        )
        self.assertEqual(self.executor.state_file.stat().st_mode & 0o777, 0o600)

    def test_explicit_rollback_restores_previous(self):
        self.start()
        self.finish()
        self.start("second", rollback=True)
        self.finish("second")
        self.assertEqual(self.running.resolve(), Path(self.old))
        self.assertEqual(self.profile.resolve(), Path(self.old))
        self.assertEqual(self.executor.read()["operation"], "rollback")

    def test_activation_failure_retains_recovery(self):
        self.start()
        self.fail_switch = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.finish()
        self.active = False
        self.assertEqual(self.executor.read()["phase"], "awaiting-rollback")
        self.assertEqual(self.profile.resolve(), Path(self.new))
        self.assertEqual(self.running.resolve(), Path(self.old))
        self.fail_switch = False
        self.start("recover", rollback=True)
        self.finish("recover")
        self.assertEqual(self.profile.resolve(), Path(self.old))

    def test_same_request_does_not_launch_twice(self):
        self.start()
        self.start()
        self.assertEqual(sum(call[0] == "systemd-run" for call in self.calls), 2)
        with self.assertRaisesRegex(RuntimeError, "different request"):
            self.executor.start("first", self.old, self.executable)

    def test_target_lock_rejects_other_process(self):
        with self.executor.lock():
            result = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    "import fcntl,sys; f=open(sys.argv[1],'a'); fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)",
                    str(self.root / "state/lock"),
                ],
                capture_output=True,
            )
        self.assertNotEqual(result.returncode, 0)

    def test_running_unit_rejects_conflicting_operation(self):
        self.start()
        with self.assertRaisesRegex(RuntimeError, "still running"):
            self.start("second")
        with self.assertRaisesRegex(RuntimeError, "still running"):
            self.start("second", rollback=True)

    def test_lost_launch_response_is_not_failed_activation(self):
        self.fail_launch = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.start()
        self.assertEqual(self.executor.read()["phase"], "queued")
        self.assertEqual(self.executor.status()["outcome"], "pending")
        self.finish()
        self.assertEqual(self.executor.status()["outcome"], "succeeded")

    def test_observation_failure_blocks_rollback(self):
        self.start()
        self.fail_observation = True
        self.assertEqual(self.executor.status()["outcome"], "unknown")
        with self.assertRaisesRegex(RuntimeError, "cannot observe"):
            self.start("second", rollback=True)

    def test_killed_worker_preserves_unknown_checkpoint(self):
        self.start()
        self.interrupt_switch = True
        process = multiprocessing.get_context("fork").Process(
            target=self.executor.worker, args=("first",)
        )
        process.start()
        try:
            deadline = time.monotonic() + 5
            while (
                not (self.root / "switch-started").exists()
                and time.monotonic() < deadline
            ):
                time.sleep(0.01)
            self.assertTrue((self.root / "switch-started").exists())
            with self.assertRaisesRegex(RuntimeError, "owns the target"):
                self.start("competing")
        finally:
            process.kill()
            process.join(5)
        self.active = False
        self.interrupt_switch = False
        self.assertIn(self.executor.read()["phase"], {"switching", "rolling-back"})
        self.assertEqual(self.executor.status()["outcome"], "pending")
        with self.assertRaisesRegex(RuntimeError, "unknown"):
            self.start("second")
        self.start("recover", rollback=True)
        self.finish("recover")
        self.assertEqual(self.running.resolve(), Path(self.old))

    def test_reboot_does_not_report_success(self):
        self.start()
        self.boot.write_text("boot-two")
        self.assertEqual(self.executor.status()["outcome"], "unknown")
        with self.assertRaisesRegex(RuntimeError, "earlier boot"):
            self.finish()

    def test_checkpoint_failure_prevents_launch(self):
        with patch.object(deploy, "atomic_json", side_effect=OSError("disk full")):
            with self.assertRaisesRegex(OSError, "disk full"):
                self.start()
        self.assertEqual(self.calls, [])

    def test_stale_worker_cannot_activate_new_request(self):
        self.start()
        self.finish()
        self.start("second", rollback=True)
        before = len(self.calls)
        with self.assertRaisesRegex(RuntimeError, "no longer queued"):
            self.executor.worker("first")
        self.assertEqual(len(self.calls), before)

    def test_success_exit_without_running_system_change_is_failure(self):
        self.start()
        with patch.object(
            self.executor,
            "run",
            side_effect=lambda args, **kwargs: (
                self.run_command(args, **kwargs)
                if args[0] == "systemctl"
                else subprocess.CompletedProcess(args, 0)
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "not active"):
                self.finish()
        self.assertEqual(self.executor.read()["phase"], "awaiting-rollback")

    def test_queued_systemd_job_is_pending(self):
        self.start()
        with patch.object(
            self.executor,
            "run",
            return_value=subprocess.CompletedProcess(
                [],
                0,
                stdout="LoadState=loaded\nActiveState=inactive\nSubState=dead\nJob=7\n",
            ),
        ):
            self.assertEqual(self.executor.status()["outcome"], "pending")
            with self.assertRaisesRegex(RuntimeError, "still running"):
                self.start("second", rollback=True)

    def test_exited_unit_with_empty_job_allows_rollback(self):
        self.start()
        self.finish()
        with patch.object(
            self.executor,
            "run",
            return_value=subprocess.CompletedProcess(
                [],
                0,
                stdout="LoadState=loaded\nActiveState=active\nSubState=exited\nJob=\n",
            ),
        ):
            self.start("second", rollback=True)
        self.assertEqual(self.executor.read()["operation"], "rollback")

    def test_corrupt_checkpoint_blocks_new_work(self):
        self.start()
        self.executor.state_file.write_text("{partial")
        with self.assertRaises(ValueError):
            self.start("second")
        self.assertEqual(sum(call[0] == "systemd-run" for call in self.calls), 2)

    def test_invalid_target_does_not_launch(self):
        with self.assertRaisesRegex(RuntimeError, "NixOS system"):
            self.executor.start("first", "/tmp/arbitrary", self.executable)
        self.assertEqual(self.calls, [])

    def test_unconfirmed_deployment_rolls_back(self):
        self.start()
        self.finish(confirm=False)
        self.assertEqual(self.executor.status()["outcome"], "pending")
        self.expire()
        self.assertEqual(self.executor.status()["outcome"], "rolled-back")
        self.assertEqual(str(self.running.resolve()), self.old)
        self.assertEqual(str(self.profile.resolve()), self.old)
        with self.assertRaisesRegex(RuntimeError, "not eligible"):
            self.executor.confirm("first")

    def test_confirmed_deployment_ignores_watchdog(self):
        self.start()
        self.finish()
        before = len(self.calls)
        self.expire()
        self.executor.confirm("first")
        self.assertEqual(len(self.calls), before)
        self.assertEqual(str(self.running.resolve()), self.new)

    def test_stale_watchdog_cannot_revert_new_deployment(self):
        self.start()
        self.finish()
        self.start("second", rollback=True)
        before = len(self.calls)
        self.executor.watchdog("first")
        self.assertEqual(len(self.calls), before)

    def test_early_watchdog_and_late_confirmation_rejected(self):
        self.start()
        self.finish(confirm=False)
        with self.assertRaisesRegex(RuntimeError, "not elapsed"):
            self.executor.watchdog("first")
        with patch.object(
            deploy.time, "monotonic", return_value=self.executor.read()["deadline"]
        ):
            with self.assertRaisesRegex(RuntimeError, "not eligible"):
                self.executor.confirm("first")

    def test_worker_cannot_start_after_deadline(self):
        self.start()
        before = len(self.calls)
        with patch.object(
            deploy.time, "monotonic", return_value=self.executor.read()["deadline"]
        ):
            self.executor.worker("first")
        self.assertEqual(len(self.calls), before)
        self.assertEqual(self.executor.read()["phase"], "awaiting-rollback")
        self.assertEqual(str(self.running.resolve()), self.old)

    def test_missing_watchdog_is_unknown_without_confirmation(self):
        self.start()
        self.finish(confirm=False)
        self.timer_active = False
        self.assertEqual(self.executor.status()["outcome"], "unknown")

    def test_watchdog_is_armed_before_activation_launch(self):
        self.start()
        launches = [call for call in self.calls if call[0] == "systemd-run"]
        self.assertEqual(len(launches), 2)
        self.assertEqual(launches[0][-2], "watchdog")
        self.assertEqual(launches[1][-2], "worker")

    def test_worker_refuses_activation_without_watchdog(self):
        self.start()
        self.timer_active = False
        with self.assertRaisesRegex(RuntimeError, "watchdog is not active"):
            self.executor.worker("first")
        self.assertFalse(any(call[0] == "nix-env" for call in self.calls))

    def test_reboot_recovers_unconfirmed_system(self):
        self.start()
        self.finish(confirm=False)
        self.boot.write_text("boot-two")
        self.executor.recover(self.executable)
        self.assertEqual(self.executor.read()["recoveryBootId"], "boot-two")
        self.executor.boot_worker("first")
        self.assertEqual(self.executor.status()["outcome"], "rolled-back")
        self.assertEqual(str(self.running.resolve()), self.old)
        self.assertEqual(str(self.profile.resolve()), self.old)

    def test_recovery_does_not_compete_with_live_switch(self):
        self.start()
        before = len(self.calls)
        with self.executor.lock():
            self.executor.recover(self.executable)
        self.assertEqual(len(self.calls), before)

    def test_reboot_preserves_confirmed_system(self):
        self.start()
        self.finish()
        self.boot.write_text("boot-two")
        before = len(self.calls)
        self.executor.recover(self.executable)
        self.assertEqual(len(self.calls), before)
        self.assertEqual(str(self.running.resolve()), self.new)

    def test_boot_recovery_launch_is_not_retried(self):
        self.start()
        self.finish(confirm=False)
        self.boot.write_text("boot-two")
        self.fail_launch = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.executor.recover(self.executable)
        before = len(self.calls)
        self.executor.recover(self.executable)
        self.boot.write_text("boot-three")
        self.executor.recover(self.executable)
        self.assertEqual(len(self.calls), before)
        with self.assertRaisesRegex(RuntimeError, "no longer belongs"):
            self.executor.boot_worker("first")

    def test_interrupted_rollback_is_not_retried_after_reboot(self):
        self.start()
        self.finish(confirm=False)
        state = self.executor.read()
        state["phase"] = "rolling-back"
        deploy.atomic_json(self.executor.state_file, state)
        self.boot.write_text("boot-two")
        before = len(self.calls)
        self.executor.recover(self.executable)
        self.assertEqual(len(self.calls), before)

    def test_stale_boot_worker_cannot_override_explicit_recovery(self):
        self.start()
        self.finish(confirm=False)
        self.boot.write_text("boot-two")
        self.executor.recover(self.executable)
        self.start("manual", rollback=True)
        with self.assertRaisesRegex(RuntimeError, "no longer belongs"):
            self.executor.boot_worker("first")

    def test_rollback_failure_retains_both_errors(self):
        self.start()
        self.fail_switch = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.finish()
        with self.assertRaises(subprocess.CalledProcessError):
            self.expire()
        state = self.executor.status()
        self.assertEqual(state["outcome"], "rollback-failed")
        self.assertIn("error", state)
        self.assertIn("rollbackError", state)


if __name__ == "__main__":
    unittest.main()
