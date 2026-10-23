import { DatabaseSync } from 'node:sqlite';
import { randomBytes, randomUUID, createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { mkdirSync, chmodSync } from 'node:fs';
import { dirname } from 'node:path';

export const SCHEMA = 'cosigna.test-merchant/v1';
const APPLICATION = 0x43534d54;
const MAX_MINOR = 1_000_000_000_000;
const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
export class LabError extends Error {
  constructor(status, code, message) { super(message); this.status = status; this.code = code; }
}
export function object(value, allowed, required = allowed) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).some(key => !allowed.includes(key)) || required.some(key => !(key in value))) {
    throw new LabError(400, 'invalid_request', 'Invalid fields in the request.');
  }
}
export function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}
export const seconds = () => Math.floor(Date.now() / 1000);

export class MerchantStore {
  constructor(path, { paymentMode = 'simulated' } = {}) {
    if (!['simulated', 'stripe-test'].includes(paymentMode)) throw new LabError(400, 'invalid_mode', 'Unsupported payment mode.');
    this.paymentMode = paymentMode;
    const application = paymentMode === 'stripe-test' ? 0x43534d53 : APPLICATION;
    mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    this.db = new DatabaseSync(path);
    if (process.platform !== 'win32') chmodSync(path, 0o600);
    this.db.exec('PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA busy_timeout=5000;');
    this.transaction(() => {
      const app = this.db.prepare('PRAGMA application_id').get().application_id;
      const version = this.db.prepare('PRAGMA user_version').get().user_version;
      const tables = this.db.prepare("SELECT count(*) AS n FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").get().n;
      if (app === 0 && version === 0 && tables === 0) {
        this.db.exec(`PRAGMA application_id=${application}; PRAGMA user_version=1;
          CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE TABLE products (id TEXT PRIMARY KEY, offer TEXT NOT NULL, stock INTEGER NOT NULL CHECK(stock>=0));
          CREATE TABLE attempts (key TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result TEXT NOT NULL);
          CREATE TABLE orders (id TEXT PRIMARY KEY, attempt_key TEXT NOT NULL UNIQUE REFERENCES attempts(key), quote TEXT NOT NULL,
            receipt TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('preparing','ready','collected','declined')), created_at INTEGER NOT NULL);
          CREATE TABLE lab_tasks (task_id TEXT PRIMARY KEY, request_key TEXT NOT NULL UNIQUE);`);
        if (paymentMode === 'stripe-test') this.db.exec(`
          CREATE TABLE stripe_attempts (
            key TEXT PRIMARY KEY REFERENCES attempts(key), request TEXT NOT NULL, order_id TEXT NOT NULL UNIQUE,
            prepared_at INTEGER NOT NULL, payment_intent_id TEXT UNIQUE, confirm_started INTEGER,
            stock_state TEXT NOT NULL CHECK(stock_state IN ('held','consumed','released')),
            payment_status TEXT NOT NULL, charge_id TEXT);
          CREATE TABLE stripe_events (id TEXT PRIMARY KEY, attempt_key TEXT NOT NULL REFERENCES stripe_attempts(key), received_at INTEGER NOT NULL);`);
        this.set('instance_id', randomUUID());
        this.set('merchant_token', randomBytes(32).toString('hex'));
        for (const [id, title, price, features, stock] of [
          ['oat-latte', 'Oat latte', 700, ['oat_milk', 'pickup'], 12],
          ['black-coffee', 'Black coffee', 450, ['black_coffee', 'pickup'], 16],
          ['butter-croissant', 'Butter croissant', 400, ['pastry', 'pickup'], 8],
        ]) {
          const offer = { id, merchant_id: 'cosigna-test-cafe', title, currency: 'USD', unit_total_minor: price,
            features, fulfillment: 'Pickup at Cosigna Test Cafe, counter A. Test location only.',
            terms: paymentMode === 'stripe-test'
              ? 'Stripe sandbox payment with a fixed Stripe test card. No real funds or physical fulfillment. All fees included.'
              : 'Test purchase. All fees included. Simulated payment; no real charge or physical fulfillment.', revision: 1, simulated: true };
          this.db.prepare('INSERT INTO products(id,offer,stock) VALUES (?,?,?)').run(id, JSON.stringify(offer), stock);
        }
      } else if (app !== application || version !== 1) {
        throw new LabError(500, 'store_format', 'Unsupported or foreign merchant database.');
      }
    });
    this.db.exec('PRAGMA journal_mode=WAL;');
  }
  transaction(fn) {
    this.db.exec('BEGIN IMMEDIATE');
    try { const value = fn(); this.db.exec('COMMIT'); return value; }
    catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }
  get(key) { return this.db.prepare('SELECT value FROM settings WHERE key=?').get(key)?.value; }
  set(key, value) { this.db.prepare('INSERT INTO settings(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value').run(key, value); }
  get instance() { return this.get('instance_id'); }
  get token() { return this.get('merchant_token'); }
  products() { return this.db.prepare('SELECT offer,stock FROM products ORDER BY id').all().map(row => ({ offer: JSON.parse(row.offer), stock: row.stock })); }
  catalog() { return this.products().filter(row => row.stock > 0).map(row => row.offer); }
  lookup(key) {
    const row = this.db.prepare('SELECT result FROM attempts WHERE key=?').get(key);
    return row ? JSON.parse(row.result) : { status: 'absent' };
  }
  submit(input, at = seconds()) {
    object(input, ['attempt_key', 'quote_digest', 'quote', 'scenario']);
    const { attempt_key: key, quote_digest: digest, quote, scenario } = input;
    if (typeof key !== 'string' || !/^[A-Za-z0-9_]{1,128}$/.test(key)
      || typeof digest !== 'string' || !/^[a-f0-9]{64}$/.test(digest)
      || !['success', 'decline', 'timeout_after_accept', 'timeout_before_accept'].includes(scenario)) {
      throw new LabError(400, 'invalid_request', 'Invalid attempt or scenario.');
    }
    object(quote, ['id', 'task_id', 'offer', 'quantity', 'total_minor', 'issued_at', 'expires_at']);
    const fingerprint = createHash('sha256').update(canonical({ quote, digest })).digest('hex');
    return this.transaction(() => {
      const existing = this.db.prepare('SELECT fingerprint,result FROM attempts WHERE key=?').get(key);
      if (existing) {
        if (existing.fingerprint !== fingerprint) throw new LabError(409, 'idempotency_conflict', 'This attempt key belongs to a different purchase.');
        return JSON.parse(existing.result);
      }
      const product = typeof quote.offer?.id === 'string'
        ? this.db.prepare('SELECT offer,stock FROM products WHERE id=?').get(quote.offer.id) : undefined;
      if (!product || !isDeepStrictEqual(JSON.parse(product.offer), quote.offer)
          || !integer(quote.quantity, 1, 10) || !integer(quote.total_minor, 1, MAX_MINOR)
          || quote.total_minor !== quote.offer.unit_total_minor * quote.quantity
          || !integer(quote.issued_at, 0, at) || !integer(quote.expires_at, at + 1, at + 3600)
          || typeof quote.id !== 'string' || quote.id.length > 150
          || typeof quote.task_id !== 'string' || quote.task_id.length > 150 || product.stock < quote.quantity) {
        const rejected = { status: 'rejected' };
        this.db.prepare('INSERT INTO attempts VALUES (?,?,?)').run(key, fingerprint, JSON.stringify(rejected));
        return rejected;
      }
      if (scenario === 'timeout_before_accept') return { status: 'unknown' };
      const accepted = scenario !== 'decline';
      const receipt = { attempt_key: key, quote_digest: digest, order_id: `test_order_${randomUUID()}`,
        total_minor: quote.total_minor, currency: quote.offer.currency, accepted, simulated: true };
      const observed = { status: 'observed', receipt };
      this.db.prepare('INSERT INTO attempts VALUES (?,?,?)').run(key, fingerprint, JSON.stringify(observed));
      this.db.prepare('INSERT INTO orders VALUES (?,?,?,?,?,?)').run(receipt.order_id, key,
        JSON.stringify(quote), JSON.stringify(receipt), accepted ? 'preparing' : 'declined', at);
      if (accepted) this.db.prepare('UPDATE products SET stock=stock-? WHERE id=?').run(quote.quantity, quote.offer.id);
      return observed;
    });
  }
  orders() {
    return this.db.prepare('SELECT id,quote,receipt,state,created_at FROM orders ORDER BY created_at DESC,rowid DESC LIMIT 50').all()
      .map(row => ({ ...row, quote: JSON.parse(row.quote), receipt: JSON.parse(row.receipt) }));
  }
  fulfill(id, state) {
    const current = this.db.prepare('SELECT state FROM orders WHERE id=?').get(id)?.state;
    if (!current) throw new LabError(404, 'not_found', 'Order not found.');
    if (current === state) return;
    if (!((current === 'preparing' && state === 'ready') || (current === 'ready' && state === 'collected'))) {
      throw new LabError(409, 'fulfillment_conflict', 'The order cannot move to that fulfillment state.');
    }
    this.db.prepare('UPDATE orders SET state=? WHERE id=? AND state=?').run(state, id, current);
  }
  rememberTask(id, key) { this.db.prepare('INSERT OR IGNORE INTO lab_tasks VALUES (?,?)').run(id, key); }
  ownsTask(id) { return typeof id === 'string' && Boolean(this.db.prepare('SELECT task_id FROM lab_tasks WHERE task_id=?').get(id)); }
  tasks() { return this.db.prepare('SELECT task_id FROM lab_tasks ORDER BY rowid DESC LIMIT 20').all().map(row => row.task_id); }
  close() { this.db.close(); }
}
