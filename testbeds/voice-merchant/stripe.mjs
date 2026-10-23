import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { homedir } from 'node:os';
import { createHash, createHmac, timingSafeEqual } from 'node:crypto';
import { LabError } from './store.mjs';

export async function loadStripeTestKey() {
  if (process.env.COSIGNA_STRIPE_TEST_SECRET_KEY) return process.env.COSIGNA_STRIPE_TEST_SECRET_KEY.trim();
  // Explicitly named existing Northwind credential only. No environment dumps or secret scans.
  const data = process.env.LOCALAPPDATA ?? (process.platform === 'darwin' ? join(homedir(), 'Library', 'Application Support') : join(homedir(), '.local', 'share'));
  try { return (await readFile(join(process.env.FABRIC_HOME ?? join(data, 'fabric'), 'secrets', 'stripe-test'), 'utf8')).trim(); }
  catch { throw new LabError(503, 'stripe_unconfigured', 'Set COSIGNA_STRIPE_TEST_SECRET_KEY or configure the existing stripe-test credential locally.'); }
}

export class StripeTestProcessor {
  #key;
  constructor(key, { transport = fetch } = {}) {
    if (typeof key !== 'string' || !/^(sk|rk)_test_[A-Za-z0-9_]+$/.test(key)) {
      throw new LabError(400, 'test_key_required', 'Only Stripe sandbox/test secret keys are accepted. Live and publishable keys are refused.');
    }
    this.#key = key; this.transport = transport;
    this.fingerprint = createHash('sha256').update(key).digest('hex');
  }
  async request(method, path, { params, idempotencyKey } = {}) {
    let response;
    try {
      response = await this.transport(`https://api.stripe.com/v1${path}`, { method,
        headers: { Authorization: `Bearer ${this.#key}`, ...(params ? { 'Content-Type': 'application/x-www-form-urlencoded' } : {}),
          ...(idempotencyKey ? { 'Idempotency-Key': idempotencyKey } : {}) },
        body: params ? new URLSearchParams(params).toString() : undefined,
        signal: AbortSignal.timeout(15_000), redirect: 'error' });
    } catch { throw new LabError(503, 'stripe_uncertain', 'Stripe did not return a reliable response. Keep this purchase reserved and check status.'); }
    let body;
    try {
      const reader = response.body.getReader(); const parts = []; let length = 0;
      for (;;) { const { done, value } = await reader.read(); if (done) break;
        length += value.length; if (length > 512 * 1024) { await reader.cancel(); throw new Error('size'); } parts.push(value); }
      body = JSON.parse(Buffer.concat(parts).toString('utf8'));
    } catch { throw new LabError(502, 'stripe_invalid', 'Stripe returned an invalid response. Keep the purchase reserved.'); }
    if (!response.ok) {
      // A declined confirmation can return its PaymentIntent in an HTTP 402 error.
      // The orchestrator still validates it and retrieves authoritative status by ID.
      if (method === 'POST' && path.endsWith('/confirm') && response.status === 402 && body.error?.payment_intent?.object === 'payment_intent') return body.error.payment_intent;
      throw new LabError(503, 'stripe_unavailable', 'Stripe rejected the request. Check test-key access and reconcile this purchase before retrying.');
    }
    return body;
  }
  async checkAccess() {
    const balance = await this.request('GET', '/balance');
    if (balance.object !== 'balance' || balance.livemode !== false) throw new LabError(400, 'test_mode_required', 'Stripe must confirm a test-mode connection.');
  }
  create(record, instance) {
    const { quote, quote_digest: digest } = record.input;
    return this.request('POST', '/payment_intents', { idempotencyKey: `cosigna:${instance}:${record.key}:create`, params: {
      amount: String(quote.total_minor), currency: quote.offer.currency.toLowerCase(), confirmation_method: 'manual', capture_method: 'automatic',
      'allowed_payment_method_types[]': 'card', description: `Cosigna Test Cafe: ${quote.quantity} x ${quote.offer.title}`,
      'metadata[cosigna_attempt]': record.key, 'metadata[cosigna_quote_digest]': digest,
      'metadata[cosigna_order]': record.order_id, 'metadata[cosigna_task]': quote.task_id,
      'metadata[cosigna_merchant_instance]': instance, 'metadata[cosigna_environment]': 'stripe-test',
    } });
  }
  confirm(record, instance) {
    return this.request('POST', `/payment_intents/${record.payment_intent_id}/confirm`, {
      idempotencyKey: `cosigna:${instance}:${record.key}:confirm`,
      params: { payment_method: record.input.scenario === 'decline' ? 'pm_card_visa_chargeDeclined' : 'pm_card_visa', error_on_requires_action: 'true' },
    });
  }
  retrieve(id) {
    if (!/^pi_[A-Za-z0-9_]+$/.test(id)) throw new LabError(400, 'invalid_payment', 'Invalid Stripe payment ID.');
    return this.request('GET', `/payment_intents/${id}`);
  }
  cancel(record, instance) {
    return this.request('POST', `/payment_intents/${record.payment_intent_id}/cancel`, {
      idempotencyKey: `cosigna:${instance}:${record.key}:cancel`, params: { cancellation_reason: 'abandoned' },
    });
  }
  async find(record, instance) {
    // Search is eventually consistent and unavailable in some markets. Bounded list lookup
    // is recovery only: an absent result never proves no payment exists.
    let after;
    for (let page = 0; page < 8; page++) {
      const query = new URLSearchParams({ limit: '25', 'created[gte]': String(record.prepared_at - 60) });
      if (after) query.set('starting_after', after);
      const list = await this.request('GET', `/payment_intents?${query}`);
      if (list.object !== 'list' || !Array.isArray(list.data)) return null;
      const matches = list.data.filter(intent => intent.metadata?.cosigna_attempt === record.key && intent.metadata?.cosigna_merchant_instance === instance);
      if (matches.length > 1) return null;
      if (matches.length === 1) return this.retrieve(matches[0].id);
      if (!list.has_more || !list.data.length) return null;
      after = list.data.at(-1).id;
    }
    return null;
  }
}

export function verifyStripeEvent(raw, signature, secret, at = Math.floor(Date.now()/1000)) {
  if (!secret?.startsWith('whsec_') || typeof signature !== 'string' || signature.length > 2048) throw new LabError(400, 'invalid_signature', 'A valid Stripe webhook signature is required.');
  const fields = signature.split(',').map(value => value.split('='));
  const timestamps = fields.filter(([key]) => key === 't');
  const timestamp = timestamps.length === 1 ? timestamps[0][1] : '';
  if (!/^\d+$/.test(timestamp) || Math.abs(at - Number(timestamp)) > 300) throw new LabError(400, 'invalid_signature', 'Stripe webhook timestamp is invalid or expired.');
  const expected = createHmac('sha256', secret).update(`${timestamp}.`).update(raw).digest();
  const valid = fields.filter(([key]) => key === 'v1').some(([, digest]) => /^[a-f0-9]{64}$/.test(digest ?? '') && timingSafeEqual(expected, Buffer.from(digest, 'hex')));
  if (!valid) throw new LabError(400, 'invalid_signature', 'Stripe webhook signature does not match.');
  let event;
  try { event = JSON.parse(raw.toString('utf8')); } catch { throw new LabError(400, 'invalid_event', 'Invalid webhook JSON.'); }
  if (typeof event.id !== 'string' || !/^evt_[A-Za-z0-9_]+$/.test(event.id) || event.livemode !== false) throw new LabError(400, 'invalid_event', 'Only test-mode Stripe events are accepted.');
  return event;
}
