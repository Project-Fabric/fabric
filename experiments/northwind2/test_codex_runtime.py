import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codex_runtime
import run
from offline_check import FakeStripe, KEY


class RuntimeBoundary(unittest.TestCase):
    def test_credentials_and_unrelated_tools_are_not_inherited(self):
        config = {'mcpServers': {'northwind': {'command': 'python', 'args': ['tools.py', '--mode', 'read'],
                                            'env': {'NORTHWIND_BACKEND_TOKEN': 'scoped-test-token'}}}}
        with patch.dict(os.environ, {'UNEXPECTED_SECRET': 'synthetic-secret', 'PATH': 'bin',
                                    'COSIGNA_STRIPE_TEST_SECRET_KEY': 'sk_test_synthetic'}, clear=True):
            cmd, env, permitted = codex_runtime.command(config, 'work')
        self.assertNotIn('UNEXPECTED_SECRET', env)
        self.assertNotIn('COSIGNA_STRIPE_TEST_SECRET_KEY', env)
        self.assertNotIn('scoped-test-token', ' '.join(cmd))
        self.assertEqual(env['NORTHWIND_BACKEND_TOKEN'], 'scoped-test-token')
        self.assertEqual(permitted['northwind'], {'northwind_refund_history'})
        for setting in ['features.shell_tool=false', 'features.hooks=false', 'features.apps=false',
                        'features.multi_agent=false', 'web_search="disabled"']:
            self.assertIn(setting, cmd)

    def test_completion_cannot_hide_failure_or_unexpected_execution(self):
        complete = {'type': 'turn.completed', 'usage': {'input_tokens': 12}}
        allowed = {'northwind': {'northwind_refund_history'}}
        for event, code in [({'type': 'turn.failed'}, 0), ({'type': 'error'}, 0),
                            ({'type': 'item.completed', 'item': {'type': 'command_execution'}}, 0),
                            ({'type': 'item.completed', 'item': {'type': 'mcp_tool_call', 'server': 'northwind',
                                                               'tool': 'northwind_refund'}}, 0),
                            ({'type': 'thread.started'}, 1)]:
            result = codex_runtime.parse_events('\n'.join(map(json.dumps, [event, complete])), code, allowed)
            self.assertTrue(result['is_error'])
            if result.get('error') == 'unexpected_codex_tool':
                self.assertEqual(result['unexpected_tool'], {'server': 'northwind', 'tool': 'northwind_refund'})
        result = codex_runtime.parse_events(json.dumps(complete), 0, allowed)
        self.assertFalse(result['is_error'])
        self.assertIsNone(result['cost_usd'])

    def test_agent_failure_preserves_incomplete_result_instead_of_success_summary(self):
        fake = FakeStripe()
        try:
            with tempfile.TemporaryDirectory(prefix='northwind-runtime-test-') as work:
                output = Path(work) / 'results'
                args = ['run.py', '--binary', 'unused', '--runs', '1', '--out', str(output), '--rail', 'stripe_test']
                with patch.object(sys, 'argv', args), patch.dict(os.environ, {'COSIGNA_STRIPE_TEST_SECRET_KEY': KEY,
                                            'NORTHWIND_STRIPE_BASE': fake.base}), \
                        patch.object(run, 'run_agent', return_value={'is_error': True, 'error': 'synthetic_failure'}):
                    with self.assertRaisesRegex(RuntimeError, 'experiment incomplete'):
                        run.main()
                self.assertFalse((output / 'summary.json').exists())
                failure = json.loads((output / 'run1-unguarded-incomplete.json').read_text())
                self.assertFalse(failure['complete'])
                self.assertEqual(failure['schema'], 'cosigna.northwind-incomplete/v2')
                self.assertEqual(failure['stripe_refunds'], [])
                self.assertEqual(failure['tickets'][0]['error'], 'synthetic_failure')
        finally:
            fake.server.shutdown()
            fake.server.server_close()


if __name__ == '__main__':
    unittest.main()
