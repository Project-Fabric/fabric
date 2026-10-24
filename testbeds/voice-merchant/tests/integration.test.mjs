import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:net';
import { request as httpRequest } from 'node:http';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { launch, agentBinary, agentBuildDirectory } from '../dev.mjs';

const root = fileURLToPath(new URL('../../../', import.meta.url));
let lab, directory, cookie, csrf, port, agentPort;
async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
function execute(program, args, env = process.env) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { cwd: root, windowsHide: true, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = ''; child.stdout.on('data', bytes => { output += bytes; }); child.stderr.on('data', bytes => { output += bytes; });
    child.once('error', reject); child.once('exit', code => resolve({ code, output }));
  });
}
async function call(path, body, extra = {}) {
  const response = await fetch(`${lab.url}${path}`, { method: body === undefined ? 'GET' : 'POST',
    headers: { Cookie: cookie, ...(body === undefined ? {} : { Origin: lab.url, 'X-Cosigna-CSRF': csrf, 'Content-Type': 'application/json' }), ...extra },
    body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
async function raw(role, method, path, body) {
  const response = await fetch(`${lab.config.agentUrl}${path}`, { method, headers: { Authorization: `Bearer ${lab.config.credentials[role]}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
before(async () => {
  const built = await execute('cargo', ['build', '-p', 'cosigna-agent', '--target-dir', agentBuildDirectory]); assert.equal(built.code, 0, built.output);
  directory = await mkdtemp(join(tmpdir(), 'cosigna-voice-e2e-')); port = await freePort(); agentPort = await freePort();
  lab = await launch({ directory, port, agentPort, mode: 'fixtures', apiKey: 'synthetic-test-key', build: false, quiet: true,
    transcriber: async (bytes, contentType) => { assert.equal(bytes.toString(), 'synthetic-audio'); assert.equal(contentType, 'audio/webm'); return 'Order one oat latte'; } });
  const page = await fetch(lab.url); cookie = page.headers.get('set-cookie').split(';')[0];
  csrf = (await call('/api/state')).value.data.csrf;
});
after(async () => { if (lab) await lab.close(); if (directory) await rm(directory, { recursive: true, force: true }); });

test('browser session, origin, CSRF and merchant instance checks protect the API boundary', async () => {
  assert.equal((await fetch(`${lab.url}/api/state`)).status, 403);
  assert.equal((await call('/api/budget', { limit_minor: 2000, per_purchase_minor: 1000 }, { 'X-Cosigna-CSRF': 'invalid' })).status, 403);
  assert.equal((await call('/api/state', undefined, { Origin: 'https://untrusted.example' })).status, 403);
  // fetch normalizes Host to the destination. Use raw HTTP to exercise DNS-rebinding protection.
  const wrongHost = await new Promise((resolve, reject) => {
    const request = httpRequest(`${lab.url}/api/state`, { headers: { Host: 'untrusted.example', Cookie: cookie } }, response => { response.resume(); resolve(response.statusCode); });
    request.once('error', reject); request.end();
  });
  assert.equal(wrongHost, 403);
  assert.equal((await fetch(`${lab.url}/merchant/v1/catalog`)).status, 403);
  assert.equal((await fetch(`${lab.url}/merchant/v1/catalog`, { headers: { Authorization: `Bearer ${lab.store.token}`, 'X-Cosigna-Merchant-Instance': 'wrong' } })).status, 409);
  for (const path of ['/', '/app.js', '/styles.css']) {
    const response = await fetch(`${lab.url}${path}`); const source = await response.text(); assert.equal(response.status, 200);
    assert(!source.includes(lab.store.token)); assert(!source.includes(lab.config.credentials.owner)); assert(!source.includes('synthetic-test-key'));
  }
});

test('voice transcript to quote to approved API order to staff collection', async () => {
  assert.equal((await call('/api/budget', { limit_minor: 1000, per_purchase_minor: 1000 })).status, 200);
  const voice = await fetch(`${lab.url}/api/transcribe`, { method: 'POST', headers: { Cookie: cookie, Origin: lab.url, 'X-Cosigna-CSRF': csrf, 'Content-Type': 'audio/webm' }, body: 'synthetic-audio' });
  assert.equal(voice.status, 200); const transcript = (await voice.json()).data.text;
  const plan = await call('/api/plan', { request: transcript, idempotency_key: randomUUID() });
  assert.equal(plan.status, 200); const task = plan.value.data.data; const digest = plan.value.data.approval_digest;
  assert.equal(task.stage, 'quoted'); assert.equal(task.quote.offer.merchant_id, 'cosigna-test-cafe');
  assert.equal(task.quote.total_minor, 700); assert.equal(lab.store.orders().length, 0);
  assert.equal((await call('/api/approve', { task_id: task.id, quote_digest: 'a'.repeat(64) })).status, 409);
  const results = await Promise.all([call('/api/approve', { task_id: task.id, quote_digest: digest }), call('/api/approve', { task_id: task.id, quote_digest: digest })]);
  for (const result of results) assert.equal(result.status, 200);
  const status = await call(`/api/tasks/${task.id}`); assert.equal(status.value.data.data.stage, 'confirmed');
  const orders = lab.store.orders(); assert.equal(orders.length, 1); assert.equal(orders[0].state, 'preparing');
  assert.equal(lab.store.products().find(product => product.offer.id === 'oat-latte').stock, 11);
  assert.equal((await call('/api/fulfill', { order_id: orders[0].id, state: 'ready' })).status, 200);
  assert.equal((await call('/api/fulfill', { order_id: orders[0].id, state: 'collected' })).status, 200);
  assert.equal(lab.store.orders()[0].state, 'collected');
  const balance = (await call('/api/state')).value.data.balance;
  assert.equal(balance.spent_minor, 700); assert.equal(balance.available_minor, 300);
  const over = (await call('/api/plan', { request: transcript, idempotency_key: randomUUID() })).value.data;
  const blocked = await call('/api/approve', { task_id: over.data.id, quote_digest: over.approval_digest });
  assert.equal(blocked.status, 409); assert.equal(blocked.value.error.code, 'budget_exceeded');
  assert.equal(lab.store.orders().length, 1);
});

test('real HTTP response loss retains reservation and lookup reconciles without resubmission', async () => {
  const budget = (await raw('owner', 'POST', '/v1/budgets', { currency: 'USD', limit_minor: 2000, per_purchase_minor: 1000, merchants: ['cosigna-test-cafe'], expires_at: Math.floor(Date.now()/1000)+3600 })).value.data;
  const task = (await raw('planner', 'POST', '/v1/tasks', { budget_id: budget.id, request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  const plan = (await raw('planner', 'POST', `/v1/tasks/${task.id}/plan`, {})).value;
  await raw('owner', 'POST', `/v1/tasks/${task.id}/approve`, { quote_digest: plan.approval_digest });
  const result = await raw('executor', 'POST', `/v1/tasks/${task.id}/execute`, { scenario: 'timeout_after_accept' });
  assert.equal(result.value.data.stage, 'unknown');
  const balance = (await raw('owner', 'GET', `/v1/budgets/${budget.id}`)).value.data; assert.equal(balance.reserved_minor, 700);
  const count = lab.store.orders().length;
  await raw('executor', 'POST', `/v1/tasks/${task.id}/execute`, {});
  assert.equal(lab.store.orders().length, count);
  // An inconsistent merchant receipt must not convert reserved funds to spent/released.
  const key = result.value.data.attempt_key;
  const receipt = lab.store.lookup(key);
  const corrupted = structuredClone(receipt); corrupted.receipt.total_minor = 1;
  lab.store.db.prepare('UPDATE attempts SET result=? WHERE key=?').run(JSON.stringify(corrupted), key);
  assert.equal((await raw('planner', 'POST', `/v1/tasks/${task.id}/reconcile`, {})).value.data.stage, 'unknown');
  lab.store.db.prepare('UPDATE attempts SET result=? WHERE key=?').run(JSON.stringify(receipt), key);
  assert.equal((await raw('planner', 'POST', `/v1/tasks/${task.id}/reconcile`, {})).value.data.stage, 'confirmed');
  assert.equal(lab.store.orders().length, count);
});

test('owner resolve voids a lost attempt at the HTTP merchant and releases the hold exactly once', async () => {
  const budget = (await raw('owner', 'POST', '/v1/budgets', { currency: 'USD', limit_minor: 2000, per_purchase_minor: 1000, merchants: ['cosigna-test-cafe'], expires_at: Math.floor(Date.now()/1000)+3600 })).value.data;
  const task = (await raw('planner', 'POST', '/v1/tasks', { budget_id: budget.id, request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  const plan = (await raw('planner', 'POST', `/v1/tasks/${task.id}/plan`, {})).value;
  await raw('owner', 'POST', `/v1/tasks/${task.id}/approve`, { quote_digest: plan.approval_digest });
  const lost = (await raw('executor', 'POST', `/v1/tasks/${task.id}/execute`, { scenario: 'timeout_before_accept' })).value.data;
  assert.equal(lost.stage, 'unknown');
  assert.equal((await raw('planner', 'POST', `/v1/tasks/${task.id}/reconcile`, {})).value.data.stage, 'unknown');
  for (const role of ['planner', 'executor']) assert.equal((await raw(role, 'POST', `/v1/tasks/${task.id}/resolve`, {})).status, 403);
  const resolved = (await raw('owner', 'POST', `/v1/tasks/${task.id}/resolve`, {})).value.data;
  assert.equal(resolved.stage, 'declined');
  assert.equal((await raw('owner', 'GET', `/v1/budgets/${budget.id}`)).value.data.reserved_minor, 0);
  assert.deepEqual(lab.store.lookup(lost.attempt_key), { status: 'voided' });
  const orders = lab.store.orders().length;
  // The lost request arriving late is refused, and nothing is ordered.
  const late = lab.store.submit({ attempt_key: lost.attempt_key, quote_digest: plan.approval_digest, quote: lost.quote, scenario: 'success' });
  assert.deepEqual(late, { status: 'voided' }); assert.equal(lab.store.orders().length, orders);
});

test('a remote ledger cannot silently switch to fixtures or a replaced merchant', async () => {
  const binary = agentBinary;
  const directoryArg = join(directory, 'agent');
  const fixtures = await execute(binary, ['init', '--data-dir', directoryArg]);
  assert.notEqual(fixtures.code, 0); assert(fixtures.output.includes('configured HTTP test merchant'));
  const instance = lab.store.instance;
  lab.store.set('instance_id', randomUUID());
  try {
    assert.equal((await raw('planner', 'GET', '/v1/catalog')).status, 503);
    const switched = await execute(binary, ['init', '--data-dir', directoryArg, '--test-merchant-url', lab.url], { ...process.env, COSIGNA_TEST_MERCHANT_TOKEN: lab.store.token });
    assert.notEqual(switched.code, 0); assert(switched.output.includes('binding'));
  } finally { lab.store.set('instance_id', instance); }
});
