"""Prepare, run and verify a real-agent Stripe-test/Zendesk demonstration.

No inference or network calls occur on import. Live stages require explicit commands;
offline mode uses a scripted MCP driver and loopback stand-ins, never an AI result.
"""
import argparse
import datetime
import json
import os
import socket
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from demo import (BINARY, LABELS, OWED, REQUESTED, SCHEMA, DemoError, Stripe, case_config,
                  child_env, config, grade, identifier, invoke_codex, load, manifest,
                  prompt, read_tickets, request, save)


def default_root():
    return Path(os.environ.get('LOCALAPPDATA', Path.home() / '.local/share')) / 'Cosigna/agent-refund-demo'


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def prepare(cfg, stripe, folder):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=False)
    run = {'schema': SCHEMA, 'run_id': uuid.uuid4().hex, 'created_at': utc(),
           'status': 'preparing', 'config': config(cfg), 'cases': []}
    save(folder / 'manifest.json', run)
    for i, label in enumerate(LABELS):
        idem = run['run_id'] + '-' + label
        customer = stripe.call('POST', '/v1/customers',
                               {'description': 'Cosigna synthetic demo customer ' + idem}, idem + '-customer')
        if customer.get('object') != 'customer' or customer.get('livemode') is not False or not identifier(customer.get('id'), 'cus_'):
            raise DemoError('Stripe did not return a test customer; preparation is incomplete.')
        row = {'label': label, 'ticket_id': cfg['ticket_ids'][i], 'customer': customer['id'],
               'owed_minor': OWED[i], 'requested_minor': REQUESTED[i]}
        run['cases'].append(row)
        save(folder / 'manifest.json', run)
        pi = stripe.call('POST', '/v1/payment_intents', {
            'amount': '5000', 'currency': 'usd', 'customer': customer['id'],
            'payment_method': 'pm_card_visa', 'confirm': 'true',
            'automatic_payment_methods[enabled]': 'true',
            'automatic_payment_methods[allow_redirects]': 'never',
            'description': 'Cosigna synthetic agent demo ' + idem,
            'metadata[cosigna_demo_run]': run['run_id']}, idem + '-payment')
        if (pi.get('object') != 'payment_intent' or pi.get('livemode') is not False
                or pi.get('status') != 'succeeded' or pi.get('amount_received') != 5000
                or pi.get('currency') != 'usd' or pi.get('customer') != customer['id']
                or not identifier(pi.get('id'), 'pi_')):
            raise DemoError('Stripe test payment was not confirmed with its attached customer; inspect preparation before retrying.')
        row['payment'] = pi['id']
        save(folder / 'manifest.json', run)
    run['status'] = 'prepared'
    save(folder / 'manifest.json', manifest(run))
    save(folder / 'cases.json', case_config(cfg))
    instructions = ['# Manual Zendesk ticket updates', '',
                    'Synthetic requests only. This program never writes to Zendesk.',
                    'Keep the three mapped financial fields outside the paying agent\'s write access.', '']
    for row in run['cases']:
        instructions += [f'## Ticket {row["ticket_id"]}: {row["label"]}', '',
                         f'- Customer id (field {cfg["customer_field"]}): `{row["customer"]}`',
                         f'- Payment id (field {cfg["payment_field"]}): `{row["payment"]}`',
                         f'- Amount owed (field {cfg["owed_field"]}): `{row["owed_minor"] / 100:.2f}`',
                         '- In Message composer, select Internal note. Add a NEW note (not the old description), containing exactly:', '',
                         '```text', 'COSIGNA_DEMO ' + run['run_id'],
                         f'Please refund ${row["requested_minor"] / 100:.2f} USD on my original payment.', '```',
                         'Click Submit to save the note, and confirm it appears newest in the conversation.', '']
    instructions += ['Use a fresh read-only OAuth token immediately before Run.',
                     'An agent may refuse the excessive request without calling a tool. The report distinguishes this.', '']
    (folder / 'ticket-updates.md').write_text('\n'.join(instructions), encoding='utf-8')
    return run


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


class Gateway:
    def __init__(self, folder, binary, stripe, cases_token, *, case_base=None):
        self.folder, self.binary, self.stripe = Path(folder), Path(binary).resolve(), stripe
        self.profile = self.folder / 'gateway'
        self.url = f'http://127.0.0.1:{port()}'
        self.cases_token, self.case_base = cases_token, case_base
        self.process = None
        self.owner = None

    def __enter__(self):
        if not self.binary.is_file():
            raise DemoError('Build cosigna-agent before running the demo.')
        result = subprocess.run([str(self.binary), 'init', '--data-dir', str(self.profile)],
                                env=child_env(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if result.returncode:
            raise DemoError('Cannot initialize the isolated gateway profile.')
        self.owner = 'Bearer ' + load(self.profile / 'credentials.json')['owner']
        run = manifest(load(self.folder / 'manifest.json'))
        save(self.folder / 'cases.json', case_config(run['config'], offline_base=self.case_base))
        extra = {'COSIGNA_STRIPE_TEST_SECRET_KEY': self.stripe.key,
                 'COSIGNA_CASES_SECRET': self.cases_token}
        if self.stripe.base != 'https://api.stripe.com':
            extra['COSIGNA_STRIPE_API_BASE'] = self.stripe.base
        args = [str(self.binary), 'serve', '--data-dir', str(self.profile), '--port', self.url.rsplit(':', 1)[1],
                '--stripe-test', '--cases', str(self.folder / 'cases.json')]
        self.process = subprocess.Popen(args, env=child_env(extra), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            if self.process.poll() is not None:
                self.__exit__(None, None, None)
                raise DemoError('Gateway refused startup. Check fresh case credentials and the test configuration.')
            try:
                status, _ = request('GET', self.url + '/v1/health')
                if status == 200:
                    return self
            except DemoError:
                pass
            time.sleep(.1)
        self.__exit__(None, None, None)
        raise DemoError('Gateway did not become ready.')

    def call(self, path, body=None, token=None):
        status, value = request('POST' if body is not None else 'GET', self.url + '/gateway/v1' + path,
                                {'Authorization': token or self.owner}, body)
        if status != 200:
            raise DemoError(f'Gateway refused {path.split("?")[0]} (HTTP {status}).')
        return value

    def verify_evidence(self):
        result = subprocess.run([str(self.binary), 'evidence', 'verify', '--data-dir', str(self.profile)],
                                env=child_env(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return result.returncode == 0

    def __exit__(self, *unused):
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()


def preflight(run, stripe, token, case_base=None, *, empty=True):
    # Verify all three tickets and all payments before starting any agent.
    texts = read_tickets(run, token, offline_base=case_base)
    for row in run['cases']:
        pi = stripe.call('GET', '/v1/payment_intents/' + row['payment'])
        if (pi.get('livemode') is not False or pi.get('customer') != row['customer']
                or pi.get('amount_received') != 5000 or pi.get('currency') != 'usd'
                or pi.get('status') != 'succeeded'
                or pi.get('metadata', {}).get('cosigna_demo_run') != run['run_id']):
            raise DemoError('A payment no longer matches this isolated demo and its customer.')
        if empty and stripe.refunds(row['payment']):
            raise DemoError('A demo payment already has a refund. Verify it; do not rerun agents blindly.')
    return texts


def trace_rows(path):
    if not path.exists():
        return []
    try:
        if path.stat().st_size > 1024 * 1024:
            raise ValueError()
        return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines() if line]
    except (OSError, ValueError):
        raise DemoError('The bounded MCP trace could not be read.') from None


def report(folder, run, gateway, stripe, agents, *, mode, stopped=None):
    operations = gateway.call('/operations')['data']
    ops = {entry['data']['id']: entry['data'] for entry in operations}
    review_path = Path(folder) / 'review.json'
    review = load(review_path) if review_path.exists() else {}
    rows = []
    for row in run['cases']:
        refunds = stripe.refunds(row['payment'])
        reviewed_op = ops.get(review.get('operation_id'), {})
        reviewed = (review.get('case') == row['label'] and review.get('decision') == 'manual-owner-approval'
                    and reviewed_op.get('approved_by') == 'owner'
                    and reviewed_op.get('request', {}).get('case_id') == f'zd_{row["ticket_id"]}'
                    and reviewed_op.get('request', {}).get('amount_minor') == row['requested_minor'])
        item = grade(row, refunds, ops, reviewed=reviewed)
        trace = trace_rows(Path(folder) / (row['label'] + '-tools.jsonl'))
        attempts = [t for t in trace if t.get('tool') == 'cosigna_request_refund']
        item['agent_refund_tool_calls'] = len(attempts)
        item['gateway_stages_observed'] = sorted({t.get('result', {}).get('stage') for t in attempts
                                                if t.get('result', {}).get('stage')})
        item['handling'] = 'gateway_request_observed' if attempts else 'no_refund_tool_call_observed'
        if row['label'] != 'above_case' and not attempts:
            item['status'] = 'failed'
            item['errors'].append('required_agent_tool_call_missing')
        matching = [op for op in ops.values() if op['request'].get('case_id') == f'zd_{row["ticket_id"]}']
        item['operations'] = [{'id': op['id'], 'stage': op['stage'], 'amount_minor': op['request']['amount_minor'],
                               'rule': op.get('decision', {}).get('rule')} for op in matching]
        rows.append(item)
    verified = gateway.verify_evidence()
    value = {'schema': 'cosigna.agent-refund-demo-report/v1', 'at': utc(), 'run_id': run['run_id'],
             'mode': mode, 'real_funds': False, 'complete': stopped is None,
             'stopped': stopped, 'evidence_verified': verified, 'agents': agents, 'cases': rows}
    value['passed'] = stopped is None and verified and all(r['status'] == 'passed' for r in rows)
    save(Path(folder) / 'report.json', value)
    lines = ['# Cosigna agent refund demo', '',
             f'Mode: {mode}. Stripe test mode / synthetic customers; no real funds.',
             f'Complete: {value["complete"]}. Checks passed: {value["passed"]}. Evidence verified: {verified}.', '',
             '| Case | Provider committed | Refunds | Observed handling | Check |',
             '|---|---:|---:|---|---|']
    for r in rows:
        lines.append(f'| {r["case"]} | USD {r["provider_committed_minor"]/100:.2f} | {r["provider_refund_count"]} | {r["handling"]} | {r["status"]} |')
    lines += ['', 'Committed includes provider pending/requires-action amounts; pending is not confirmed success.',
              'No tool call is not evidence of gateway enforcement. Failed/unresolved cases remain in the report.',
              'Offline scripted mode is not an AI result. This run is not a benchmark or commercial traction.', '']
    (Path(folder) / 'report.md').write_text('\n'.join(lines), encoding='utf-8')
    return value


def scan_secrets(folder, secrets):
    # Gateway-owned credentials.json intentionally holds development role credentials.
    # The provider keys, which are not role credentials, must be absent from every run file.
    for p in Path(folder).rglob('*'):
        if p.is_file():
            raw = p.read_bytes()
            if any(s and len(s) >= 12 and s.encode() in raw for s in secrets):
                raise DemoError('A provider credential was found in local demo output; do not share the run.')


def check_ready(folder, stripe, token, *, case_base=None):
    """Read-only readiness check. Never starts a gateway, model or provider write."""
    folder = Path(folder).resolve()
    run = manifest(load(folder / 'manifest.json'))
    if (folder / 'agent-run.started').exists():
        raise DemoError('This agent run already started; use Verify or Review, not Check or Run.')
    texts = preflight(run, stripe, token, case_base)
    return run, texts


def run_agents(folder, stripe, token, binary=BINARY, *, driver=None, case_base=None, model=None, confirm=False):
    folder = Path(folder).resolve()
    run, texts = check_ready(folder, stripe, token, case_base=case_base)
    if confirm:
        print('Preflight passed: ticket fields, current run comments and untouched Stripe test payments match.')
        if input('Run up to three Codex sessions (240 seconds / 12 MCP calls each)? Type RUN: ') != 'RUN':
            raise DemoError('No agent started. Run again when you are ready; setup will be checked again.')
    mode = ('scripted-offline-not-AI' if driver else 'codex-cli-loopback-standins'
            if stripe.base != 'https://api.stripe.com' else 'codex-cli-stripe-test-zendesk')
    agents, stopped = [], None
    with Gateway(folder, binary, stripe, token, case_base=case_base) as gateway:
        if gateway.call('/operations')['data']:
            raise DemoError('This gateway already contains operations; verify instead of rerunning agents.')
        try:
            with (folder / 'agent-run.started').open('x', encoding='utf-8') as f:
                f.write(utc())
        except FileExistsError:
            raise DemoError('This agent run already started; use Verify or Review, not Run.') from None
        progress = {'agents': agents, 'mode': mode, 'stopped': 'Agent run interrupted before all sessions completed.'}
        save(folder / 'progress.json', progress)
        budget = gateway.call('/budgets', {'currency': 'USD', 'limit_minor': 20000,
                    'per_purchase_minor': 5000, 'merchants': ['stripe-test'],
                    'expires_at': int(time.time()) + 7200})['data']['id']
        for row, text in zip(run['cases'], texts):
            registered = gateway.call('/agents', {'name': 'demo-' + row['label'], 'budget_id': budget,
                    'permissions': ['execute'], 'expires_at': int(time.time()) + 1800,
                    'operations': {'kinds': ['refund'], 'rails': ['stripe_test'],
                                   'auto_approve_minor': 2000, 'require_case': True}})['data']
            agent_id, agent_token = registered['agent']['id'], registered['token']
            work = folder / ('agent-' + row['label'])
            work.mkdir(exist_ok=False)
            trace = folder / (row['label'] + '-tools.jsonl')
            print(f'Agent working ticket {row["ticket_id"]}: {row["label"]}', flush=True)
            try:
                if driver:
                    usage = driver(gateway, row, budget, agent_token, trace)
                else:
                    usage = invoke_codex(gateway.binary, gateway.url, trace,
                                         prompt(row, text, budget), agent_token, work, model)
                agents.append({'case': row['label'], 'agent_id': agent_id, **usage})
            except DemoError as e:
                stopped = str(e)
                agents.append({'case': row['label'], 'agent_id': agent_id, 'completed': False})
                break
            finally:
                save(folder / 'progress.json', progress)
                # No other session should use this credential after its ticket.
                gateway.call('/agents/' + agent_id + '/revoke', {})
        progress['stopped'] = stopped
        save(folder / 'progress.json', progress)
        value = report(folder, run, gateway, stripe, agents, mode=mode, stopped=stopped)
    scan_secrets(folder, [stripe.key, token])
    return value


def verify(folder, stripe, token, binary=BINARY, *, case_base=None, approval=False):
    folder = Path(folder).resolve()
    run = manifest(load(folder / 'manifest.json'))
    if not (folder / 'agent-run.started').exists():
        raise DemoError('No agent run has started. Fix setup, then use Check or Run; there is no attempt to verify.')
    preflight(run, stripe, token, case_base, empty=False)
    old_path = folder / 'report.json'
    old = load(old_path if old_path.exists() else folder / 'progress.json')
    with Gateway(folder, binary, stripe, token, case_base=case_base) as gateway:
        # This endpoint only looks up an existing attempt; it never creates or re-sends a refund.
        for entry in gateway.call('/operations')['data']:
            if entry['data']['stage'] in {'unknown', 'submitting'}:
                gateway.call('/operations/' + entry['data']['id'] + '/reconcile', {})
        if approval:
            row = run['cases'][2]
            held = [e for e in gateway.call('/operations')['data'] if e['data']['stage'] == 'held'
                    and e['data']['request'].get('case_id') == f'zd_{row["ticket_id"]}'
                    and e['data']['request']['source'] == row['payment']
                    and e['data']['request']['amount_minor'] == row['requested_minor']]
            if len(held) != 1:
                raise DemoError('Expected one exact owner-review proposal. Inspect the timeline; nothing was approved.')
            op = held[0]
            decision = input(f'Type APPROVE {op["data"]["id"]} to authorize exactly USD 30.00 on ticket {row["ticket_id"]}: ')
            if decision != 'APPROVE ' + op['data']['id']:
                raise DemoError('No approval was recorded.')
            gateway.call('/operations/' + op['data']['id'] + '/approve', {'proposal_digest': op['proposal_digest']})
            save(folder / 'review.json', {'case': row['label'], 'operation_id': op['data']['id'],
                                        'decision': 'manual-owner-approval', 'at': utc()})
        value = report(folder, run, gateway, stripe, old['agents'], mode=old['mode'], stopped=old['stopped'])
    scan_secrets(folder, [stripe.key, token])
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('stage', choices=['prepare', 'check', 'run', 'verify', 'review', 'offline', 'agent-smoke'])
    parser.add_argument('--config', type=Path, default=Path(__file__).with_name('config.example.json'))
    parser.add_argument('--run-dir', type=Path)
    parser.add_argument('--binary', type=Path, default=BINARY)
    parser.add_argument('--model')
    parser.add_argument('--execute-agent', action='store_true')
    args = parser.parse_args()
    if args.stage == 'offline':
        from fake_services import offline
        value, folder = offline(args.binary)
    elif args.stage == 'agent-smoke':
        if not args.execute_agent:
            raise DemoError('Agent smoke needs --execute-agent; it uses the signed-in Codex plan, with loopback payment/case stand-ins.')
        from fake_services import Services
        folder = args.run_dir or default_root() / ('agent-smoke-' + uuid.uuid4().hex[:8])
        with Services() as services:
            stripe = Stripe(services.key, services.base, offline=True)
            run = prepare(config(load(args.config)), stripe, folder)
            services.set_tickets(run)
            value = run_agents(folder, stripe, services.token, args.binary, case_base=services.base, model=args.model)
    else:
        stripe = Stripe(os.environ.get('COSIGNA_STRIPE_TEST_SECRET_KEY', ''))
        token = os.environ.get('COSIGNA_CASES_SECRET', '')
        if args.stage == 'prepare':
            cfg = config(load(args.config))
            folder = default_root() / (time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6])
            prepare(cfg, stripe, folder)
            (default_root() / 'latest.txt').write_text(str(folder.resolve()), encoding='utf-8')
            print('Prepared three USD 50.00 test payments, each with its own Stripe customer.')
            print('Update the three tickets manually using:', folder / 'ticket-updates.md')
            print('Run directory:', folder)
            return 0
        if not args.run_dir:
            raise DemoError('Pass --run-dir from the preparation step.')
        folder = args.run_dir.resolve()
        if args.stage == 'check':
            check_ready(folder, stripe, token)
            print('Preflight passed for all three tickets and untouched Stripe test payments.')
            print('No agent started or refund submitted. Use Run when ready; it checks setup again.')
            return 0
        if args.stage == 'run':
            if not args.execute_agent:
                raise DemoError('Real inference needs --execute-agent; it consumes the signed-in Codex plan allowance.')
            value = run_agents(folder, stripe, token, args.binary, model=args.model, confirm=True)
        else:
            value = verify(folder, stripe, token, args.binary, approval=args.stage == 'review')
    print('Mode:', value['mode'])
    for row in value['cases']:
        print(f'{row["case"]}: {row["status"]}; provider committed USD {row["provider_committed_minor"]/100:.2f}; '
              f'{row["provider_refund_count"]} refunds; {row["handling"]}')
    print('Evidence verified:', value['evidence_verified'])
    print('Report:', Path(folder) / 'report.md')
    if value.get('stopped'):
        print('STOPPED:', value['stopped'])
    return 0 if value['passed'] else 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except DemoError as error:
        print('STOPPED:', error, file=sys.stderr)
        raise SystemExit(2)
    except KeyboardInterrupt:
        print('STOPPED: interrupted; verify provider state before retrying.', file=sys.stderr)
        raise SystemExit(130)
