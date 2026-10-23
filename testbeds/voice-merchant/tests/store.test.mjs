import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { MerchantStore, seconds } from '../store.mjs';

function setup(t) {
  const directory = mkdtempSync(join(tmpdir(), 'cosigna-merchant-unit-'));
  const path = join(directory, 'merchant.sqlite'); const store = new MerchantStore(path);
  t.after(() => { store.close(); rmSync(directory, { recursive: true, force: true }); });
  return { store, path };
}
function purchase(store, key = 'attempt_test', quantity = 1) {
  const offer = store.catalog().find(item => item.id === 'oat-latte');
  return { attempt_key: key, quote_digest: 'a'.repeat(64), scenario: 'success',
    quote: { id: 'quote_test', task_id: 'task_test', offer, quantity, total_minor: offer.unit_total_minor * quantity, issued_at: seconds(), expires_at: seconds()+300 } };
}

test('purchase persists and idempotent retries consume stock only once', t => {
  const { store, path } = setup(t); const input = purchase(store);
  const result = store.submit(input);
  assert.equal(result.status, 'observed'); assert.equal(result.receipt.accepted, true);
  assert.equal(result.receipt.simulated, true);
  assert.deepEqual(store.submit(input), result);
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 11);
  const reopened = new MerchantStore(path);
  try { assert.deepEqual(reopened.lookup(input.attempt_key), result); assert.equal(reopened.orders().length, 1); }
  finally { reopened.close(); }
  input.quote.quantity = 2; input.quote.total_minor = 1400;
  assert.throws(() => store.submit(input), error => error.code === 'idempotency_conflict');
});

test('stock is checked atomically and never becomes negative', t => {
  const { store } = setup(t);
  assert.equal(store.submit(purchase(store, 'first', 10)).status, 'observed');
  assert.equal(store.submit(purchase(store, 'second', 3)).status, 'rejected');
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 2);
  assert.equal(store.orders().length, 1);
});

test('declines and pre-accept uncertainty do not consume stock', t => {
  const { store } = setup(t);
  const declined = purchase(store); declined.scenario = 'decline';
  assert.equal(store.submit(declined).receipt.accepted, false);
  const unknown = purchase(store, 'not_accepted'); unknown.scenario = 'timeout_before_accept';
  assert.equal(store.submit(unknown).status, 'unknown');
  assert.equal(store.lookup('not_accepted').status, 'absent');
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 12);
  assert.equal(store.orders()[0].state, 'declined');
});

test('changed price, currency, quote, quantity and expired requests are rejected', t => {
  const { store } = setup(t);
  const mutations = [input => { input.quote.offer.unit_total_minor = 1; input.quote.total_minor = 1; },
    input => { input.quote.offer.currency = 'INR'; }, input => { input.quote.total_minor = 1; },
    input => { input.quote.quantity = -1; }, input => { input.quote.expires_at = seconds()-1; },
    input => { input.quote.offer.terms = 'different terms'; }, input => { input.quote.offer.fulfillment = 'other pickup'; }];
  for (const [i, mutate] of mutations.entries()) {
    const input = purchase(store, `invalid_${i}`); mutate(input);
    assert.equal(store.submit(input).status, 'rejected');
  }
  assert.equal(store.orders().length, 0);
  assert.equal(store.products().find(product => product.offer.id === 'oat-latte').stock, 12);
  const extra = purchase(store, 'extra'); extra.approved = true;
  assert.throws(() => store.submit(extra), error => error.code === 'invalid_request');
});

test('fulfillment advances independently and never claims payment from a ready flag', t => {
  const { store } = setup(t); const result = store.submit(purchase(store)); const id = result.receipt.order_id;
  assert.throws(() => store.fulfill(id, 'collected'), error => error.code === 'fulfillment_conflict');
  store.fulfill(id, 'ready'); store.fulfill(id, 'ready');
  assert.equal(store.orders()[0].state, 'ready');
  store.fulfill(id, 'collected');
  assert.equal(store.orders()[0].state, 'collected');
  assert.throws(() => store.fulfill(id, 'preparing'), error => error.code === 'fulfillment_conflict');
});
