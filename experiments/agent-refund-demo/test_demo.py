import copy
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from demo import (BINARY, DemoError, Stripe, child_env, codex_command, config, grade, invoke_codex,
                  load, manifest, read_tickets, safe_trace_call, safe_trace_result)
from fake_services import Services, offline
from run import Gateway, prepare, preflight, run_agents, verify


CFG = load(Path(__file__).with_name('config.example.json'))


class Boundaries(unittest.TestCase):
    def test_agent_environment_is_an_allowlist_not_a_known_key_denylist(self):
        values = {'PATH': 'bin', 'COSIGNA_STRIPE_TEST_SECRET_KEY': 'synthetic-payment-secret',
                  'COSIGNA_CASES_SECRET': 'synthetic-case-secret', 'UNEXPECTED_PROVIDER_SECRET': 'synthetic-new-secret',
                  'CONSOLE_INTERNAL_KEY': 'synthetic-owner-key', 'HTTPS_PROXY': 'https://untrusted.example',
                  'OPENAI_API_KEY': 'synthetic-api-key', 'ANTHROPIC_API_KEY': 'synthetic-api-key'}
        with patch.dict(os.environ, values, clear=True):
            env = child_env({'COSIGNA_AGENT_TOKEN': 'synthetic-scoped-token'})
        self.assertEqual(env['COSIGNA_AGENT_TOKEN'], 'synthetic-scoped-token')
        self.assertFalse(set(values) - {'PATH'} & set(env))

    def test_runtime_disables_inherited_tools_and_persists_no_credential_in_arguments(self):
        cmd = codex_command(BINARY, 'http://127.0.0.1:1234', Path('trace'), Path('work'))
        self.assertIn('--ignore-user-config', cmd)
        self.assertIn('--strict-config', cmd)
        self.assertIn('--ephemeral', cmd)
        self.assertIn('read-only', cmd)
        for value in ['features.shell_tool=false', 'features.apps=false', 'features.hooks=false',
                      'web_search="disabled"', 'features.view_image=false']:
            self.assertIn(value, cmd)
        self.assertNotIn('COSIGNA_STRIPE_TEST_SECRET_KEY', ' '.join(cmd))
        self.assertNotIn('COSIGNA_CASES_SECRET', ' '.join(cmd))

    def test_live_keys_redirectable_origins_and_duplicate_tickets_are_refused(self):
        with self.assertRaises(DemoError):
            Stripe('sk_live_' + 'synthetic_example')
        with self.assertRaises(DemoError):
            Stripe('sk_test_synthetic_example', 'https://untrusted.example')
        bad = copy.deepcopy(CFG)
        bad['zendesk_subdomain'] = 'test.zendesk.com@untrusted.example'
        with self.assertRaises(DemoError):
            config(bad)
        bad = copy.deepcopy(CFG)
        bad['ticket_ids'] = [2, 2, 5]
        with self.assertRaises(DemoError):
            config(bad)
        bad = copy.deepcopy(CFG)
        bad['secret'] = 'synthetic-do-not-save'
        with self.assertRaises(DemoError):
            config(bad)

    def test_trace_never_retains_reason_prose_idempotency_text_or_gateway_messages(self):
        secret = 'synthetic-private-ticket-text'
        trace = safe_trace_call({'name': 'cosigna_request_refund', 'arguments': {
            'reason': secret, 'idempotency_key': secret, 'amount_minor': 1000,
            'source': 'pi_synthetic_1', 'case_id': 'zd_2'}})
        result = safe_trace_result({'isError': True, 'content': [{'type': 'text', 'text': json.dumps({
            'error': secret, 'next_action': secret, 'operation_id': 'op_synthetic', 'stage': 'held'})}]})
        self.assertNotIn(secret, json.dumps([trace, result]))
        self.assertEqual(result['stage'], 'held')

    def test_grade_fails_for_an_untagged_provider_refund_and_does_not_trust_success_prose(self):
        row = {'label': 'routine', 'payment': 'pi_synthetic_1', 'ticket_id': 2}
        refund = {'object': 'refund', 'id': 're_synthetic', 'status': 'succeeded',
                  'payment_intent': row['payment'], 'amount': 1000, 'currency': 'usd', 'metadata': {}}
        result = grade(row, [refund], {})
        self.assertEqual(result['status'], 'failed')
        self.assertNotIn('invalid_provider_record', result['errors'])
        self.assertIn('unlinked_provider_refund', result['errors'])
        self.assertIn('gateway_provider_disagreement', result['errors'])
        self.assertEqual(grade(row, [], {})['status'], 'failed')

    def test_successful_stripe_refund_without_livemode_is_accepted(self):
        row = {'label': 'routine', 'payment': 'pi_synthetic_1', 'ticket_id': 2}
        operation_id = 'op_synthetic'
        attempt_key = 'attempt_synthetic'
        refund = {'object': 'refund', 'id': 're_synthetic', 'status': 'succeeded',
                  'payment_intent': row['payment'], 'amount': 1000, 'currency': 'usd',
                  'metadata': {'cosigna_operation': operation_id, 'cosigna_attempt': attempt_key}}
        operation = {'request': {'case_id': 'zd_2', 'source': row['payment'],
                                 'amount_minor': 1000, 'currency': 'USD'},
                     'stage': 'succeeded', 'attempt_key': attempt_key,
                     'receipt': {'rail_reference': 're_synthetic'}}

        result = grade(row, [refund], {operation_id: operation})

        self.assertEqual(result['status'], 'passed')
        self.assertEqual(result['provider_committed_minor'], 1000)
        self.assertEqual(result['errors'], [])

    def test_pending_is_unresolved_and_a_duplicate_or_held_refund_is_a_failure(self):
        row = {'label': 'routine', 'payment': 'pi_synthetic_1', 'ticket_id': 2}
        op = {'request': {'case_id': 'zd_2', 'source': row['payment'], 'amount_minor': 1000, 'currency': 'USD'},
              'stage': 'unknown', 'attempt_key': 'attempt_synthetic'}
        r = {'object': 'refund', 'id': 're_synthetic', 'status': 'pending',
             'payment_intent': row['payment'], 'amount': 1000, 'currency': 'usd',
             'metadata': {'cosigna_operation': 'op_synthetic', 'cosigna_attempt': op['attempt_key']}}
        self.assertEqual(grade(row, [r], {'op_synthetic': op})['status'], 'unresolved')
        self.assertEqual(grade(row, [r, r], {'op_synthetic': op})['status'], 'failed')
        self.assertEqual(grade({**row, 'label': 'owner_review'}, [r], {'op_synthetic': op})['status'], 'failed')

    def test_unexpected_codex_tool_events_fail_and_raw_agent_prose_is_not_retained(self):
        completed = {'type': 'turn.completed', 'usage': {'input_tokens': 10, 'output_tokens': 5}}
        unexpected = {'type': 'item.completed', 'item': {'type': 'command_execution', 'command': 'synthetic-secret'}}
        with patch('demo.subprocess.run', return_value=SimpleNamespace(returncode=0, stdout='\n'.join(map(json.dumps, [unexpected, completed])))):
            with self.assertRaisesRegex(DemoError, 'unexpected agent capability'):
                invoke_codex(BINARY, 'http://127.0.0.1:1', Path('trace'), 'test', 'synthetic-token', Path('.'))
        message = {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'synthetic-private-prose'}}
        with patch('demo.subprocess.run', return_value=SimpleNamespace(returncode=0, stdout='\n'.join(map(json.dumps, [message, completed])))):
            result = invoke_codex(BINARY, 'http://127.0.0.1:1', Path('trace'), 'test', 'synthetic-token', Path('.'))
        self.assertNotIn('synthetic-private-prose', json.dumps(result))
        self.assertEqual(result['input_tokens'], 10)


class LocalServices(unittest.TestCase):
    def test_preparation_attaches_a_distinct_customer_to_each_payment_and_uses_no_zendesk_writes(self):
        with tempfile.TemporaryDirectory() as tmp, Services() as services:
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(CFG, stripe, Path(tmp) / 'run')
            manifest(run)
            services.set_tickets(run)
            first = services.tickets['/api/v2/tickets/2.json']['comments'][0]
            first['plain_body'] = first['body'].replace('\n', '\r\n')
            texts = preflight(run, stripe, services.token, services.base)
            self.assertEqual(len(texts), 3)
            self.assertEqual(len(set(p['customer'] for p in services.payments.values())), 3)
            self.assertEqual(services.writes, ['/v1/customers', '/v1/payment_intents'] * 3)
            raw = '\n'.join(p.read_text() for p in (Path(tmp) / 'run').iterdir())
            self.assertNotIn(services.key, raw)
            self.assertNotIn(services.token, raw)

    def test_stale_fixture_ids_bad_oauth_and_customerless_payments_stop_before_agent(self):
        with tempfile.TemporaryDirectory() as tmp, Services() as services:
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(CFG, stripe, Path(tmp) / 'run')
            services.set_tickets(run)
            with self.assertRaises(DemoError):
                read_tickets(run, 'synthetic-wrong-token', offline_base=services.base)
            ticket = services.tickets['/api/v2/tickets/2.json']
            ticket['custom_fields'][1]['value'] = 'pay_fixture_2041'
            with self.assertRaises(DemoError):
                preflight(run, stripe, services.token, services.base)
            services.set_tickets(run)
            services.payments[run['cases'][0]['payment']]['customer'] = None
            with self.assertRaises(DemoError):
                preflight(run, stripe, services.token, services.base)

    def test_old_ticket_comments_cannot_be_reused_for_a_new_run(self):
        with tempfile.TemporaryDirectory() as tmp, Services() as services:
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(CFG, stripe, Path(tmp) / 'run')
            services.set_tickets(run)
            services.tickets['/api/v2/tickets/2.json']['comments'][0]['body'] = 'Please refund $10.00'
            with self.assertRaisesRegex(DemoError, 'current run comment'):
                read_tickets(run, services.token, offline_base=services.base)

    @unittest.skipUnless(BINARY.is_file(), 'build cosigna-agent for the combined wiring regression')
    def test_real_gateway_mcp_case_and_stripe_adapters_repeat_and_hold(self):
        with tempfile.TemporaryDirectory() as tmp:
            result, folder = offline(BINARY, Path(tmp) / 'run')
            self.assertTrue(result['passed'], result)
            self.assertEqual(result['mode'], 'scripted-offline-not-AI')
            self.assertEqual([r['provider_refund_count'] for r in result['cases']], [1, 0, 0])
            self.assertEqual(result['cases'][0]['agent_refund_tool_calls'], 2)
            self.assertIn('held', result['cases'][1]['gateway_stages_observed'])
            self.assertIn('held', result['cases'][2]['gateway_stages_observed'])
            self.assertTrue((folder / 'agent-run.started').exists())

    @unittest.skipUnless(BINARY.is_file(), 'build cosigna-agent for the owner-review regression')
    def test_owner_review_is_explicit_and_only_the_exact_review_case_is_paid(self):
        from fake_services import scripted_driver
        with tempfile.TemporaryDirectory() as tmp, Services() as services:
            folder = Path(tmp) / 'run'
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(CFG, stripe, folder)
            services.set_tickets(run)
            run_agents(folder, stripe, services.token, BINARY, driver=scripted_driver, case_base=services.base)
            with Gateway(folder, BINARY, stripe, services.token, case_base=services.base) as gateway:
                held = gateway.call('/operations')['data']
                operation = next(e['data']['id'] for e in held
                                 if e['data']['request'].get('case_id') == 'zd_5')
            with patch('builtins.input', return_value='no'):
                with self.assertRaisesRegex(DemoError, 'No approval'):
                    verify(folder, stripe, services.token, BINARY, case_base=services.base, approval=True)
            self.assertEqual(len(services.refunds), 1)
            with patch('builtins.input', return_value='APPROVE ' + operation):
                result = verify(folder, stripe, services.token, BINARY, case_base=services.base, approval=True)
            self.assertTrue(result['passed'], result)
            self.assertEqual([r['provider_committed_minor'] for r in result['cases']], [1000, 0, 3000])
            self.assertEqual(len(services.refunds), 2)
            before = len(services.writes)
            with self.assertRaises(DemoError):
                run_agents(folder, stripe, services.token, BINARY, driver=scripted_driver, case_base=services.base)
            self.assertEqual(len(services.writes), before)

    @unittest.skipUnless(BINARY.is_file(), 'build cosigna-agent for pending-result recovery')
    def test_verify_reconciles_pending_to_success_without_another_provider_write(self):
        from fake_services import scripted_driver
        with tempfile.TemporaryDirectory() as tmp, Services() as services:
            folder = Path(tmp) / 'run'
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(CFG, stripe, folder)
            services.set_tickets(run)
            services.status = 'pending'
            first = run_agents(folder, stripe, services.token, BINARY, driver=scripted_driver, case_base=services.base)
            self.assertFalse(first['passed'])
            self.assertEqual(first['cases'][0]['status'], 'unresolved')
            writes = len(services.writes)
            services.refunds[0]['status'] = 'succeeded'
            final = verify(folder, stripe, services.token, BINARY, case_base=services.base)
            self.assertTrue(final['passed'], final)
            self.assertEqual(len(services.writes), writes)
            self.assertEqual(len(services.refunds), 1)


if __name__ == '__main__':
    unittest.main()
