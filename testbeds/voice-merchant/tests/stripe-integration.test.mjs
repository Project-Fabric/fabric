import { before, after, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:net';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID, createHmac } from 'node:crypto';
import { launch, agentBuildDirectory } from '../dev.mjs';
import { fakeStripe } from './fake-stripe.mjs';

let lab, directory, cookie, csrf; const fake = fakeStripe();
async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
async function call(path, body) {
  const response = await fetch(`${lab.url}${path}`, { method: body === undefined ? 'GET' : 'POST', headers: { Cookie: cookie,
    ...(body === undefined ? {} : { Origin: lab.url, 'X-Cosigna-CSRF': csrf, 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
async function raw(role, method, path, body) {
  const response = await fetch(`${lab.config.agentUrl}${path}`, { method,
    headers: { Authorization: `Bearer ${lab.config.credentials[role]}`, 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
before(async () => {
  await promisify(execFile)('cargo', ['build', '-p', 'cosigna-agent', '--target-dir', agentBuildDirectory], { cwd: new URL('../../../', import.meta.url), windowsHide: true });
  directory = await mkdtemp(join(tmpdir(), 'cosigna-stripe-http-'));
  lab = await launch({ directory, mode: 'fixtures', paymentMode: 'stripe-test', stripeKey: 'sk_test_synthetic_fixture_key', stripeTransport: fake.transport,
    webhookSecret: 'whsec_fixture', port: await freePort(), agentPort: await freePort(), build: false, quiet: true });
  const response = await fetch(lab.url); cookie = response.headers.get('set-cookie').split(';')[0]; csrf = (await call('/api/state')).value.data.csrf;
});
after(async () => { if (lab) await lab.close(); if (directory) await rm(directory, { recursive: true, force: true }); });

test('Rust approval and budget enforcement precede Stripe; payment IDs reach the UI', async () => {
  assert.equal((await call('/api/state')).value.schema, 'cosigna.voice-lab/v3');
  assert.equal((await call('/api/budget', { limit_minor: 1000, per_purchase_minor: 1000 })).status, 200);
  const plan = (await call('/api/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  const task = plan.data;
  assert.equal(fake.intents.size, 0);
  assert.equal((await call('/api/approve', { task_id: task.id, quote_digest: 'b'.repeat(64) })).status, 409);
  assert.equal(fake.intents.size, 0);
  const approved = await call('/api/approve', { task_id: task.id, quote_digest: plan.approval_digest });
  assert.equal(approved.status, 200); assert.equal(approved.value.data.data.stage, 'confirmed');
  const id = [...fake.intents.keys()][0]; assert.equal(fake.intents.get(id).metadata.cosigna_quote_digest, plan.approval_digest);
  const state = (await call('/api/state')).value.data;
  assert.equal(state.payment_mode, 'stripe-test'); assert.equal(state.orders[0].payment.payment_intent_id, id);
  assert.equal(state.orders[0].payment.status, 'succeeded'); assert.equal(state.balance.spent_minor, 700); assert.equal(state.balance.available_minor, 300);
  assert(!JSON.stringify(state).includes('sk_test_synthetic_fixture_key'));
  await call('/api/approve', { task_id: task.id, quote_digest: plan.approval_digest }); assert.equal(fake.intents.size, 1);
  const second = (await call('/api/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  assert.equal((await call('/api/approve', { task_id: second.data.id, quote_digest: second.approval_digest })).value.error.code, 'budget_exceeded');
  assert.equal(fake.intents.size, 1);
});

test('processor decline releases a Rust reservation and lost processor success is reconciled once', async () => {
  const budget = (await raw('owner', 'POST', '/v1/budgets', { currency: 'USD', limit_minor: 2000, per_purchase_minor: 1000, merchants: ['cosigna-test-cafe'], expires_at: Math.floor(Date.now()/1000)+3600 })).value.data;
  for (const scenario of ['decline', 'success']) {
    const task = (await raw('planner', 'POST', '/v1/tasks', { budget_id: budget.id, request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
    const plan = (await raw('planner', 'POST', `/v1/tasks/${task.id}/plan`, {})).value;
    await raw('owner', 'POST', `/v1/tasks/${task.id}/approve`, { quote_digest: plan.approval_digest });
    if (scenario === 'success') fake.loseNextConfirm();
    const result = await raw('executor', 'POST', `/v1/tasks/${task.id}/execute`, { scenario });
    assert.equal(result.value.data.stage, scenario === 'decline' ? 'declined' : 'unknown');
    if (scenario === 'success') {
      const count = fake.intents.size;
      assert.equal((await raw('planner', 'POST', `/v1/tasks/${task.id}/reconcile`, {})).value.data.stage, 'confirmed');
      await raw('executor', 'POST', `/v1/tasks/${task.id}/execute`, {}); assert.equal(fake.intents.size, count);
    }
  }
  const balance = (await raw('owner', 'GET', `/v1/budgets/${budget.id}`)).value.data;
  assert.equal(balance.reserved_minor, 0); assert.equal(balance.spent_minor, 700);
});

test('webhook route rejects bad signatures and persists a valid event once', async () => {
  const intent = [...fake.intents.values()][0];
  const event = { id: 'evt_http_one', livemode: false, type: 'payment_intent.succeeded', data: { object: { id: intent.id } } };
  const raw = JSON.stringify(event); const timestamp = Math.floor(Date.now()/1000);
  const signature = `t=${timestamp},v1=${createHmac('sha256', 'whsec_fixture').update(`${timestamp}.${raw}`).digest('hex')}`;
  assert.equal((await fetch(`${lab.url}/stripe/webhook`, { method: 'POST', headers: { 'Stripe-Signature': 'bad' }, body: raw })).status, 400);
  for (let i = 0; i < 2; i++) assert.equal((await fetch(`${lab.url}/stripe/webhook`, { method: 'POST', headers: { 'Stripe-Signature': signature }, body: raw })).status, 200);
  assert.equal(lab.store.db.prepare('SELECT count(*) AS count FROM stripe_events').get().count, 1);
});
