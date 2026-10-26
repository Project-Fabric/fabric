"""Loopback stand-ins and scripted MCP client. Never presented as a real-agent run."""
import json
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from demo import DemoError, Stripe, child_env, load


class Services:
    def __init__(self):
        self.key = 'sk_test_synthetic_demo_only'
        self.token = 'synthetic_read_only_case_token'
        self.customers, self.payments, self.refunds, self.tickets = {}, {}, [], {}
        self.idempotency = {}
        self.writes = []
        self.status = 'succeeded'
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                self.handle_api('GET')

            def do_POST(self):
                self.handle_api('POST')

            def handle_api(self, method):
                try:
                    length = int(self.headers.get('Content-Length', 0))
                    form = {k: v[-1] for k, v in parse_qs(self.rfile.read(length).decode()).items()}
                    status, value = outer.handle(method, self.path, self.headers, form)
                    body = json.dumps(value).encode()
                    self.send_response(status)
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Content-Length', str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.base = 'http://127.0.0.1:' + str(self.server.server_port)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *args):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def handle(self, method, path, headers, form):
        parsed = urlsplit(path)
        path = parsed.path
        if path.startswith('/api/v2/tickets/'):
            if method != 'GET' or headers.get('Authorization') != 'Bearer ' + self.token:
                return 401, {}
            if path.endswith('/comments.json'):
                ticket = self.tickets.get(path.replace('/comments.json', '.json'))
                return (200, {'comments': ticket['comments']}) if ticket else (404, {})
            return (200, {'ticket': self.tickets[path]}) if path in self.tickets else (404, {})
        if headers.get('Authorization') != 'Bearer ' + self.key:
            return 401, {}
        if method == 'POST':
            self.writes.append(path)
            idem = headers.get('Idempotency-Key')
            if idem and idem in self.idempotency:
                return 200, self.idempotency[idem]
            if path == '/v1/customers':
                customer = {'object': 'customer', 'id': 'cus_synthetic_' + str(len(self.customers) + 1), 'livemode': False}
                self.customers[customer['id']] = customer
                result = customer
            elif path == '/v1/payment_intents':
                if form.get('customer') not in self.customers:
                    return 400, {}
                payment = {'object': 'payment_intent', 'id': 'pi_synthetic_' + str(len(self.payments) + 1),
                           'livemode': False, 'status': 'succeeded', 'amount_received': int(form['amount']),
                           'currency': form['currency'], 'customer': form['customer'],
                           'metadata': {'cosigna_demo_run': form['metadata[cosigna_demo_run]']}}
                self.payments[payment['id']] = payment
                result = payment
            elif path == '/v1/refunds':
                pi, amount = form.get('payment_intent'), int(form.get('amount', 0))
                if pi not in self.payments or amount <= 0:
                    return 400, {'error': {'type': 'invalid_request_error'}}
                committed = sum(r['amount'] for r in self.refunds if r['payment_intent'] == pi
                                and r['status'] in {'succeeded', 'pending', 'requires_action'})
                if amount + committed > self.payments[pi]['amount_received']:
                    return 400, {'error': {'type': 'invalid_request_error'}}
                result = {'object': 'refund', 'id': 're_synthetic_' + str(len(self.refunds) + 1),
                          'livemode': False, 'status': self.status, 'payment_intent': pi,
                          'amount': amount, 'currency': self.payments[pi]['currency'],
                          'metadata': {k[9:-1]: v for k, v in form.items() if k.startswith('metadata[')}}
                self.refunds.append(result)
            else:
                return 404, {}
            if idem:
                self.idempotency[idem] = result
            return 200, result
        if path.startswith('/v1/payment_intents/'):
            value = self.payments.get(path.rsplit('/', 1)[1])
            return (200, value) if value else (404, {})
        if path == '/v1/refunds':
            payment = parse_qs(parsed.query).get('payment_intent', [None])[0]
            return 200, {'object': 'list', 'has_more': False,
                         'data': [r for r in self.refunds if r['payment_intent'] == payment]}
        if path.startswith('/v1/refunds/'):
            value = next((r for r in self.refunds if r['id'] == path.rsplit('/', 1)[1]), None)
            return (200, value) if value else (404, {})
        return 404, {}

    def set_tickets(self, run):
        cfg = run['config']
        for row in run['cases']:
            self.tickets[f'/api/v2/tickets/{row["ticket_id"]}.json'] = {
                'id': row['ticket_id'],
                'description': f'Please refund ${row["requested_minor"]/100:.2f} USD on my original payment.',
                'comments': [{'body': 'COSIGNA_DEMO ' + run['run_id'] + '\n'
                              f'Please refund ${row["requested_minor"]/100:.2f} USD on my original payment.'}],
                'custom_fields': [{'id': cfg['customer_field'], 'value': row['customer']},
                                  {'id': cfg['payment_field'], 'value': row['payment']},
                                  {'id': cfg['owed_field'], 'value': f'{row["owed_minor"]/100:.2f}'}]}


class Mcp:
    def __init__(self, gateway, token, trace):
        relay = Path(__file__).with_name('mcp_relay.py')
        self.p = subprocess.Popen([sys.executable, '-I', str(relay), str(gateway.binary), gateway.url, str(trace)],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  env=child_env({'COSIGNA_AGENT_TOKEN': token}), encoding='utf-8')
        self.seq = 0
        self.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                'clientInfo': {'name': 'scripted-offline-not-AI', 'version': '1'}})
        self.p.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
        self.p.stdin.flush()

    def rpc(self, method, params):
        self.seq += 1
        self.p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': self.seq, 'method': method, 'params': params}) + '\n')
        self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise DemoError('Offline MCP process stopped unexpectedly.')
            value = json.loads(line)
            if value.get('id') == self.seq:
                return value['result']

    def close(self):
        self.p.stdin.close()
        try:
            self.p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
        self.p.stdout.close()


def scripted_driver(gateway, row, budget, token, trace):
    mcp = Mcp(gateway, token, trace)
    try:
        args = {'budget_id': budget, 'rail': 'stripe_test', 'source': row['payment'],
                'case_id': f'zd_{row["ticket_id"]}', 'amount_minor': row['requested_minor'],
                'currency': 'USD', 'reason': 'Synthetic offline wiring probe',
                'idempotency_key': 'offline-' + row['label']}
        reply = mcp.rpc('tools/call', {'name': 'cosigna_request_refund', 'arguments': args})
        if reply.get('isError'):
            raise DemoError('Offline MCP refund request failed.')
        if row['label'] == 'routine':
            # The transport repeat is deliberately scripted, never described as model behavior.
            mcp.rpc('tools/call', {'name': 'cosigna_request_refund', 'arguments': args})
        return {'runtime': 'scripted-offline-not-AI', 'completed': True}
    finally:
        mcp.close()


def offline(binary, folder=None):
    from run import default_root, prepare, run_agents
    folder = folder or default_root() / ('offline-' + time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6])
    cfg = load(Path(__file__).with_name('config.example.json'))
    with Services() as services:
        stripe = Stripe(services.key, services.base, offline=True)
        run = prepare(cfg, stripe, folder)
        services.set_tickets(run)
        value = run_agents(folder, stripe, services.token, binary, driver=scripted_driver, case_base=services.base)
    return value, folder
