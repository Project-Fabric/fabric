"""Runs the lessons experiment (docs/EXPERIMENT_LESSONS.md) with real Claude Code.

For each trap and run:
  phase 1  Claude works on a task likely to break the trap's rule, in a project with Fabric
           recording and Claude's auto memory on (stored inside the run directory).
           If the trap fires, Fabric finds the breaking step, the project is restored, the
           scripted developer correction is sent to the same Claude session, and Fabric records
           a lesson from the evidence.
  phase 2  A different task touching the same rule, in a fresh session, in four setups:
             A  nothing           B  Claude auto memory (from phase 1)
             C  Fabric lessons    D  Fabric lessons + learned guard
  control  An unrelated task in setups A, C and D, to measure harm.

Usage:  python -I run.py [trap ...]     env: RUNS (default 2), FABRIC_BIN, CLAUDE_BIN, OUT
"""

import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
TRAPS_DIR = HERE / "traps"
FABRIC = os.environ.get("FABRIC_BIN", "fabric")
CLAUDE = os.environ.get("CLAUDE_BIN", "claude")
OUT = Path(os.environ.get("OUT", HERE / "runs"))
RUNS = int(os.environ.get("RUNS", "2"))
FLAGS = ("FABRIC_EXPERIMENT_LESSONS", "FABRIC_EXPERIMENT_LEARNED_GUARD", "CLAUDE_CODE_DISABLE_AUTO_MEMORY")


def log(msg):
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def run(cmd, cwd, env=None, timeout=1200):
    full = {k: v for k, v in os.environ.items() if k not in FLAGS}
    full.update(env or {})
    try:
        p = subprocess.run([str(c) for c in cmd], cwd=cwd, env=full, capture_output=True,
                           text=True, encoding="utf-8", errors="replace", timeout=timeout)
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except subprocess.TimeoutExpired:
        return -1, "TIMEOUT"


def remove_tree(path):
    def writable(func, p, _exc):
        os.chmod(p, stat.S_IWRITE)
        func(p)
    if Path(path).exists():
        shutil.rmtree(path, onexc=writable)


def fabric_json(args, cwd, env):
    code, out = run([FABRIC] + args + ["--json"], cwd, env)
    try:
        # Progress lines on stderr can follow the JSON; decode just the JSON value.
        return code, json.JSONDecoder().raw_decode(out[out.index("{"):])[0]
    except ValueError:
        return code, {"raw": out[-500:]}


def claude(prompt, cwd, env, resume=None):
    cmd = [CLAUDE, "-p", prompt, "--permission-mode", "acceptEdits",
           "--allowedTools", "Bash", "--output-format", "json"]
    if resume:
        cmd += ["--resume", resume]
    start = time.time()
    code, out = run(cmd, cwd, env)
    info = {"exit": code, "seconds": round(time.time() - start, 1)}
    try:
        data = json.loads(out[out.index("{"):])
        info.update(session=data.get("session_id"), cost_usd=data.get("total_cost_usd"),
                    turns=data.get("num_turns"), result=(data.get("result") or "")[:600])
    except ValueError:
        info["raw"] = out[-600:]
    return info


def test_files_digest(project):
    h = hashlib.sha256()
    for p in sorted((project / "tests").glob("test_*.py")):
        if not p.name.startswith("test_hidden"):
            h.update(p.name.encode() + p.read_bytes().replace(b"\r\n", b"\n"))
    return h.hexdigest()


def unittest(project, modules):
    code, out = run([sys.executable, "-m", "unittest", "-q"] + modules, project)
    failing = re.findall(r"^(?:FAIL|ERROR): (\S+)", out, re.M)
    return code == 0, failing, out[-1500:]


def hidden(project, trap_dir, filename):
    shutil.copy(trap_dir / "hidden" / filename, project / "tests" / filename)
    try:
        ok, failing, out = unittest(project, ["tests." + filename[:-3]])
    finally:
        (project / "tests" / filename).unlink(missing_ok=True)
    return ok, failing


def prepare(workdir, trap_dir, fabric, memory_dir=None):
    """A fresh copy of the trap project; with Fabric recording if `fabric`."""
    project = workdir / "project"
    shutil.copytree(trap_dir / "repo", project)
    run(["git", "init", "-q"], project)
    run(["git", "config", "user.email", "exp@example.com"], project)
    run(["git", "config", "user.name", "exp"], project)
    (project / ".claude").mkdir()
    settings = ({"autoMemoryEnabled": True, "autoMemoryDirectory": str(memory_dir)}
                if memory_dir else {"autoMemoryEnabled": False})
    (project / ".claude" / "settings.json").write_text(json.dumps(settings, indent=2))
    run(["git", "add", "-A"], project)
    run(["git", "commit", "-q", "-m", "base"], project)
    env = {}
    if fabric:
        env = {"FABRIC_HOME": str(workdir / "fabric-home"),
               "FABRIC_CLAUDE_BIN": str(workdir / "no-claude")}
        fabric_json(["init"], project, env)
        fabric_json(["integrate", "claude-code"], project, env)
        _, cp = fabric_json(["checkpoint", "-m", "experiment setup"], project, env)
        env["_BASE"] = str((cp.get("checkpoint") or {}).get("seq", 1))
    return project, env


def fabric_project_dir(workdir):
    return next((workdir / "fabric-home" / "projects").iterdir())


def setup_env(env, condition):
    e = {k: v for k, v in env.items() if not k.startswith("_")}
    if condition != "B":
        e["CLAUDE_CODE_DISABLE_AUTO_MEMORY"] = "1"
    if condition in ("C", "D"):
        e["FABRIC_EXPERIMENT_LESSONS"] = "1"
    if condition == "D":
        e["FABRIC_EXPERIMENT_LEARNED_GUARD"] = "1"
    return e


def warnings(workdir):
    path = workdir / "fabric-home"
    if not path.exists():
        return 0
    f = fabric_project_dir(workdir) / "learned-guard.jsonl"
    return len(f.read_text().splitlines()) if f.exists() else 0


def evaluate(project, trap_dir, trap, task_file, before_digest):
    repo_ok, repo_failing, _ = unittest(project, [])
    ci_ok, ci_failing = hidden(project, trap_dir, trap["ci_tests"])
    task_ok, task_failing = hidden(project, trap_dir, task_file)
    return {
        "repo_tests_ok": repo_ok,
        "visible_ok": ci_ok,
        "visible_failing": ci_failing + repo_failing,
        "task_ok": task_ok,
        "task_failing": task_failing,
        "tests_modified": test_files_digest(project) != before_digest,
    }


def one_run(trap_name, index, results):
    trap_dir = TRAPS_DIR / trap_name
    trap = json.loads((trap_dir / "trap.json").read_text(encoding="utf-8-sig"))
    base = OUT / trap_name / f"run{index}"
    remove_tree(base)
    base.mkdir(parents=True)
    record = {"trap": trap_name, "run": index}

    # ---- phase 1, in the setup that later becomes B (memory kept in the run directory) ----
    wb = base / "B"
    wb.mkdir()
    memory = base / "claude-memory"
    memory.mkdir()
    project_b, env_b = prepare(wb, trap_dir, fabric=True, memory_dir=memory)
    digest = test_files_digest(project_b)
    log(f"{trap_name} run{index}: phase 1")
    p1 = claude(trap["phase1"], project_b, setup_env(env_b, "B"))
    triggered_ok, failing = hidden(project_b, trap_dir, trap["ci_tests"])
    record["phase1"] = {"claude": p1, "trap_fired": not triggered_ok, "failing": failing}
    if triggered_ok:
        log(f"{trap_name} run{index}: trap did not fire in phase 1; run dropped")
        record["dropped"] = "trap did not fire"
        results.append(record)
        return

    # Bisect with the CI tests, which live outside the project like a real CI suite.
    test_cmd = f'python -I "{HERE / "ci.py"}" "{trap_dir / "hidden" / trap["ci_tests"]}"'
    _, bis = fabric_json(["bisect", "--test", test_cmd, "--good", env_b["_BASE"]], project_b, env_b)
    bad = (bis.get("first_bad") or {}).get("seq")
    fabric_json(["restore", env_b["_BASE"], "--yes"], project_b, env_b)
    record["phase1"]["breaking_step"] = bad
    correction = claude(trap["correction"], project_b, setup_env(env_b, "B"), resume=p1.get("session"))
    record["phase1"]["correction"] = correction
    # Every setup must start phase 2 from the same project, even if Claude edited code while
    # acknowledging the correction.
    fabric_json(["restore", env_b["_BASE"], "--yes"], project_b, env_b)
    lesson_args = ["lesson", "add", "--step", str(bad), "--rule", trap["correction"]]
    for t in failing:
        lesson_args += ["--failing-test", t]
    _, lesson = fabric_json(lesson_args, project_b, env_b)
    record["lesson"] = lesson.get("lesson")
    lessons_file = fabric_project_dir(wb) / "lessons.json"
    record["claude_memory_files"] = [p.name for p in memory.rglob("*.md")]
    # B keeps Claude's memory but not Fabric's hooks in phase 2.
    (project_b / ".claude" / "settings.local.json").unlink(missing_ok=True)

    # ---- phase 2 ----
    record["phase2"] = {}
    for condition in "ABCD":
        log(f"{trap_name} run{index}: phase 2 {condition}")
        if condition == "B":
            project, env, wd = project_b, {}, wb
        else:
            wd = base / condition
            wd.mkdir()
            project, env = prepare(wd, trap_dir, fabric=condition in "CD")
            if condition in "CD" and lessons_file.exists():
                shutil.copy(lessons_file, fabric_project_dir(wd) / "lessons.json")
        before = test_files_digest(project)
        info = claude(trap["phase2"], project, setup_env(env, condition))
        result = evaluate(project, trap_dir, trap, trap["hidden_task"], before)
        trap_tests_failed = not result["visible_ok"] or (
            trap.get("hidden_task_is_trap") and not result["task_ok"])
        result.update(claude=info, repeated_mistake=trap_tests_failed,
                      success=result["visible_ok"] and result["repo_tests_ok"] and result["task_ok"]
                      and not result["tests_modified"],
                      guard_warnings=warnings(wd) if condition == "D" else 0)
        record["phase2"][condition] = result

    # ---- control: unrelated task, measures harm ----
    record["control"] = {}
    for condition in "ACD":
        log(f"{trap_name} run{index}: control {condition}")
        wd = base / f"control-{condition}"
        wd.mkdir()
        project, env = prepare(wd, trap_dir, fabric=condition in "CD")
        if condition in "CD" and lessons_file.exists():
            shutil.copy(lessons_file, fabric_project_dir(wd) / "lessons.json")
        before = test_files_digest(project)
        info = claude(trap["control"], project, setup_env(env, condition))
        result = evaluate(project, trap_dir, trap, trap["hidden_control"], before)
        result.update(claude=info, success=result["visible_ok"] and result["repo_tests_ok"] and result["task_ok"],
                      guard_warnings=warnings(wd) if condition == "D" else 0)
        record["control"][condition] = result
    results.append(record)


def summarize(results):
    kept = [r for r in results if "dropped" not in r]
    print("\n=== Summary ===")
    print(f"runs: {len(results)}  kept: {len(kept)}  dropped (trap did not fire): {len(results) - len(kept)}")
    for c in "ABCD":
        cells = [r["phase2"][c] for r in kept]
        if cells:
            rep = sum(x["repeated_mistake"] for x in cells)
            ok = sum(x["success"] for x in cells)
            print(f"phase 2 {c}: repeated mistake {rep}/{len(cells)}   full success {ok}/{len(cells)}"
                  + (f"   learned-guard warnings {sum(x['guard_warnings'] for x in cells)}" if c == "D" else ""))
    for c in "ACD":
        cells = [r["control"][c] for r in kept]
        if cells:
            ok = sum(x["success"] for x in cells)
            print(f"control {c}: success {ok}/{len(cells)}"
                  + (f"   learned-guard warnings (false alarms) {sum(x['guard_warnings'] for x in cells)}" if c == "D" else ""))


def main():
    traps = sys.argv[1:] or sorted(p.name for p in TRAPS_DIR.iterdir() if p.is_dir())
    OUT.mkdir(parents=True, exist_ok=True)
    results = []
    out_file = OUT / "results.json"
    for trap in traps:
        for i in range(1, RUNS + 1):
            try:
                one_run(trap, i, results)
            except Exception as e:
                log(f"{trap} run{i}: harness error {e!r}")
                results.append({"trap": trap, "run": i, "dropped": f"harness error: {e!r}"})
            out_file.write_text(json.dumps(results, indent=2))
    summarize(results)


if __name__ == "__main__":
    main()
