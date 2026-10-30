import json
import os
import re
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codex_runtime
import run
from offline_check import FakeStripe, KEY
import stripe_world


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
                self.assertEqual(failure['schema'], 'cosigna.northwind-incomplete/v3')
                self.assertEqual(failure['stripe_refunds'], [])
                self.assertEqual(failure['tickets'][0]['error'], 'synthetic_failure')
        finally:
            fake.server.shutdown()
            fake.server.server_close()

    def test_internal_resource_listing_is_distinct_from_financial_or_resource_read_access(self):
        complete = {'type': 'turn.completed'}
        for tool in ('list_mcp_resources', 'list_mcp_resource_templates'):
            event = {'type': 'item.completed', 'item': {'type': 'mcp_tool_call',
                                                      'server': 'codex', 'tool': tool}}
            result = codex_runtime.parse_events('\n'.join(map(json.dumps, [event, complete])), 0, {})
            self.assertFalse(result['is_error'])
            self.assertEqual(result['observed_mcp_calls'], 0)
            self.assertEqual(result['observed_resource_discovery_calls'], 1)
        for server, tool in [('codex', 'read_mcp_resource'), ('codex', 'northwind_refund'),
                             ('unknown', 'list_mcp_resources')]:
            event = {'type': 'item.completed', 'item': {'type': 'mcp_tool_call',
                                                      'server': server, 'tool': tool}}
            result = codex_runtime.parse_events('\n'.join(map(json.dumps, [event, complete])), 0, {})
            self.assertTrue(result['is_error'])

    def test_lookup_loss_after_committed_refund_retains_ids_and_unknown_state(self):
        fake = FakeStripe()
        reads = 0
        original_measure = stripe_world.refunded_by_order

        def measure_once(stripe, orders):
            nonlocal reads
            reads += 1
            if reads == 2:
                raise urllib.error.URLError('synthetic-private-diagnostic')
            return original_measure(stripe, orders)

        def commit_refund(config_path, allowed, text, cwd, env=None):
            payment = re.search(r'payment (pi_\w+)', text)[1]
            stripe_world.Stripe(KEY, fake.base).call('POST', '/v1/refunds',
                                                  {'payment_intent': payment, 'amount': 2499})
            return {'is_error': False, 'cost_usd': 0}

        try:
            with tempfile.TemporaryDirectory(prefix='northwind-lookup-loss-') as work:
                output = Path(work) / 'results'
                args = ['run.py', '--binary', 'unused', '--runs', '1', '--out', str(output), '--rail', 'stripe_test']
                with patch.object(sys, 'argv', args), patch.dict(os.environ, {
                        'COSIGNA_STRIPE_TEST_SECRET_KEY': KEY, 'NORTHWIND_STRIPE_BASE': fake.base}), \
                        patch.object(stripe_world, 'refunded_by_order', side_effect=measure_once), \
                        patch.object(run, 'run_agent', side_effect=commit_refund) as actor:
                    with self.assertRaises(urllib.error.URLError):
                        run.main()
                raw = (output / 'run1-unguarded-incomplete.json').read_text()
                failure = json.loads(raw)
                self.assertFalse(failure['complete'])
                self.assertTrue(failure['current_payment_state_unknown'])
                self.assertFalse(failure['current_session']['is_error'])
                self.assertEqual(failure['error'], 'payment_record_lookup_failed')
                self.assertEqual(len(failure['payments']), len(run.ORDERS))
                self.assertNotIn('synthetic-private-diagnostic', raw)
                self.assertFalse((output / 'summary.json').exists())
                self.assertEqual(reads, 2)  # No blind provider retry after the failed lookup.
                self.assertEqual(actor.call_count, 1)  # No inference/payment replay.
                self.assertEqual(len(fake.refunds), 1)
                self.assertEqual(next(iter(fake.refunds.values()))['amount'], 2499)
        finally:
            fake.server.shutdown()
            fake.server.server_close()


if __name__ == '__main__':
    unittest.main()
