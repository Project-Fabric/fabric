"""Runs a CI test file against the project in the current directory.

The CI tests live outside the project (like a real CI suite or downstream service), so agents
cannot read them. Usage, from the project root:  python -I ci.py <path/to/test_ci.py>
Exits 0 when every test passes.
"""

import importlib.util
import os
import sys
import unittest

sys.path.insert(0, os.getcwd())
spec = importlib.util.spec_from_file_location("ci_tests", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
suite = unittest.defaultTestLoader.loadTestsFromModule(module)
result = unittest.TextTestRunner(stream=sys.stderr, verbosity=1).run(suite)
sys.exit(0 if result.wasSuccessful() else 1)
