import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { JSDOM } from 'jsdom';

const html = await readFile(new URL('../public/index.html', import.meta.url), 'utf8');
const script = await readFile(new URL('../public/app.js', import.meta.url), 'utf8');
const waitFor = async check => {
  const started = Date.now();
  while (!check()) { assert(Date.now()-started < 2000, 'UI did not reach expected state'); await new Promise(resolve => setImmediate(resolve)); }
};
function fixture(t, permissionDenied = false, { mode = 'openai', voice = true, initialTask = null, paymentMode = 'simulated' } = {}) {
  const dom = new JSDOM(html, { url: 'http://127.0.0.1:3093', runScripts: 'outside-only' });
  t.after(() => dom.window.close());
  const w = dom.window; const requests = []; let tracksStopped = 0;
  Object.defineProperty(w.crypto, 'randomUUID', { value: randomUUID });
  const stream = { getTracks: () => [{ stop: () => tracksStopped++ }] };
  Object.defineProperty(w.navigator, 'mediaDevices', { value: { getUserMedia: async () => {
    if (permissionDenied) throw Object.assign(new Error('denied'), { name: 'NotAllowedError' }); return stream;
  } } });
  class Recorder extends w.EventTarget {
    static isTypeSupported(type) { return type.startsWith('audio/webm'); }
    constructor(stream) { super(); this.stream = stream; this.state = 'inactive'; }
    start() { this.state = 'recording'; }
    stop() {
      this.state = 'inactive';
      const event = new w.Event('dataavailable'); event.data = new w.Blob(['synthetic']); this.dispatchEvent(event); this.dispatchEvent(new w.Event('stop'));
    }
  }
  w.MediaRecorder = Recorder;
  w.speechSynthesis = { cancel() {}, speak() {} }; w.SpeechSynthesisUtterance = class { constructor(text) { this.text = text; } };
  const offer = { id: 'oat-latte', merchant_id: 'cosigna-test-cafe', title: '<img src=x onerror=alert(1)> Oat latte', currency: 'USD', unit_total_minor: 700,
    fulfillment: 'Test pickup counter', terms: 'Simulated payment', simulated: true };
  const quoted = { id: `task_${'a'.repeat(64)}`, stage: 'quoted', quote: { quantity: 1, offer, total_minor: 700, expires_at: Math.floor(Date.now()/1000)+300 } };
  let task = initialTask ?? quoted; let orders = [];
  w.fetch = async (path, options) => {
    requests.push({ path, options });
    let data;
    if (path === '/api/state') data = { csrf: 'test-csrf', mode, ai_provider: mode === 'groq' ? 'groq' : 'openai', payment_mode: paymentMode, voice_available: voice, products: [{ offer, stock: 12 }], orders, recent_tasks: initialTask ? [initialTask.id] : [],
      balance: { available_minor: 2000, reserved_minor: 0, spent_minor: 0, budget: { revoked: false, terms: { expires_at: Math.floor(Date.now()/1000)+3600, per_purchase_minor: 1000 } } } };
    else if (path === '/api/transcribe') data = { text: 'Order one oat latte' };
    else if (path === '/api/plan') {
      const request = JSON.parse(options.body).request;
      if (mode === 'fixtures') task = request === 'Order one oat latte' ? quoted : { ...quoted, stage: 'needs_input', quote: null, clarification: "The fixture planner only supports 'Order one oat latte' and 'Order one black coffee'." };
      data = { data: task, approval_digest: task.quote ? 'b'.repeat(64) : null };
    }
    else if (path.startsWith('/api/tasks/') || path === '/api/reconcile') data = { data: task, approval_digest: task.quote ? 'b'.repeat(64) : null };
    else if (path === '/api/approve') {
      assert.deepEqual(JSON.parse(options.body), { task_id: task.id, quote_digest: 'b'.repeat(64) });
      task = { ...task, stage: 'confirmed', receipt: { order_id: 'test_order_1' } };
      data = { data: task, approval_digest: 'b'.repeat(64) };
    } else throw new Error(`Unexpected UI route ${path}`);
    return new Response(JSON.stringify({ data }), { headers: { 'Content-Type': 'application/json' } });
  };
  w.eval(script);
  return { w, requests, stopped: () => tracksStopped, setOrders: value => { orders = value; } };
}

test('push-to-talk plans an order but only an explicit click submits its exact quote', async t => {
  const { w, requests, stopped } = fixture(t);
  const $ = selector => w.document.querySelector(selector);
  await waitFor(() => !$('#record').disabled);
  $('#record').click(); await waitFor(() => $('#record-label').textContent === 'Stop recording');
  $('#record').click(); await waitFor(() => !$('#quote-card').hidden && !$('#approve').disabled);
  assert.equal(stopped(), 1); assert.equal($('#request').value, 'Order one oat latte');
  assert.equal(requests.filter(request => request.path === '/api/transcribe').length, 1);
  assert.equal(requests.filter(request => request.path === '/api/plan').length, 1);
  assert.equal(requests.filter(request => request.path === '/api/approve').length, 0);
  assert.equal($('#quote-detail img'), null, 'merchant text must not become HTML');
  assert($('#quote-detail').textContent.includes('<img src=x'));
  $('#approve').click(); await waitFor(() => $('#task-stage').textContent === 'confirmed' && !$('#refresh').disabled);
  assert.equal(requests.filter(request => request.path === '/api/approve').length, 1);
  assert($('#approve').hidden); assert($('#task-note').textContent.includes('The merchant accepted your test order.'));
});

test('microphone denial is recoverable and typed ordering remains available', async t => {
  const { w, requests } = fixture(t, true); const $ = selector => w.document.querySelector(selector);
  await waitFor(() => !$('#record').disabled); $('#record').click();
  await waitFor(() => $('#feedback').textContent.includes('Microphone permission was denied'));
  assert.equal($('#plan').disabled, false); assert.equal(requests.some(request => request.path === '/api/transcribe'), false);
  $('#request').value = 'Order one oat latte'; $('#request').dispatchEvent(new w.Event('input'));
  $('#request-form').dispatchEvent(new w.Event('submit', { cancelable: true }));
  await waitFor(() => !$('#quote-card').hidden);
  assert.equal(requests.filter(request => request.path === '/api/plan').length, 1);
});

test('unsupported offline wording explains no order exists and examples recover to an approvable quote', async t => {
  const { w, requests } = fixture(t, false, { mode: 'fixtures', voice: false });
  const $ = selector => w.document.querySelector(selector);
  await waitFor(() => !$('#plan').disabled);
  assert.equal($('#fixture-help').hidden, false);
  assert.equal($('#request').placeholder, 'Order one oat latte');
  $('#request').value = 'order one oat latte for pickup on my way to office';
  $('#request').dispatchEvent(new w.Event('input'));
  $('#request-form').dispatchEvent(new w.Event('submit', { cancelable: true }));
  await waitFor(() => $('#task-stage').textContent === 'needs input' && !$('#plan').disabled);
  assert.equal($('#quote-title').textContent, 'Your request');
  assert($('#task-note').textContent.includes('No order was placed and no money was spent.'));
  assert($('#task-note').textContent.includes('Use an example above'));
  assert.equal($('#check').hidden, true); assert.equal($('#check').disabled, true);
  $('#check').click(); assert.equal(requests.some(request => request.path === '/api/reconcile'), false);
  $('#example-latte').click();
  assert.equal($('#request').value, 'Order one oat latte');
  assert.equal(requests.filter(request => request.path === '/api/plan').length, 1, 'choosing an example must not submit it');
  $('#request-form').dispatchEvent(new w.Event('submit', { cancelable: true }));
  await waitFor(() => !$('#approve').disabled);
  assert.equal($('#quote-title').textContent, 'Review your quote');
  assert.equal($('#check').hidden, true);
  assert.equal(requests.some(request => request.path === '/api/approve'), false);
  assert.equal(JSON.parse(requests.filter(request => request.path === '/api/plan')[1].options.body).request, 'Order one oat latte');
});

test('status checks show the merchant pickup state for a submitted order', async t => {
  const initialTask = { id: `task_${'c'.repeat(64)}`, stage: 'confirmed', quote: null, receipt: { order_id: 'test_order_ready' } };
  const { w, requests, setOrders } = fixture(t, false, { initialTask });
  const $ = selector => w.document.querySelector(selector);
  await waitFor(() => !$('#check').disabled);
  assert.equal($('#check').hidden, false); assert.equal($('#quote-title').textContent, 'Your order');
  setOrders([{ id: 'test_order_ready', quote: { quantity: 1, offer: { title: 'Oat latte' } }, receipt: { total_minor: 700, accepted: true }, state: 'ready' }]);
  $('#check').click();
  await waitFor(() => $('#task-note').textContent.includes('Your test order is ready for pickup.'));
  assert.equal(requests.filter(request => request.path === '/api/reconcile').length, 1);
  assert.equal(requests.some(request => request.path === '/api/approve'), false);
});

test('Stripe mode labels the actual test processor and displays a verifiable PaymentIntent link', async t => {
  const initialTask = { id: `task_${'d'.repeat(64)}`, stage: 'confirmed', quote: null, receipt: { order_id: 'stripe_order_1' } };
  const { w, setOrders } = fixture(t, false, { initialTask, paymentMode: 'stripe-test' });
  const $ = selector => w.document.querySelector(selector);
  await waitFor(() => !$('#check').disabled);
  setOrders([{ id: 'stripe_order_1', state: 'preparing', quote: { quantity: 1, offer: { title: 'Oat latte' } }, receipt: { accepted: true, total_minor: 700 },
    payment: { provider: 'stripe', status: 'succeeded', payment_intent_id: 'pi_test_123' } }]);
  $('#refresh').click();
  await waitFor(() => Boolean($('#orders a')));
  assert.equal($('#payment-badge').textContent, 'Stripe sandbox · no real funds');
  assert($('#orders').textContent.includes('Stripe test payment: succeeded'));
  assert.equal($('#orders a').href, 'https://dashboard.stripe.com/test/payments/pi_test_123');
  assert($('#task-note').textContent.includes('Payment was verified in Stripe sandbox'));
});
