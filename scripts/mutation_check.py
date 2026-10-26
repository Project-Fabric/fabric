"""Checks that a test really guards a line of code: break the line, expect the test to fail,
restore the file, expect the test to pass again.

The file is restored by writing its original bytes back (never by moving a backup into place),
so its modification time is new and the build tool always rebuilds; restoring happens even if
the run is interrupted. The original text must occur exactly once. The break must still
compile: a build error also fails the test command, which proves nothing about the test.

Usage:
  python -I scripts/mutation_check.py FILE FIND REPLACE -- TEST COMMAND...
Example:
  python -I scripts/mutation_check.py crates/cosigna-agent/src/approvers.rs \\
    "if approver.revoked {" "if false {" -- cargo test -p cosigna-agent --lib approvers
Exit code 0: the test failed with the break and passed after restoring. 1: it did not.
"""
import subprocess
import sys
from pathlib import Path


def run(command):
    return subprocess.run(command, capture_output=True, text=True).returncode


def main(argv):
    if "--" not in argv or argv.index("--") != 3:
        print(__doc__)
        return 2
    path, find, replace = Path(argv[0]), argv[1], argv[2]
    command = argv[4:]
    original = path.read_bytes()
    text = original.decode("utf-8")
    if text.count(find) != 1:
        print(f"the text to break must occur exactly once in {path} (found {text.count(find)})")
        return 2
    try:
        path.write_text(text.replace(find, replace), encoding="utf-8", newline="")
        broken = run(command)
    finally:
        path.write_bytes(original)  # fresh modification time: the next build sees the restore
    if path.read_bytes() != original:
        print(f"RESTORE FAILED for {path}: check it by hand")
        return 1
    restored = run(command)
    caught = broken != 0
    print(f"with the break: {'FAILED (good)' if caught else 'PASSED (the test does not guard this line)'}")
    print(f"after restoring: {'passed' if restored == 0 else 'FAILED: investigate before trusting this result'}")
    return 0 if caught and restored == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
