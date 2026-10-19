"""Benchmark: Fabric vs Turnback vs agent-safepoint vs plain git.

Every tool gets the same scripted scenarios on fresh, identical projects, each in its own
directory with isolated storage. Agent steps are simulated by running Python snippets; each tool
records them its own documented way. Results go to results.json and are printed as tables.
"""

import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

BENCH = Path(os.environ.get("BENCH_DIR", Path(__file__).resolve().parent))
RUNS = BENCH / "runs"
FABRIC = Path(os.environ.get("FABRIC_BIN", "fabric"))
TURNBACK = BENCH / "tools" / "npm" / "node_modules" / "turnback" / "dist" / "cli.js"
SAFEPOINT = BENCH / "tools" / "safepoint-target" / "release" / "safepoint.exe"
PY = "python"
TEST_CMD = "python -m unittest -q"

ORIGINAL_APP = "def total(amount, discount=0.0):\n    return round(amount * (1 - discount), 2)\n"
BROKEN_APP = "def total(amount, discount=0.0):\n    return round(amount - discount, 2)\n"
TEST_APP = (
    "import unittest\nfrom src.app import total\n\n\nclass T(unittest.TestCase):\n"
    "    def test_discount(self):\n        self.assertEqual(total(100, 0.1), 90.0)\n"
)


def run(cmd, cwd, env=None, stdin=None, timeout=600):
    full_env = dict(os.environ)
    full_env.update(env or {})
    try:
        p = subprocess.run(cmd, cwd=cwd, env=full_env, input=stdin, capture_output=True,
                           text=True, encoding="utf-8", errors="replace", timeout=timeout)
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except subprocess.TimeoutExpired:
        return -1, "TIMEOUT"


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read(path):
    try:
        return path.read_text(encoding="utf-8")
    except (FileNotFoundError, UnicodeDecodeError):
        return None


def make_project(proj):
    write(proj / ".gitignore", ".env\nbuild/\n")
    write(proj / ".env", "SECRET=original\n")
    write(proj / "src" / "__init__.py", "")
    write(proj / "src" / "app.py", ORIGINAL_APP)
    write(proj / "tests" / "__init__.py", "")
    write(proj / "tests" / "test_app.py", TEST_APP)
    write(proj / "README.md", "# shop\n")
    run(["git", "init", "-q"], proj)
    run(["git", "config", "user.email", "bench@example.com"], proj)
    run(["git", "config", "user.name", "bench"], proj)
    run(["git", "config", "core.autocrlf", "false"], proj)
    run(["git", "add", "-A"], proj)
    run(["git", "commit", "-q", "-m", "base"], proj)


def remove_tree(path):
    """Deletes a directory even when it holds read-only files (Fabric and git objects)."""
    def make_writable(func, p, _exc):
        os.chmod(p, stat.S_IWRITE)
        func(p)
    if Path(path).exists():
        shutil.rmtree(path, onexc=make_writable)


def tests_pass(proj):
    code, _ = run([PY, "-m", "unittest", "-q"], proj)
    return code == 0


def dir_size(path):
    total = 0
    for root, _, files in os.walk(path):
        for f in files:
            try:
                total += os.path.getsize(os.path.join(root, f))
            except OSError:
                pass
    return total


# ----------------------------------------------------------------------------- adapters

class Adapter:
    name = "?"

    def __init__(self, workdir):
        self.dir = workdir
        self.proj = workdir / "project"
        self.data = workdir / "data"
        self.data.mkdir(parents=True, exist_ok=True)
        self.steps = []
        self.log = []

    def env(self):
        return {}

    def cmd(self, args, stdin=None, cwd=None):
        code, out = run(args, cwd or self.proj, self.env(), stdin)
        self.log.append({"cmd": [str(a) for a in args], "code": code, "out": out[-2000:]})
        return code, out

    def exec_step(self, code):
        return self.cmd([PY, "-c", code])

    # Defaults: unsupported.
    def setup(self): pass
    def revert_step(self, i, paths): return "unsupported"
    def bisect(self): return "unsupported"
    def guard(self, command): return "unsupported"
    def recover_last_restore(self): return "unsupported"
    def storage_dirs(self): return []
    def data_dirs(self): return [self.data]


class Fabric(Adapter):
    name = "Fabric"

    def env(self):
        return {"FABRIC_HOME": str(self.data / "fabric-home"),
                "FABRIC_CLAUDE_BIN": str(self.data / "no-claude")}

    def j(self, args):
        code, out = self.cmd([FABRIC] + args + ["--json"])
        try:
            return code, json.loads(out[out.index("{"):])
        except ValueError:
            return code, {}

    def setup(self):
        _, out = self.j(["init"])
        self.seqs = [out.get("checkpoint", {}).get("seq", 1)]

    def step(self, label, code):
        self.exec_step(code)
        _, out = self.j(["checkpoint", "-m", label])
        self.seqs.append((out.get("checkpoint") or {}).get("seq"))
        self.steps.append(label)

    def checkpoint(self, label):
        return self.j(["checkpoint", "-m", label])[0] == 0

    def restore_before(self, i):
        code, out = self.j(["restore", str(self.seqs[i]), "--yes"])
        self.last_undo = out.get("undo_checkpoint")
        return code == 0

    def undo_last(self):
        return self.restore_before(len(self.steps) - 1)

    def revert_step(self, i, paths):
        return "ok" if self.j(["revert", str(self.seqs[i + 1]), "--yes"])[0] == 0 else "failed"

    def bisect(self):
        code, out = self.j(["bisect", "--test", TEST_CMD])
        seq = (out.get("first_bad") or {}).get("seq")
        return self.seqs.index(seq) - 1 if seq in self.seqs else None

    def guard(self, command):
        _, out = self.j(["guard", "check", command])
        return (out.get("verdict") or {}).get("decision", "error")

    def recover_last_restore(self):
        if not getattr(self, "last_undo", None):
            return "nothing_to_recover"
        return "ok" if self.j(["restore", str(self.last_undo), "--yes"])[0] == 0 else "failed"

    def storage_dirs(self):
        return [self.data / "fabric-home" / "store" / "objects"]


class Turnback(Adapter):
    name = "Turnback"

    def env(self):
        return {"TURNBACK_HOME": str(self.data / "tb-home")}

    def tb(self, args):
        return self.cmd(["node", TURNBACK] + args)

    def turn_id(self, label):
        _, out = self.tb(["list", "--json"])
        try:
            turns = json.loads(out[out.index("["):])
        except ValueError:
            return None
        return next((t["id"] for t in turns if t.get("prompt") == label), None)

    def step(self, label, code):
        self.tb(["run", "--label", label, "--", PY, "-c", code])
        self.steps.append(label)

    def checkpoint(self, label):
        return self.tb(["mark", label])[0] == 0

    def restore_before(self, i):
        turn = self.turn_id(self.steps[i])
        if turn is None:  # history unreadable (e.g. damaged storage): nothing restored
            return False
        return self.tb(["restore", turn, "--yes"])[0] == 0

    def undo_last(self):
        return self.tb(["undo", "--yes"])[0] == 0

    def revert_step(self, i, paths):
        # No single-step revert; the closest is restoring that turn's start for chosen paths.
        args = ["restore", self.turn_id(self.steps[i]), "--yes"]
        for p in paths:
            args += ["--path", p]
        return "manual" if self.tb(args)[0] == 0 else "failed"

    def recover_last_restore(self):
        return "ok" if self.tb(["redo", "--yes"])[0] == 0 else "failed"

    def storage_dirs(self):
        return [p for p in (self.data / "tb-home").rglob("objects") if p.is_dir()]


class Safepoint(Adapter):
    name = "agent-safepoint"

    def sp(self, args):
        return self.cmd([SAFEPOINT] + args, stdin="y\n")

    def setup(self):
        self.sp(["checkpoint", "-m", "base"])

    def step(self, label, code):
        self.exec_step(code)
        self.sp(["checkpoint", "-m", label])
        self.steps.append(label)

    def checkpoint(self, label):
        return self.sp(["checkpoint", "-m", label])[0] == 0

    def count(self):
        _, out = self.sp(["timeline"])
        return len(re.findall(r"^@\d+", out, re.M))

    def restore_before(self, i):
        self.before_undo = self.count()
        return self.sp(["undo", f"@{i + 1}"])[0] == 0

    def undo_last(self):
        return self.restore_before(len(self.steps) - 1)

    def recover_last_restore(self):
        after = self.count()
        if after <= getattr(self, "before_undo", after):
            return "unsupported"
        return "ok" if self.sp(["undo", f"@{after}"])[0] == 0 else "failed"

    def storage_dirs(self):
        return [self.proj / ".safepoint" / "objects"]

    def data_dirs(self):
        return [self.proj / ".safepoint"]


class Git(Adapter):
    name = "git (commit every step)"

    def setup(self):
        self.commits = [self.head()]

    def head(self):
        return self.cmd(["git", "rev-parse", "HEAD"])[1].strip()

    def step(self, label, code):
        self.exec_step(code)
        self.checkpoint(label)
        self.commits.append(self.head())
        self.steps.append(label)

    def checkpoint(self, label):
        self.cmd(["git", "add", "-A"])
        return self.cmd(["git", "commit", "-q", "--allow-empty", "-m", label])[0] == 0

    def restore_before(self, i):
        return self.cmd(["git", "reset", "--hard", "-q", self.commits[i]])[0] == 0

    def undo_last(self):
        return self.cmd(["git", "revert", "--no-edit", "HEAD"])[0] == 0

    def revert_step(self, i, paths):
        return "ok" if self.cmd(["git", "revert", "--no-edit", self.commits[i + 1]])[0] == 0 else "failed"

    def bisect(self):
        self.cmd(["git", "bisect", "start", self.commits[-1], self.commits[0]])
        self.cmd(["git", "bisect", "run", PY, "-m", "unittest", "-q"])
        # Newer git versions word the result differently; the ref is stable.
        bad = self.cmd(["git", "rev-parse", "refs/bisect/bad"])[1].strip()
        self.cmd(["git", "bisect", "reset"])
        return self.commits.index(bad) - 1 if bad in self.commits else None

    def recover_last_restore(self):
        return "ok" if self.cmd(["git", "reset", "--hard", "-q", "ORIG_HEAD"])[0] == 0 else "failed"

    def storage_dirs(self):
        return [self.proj / ".git" / "objects"]

    def data_dirs(self):
        return [self.proj / ".git"]


TOOLS = [Fabric, Turnback, Safepoint, Git]

# ----------------------------------------------------------------------------- scenarios

def W(path, text):
    return f"import os;os.makedirs(os.path.dirname({path!r}) or '.',exist_ok=True);open({path!r},'w',newline='\\n').write({text!r})"


def s1_undo_last(t):
    t.step("add feature", W("src/feature.py", "x = 1\n"))
    t.step("write notes", W("notes.md", "notes\n"))
    t.undo_last()
    ok = not (t.proj / "notes.md").exists() and (t.proj / "src/feature.py").exists()
    return ok, "notes.md removed, feature.py kept" if ok else "wrong state after undo"


def s2_shell_delete(t):
    t.step("rm -rf src", "import shutil;shutil.rmtree('src')")
    t.restore_before(0)
    ok = read(t.proj / "src/app.py") == ORIGINAL_APP
    return ok, "src/ restored" if ok else "src/ not restored"


def s3_middle_step(t):
    t.step("add feature", W("src/feature.py", "x = 1\n"))
    t.step("break discount", W("src/app.py", BROKEN_APP))
    t.step("add docs", W("docs.md", "docs\n"))
    status = t.revert_step(1, ["src/app.py"])
    if status == "unsupported":
        return None, "no single-step undo"
    ok = (read(t.proj / "src/app.py") == ORIGINAL_APP and (t.proj / "src/feature.py").exists()
          and (t.proj / "docs.md").exists() and tests_pass(t.proj))
    note = "bad step undone, later work kept" if ok else "later work lost or bad step remains"
    if status == "manual":
        note += " (manual: restore turn start for chosen paths, no conflict check)"
    return ok, note


def s4_unrecorded_work(t):
    t.step("edit app", W("src/app.py", ORIGINAL_APP + "# tweak\n"))
    write(t.proj / ".env", "SECRET=changed-by-user\n")  # ignored file, edited by the person
    write(t.proj / "local.txt", "my own notes\n")       # the person's own new work
    t.restore_before(0)
    env_untouched = read(t.proj / ".env") == "SECRET=changed-by-user\n"
    local_kept = (t.proj / "local.txt").exists()
    recovery = "not needed"
    if not (env_untouched and local_kept):
        r = t.recover_last_restore()
        recovery = "ok" if r == "ok" else r
    env_final = read(t.proj / ".env") == "SECRET=changed-by-user\n"
    local_final = (t.proj / "local.txt").exists()
    env_note = "untouched" if env_untouched else (
        "overwritten by the restore, back after undoing it" if env_final else "overwritten, NOT recoverable")
    local_note = "kept" if local_kept else (
        "deleted, back after undoing the restore" if local_final else f"deleted, NOT recoverable ({recovery})")
    note = f"your edited .env: {env_note}; your new file: {local_note}"
    if not (env_final and local_final):
        return False, note
    # Everything recoverable; a restore that silently overwrites an ignored file you edited
    # yourself is only partial.
    return (True if env_untouched else "partial"), note


def s5_corruption(t):
    t.step("v2", W("src/app.py", "def total(a, d=0.0):\n    return 'v2'\n"))
    corrupted = 0
    for d in t.storage_dirs():
        for root, _, files in os.walk(d):
            for f in files:
                p = os.path.join(root, f)
                try:
                    os.chmod(p, stat.S_IWRITE | stat.S_IREAD)
                    with open(p, "r+b") as fh:
                        fh.seek(0, 2)
                        size = fh.tell()
                        if size == 0:
                            continue
                        fh.seek(size - 1)
                        b = fh.read(1)
                        fh.seek(size - 1)
                        fh.write(bytes([b[0] ^ 0xFF]))
                    corrupted += 1
                except OSError:
                    pass
    if corrupted == 0:
        return None, "could not locate stored content"
    reported_ok = t.restore_before(0)
    content = read(t.proj / "src/app.py")
    if content == ORIGINAL_APP:
        return True, "restored correct content"
    if content is not None and "'v2'" in content and not reported_ok:
        return True, "refused and reported an error; project untouched"
    if content is not None and "'v2'" in content and reported_ok:
        return False, "reported success but did not restore"
    return False, "wrote damaged or missing content"


def s6_tool_data_wiped(t):
    t.step("v2", W("src/app.py", ORIGINAL_APP + "# v2\n"))
    t.step("clean hidden dirs",
           "import os,shutil\nfor p in os.listdir('.'):\n    if p.startswith('.') and os.path.isdir(p) and p!='.git': shutil.rmtree(p)")
    t.restore_before(0)
    ok = read(t.proj / "src/app.py") == ORIGINAL_APP
    return ok, "recovered" if ok else "history lost with the project's hidden folders"


def s7_git_clean(t):
    t.step("new module", W("src/new_module.py", "y = 2\n"))
    if isinstance(t, Git):
        pass  # committed by the step, so git clean will not remove it
    t.step("git clean -fdx", "import subprocess;subprocess.run(['git','clean','-fdx','-q'])")
    t.restore_before(1)
    work = (t.proj / "src/new_module.py").exists()
    env = read(t.proj / ".env") == "SECRET=original\n"
    return work, f"new work file {'recovered' if work else 'LOST'}; .env {'recovered' if env else 'not recovered'}"


def s8_bisect(t):
    t.step("add feature", W("src/feature.py", "x = 1\n"))
    t.step("readme", W("README.md", "# shop v2\n"))
    t.step("break discount", W("src/app.py", BROKEN_APP))
    t.step("docs", W("docs.md", "docs\n"))
    t.step("more", W("src/more.py", "z = 3\n"))
    found = t.bisect()
    if found == "unsupported":
        return None, "no automatic search"
    ok = found == 2
    return ok, "found the breaking step" if ok else f"wrong or no answer ({found})"


def s9_guard(t):
    a = t.guard("rm -rf src")
    if a == "unsupported":
        return None, "no prevention"
    b = t.guard("git clean -fdx")
    ok = a in ("ask", "deny") and b in ("ask", "deny")
    return ok, f"rm -rf: {a}; git clean -fdx: {b}"


SCENARIOS = [
    ("S1", "Undo the last step", s1_undo_last),
    ("S2", "Undo an rm -rf done by a shell command", s2_shell_delete),
    ("S3", "Undo a middle step, keep later steps", s3_middle_step),
    ("S4", "Restore never destroys your unrecorded work", s4_unrecorded_work),
    ("S5", "Damaged storage is detected, not restored", s5_corruption),
    ("S6", "History survives the agent deleting hidden folders", s6_tool_data_wiped),
    ("S7", "Recover from `git clean -fdx`", s7_git_clean),
    ("S8", "Find the step that broke the tests", s8_bisect),
    ("S9", "Stop rm -rf / git clean before they run", s9_guard),
]


def perf(tool_cls, files, size):
    w = RUNS / "perf" / tool_cls.__name__
    remove_tree(w)
    t = tool_cls(w)
    t.proj.mkdir(parents=True)
    make_project(t.proj)
    for i in range(files):
        write(t.proj / f"pkg{i % 10}" / f"mod{i % 50}" / f"f{i}.py", f"# file {i}\n" * (size // 10))
    run(["git", "add", "-A"], t.proj)
    run(["git", "commit", "-q", "-m", "files"], t.proj)
    start = time.perf_counter()
    if isinstance(t, Fabric):
        t.setup()
    else:
        t.setup()
        t.checkpoint("initial")
    first = time.perf_counter() - start
    start = time.perf_counter()
    t.checkpoint("unchanged")
    unchanged = time.perf_counter() - start
    times = []
    for k in range(10):
        write(t.proj / "pkg0" / "mod0" / "f0.py", f"# change {k}\n" * 50)
        s = time.perf_counter()
        t.checkpoint(f"change {k}")
        times.append(time.perf_counter() - s)
    return {
        "first_s": round(first, 2),
        "unchanged_s": round(unchanged, 2),
        "one_file_change_s": round(sorted(times)[len(times) // 2], 2),
        "storage_mb": round(sum(dir_size(d) for d in t.data_dirs()) / 1e6, 1),
    }


def main():
    remove_tree(RUNS)
    results = {"scenarios": {}, "perf": {}, "logs": {}}
    for sid, title, fn in SCENARIOS:
        results["scenarios"][sid] = {"title": title}
        for cls in TOOLS:
            w = RUNS / sid / cls.__name__
            t = cls(w)
            t.proj.mkdir(parents=True)
            make_project(t.proj)
            t.setup()
            try:
                ok, note = fn(t)
            except Exception as e:  # a crash in a tool counts against it, with the reason
                ok, note = False, f"error: {e!r}"
            results["scenarios"][sid][cls.name] = {"pass": ok, "note": note}
            results["logs"][f"{sid}/{cls.name}"] = t.log
            mark = {True: "PASS", False: "FAIL", None: " -- ", "partial": "PART"}[ok]
            print(f"{sid} {cls.name:<24} {mark}  {note}", flush=True)
    files = int(os.environ.get("BENCH_FILES", "3000"))
    for cls in TOOLS:
        results["perf"][cls.name] = perf(cls, files, 4000)
        print(f"perf {cls.name:<24} {results['perf'][cls.name]}", flush=True)
    results["perf_files"] = files
    (BENCH / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
