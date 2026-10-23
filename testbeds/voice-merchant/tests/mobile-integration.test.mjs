import { before, after, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:net';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomUUID } from 'node:crypto';
import { launch, agentBuildDirectory } from '../dev.mjs';
import { fakeStripe } from './fake-stripe.mjs';

let lab, directory, cookie, csrf, token, transcriptions = 0;
const transcriptionProviders = [];
const fake = fakeStripe();
async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
async function desktop(path, body) {
  const response = await fetch(`${lab.url}${path}`, { method: body === undefined ? 'GET' : 'POST', headers: { Cookie: cookie,
    ...(body === undefined ? {} : { Origin: lab.url, 'X-Cosigna-CSRF': csrf, 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
async function mobile(path, body, headers = {}) {
  const response = await fetch(`${lab.url}/mobile/v3${path}`, { method: body === undefined ? 'GET' : 'POST',
    headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...headers },
    body: body === undefined ? undefined : JSON.stringify(body) });
  return { status: response.status, value: await response.json() };
}
before(async () => {
  await promisify(execFile)('cargo', ['build', '-p', 'cosigna-agent', '--target-dir', agentBuildDirectory], { cwd: new URL('../../../', import.meta.url), windowsHide: true });
  directory = await mkdtemp(join(tmpdir(), 'cosigna-mobile-http-'));
  lab = await launch({ directory, mode: 'fixtures', mobile: true, apiKey: 'synthetic-test-key',
    transcriber: async (bytes, _type, options) => { transcriptions++; transcriptionProviders.push(options.provider); assert.equal(bytes.toString(), 'synthetic-audio'); return 'Order one oat latte'; },
    paymentMode: 'stripe-test', stripeKey: 'sk_test_synthetic_mobile', stripeTransport: fake.transport,
    port: await freePort(), agentPort: await freePort(), build: false, quiet: true });
  const response = await fetch(lab.url); cookie = response.headers.get('set-cookie').split(';')[0];
  csrf = (await desktop('/api/state')).value.data.csrf;
});
after(async () => { if (lab) await lab.close(); if (directory) await rm(directory, { recursive: true, force: true }); });

test('native routes require explicit one-use pairing and reject browser origins, cookies and staff actions', async () => {
  assert.equal((await mobile('/state')).status, 401);
  assert.equal((await desktop('/api/device-pairing', {})).status, 200);
  const { code } = (await desktop('/api/device-pairing', {})).value.data;
  assert.match(code, /^\d{6}$/);
  assert.equal((await mobile('/pair', { code }, { Origin: lab.url })).status, 403);
  const paired = await mobile('/pair', { code }); assert.equal(paired.status, 200); token = paired.value.data.token;
  assert.equal((await mobile('/pair', { code })).status, 403);
  for (const headers of [{ Origin: lab.url }, { Cookie: cookie }, { 'Sec-Fetch-Site': 'same-origin' }]) {
    assert.equal((await mobile('/state', undefined, headers)).status, 403);
  }
  assert.equal((await mobile('/fulfill', {})).status, 404);
  assert.equal((await mobile('/device-pairing', {})).status, 404);
  assert.equal((await mobile('/budget', { limit_minor: 1000, per_purchase_minor: 1000 })).status, 200);
  const state = await mobile('/state'); assert.equal(state.value.schema, 'cosigna.mobile-lab/v3');
  assert(!('csrf' in state.value.data)); assert.equal(state.value.data.payment_mode, 'stripe-test');
  for (const secret of [csrf, lab.store.token, lab.config.credentials.owner, 'synthetic-test-key', 'sk_test_synthetic_mobile']) assert(!JSON.stringify(state).includes(secret));
});

test('hosted transcription and planning are separate consent gates and never approve payment', async () => {
  const audio = async consent => fetch(`${lab.url}/mobile/v3/transcribe`, { method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'audio/mp4', ...(consent ? { 'X-Cosigna-Hosted-Consent': consent } : {}) }, body: 'synthetic-audio' });
  assert.equal((await audio()).status, 403); assert.equal(transcriptions, 0);
  assert.equal((await audio('planner-v1')).status, 403); assert.equal(transcriptions, 0);
  const transcript = await audio('voice-v1'); assert.equal(transcript.status, 200);
  assert.equal((await transcript.json()).data.text, 'Order one oat latte'); assert.equal(transcriptions, 1);
  assert.equal(lab.store.tasks().length, 0); assert.equal(fake.intents.size, 0);
  // Gate test only: the underlying Rust process remains the fixture planner.
  lab.config.mode = 'openai';
  try {
    assert.equal((await mobile('/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).status, 403);
    assert.equal(lab.store.tasks().length, 0);
  } finally { lab.config.mode = 'fixtures'; }
  // Provider-bound consent must be renewed; an OpenAI opt-in cannot authorize Groq.
  lab.config.mode = 'groq';
  try {
    const state = (await mobile('/state')).value.data;
    assert.equal(state.ai_provider, 'groq'); assert.match(state.privacy.audio, /Groq/); assert.match(state.privacy.planning, /Groq/);
    assert.equal((await audio('voice-v1')).status, 403);
    assert.equal((await audio('planner-groq-v1')).status, 403);
    assert.equal((await audio('voice-groq-v1')).status, 200);
    assert.deepEqual(transcriptionProviders, ['openai', 'groq']);
    const body = { request: 'Order one oat latte', idempotency_key: randomUUID() };
    assert.equal((await mobile('/plan', body, { 'X-Cosigna-Hosted-Consent': 'planner-v1' })).status, 403);
    assert.equal(lab.store.tasks().length, 0); assert.equal(fake.intents.size, 0);
    assert.equal((await mobile('/plan', body, { 'X-Cosigna-Hosted-Consent': 'planner-groq-v1' })).status, 200);
    assert.equal(fake.intents.size, 0);
  } finally { lab.config.mode = 'fixtures'; }
});

test('phone approval goes through Rust budget controls; replay cannot create another payment; status survives re-pair', async () => {
  const plan = (await mobile('/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  assert.equal(plan.data.stage, 'quoted'); assert.equal(fake.intents.size, 0);
  const body = { task_id: plan.data.id, quote_digest: plan.approval_digest };
  assert.equal((await mobile('/approve', { ...body, quote_digest: 'f'.repeat(64) })).status, 409);
  assert.equal(fake.intents.size, 0);
  const approved = await mobile('/approve', body); assert.equal(approved.value.data.data.stage, 'confirmed');
  await mobile('/approve', body); assert.equal(fake.intents.size, 1);
  const second = (await mobile('/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  assert.equal((await mobile('/approve', { task_id: second.data.id, quote_digest: second.approval_digest })).value.error.code, 'budget_exceeded');
  assert.equal(fake.intents.size, 1);
  const balance = (await mobile('/state')).value.data.balance;
  assert.equal(balance.available_minor, 300); assert.equal(balance.spent_minor, 700);
  await mobile('/revoke-budget', {});
  assert.equal((await mobile('/state')).value.data.balance.budget.revoked, true);
  assert.equal((await mobile('/disconnect', {})).status, 200); assert.equal((await mobile('/state')).status, 401);
  const pairing = (await desktop('/api/device-pairing', {})).value.data;
  token = (await mobile('/pair', { code: pairing.code })).value.data.token;
  assert.equal((await mobile(`/tasks/${plan.data.id}`)).value.data.data.stage, 'confirmed');
  assert.equal(fake.intents.size, 1);
  await desktop('/api/revoke-devices', {});
  assert.equal((await mobile('/state')).status, 401);
});
