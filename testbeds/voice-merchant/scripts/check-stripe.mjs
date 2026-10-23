// Explicit opt-in integration check against the real Stripe API, test credentials only.
import assert from 'node:assert/strict';
import { mkdtemp, rm, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, basename, extname } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:net';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { launch } from '../dev.mjs';
import { loadStripeTestKey } from '../stripe.mjs';

async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
const groq = process.argv.includes('--groq');
if (groq && !process.env.COSIGNA_GROQ_API_KEY?.trim()) {
  console.error('Set COSIGNA_GROQ_API_KEY in this terminal before running the Groq check.');
  process.exit(1);
}
const directory = await mkdtemp(join(tmpdir(), 'cosigna-stripe-protocol-'));
let lab;
try {
  lab = await launch({ directory, mode: groq ? 'groq' : 'fixtures', paymentMode: 'stripe-test', stripeKey: await loadStripeTestKey(),
    port: await freePort(), agentPort: await freePort(), quiet: true });
  const page = await fetch(lab.url); const cookie = page.headers.get('set-cookie').split(';')[0]; let csrf;
  const call = async (path, body) => {
    const response = await fetch(`${lab.url}${path}`, { method: body === undefined ? 'GET' : 'POST', headers: { Cookie: cookie,
      ...(body === undefined ? {} : { Origin: lab.url, 'X-Cosigna-CSRF': csrf, 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
    return { status: response.status, value: await response.json() };
  };
  csrf = (await call('/api/state')).value.data.csrf;
  let request = 'Order one oat latte';
  if (groq) {
    const audioArg = process.argv.indexOf('--audio');
    let audioPath = audioArg >= 0 ? process.argv[audioArg + 1] : undefined;
    if (audioArg >= 0 && !audioPath) throw new Error('Provide an audio file after --audio.');
    if (!audioPath) {
      if (process.platform !== 'win32') throw new Error('Pass --audio <recording.wav> on this platform.');
      audioPath = join(directory, 'synthetic-order.wav');
      const env = { ...process.env, COSIGNA_SYNTHETIC_SPEECH_FILE: audioPath };
      delete env.COSIGNA_GROQ_API_KEY; delete env.COSIGNA_OPENAI_API_KEY;
      await promisify(execFile)('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
        "$ErrorActionPreference = 'Stop'; Add-Type -AssemblyName System.Speech; $speech = [System.Speech.Synthesis.SpeechSynthesizer]::new(); try { $speech.SetOutputToWaveFile($env:COSIGNA_SYNTHETIC_SPEECH_FILE); $speech.Speak('Order one oat latte') } finally { $speech.Dispose() }"], { env, windowsHide: true });
    }
    const mime = { '.wav': 'audio/wav', '.mp3': 'audio/mpeg', '.m4a': 'audio/x-m4a', '.mp4': 'audio/mp4', '.webm': 'audio/webm' }[extname(audioPath).toLowerCase()];
    if (!mime) throw new Error('Use a WAV, MP3, M4A, MP4 or WebM recording of an order for one oat latte.');
    const audio = await readFile(audioPath);
    if (!audio.length || audio.length > 8 * 1024 * 1024) throw new Error('Use a nonempty recording smaller than 8 MiB.');
    const speech = await fetch(`${lab.url}/api/transcribe`, { method: 'POST', headers: { Cookie: cookie, Origin: lab.url,
      'X-Cosigna-CSRF': csrf, 'Content-Type': mime }, body: audio, signal: AbortSignal.timeout(40_000) });
    const transcript = await speech.json();
    if (!speech.ok) throw new Error(transcript.error?.message ?? 'Groq transcription failed.');
    request = transcript.data.text;
  }
  assert.equal((await call('/api/budget', { limit_minor: 1000, per_purchase_minor: 1000 })).status, 200);
  const planned = await call('/api/plan', { request, idempotency_key: randomUUID() });
  if (planned.status !== 200) throw new Error(planned.value.error?.message ?? 'Hosted planning failed.');
  const plan = planned.value.data;
  assert.equal(plan.data.stage, 'quoted'); assert.equal(lab.store.orders().length, 0);
  assert.equal(plan.data.quote.offer.id, 'oat-latte'); assert.equal(plan.data.quote.quantity, 1);
  assert.equal(plan.data.quote.total_minor, 700, 'The test driver only approves one $7 oat latte.');
  const unapproved = await fetch(`${lab.config.agentUrl}/v1/tasks/${plan.data.id}/execute`, { method: 'POST', headers: {
    Authorization: `Bearer ${lab.config.credentials.executor}`, 'Content-Type': 'application/json' }, body: '{}' });
  assert.equal(unapproved.status, 409);
  assert.equal((await call('/api/approve', { task_id: plan.data.id, quote_digest: '0'.repeat(64) })).status, 409);
  assert.equal(lab.store.payments().length, 0, 'Stripe must not be invoked before valid approval');
  const body = { task_id: plan.data.id, quote_digest: plan.approval_digest };
  let purchase = await call('/api/approve', body); assert.equal(purchase.status, 200);
  for (let i = 0; i < 5 && purchase.value.data.data.stage === 'unknown'; i++) {
    await delay(1000); purchase = await call('/api/reconcile', { task_id: plan.data.id });
  }
  assert.equal(purchase.value.data.data.stage, 'confirmed', 'The sandbox purchase is unresolved. Inspect the saved Stripe profile before rerunning.');
  const order = lab.store.orders()[0]; assert.equal(order.payment.status, 'succeeded');
  const intent = await lab.store.processor.retrieve(order.payment.payment_intent_id);
  assert.equal(intent.livemode, false); assert.equal(intent.status, 'succeeded'); assert.equal(intent.amount_received, 700);
  assert.equal(intent.metadata.cosigna_quote_digest, plan.approval_digest);
  const duplicate = await call('/api/approve', body); assert.equal(duplicate.value.data.data.receipt.order_id, order.id);
  assert.equal(lab.store.orders().length, 1);
  const second = (await call('/api/plan', { request: 'Order one oat latte', idempotency_key: randomUUID() })).value.data;
  const blocked = await call('/api/approve', { task_id: second.data.id, quote_digest: second.approval_digest });
  assert.equal(blocked.value.error.code, 'budget_exceeded'); assert.equal(lab.store.payments().length, 1);
  const list = await lab.store.processor.request('GET', `/payment_intents?limit=100&created%5Bgte%5D=${lab.store.record(order.receipt.attempt_key).prepared_at-60}`);
  const matching = list.data.filter(intent => intent.metadata.cosigna_merchant_instance === lab.store.instance);
  assert.equal(matching.length, 1, 'Duplicate Stripe objects found for the isolated merchant');
  const state = (await call('/api/state')).value.data;
  assert.equal(state.balance.spent_minor, 700); assert.equal(state.balance.reserved_minor, 0); assert.equal(state.balance.available_minor, 300);
  const report = { schema: groq ? 'cosigna.groq-protocol-check/v1' : 'cosigna.stripe-protocol-check/v1', at: new Date().toISOString(), stripe_api_test_verified: true,
    live_money_moved: false, planner: groq ? 'Groq / openai/gpt-oss-120b' : 'fixture-script (not AI)', approval_source: 'test-driver owner role',
    ...(groq ? { transcription: 'Groq / whisper-large-v3-turbo', transcript: request, audio_source: process.argv.includes('--audio') ? 'provided recording' : 'Windows synthetic speech' } : {}),
    order_id: order.id, payment_intent_id: intent.id, charge_id: intent.latest_charge, stripe_status: intent.status,
    amount_minor: intent.amount_received, currency: intent.currency, quote_digest: plan.approval_digest,
    checks: { unapproved_execution_blocked: true, wrong_quote_blocked: true, approved_purchase_succeeded: true,
      approval_replay_reused_order: true, matching_stripe_payment_intents: matching.length, over_budget_blocked_before_stripe: true },
    budget: { limit_minor: 1000, spent_minor: 700, reserved_minor: 0, available_minor: 300 },
    dashboard_url: `https://dashboard.stripe.com/test/payments/${intent.id}` };
  const reportDirectory = new URL('../../../reports/', import.meta.url);
  await mkdir(reportDirectory, { recursive: true });
  const file = new URL(`${groq ? 'groq' : 'stripe'}-protocol-${Date.now()}.json`, reportDirectory);
  await writeFile(file, JSON.stringify(report, null, 2)+'\n', { flag: 'wx' });
  console.log(JSON.stringify(report, null, 2));
  console.log(`Evidence: ${file.pathname}`);
} catch (error) {
  // Preserve unresolved intent/budget state for investigation; never rerun a new charge blindly.
  console.error(`Stripe protocol check stopped: ${error.message}\nDiagnostic profile preserved: ${directory}`);
  process.exitCode = 1;
} finally {
  if (lab) await lab.close();
  if (!process.exitCode) {
    const checked = resolve(directory); const allowedRoot = resolve(tmpdir());
    if (checked.startsWith(allowedRoot + (process.platform === 'win32' ? '\\' : '/')) && basename(checked).startsWith('cosigna-stripe-protocol-')) await rm(checked, { recursive: true, force: true });
  }
}
