import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHmac } from 'node:crypto';
import { StripeTestProcessor, verifyStripeEvent } from '../stripe.mjs';
import { StripeMerchantStore } from '../stripe-store.mjs';
import { seconds } from '../store.mjs';
import { fakeStripe } from './fake-stripe.mjs';

function setup(t) {
  const directory = mkdtempSync(join(tmpdir(), 'cosigna-stripe-unit-')); const path = join(directory, 'merchant.sqlite');
  const fake = fakeStripe(); const processor = new StripeTestProcessor('sk_test_synthetic_fixture_key', { transport: fake.transport });
  const store = new StripeMerchantStore(path, processor);
  t.after(() => { store.close(); rmSync(directory, { recursive: true, force: true }); });
  const offer = store.catalog().find(item => item.id === 'oat-latte');
  const input = { attempt_key: 'attempt_stripe_test', quote_digest: 'a'.repeat(64), scenario: 'success', quote: {
    id: 'quote_stripe', task_id: 'task_stripe', offer, quantity: 1, total_minor: 700, issued_at: seconds(), expires_at: seconds()+300 } };
  return { path, fake, processor, store, input };
}

test('Stripe test mode preserves exact approval metadata, confirms once and persists the payment ID', async t => {
  const { store, processor, fake, input } = setup(t); await processor.checkAccess();
  const [one, two] = await Promise.all([store.submit(input), store.submit(input)]);
  assert.equal(one.status, 'observed'); assert(['unknown', 'observed'].includes(two.status));
  assert.equal(one.receipt.accepted, true); assert.equal(one.receipt.simulated, true);
  assert.equal(fake.intents.size, 1);
  assert.equal(fake.requests.filter(request => request.path.endsWith('/confirm')).length, 1);
  const create = fake.requests.find(request => request.path === '/v1/payment_intents' && request.options.method === 'POST');
  assert.equal(create.params.get('amount'), '700'); assert.equal(create.params.get('currency'), 'usd');
  assert.equal(create.params.get('allowed_payment_method_types[]'), 'card');
  assert.equal(create.params.has('payment_method_types[]'), false);
  assert.equal(create.params.get('metadata[cosigna_quote_digest]'), input.quote_digest);
  assert.equal(create.params.get('metadata[cosigna_task]'), input.quote.task_id);
  assert.equal(create.params.get('metadata[cosigna_environment]'), 'stripe-test');
  assert.equal(create.params.has('confirm'), false, 'ID must be persisted before confirmation');
  assert.equal(store.orders()[0].payment.payment_intent_id, one.receipt.accepted ? 'pi_test_1' : '');
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 11);
  assert.deepEqual(await store.submit(input), one);
});

test('declined confirmation is canceled authoritatively before releasing stock and returning a decline', async t => {
  const { store, fake, input } = setup(t); input.scenario = 'decline';
  const result = await store.submit(input);
  assert.equal(result.status, 'observed'); assert.equal(result.receipt.accepted, false);
  assert.equal(store.orders()[0].payment.status, 'canceled'); assert.equal(store.orders()[0].state, 'declined');
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 12);
  assert.equal(fake.requests.filter(request => request.path.endsWith('/cancel')).length, 1);
  await store.lookup(input.attempt_key);
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 12);
});

test('lost confirmation response reconciles after reopen without another create or confirmation', async t => {
  const { store, fake, input, path, processor } = setup(t); fake.loseNextConfirm();
  assert.equal((await store.submit(input)).status, 'unknown'); assert.equal(store.orders().length, 0);
  const reopened = new StripeMerchantStore(path, processor);
  try { assert.equal((await reopened.lookup(input.attempt_key)).receipt.accepted, true); assert.equal(reopened.orders().length, 1); }
  finally { reopened.close(); }
  assert.equal(fake.intents.size, 1); assert.equal(fake.requests.filter(request => request.path.endsWith('/confirm')).length, 1);
});

test('lost creation response does not blindly re-create or confirm; incomplete lookup stays reserved', async t => {
  const { store, fake, input } = setup(t); fake.loseNextCreate();
  assert.equal((await store.submit(input)).status, 'unknown');
  assert.equal((await store.lookup(input.attempt_key)).status, 'unknown');
  assert.equal(fake.intents.size, 1); assert.equal(fake.requests.filter(request => request.path.endsWith('/confirm')).length, 0);
  assert.equal(store.record(input.attempt_key).stock_state, 'held');
});

test('invalid quote or stock cannot create any Stripe object', async t => {
  const { store, fake, input } = setup(t); input.quote.total_minor = 1;
  assert.equal((await store.submit(input)).status, 'rejected'); assert.equal(fake.requests.length, 0);
});

test('live keys, publishable keys, mismatched amounts and live responses fail closed', async t => {
  for (const key of ['sk_live_real', 'rk_live_real', 'pk_test_public', '', 'not-a-key']) assert.throws(() => new StripeTestProcessor(key), error => error.code === 'test_key_required');
  const { store, fake, input } = setup(t); fake.loseNextConfirm(); await store.submit(input);
  const intent = [...fake.intents.values()][0]; intent.livemode = true;
  assert.equal((await store.lookup(input.attempt_key)).status, 'unknown');
  intent.livemode = false; intent.amount = 1;
  assert.equal((await store.lookup(input.attempt_key)).status, 'unknown');
  intent.amount = 700; intent.metadata.cosigna_quote_digest = 'b'.repeat(64);
  assert.equal((await store.lookup(input.attempt_key)).status, 'unknown');
  assert.equal(store.orders().length, 0); assert.equal(store.record(input.attempt_key).stock_state, 'held');
});

test('processing does not count as a paid order or release its reservation', async t => {
  const { store, fake, input } = setup(t); fake.setConfirmationStatus('processing');
  assert.equal((await store.submit(input)).status, 'unknown');
  assert.equal(store.orders().length, 0); assert.equal(store.record(input.attempt_key).stock_state, 'held');
  const intent = [...fake.intents.values()][0]; intent.status = 'succeeded'; intent.amount_received = 700;
  assert.equal((await store.lookup(input.attempt_key)).receipt.accepted, true);
});

test('signed webhooks are fresh, test-only, deduplicated and revalidated against Stripe', async t => {
  const { store, fake, input } = setup(t); fake.loseNextConfirm(); await store.submit(input);
  const intent = [...fake.intents.values()][0];
  const event = { id: 'evt_test_one', livemode: false, type: 'payment_intent.succeeded', data: { object: { id: intent.id } } };
  const raw = Buffer.from(JSON.stringify(event)); const secret = 'whsec_synthetic'; const timestamp = seconds();
  const signature = `t=${timestamp},v1=${createHmac('sha256', secret).update(`${timestamp}.`).update(raw).digest('hex')}`;
  const verified = verifyStripeEvent(raw, signature, secret);
  assert.deepEqual(await store.webhook(verified), { received: true });
  assert.deepEqual(await store.webhook(verified), { duplicate: true });
  assert.equal(store.orders().length, 1);
  assert.throws(() => verifyStripeEvent(Buffer.from('{}'), signature, secret), error => error.code === 'invalid_signature');
  assert.throws(() => verifyStripeEvent(raw, signature, secret, timestamp+301), error => error.code === 'invalid_signature');
  const live = Buffer.from(JSON.stringify({ ...event, livemode: true }));
  const liveSignature = `t=${timestamp},v1=${createHmac('sha256', secret).update(`${timestamp}.`).update(live).digest('hex')}`;
  assert.throws(() => verifyStripeEvent(live, liveSignature, secret), error => error.code === 'invalid_event');
});
