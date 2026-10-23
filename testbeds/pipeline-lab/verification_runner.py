"""Fixed fixture checks. Airflow is optional; this module has no provider credentials."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

SCHEMA = "cosigna.verification/v1"
SUITES = {
    "crash": ("crash_recovery",),
    "rules": ("gateway", "transactions", "pipeline", "worker", "http", "planner"),
    "service": ("service",),
}
ENVIRONMENT_KEYS = {"PATH", "TMPDIR", "TEMP", "TMP", "SystemRoot", "WINDIR", "LD_LIBRARY_PATH", "LANG"}
MAX_LOG_BYTES = 65_536


def run_directory(reports, run_id):
    # Airflow run IDs never become path components or shell commands.
    return Path(reports) / hashlib.sha256(run_id.encode()).hexdigest()[:24]


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def run_suite(suite, run_id, root="/opt/cosigna", execute=subprocess.run):
    if suite not in SUITES:
        raise ValueError("unknown verification suite")
    root = Path(root)
    directory = run_directory(root / "reports", run_id)
    directory.mkdir(parents=True, exist_ok=True)
    environment = {key: value for key, value in os.environ.items() if key in ENVIRONMENT_KEYS}
    results = []
    logs = []
    for name in SUITES[suite]:
        command = (["node", str(root / "scripts/check-gateway-service.mjs"), str(root / "target/debug/cosigna-agent")]
                   if name == "service" else [str(root / "tests" / name)])
        started = time.monotonic()
        try:
            completed = execute(command, cwd=root, env=environment, capture_output=True,
                                text=True, timeout=120, check=False)
            code = completed.returncode
            status = "passed" if code == 0 else "failed"
            output = (completed.stdout or "") + (completed.stderr or "")
            reason = None if code == 0 else "nonzero_exit"
        except subprocess.TimeoutExpired:
            code, status, output, reason = None, "failed", "", "timeout"
        except OSError:
            code, status, output, reason = None, "failed", "", "cannot_start"
        results.append({"check": name, "status": status, "exit_code": code,
                        "reason": reason, "duration_ms": round((time.monotonic() - started) * 1000)})
        logs.append(f"Check: {name}\n{output}")
        if status != "passed":
            break
    log = "\n".join(logs).encode("utf-8")
    record = {"schema": SCHEMA, "simulated": True, "suite": suite,
              "status": "passed" if all(r["status"] == "passed" for r in results) else "failed",
              "checks": results, "log_truncated": len(log) > MAX_LOG_BYTES}
    (directory / f"{suite}.log").write_bytes(log[:MAX_LOG_BYTES].decode("utf-8", errors="ignore").encode("utf-8"))
    write_json(directory / f"{suite}.json", record)
    if record["status"] != "passed":
        raise RuntimeError(f"fixture verification failed: {suite}; inspect its bounded local report")
    return record


def summarize(run_id, root="/opt/cosigna"):
    directory = run_directory(Path(root) / "reports", run_id)
    suites = []
    for name in SUITES:
        try:
            record = json.loads((directory / f"{name}.json").read_text(encoding="utf-8"))
            expected = list(SUITES[name])
            valid = (record["schema"] == SCHEMA and record["simulated"] is True
                     and record["suite"] == name and record["status"] == "passed"
                     and [r["check"] for r in record["checks"]] == expected
                     and all(r["status"] == "passed" and r["exit_code"] == 0 for r in record["checks"]))
        except (OSError, ValueError, KeyError, TypeError):
            valid = False
        suites.append({"suite": name, "status": "passed" if valid else "failed"})
    report = {"schema": SCHEMA, "simulated": True, "suite": "summary",
              "status": "passed" if all(s["status"] == "passed" for s in suites) else "failed",
              "suites": suites}
    write_json(directory / "summary.json", report)
    if report["status"] != "passed":
        raise RuntimeError("verification incomplete or failed; see summary.json")
    return report
