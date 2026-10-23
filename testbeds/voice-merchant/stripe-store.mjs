import { createHash, randomUUID } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { MerchantStore, LabError, canonical, object, seconds } from './store.mjs';

const unknown = () => ({ status: 'unknown' });
const validInteger = (value, minimum, maximum) => Number.isSafeInteger(value) && value >= minimum && value <= maximum;

export class StripeMerchantStore extends MerchantStore {
  constructor(path, processor) {
    super(path, { paymentMode: 'stripe-test' });
    this.processor = processor; this.inflight = new Map();
    this.transaction(() => {
      const saved = this.get('stripe_key_fingerprint');
      if (saved && saved !== processor.fingerprint) throw new LabError(409, 'stripe_binding_changed', 'This profile is bound to a different Stripe test credential. Use the original key or a separate profile.');
      this.set('stripe_key_fingerprint', processor.fingerprint);
    });
  }
  record(key) {
    if (typeof key !== 'string') return null;
    const row = this.db.prepare('SELECT * FROM stripe_attempts WHERE key=?').get(key);
    return row ? { ...row, input: JSON.parse(row.request) } : null;
  }
  prepare(input, at) {
    object(input, ['attempt_key', 'quote_digest', 'quote', 'scenario']);
    const { attempt_key: key, quote_digest: digest, quote, scenario } = input;
    if (typeof key !== 'string' || !/^[A-Za-z0-9_]{1,128}$/.test(key)
        || typeof digest !== 'string' || !/^[a-f0-9]{64}$/.test(digest)
        || !['success', 'decline', 'timeout_after_accept', 'timeout_before_accept'].includes(scenario)) {
      throw new LabError(400, 'invalid_request', 'Invalid Stripe test attempt.');
    }
    object(quote, ['id', 'task_id', 'offer', 'quantity', 'total_minor', 'issued_at', 'expires_at']);
    const fingerprint = createHash('sha256').update(canonical({ quote, digest })).digest('hex');
    return this.transaction(() => {
      const existing = this.db.prepare('SELECT fingerprint,result FROM attempts WHERE key=?').get(key);
      if (existing) {
        if (existing.fingerprint !== fingerprint) throw new LabError(409, 'idempotency_conflict', 'This attempt key belongs to a different purchase.');
        return { fresh: false, result: JSON.parse(existing.result), record: this.record(key) };
      }
      const product = typeof quote.offer?.id === 'string' ? this.db.prepare('SELECT offer,stock FROM products WHERE id=?').get(quote.offer.id) : null;
      if (!product || !isDeepStrictEqual(JSON.parse(product.offer), quote.offer)
          || !validInteger(quote.quantity, 1, 10) || !validInteger(quote.total_minor, 1, 100_000)
          || quote.total_minor !== quote.offer.unit_total_minor * quote.quantity
          || !validInteger(quote.issued_at, 0, at) || !validInteger(quote.expires_at, at + 1, at + 3600)
          || typeof quote.id !== 'string' || quote.id.length > 150 || typeof quote.task_id !== 'string' || quote.task_id.length > 150
          || product.stock < quote.quantity) {
        const rejected = { status: 'rejected' };
        this.db.prepare('INSERT INTO attempts VALUES (?,?,?)').run(key, fingerprint, JSON.stringify(rejected));
        return { fresh: false, result: rejected, record: null };
      }
      // Durable intent and local stock reservation precede every Stripe mutation.
      this.db.prepare('INSERT INTO attempts VALUES (?,?,?)').run(key, fingerprint, JSON.stringify(unknown()));
      this.db.prepare("INSERT INTO stripe_attempts(key,request,order_id,prepared_at,stock_state,payment_status) VALUES (?,?,?,?,'held','prepared')")
        .run(key, JSON.stringify(input), `stripe_test_order_${randomUUID()}`, at);
      this.db.prepare('UPDATE products SET stock=stock-? WHERE id=?').run(quote.quantity, quote.offer.id);
      return { fresh: true, result: unknown(), record: this.record(key) };
    });
  }
  validateIntent(record, intent) {
    const { quote, quote_digest: digest } = record.input; const metadata = intent?.metadata;
    if (intent?.object !== 'payment_intent' || !/^pi_[A-Za-z0-9_]+$/.test(intent.id ?? '') || intent.livemode !== false
        || (record.payment_intent_id && record.payment_intent_id !== intent.id)
        || intent.amount !== quote.total_minor || intent.currency !== quote.offer.currency.toLowerCase()
        || intent.capture_method !== 'automatic' || intent.confirmation_method !== 'manual'
        || metadata?.cosigna_attempt !== record.key || metadata.cosigna_quote_digest !== digest
        || metadata.cosigna_order !== record.order_id || metadata.cosigna_task !== quote.task_id
        || metadata.cosigna_merchant_instance !== this.instance || metadata.cosigna_environment !== 'stripe-test') {
      throw new LabError(502, 'stripe_mismatch', 'Stripe payment does not match the approved test purchase. Keep funds reserved.');
    }
  }
  bind(record, intent) {
    this.validateIntent(record, intent);
    this.db.prepare("UPDATE stripe_attempts SET payment_intent_id=?,payment_status=CASE WHEN stock_state='held' THEN ? ELSE payment_status END WHERE key=? AND (payment_intent_id IS NULL OR payment_intent_id=?)")
      .run(intent.id, intent.status, record.key, intent.id);
    return this.record(record.key);
  }
  async submit(input, at = seconds()) {
    // A replay uses the persisted request. It cannot select a new payment source or re-confirm.
    const prepared = this.prepare(input, at);
    if (!prepared.fresh) return prepared.result.status === 'unknown' ? this.lookup(input.attempt_key) : prepared.result;
    if (input.scenario === 'timeout_before_accept') return unknown();
    const promise = this.perform(prepared.record).finally(() => this.inflight.delete(input.attempt_key));
    this.inflight.set(input.attempt_key, promise);
    return promise;
  }
  async perform(record) {
    try {
      record = this.bind(record, await this.processor.create(record, this.instance));
      if (record.input.quote.expires_at <= seconds()) return this.resolve(record, await this.processor.cancel(record, this.instance));
      const claimed = this.db.prepare("UPDATE stripe_attempts SET confirm_started=? WHERE key=? AND confirm_started IS NULL AND stock_state='held'").run(seconds(), record.key).changes;
      if (!claimed) return this.lookup(record.key);
      record = this.record(record.key);
      await this.processor.confirm(record, this.instance);
      // Confirm responses alone do not settle the ledger. Retrieve and validate authoritative status.
      return await this.resolve(record, await this.processor.retrieve(record.payment_intent_id), true);
    } catch { return unknown(); }
  }
  async resolve(record, intent, allowCancel = false) {
    this.validateIntent(record, intent);
    record = this.bind(record, intent);
    if (record.stock_state !== 'held') return super.lookup(record.key);
    if (intent.status === 'succeeded') {
      if (intent.amount_received !== record.input.quote.total_minor) return unknown();
      return this.finish(record, intent, true);
    }
    if (intent.status === 'canceled' && intent.amount_received === 0) return this.finish(record, intent, false);
    // No interactive authentication in this fixed test-card slice. Cancel before releasing.
    if (record.confirm_started && (allowCancel || seconds() - record.confirm_started > 30)
        && ['requires_action', 'requires_payment_method', 'requires_confirmation'].includes(intent.status)) {
      const canceled = await this.processor.cancel(record, this.instance);
      this.validateIntent(record, canceled);
      if (canceled.status === 'canceled' && canceled.amount_received === 0) return this.finish(record, canceled, false);
    }
    return unknown();
  }
  finish(record, intent, accepted) {
    return this.transaction(() => {
      const current = this.record(record.key);
      if (current.stock_state !== 'held') return super.lookup(record.key);
      const receipt = { attempt_key: record.key, quote_digest: record.input.quote_digest, order_id: record.order_id,
        total_minor: record.input.quote.total_minor, currency: record.input.quote.offer.currency, accepted,
        // Stripe test mode simulates funds. The v1 receipt never asserts real money moved.
        simulated: true };
      const result = { status: 'observed', receipt };
      this.db.prepare('UPDATE stripe_attempts SET stock_state=?,payment_status=?,charge_id=? WHERE key=?')
        .run(accepted ? 'consumed' : 'released', intent.status, typeof intent.latest_charge === 'string' ? intent.latest_charge : null, record.key);
      this.db.prepare('UPDATE attempts SET result=? WHERE key=?').run(JSON.stringify(result), record.key);
      this.db.prepare('INSERT INTO orders VALUES (?,?,?,?,?,?)').run(record.order_id, record.key, JSON.stringify(record.input.quote),
        JSON.stringify(receipt), accepted ? 'preparing' : 'declined', seconds());
      if (!accepted) this.db.prepare('UPDATE products SET stock=stock+? WHERE id=?').run(record.input.quote.quantity, record.input.quote.offer.id);
      return result;
    });
  }
  async lookup(key) {
    if (this.inflight.has(key)) return unknown();
    const saved = super.lookup(key);
    if (saved.status !== 'unknown') return saved;
    let record = this.record(key);
    if (!record) return saved;
    try {
      const intent = record.payment_intent_id ? await this.processor.retrieve(record.payment_intent_id) : await this.processor.find(record, this.instance);
      if (!intent) return unknown();
      record = this.bind(record, intent);
      return await this.resolve(record, intent);
    } catch { return unknown(); }
  }
  orders() {
    return super.orders().map(order => {
      const payment = this.db.prepare('SELECT payment_intent_id,payment_status,charge_id FROM stripe_attempts WHERE order_id=?').get(order.id);
      return { ...order, payment: { provider: 'stripe', environment: 'test', payment_intent_id: payment.payment_intent_id,
        status: payment.payment_status, charge_id: payment.charge_id, livemode: false } };
    });
  }
  payments() {
    return this.db.prepare('SELECT key,order_id,payment_intent_id,payment_status,stock_state FROM stripe_attempts ORDER BY rowid DESC LIMIT 50').all();
  }
  async webhook(event) {
    if (!['payment_intent.succeeded', 'payment_intent.payment_failed', 'payment_intent.canceled', 'payment_intent.processing'].includes(event.type)) return { ignored: true };
    if (this.db.prepare('SELECT id FROM stripe_events WHERE id=?').get(event.id)) return { duplicate: true };
    const id = event.data?.object?.id;
    if (typeof id !== 'string' || !/^pi_[A-Za-z0-9_]+$/.test(id)) throw new LabError(400, 'invalid_event', 'Invalid payment event.');
    // Always retrieve by ID. Signed payloads alone do not establish the quoted amount/owner.
    const intent = await this.processor.retrieve(id);
    const record = this.record(intent.metadata?.cosigna_attempt);
    if (!record) return { ignored: true };
    this.validateIntent(record, intent);
    const bound = this.bind(record, intent);
    await this.resolve(bound, intent);
    this.db.prepare('INSERT OR IGNORE INTO stripe_events VALUES (?,?,?)').run(event.id, record.key, seconds());
    return { received: true };
  }
}
