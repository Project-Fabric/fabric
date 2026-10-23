import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("runner", Path(__file__).parents[1] / "verification_runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class VerificationRunnerTests(unittest.TestCase):
    def test_complete_results_pass_and_run_ids_cannot_escape_reports(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            execute = lambda *args, **kwargs: subprocess.CompletedProcess(args[0], 0, "fixture output", "")
            for suite in runner.SUITES:
                runner.run_suite(suite, "../../outside", root, execute)
            self.assertEqual(runner.summarize("../../outside", root)["status"], "passed")
            directory = runner.run_directory(root / "reports", "../../outside")
            self.assertEqual(directory.parent, root / "reports")

    def test_missing_or_forged_success_does_not_pass_summary(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaises(RuntimeError):
                runner.summarize("missing", folder)
            directory = runner.run_directory(Path(folder) / "reports", "missing")
            for name in runner.SUITES:
                (directory / f"{name}.json").write_text(json.dumps({"schema": runner.SCHEMA, "simulated": True, "suite": name, "status": "passed", "checks": []}))
            with self.assertRaises(RuntimeError):
                runner.summarize("missing", folder)

    def test_failed_command_is_persisted_and_no_later_check_runs(self):
        with tempfile.TemporaryDirectory() as folder:
            calls = []
            def fail(command, **kwargs):
                calls.append(command)
                return subprocess.CompletedProcess(command, 7, "failed fixture", "")
            with self.assertRaises(RuntimeError):
                runner.run_suite("rules", "failure", folder, fail)
            self.assertEqual(len(calls), 1)
            report = json.loads((runner.run_directory(Path(folder) / "reports", "failure") / "rules.json").read_text())
            self.assertEqual(report["checks"][0]["exit_code"], 7)
            self.assertEqual(report["status"], "failed")

    def test_timeout_is_failure_without_recording_exception_payload(self):
        with tempfile.TemporaryDirectory() as folder:
            def timeout(command, **kwargs):
                raise subprocess.TimeoutExpired(command, 120, output="synthetic private payload")
            with self.assertRaises(RuntimeError):
                runner.run_suite("crash", "timeout", folder, timeout)
            directory = runner.run_directory(Path(folder) / "reports", "timeout")
            record = json.loads((directory / "crash.json").read_text())
            self.assertEqual(record["checks"][0]["reason"], "timeout")
            self.assertNotIn("private payload", (directory / "crash.log").read_text())

    def test_children_do_not_inherit_provider_keys_and_logs_are_bounded(self):
        with tempfile.TemporaryDirectory() as folder, patch.dict(os.environ, {"COSIGNA_GROQ_API_KEY": "synthetic-fixture-secret", "STRIPE_SECRET_KEY": "synthetic-fixture-secret"}):
            def execute(command, **kwargs):
                self.assertNotIn("COSIGNA_GROQ_API_KEY", kwargs["env"])
                self.assertNotIn("STRIPE_SECRET_KEY", kwargs["env"])
                self.assertIsInstance(command, list)
                self.assertNotIn("shell", kwargs)
                return subprocess.CompletedProcess(command, 0, "☕" * 100_000, "")
            record = runner.run_suite("crash", "bounded", folder, execute)
            self.assertTrue(record["log_truncated"])
            self.assertLessEqual((runner.run_directory(Path(folder) / "reports", "bounded") / "crash.log").stat().st_size, runner.MAX_LOG_BYTES)

    def test_unknown_suite_cannot_supply_a_command(self):
        with self.assertRaises(ValueError):
            runner.run_suite("arbitrary-command", "invalid")


if __name__ == "__main__":
    unittest.main()
