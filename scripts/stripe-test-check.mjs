// Runs the Cosigna gateway against Stripe TEST mode and prints what happened. Run it yourself,
// in your own terminal, with the test key in COSIGNA_STRIPE_TEST_SECRET_KEY:
//   cargo build -p cosigna-agent && node scripts/stripe-test-check.mjs
// Test mode moves no real money. The key is read only from the environment and never printed.
// The gateway listens on loopback; everything is written to a fresh temp directory.
import { spawn, execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

const key = (process.env.COSIGNA_STRIPE_TEST_SECRET_KEY || '').trim();
if (!/^sk_test_[A-Za-z0-9_]+$/.test(key)) {
  console.error('Set COSIGNA_STRIPE_TEST_SECRET_KEY to a Stripe TEST secret key (starts with sk_test_). Live keys are not used.');
  process.exit(2);
}
const binary = process.env.COSIGNA_BIN ?? fileURLToPath(new URL(`../target/debug/cosigna-agent${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'cosigna-stripe-'));
const run = `${Date.now()}`;

// Direct Stripe calls, used only to make test payments and to check Stripe's own records.
const stripe = async (method, path, form) => {
  const res = await fetch(`https://api.stripe.com${path}`, { method,
    headers: { Authorization: `Bearer ${key}`, ...(form ? { 'Content-Type': 'application/x-www-form-urlencoded' } : {}) },
    body: form ? new URLSearchParams(form).toString() : undefined });
  const body = await res.json();
  if (!res.ok) throw new Error(`Stripe ${method} ${path.split('?')[0]}: ${res.status} ${body.error?.type ?? ''} ${body.error?.code ?? ''}`);
  return body;
};
const payment = async amount => (await stripe('POST', '/v1/payment_intents', {
  amount: String(amount), currency: 'usd', payment_method: 'pm_card_visa', confirm: 'true',
  'automatic_payment_methods[enabled]': 'true', 'automatic_payment_methods[allow_redirects]': 'never',
  description: `Cosigna test-mode check ${run}` })).id;
const stripeRefunds = async pi => (await stripe('GET', `/v1/refunds?payment_intent=${pi}&limit=100`)).data;

const free = async () => { const s = net.createServer().listen(0, '127.0.0.1'); await new Promise(r => s.once('listening', r)); const p = s.address().port; await new Promise(r => s.close(r)); return p; };
let gateway, port;
async function start() {
  port = await free();
  gateway = spawn(binary, ['serve', '--data-dir', dir, '--port', String(port), '--stripe-test'], { stdio: 'ignore' });
  for (let i = 0; i < 200; i++) { try { await fetch(`http://127.0.0.1:${port}/v1/health`); return; } catch { await new Promise(r => setTimeout(r, 50)); } }
  throw new Error('gateway did not start');
}
async function stop() { gateway.kill(); await new Promise(r => gateway.once('exit', r)); gateway = null; }
const call = async (auth, method, path, body) => {
  const res = await fetch(`http://127.0.0.1:${port}/gateway/v1${path}`, { method,
    headers: { Authorization: auth, ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
  return { status: res.status, body: await res.json().catch(() => ({})) };
};
const show = (label, r) => {
  const d = r.body.data ?? {};
  console.log(`${label}\n   -> HTTP ${r.status}, stage: ${d.stage ?? '-'}, rule: ${d.decision?.rule ?? '-'}${d.receipt?.rail_reference ? `, Stripe refund: ${d.receipt.rail_reference}` : ''}${r.body.error ? `, error: ${r.body.error.code}` : ''}`);
  return { ...d, proposal_digest: r.body.proposal_digest };
};

try {
  console.log(`Stripe test-mode check, data in ${dir}\n`);
  const pi1 = await payment(5000), pi2 = await payment(5000);
  console.log(`Made two $50.00 test payments with Stripe's test card: ${pi1}, ${pi2}\n`);

  execFileSync(binary, ['init', '--data-dir', dir], { stdio: 'ignore' });
  const owner = 'Bearer ' + JSON.parse(readFileSync(join(dir, 'credentials.json'), 'utf8')).owner;
  await start();
  const now = Math.floor(Date.now() / 1000);
  const budget = (await call(owner, 'POST', '/budgets', { currency: 'USD', limit_minor: 20000, per_purchase_minor: 10000, merchants: ['stripe-test'], expires_at: now + 3600 })).body.data.id;
  const bot = 'Bearer ' + (await call(owner, 'POST', '/agents', { name: 'refunds-bot', budget_id: budget, permissions: ['execute'], expires_at: now + 1800,
    operations: { kinds: ['refund'], rails: ['stripe_test'], auto_approve_minor: 2000 } })).body.data.token;
  const refund = (source, amount, idem) => call(bot, 'POST', '/operations', { budget_id: budget, kind: 'refund', rail: 'stripe_test', source,
    amount_minor: amount, currency: 'USD', reason: 'Cosigna test-mode check', idempotency_key: `${run}-${idem}` });

  const a = show('A  $10.00 refund on payment 1 (agent limit $20): expect refunded automatically in Stripe', await refund(pi1, 1000, 'a'));
  const a2 = show('A2 the agent sends the SAME request again: expect the same operation, no second refund', await refund(pi1, 1000, 'a'));
  console.log(`   same operation: ${a.id === a2.id ? 'yes' : 'NO'}; refunds on payment 1 in Stripe: ${(await stripeRefunds(pi1)).length} (expect 1)`);
  show('B  $45.00 refund on payment 1 (only $40.00 left): expect denied, more than refundable', await refund(pi1, 4500, 'b'));
  const c = show('C  $30.00 refund on payment 2 (over the $20 agent limit): expect held for a person', await refund(pi2, 3000, 'c'));
  console.log(`   refunds on payment 2 in Stripe while held: ${(await stripeRefunds(pi2)).length} (expect 0)`);
  show('C2 the owner approves it: expect refunded in Stripe', await call(owner, 'POST', `/operations/${c.id}/approve`, { proposal_digest: c.proposal_digest }));
  show('D  refund on a payment that does not exist: expect denied', await refund('pi_doesnotexist000000000000', 500, 'd'));

  console.log('\nWhat Stripe itself recorded:');
  for (const [name, pi] of [['payment 1', pi1], ['payment 2', pi2]]) {
    for (const r of await stripeRefunds(pi)) {
      console.log(`   ${name}: ${r.id} $${(r.amount / 100).toFixed(2)} ${r.status}, tagged with a Cosigna operation: ${r.metadata?.cosigna_operation ? 'yes' : 'NO'}`);
    }
  }
  await stop();

  const live = spawnSync(binary, ['serve', '--data-dir', dir, '--port', String(await free()), '--stripe-test'],
    { env: { ...process.env, COSIGNA_STRIPE_TEST_SECRET_KEY: 'sk_live_' + 'x'.repeat(24) }, timeout: 10000, encoding: 'utf8' });
  console.log(`\nE  a LIVE key is refused at startup: ${live.status !== 0 && /test-mode/.test(live.stderr) ? 'yes' : `NO (exit ${live.status})`}`);

  let verified;
  try { execFileSync(binary, ['evidence', 'verify', '--data-dir', dir], { stdio: 'ignore' }); verified = 'ok'; } catch (e) { verified = `FAILED (exit ${e.status})`; }
  console.log(`Evidence verify: ${verified}`);
  const files = [];
  const walk = d => { for (const f of readdirSync(d)) { const p = join(d, f); statSync(p).isDirectory() ? walk(p) : files.push(p); } };
  walk(dir);
  const leaked = files.filter(f => readFileSync(f).includes(Buffer.from(key)));
  console.log(`Key found in data directory files: ${leaked.length ? 'YES in ' + leaked.join(', ') : 'no'} (${files.length} files checked)`);
} finally { if (gateway) gateway.kill(); }
