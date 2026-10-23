const $ = selector => document.querySelector(selector);
const state = { csrf: '', busy: false, recording: false, budget: null, task: null, digest: null,
  key: crypto.randomUUID(), voice: false, mode: null, paymentMode: 'simulated', orders: [], recorder: null, timer: null };
const money = cents => new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(cents / 100);
function node(tag, text, className) { const element = document.createElement(tag); if (text !== undefined) element.textContent = text; if (className) element.className = className; return element; }
function feedback(message = '', error = false) { $('#feedback').textContent = message; $('#feedback').classList.toggle('error', error); }
function speak(text) {
  if ('speechSynthesis' in window) { speechSynthesis.cancel(); const utterance = new SpeechSynthesisUtterance(text); utterance.rate = 1; speechSynthesis.speak(utterance); }
}
function activeBudget() { return state.budget && !state.budget.revoked && state.budget.terms.expires_at > Date.now() / 1000; }
function hasOrderAttempt() { return ['submitting', 'unknown', 'confirmed', 'declined'].includes(state.task?.stage); }
function updateTaskNote() {
  const task = state.task;
  if (!task) return;
  const fulfillment = state.orders.find(order => order.id === task.receipt?.order_id)?.state;
  const confirmed = fulfillment === 'ready' ? 'Your test order is ready for pickup.'
    : fulfillment === 'collected' ? 'Your test order has been marked collected.'
      : fulfillment === 'preparing' ? 'The merchant is preparing your test order.' : 'The merchant accepted your test order.';
  $('#task-note').textContent = task.stage === 'quoted' ? `No order has been placed yet. Review the quote and approve it to submit. Valid until ${new Date(task.quote.expires_at * 1000).toLocaleTimeString()}.`
    : task.stage === 'confirmed' ? `${confirmed} Receipt: ${task.receipt.order_id}. ${state.paymentMode === 'stripe-test' ? 'Payment was verified in Stripe sandbox; pickup is a test.' : 'Payment and pickup are simulated.'}`
      : ['submitting', 'unknown'].includes(task.stage) ? 'The outcome is uncertain. Your budget remains reserved. Check status; do not place a replacement order.'
        : task.stage === 'declined' ? 'The merchant declined this purchase. No budget was spent.'
          : task.stage === 'needs_input' ? state.mode === 'fixtures'
            ? 'No order was placed and no money was spent. This offline demo cannot interpret that wording. Use an example above, then click Find my order.'
            : 'No order was placed. Add the requested detail, then click Find my order for a new quote.'
          : task.stage === 'approved' ? 'Your quote is approved, but no submission has been confirmed. No merchant order is confirmed yet.'
            : task.stage === 'cancelled' ? 'The request was cancelled before submission.' : 'No order has been submitted.';
}
function controls() {
  const blocked = state.busy || state.recording;
  $('#set-budget').disabled = blocked || Boolean(activeBudget());
  $('#budget-limit').disabled = Boolean(activeBudget()); $('#budget-cap').disabled = Boolean(activeBudget());
  $('#plan').disabled = blocked || !activeBudget();
  $('#record').disabled = state.busy || !activeBudget() || !state.voice || !window.MediaRecorder;
  $('#request').disabled = blocked;
  $('#approve').disabled = blocked || state.task?.stage !== 'quoted' || state.task?.quote?.expires_at <= Date.now() / 1000;
  $('#check').disabled = blocked || !hasOrderAttempt(); $('#check').hidden = !hasOrderAttempt();
  $('#cancel').disabled = blocked; $('#refresh').disabled = blocked;
  $('#example-latte').disabled = blocked; $('#example-coffee').disabled = blocked;
  $('#record').classList.toggle('recording', state.recording);
  $('#record-label').textContent = state.recording ? 'Stop recording' : 'Speak your order';
}
async function api(path, body, contentType = 'application/json') {
  const response = await fetch(path, { method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin',
    headers: body === undefined ? {} : { 'Content-Type': contentType, 'X-Cosigna-CSRF': state.csrf },
    body: body === undefined ? undefined : contentType === 'application/json' ? JSON.stringify(body) : body });
  const result = await response.json();
  if (!response.ok) throw new Error(result.error?.message ?? 'The request did not complete. Check the current task status.');
  return result.data;
}
function renderTask(result) {
  state.task = result.data; state.digest = result.approval_digest;
  const task = state.task;
  $('#quote-title').textContent = hasOrderAttempt() ? 'Your order' : task.quote ? 'Review your quote' : 'Your request';
  $('#quote-card').hidden = false; $('#task-stage').textContent = task.stage.replaceAll('_', ' ');
  const detail = $('#quote-detail'); detail.replaceChildren();
  if (task.quote) {
    const q = task.quote;
    detail.append(node('p', `${q.quantity} × ${q.offer.title}`, 'quote-title'), node('p', money(q.total_minor), 'quote-total'),
      node('p', q.offer.fulfillment, 'quote-meta'), node('p', `Seller: ${q.offer.merchant_id} · ${q.offer.currency} · all fees included`, 'quote-meta'),
      node('p', q.offer.terms, 'quote-terms'));
  } else if (task.clarification) { detail.append(node('p', task.clarification, 'quote-meta')); speak(task.clarification); }
  $('#approve').hidden = task.stage !== 'quoted';
  $('#cancel').hidden = !['draft', 'needs_input', 'quoted', 'approved'].includes(task.stage);
  updateTaskNote();
  controls();
}
function renderProducts(products) {
  const container = $('#products'); container.replaceChildren();
  for (const { offer, stock } of products) {
    const card = node('article', undefined, 'product'); const art = node('div', undefined, 'product-art'); art.setAttribute('aria-hidden', 'true');
    art.append(node('div', offer.id === 'butter-croissant' ? '◒' : undefined, offer.id === 'butter-croissant' ? 'pastry' : 'cup'));
    const info = node('div', undefined, 'product-info'); const price = node('div', undefined, 'product-price');
    price.append(node('span', money(offer.unit_total_minor)), node('span', `${stock} left`));
    info.append(node('h3', offer.title), price); card.append(art, info); container.append(card);
  }
}
function renderOrders(orders) {
  const container = $('#orders'); container.replaceChildren();
  if (!orders.length) { const empty = node('div', 'No orders at the counter yet.', 'empty-counter'); empty.append(node('span', 'Your first approved order will appear here.')); container.append(empty); }
  for (const order of orders) {
    const row = node('article', undefined, 'order-row'); const top = node('div', undefined, 'order-top');
    top.append(node('h4', `${order.quote.quantity} × ${order.quote.offer.title}`), node('span', order.state, 'stage'));
    const paymentLabel = order.payment?.provider === 'stripe'
      ? `Stripe test payment: ${order.payment.status}` : order.receipt.accepted ? 'Simulated payment accepted' : 'Payment declined';
    row.append(top, node('p', `${money(order.receipt.total_minor)} · ${paymentLabel}`), node('p', order.id));
    if (order.payment?.payment_intent_id) {
      row.append(node('p', `PaymentIntent: ${order.payment.payment_intent_id}`));
      if (/^pi_[A-Za-z0-9_]+$/.test(order.payment.payment_intent_id)) {
        const link = node('a', 'View Stripe test payment ↗', 'button text-button');
        link.href = `https://dashboard.stripe.com/test/payments/${order.payment.payment_intent_id}`;
        link.target = '_blank'; link.rel = 'noreferrer'; row.append(link);
      }
    }
    if (['preparing', 'ready'].includes(order.state)) {
      const button = node('button', order.state === 'preparing' ? 'Mark ready for pickup' : 'Mark collected', 'button secondary'); button.type = 'button';
      button.disabled = state.busy;
      button.addEventListener('click', () => run(async () => { await api('/api/fulfill', { order_id: order.id, state: order.state === 'preparing' ? 'ready' : 'collected' }); await refresh(); }));
      row.append(button);
    }
    container.append(row);
  }
}
async function refresh() {
  const data = await api('/api/state'); state.csrf = data.csrf; state.voice = data.voice_available; state.mode = data.mode; state.orders = data.orders; state.budget = data.balance?.budget ?? null;
  state.paymentMode = data.payment_mode ?? 'simulated';
  $('#payment-badge').textContent = state.paymentMode === 'stripe-test' ? 'Stripe sandbox · no real funds' : 'Simulated payments';
  $('#environment-note').textContent = state.paymentMode === 'stripe-test'
    ? 'Orders use the real Stripe API in sandbox/test mode. No real money moves. The café and physical pickup remain test data.'
    : 'This café, its stock, payments and pickup are test data. No money moves and no physical order is placed.';
  $('#available').textContent = money(data.balance?.available_minor ?? 0);
  $('#budget-status').textContent = activeBudget() ? 'Active' : 'Not active';
  $('#budget-summary').textContent = data.balance ? `${money(data.balance.spent_minor)} spent · ${money(data.balance.reserved_minor)} reserved · ${money(data.balance.budget.terms.per_purchase_minor)} per-order cap · expires ${new Date(data.balance.budget.terms.expires_at * 1000).toLocaleTimeString()}` : 'Set an allowance to begin. Every purchase still needs your approval.';
  const provider = data.ai_provider === 'groq' ? 'Groq' : 'OpenAI';
  $('#mode-label').textContent = data.mode !== 'fixtures' ? `${provider} AI planner · independent merchant API` : 'Fixture script · not AI · independent merchant API';
  $('#voice-note').textContent = state.voice ? `Press to record, then stop. Your audio is sent to ${provider} for transcription. Maximum 20 seconds.` : 'Voice is not configured. Type a request to continue.';
  $('#fixture-help').hidden = state.mode !== 'fixtures';
  $('#request').placeholder = state.mode === 'fixtures' ? 'Order one oat latte' : 'Order one oat latte for pickup.';
  renderProducts(data.products); renderOrders(data.orders); updateTaskNote(); controls();
  if (!state.task && data.recent_tasks[0]) renderTask(await api(`/api/tasks/${data.recent_tasks[0]}`));
}
async function run(action) {
  if (state.busy) return;
  state.busy = true; controls(); feedback();
  try { await action(); } catch (error) { feedback(error.message, true); }
  finally { state.busy = false; controls(); }
}
function cents(value) {
  if (!/^\d+(\.\d{1,2})?$/.test(value.trim())) throw new Error('Use a positive amount with up to two decimal places.');
  const [whole, fraction = ''] = value.trim().split('.'); const total = Number(whole) * 100 + Number(fraction.padEnd(2, '0'));
  if (!Number.isSafeInteger(total)) throw new Error('This amount is too large.'); return total;
}
$('#budget-form').addEventListener('submit', event => { event.preventDefault(); run(async () => { await api('/api/budget', { limit_minor: cents($('#budget-limit').value), per_purchase_minor: cents($('#budget-cap').value) }); await refresh(); feedback('Test budget set. Speak or type your first order.'); }); });
$('#request').addEventListener('input', () => { state.key = crypto.randomUUID(); });
for (const [id, request] of [['example-latte', 'Order one oat latte'], ['example-coffee', 'Order one black coffee']]) {
  $(`#${id}`).addEventListener('click', () => {
    $('#request').value = request; state.key = crypto.randomUUID(); $('#request').focus();
    feedback('Example added. Click Find my order to get a quote. Nothing will be placed until you approve.');
  });
}
async function plan() {
  feedback('Finding a matching offer and preparing your quote…');
  if (state.task?.quote?.expires_at <= Date.now() / 1000 || state.task?.stage === 'cancelled') state.key = crypto.randomUUID();
  const result = await api('/api/plan', { request: $('#request').value.trim(), idempotency_key: state.key }); renderTask(result);
  if (state.task.stage === 'quoted') { feedback('Your quote is ready. Review it before placing the test order.'); speak(`${state.task.quote.quantity} ${state.task.quote.offer.title}, total ${money(state.task.quote.total_minor)}. Review the quote and press approve to place the test order.`); }
  else if (state.task.stage === 'needs_input') feedback($('#task-note').textContent);
  else feedback($('#task-note').textContent);
}
$('#request-form').addEventListener('submit', event => { event.preventDefault(); run(plan); });
$('#approve').addEventListener('click', () => run(async () => {
  feedback('Submitting your approved test order…');
  renderTask(await api('/api/approve', { task_id: state.task.id, quote_digest: state.digest }));
  await refresh();
  if (state.task.stage === 'confirmed') { feedback('The merchant accepted your test order. It is now at the counter.'); speak('Your test order was accepted. The merchant will mark it ready at the counter.'); }
  else feedback($('#task-note').textContent, state.task.stage !== 'confirmed');
}));
$('#check').addEventListener('click', () => run(async () => { renderTask(await api('/api/reconcile', { task_id: state.task.id })); await refresh(); feedback($('#task-note').textContent); }));
$('#cancel').addEventListener('click', () => run(async () => { renderTask(await api('/api/cancel', { task_id: state.task.id })); state.key = crypto.randomUUID(); await refresh(); }));
$('#refresh').addEventListener('click', () => run(refresh));
$('#pair-device').addEventListener('click', () => run(async () => {
  const pairing = await api('/api/device-pairing', {});
  $('#pairing-code').textContent = `${pairing.code} · valid until ${new Date(pairing.expires_at * 1000).toLocaleTimeString()}. Enter this code in the phone's Order lab tab.`;
  setTimeout(() => { $('#pairing-code').textContent = ''; }, 300_000);
}));
$('#revoke-devices').addEventListener('click', () => run(async () => {
  await api('/api/revoke-devices', {}); $('#pairing-code').textContent = 'Phone access revoked. Pending orders and records remain.';
}));
$('#record').addEventListener('click', async () => {
  if (state.recording) { state.recorder.stop(); return; }
  if (state.busy) return;
  let stream;
  state.busy = true; controls();
  try {
    if ('speechSynthesis' in window) speechSynthesis.cancel();
    stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const mimeType = ['audio/webm;codecs=opus', 'audio/mp4', 'audio/webm'].find(type => MediaRecorder.isTypeSupported(type));
    if (!mimeType) throw new Error('This browser cannot record a supported audio format. Type your order instead.');
    const recorder = new MediaRecorder(stream, { mimeType }); const chunks = []; let failed = false;
    state.recorder = recorder; state.recording = true; state.busy = false; controls(); feedback('Listening… Press stop when you have finished.');
    recorder.addEventListener('dataavailable', event => { if (event.data.size) chunks.push(event.data); });
    recorder.addEventListener('stop', () => {
      clearTimeout(state.timer); stream.getTracks().forEach(track => track.stop()); state.recording = false; state.recorder = null; controls();
      if (failed) return;
      run(async () => {
        feedback('Transcribing your voice request…');
        const blob = new Blob(chunks, { type: mimeType });
        const result = await api('/api/transcribe', blob, mimeType);
        $('#request').value = result.text; state.key = crypto.randomUUID(); await plan();
      });
    }, { once: true });
    recorder.addEventListener('error', () => { failed = true; stream.getTracks().forEach(track => track.stop()); clearTimeout(state.timer); state.recording = false; controls(); feedback('Recording failed. Please try again or type your request.', true); });
    recorder.start(); state.timer = setTimeout(() => { if (recorder.state === 'recording') recorder.stop(); }, 20_000);
  } catch (error) { stream?.getTracks().forEach(track => track.stop()); state.recording = false; state.busy = false; controls(); feedback(error.name === 'NotAllowedError' ? 'Microphone permission was denied. Allow it in your browser settings, or type your order.' : error.message, true); }
});
window.addEventListener('pagehide', () => { clearTimeout(state.timer); state.recorder?.stream.getTracks().forEach(track => track.stop()); });
run(refresh);
setInterval(() => { if (!state.busy && !state.recording) refresh().catch(() => {}); }, 5000);
setInterval(controls, 1000);
