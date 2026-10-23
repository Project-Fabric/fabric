export function fakeStripe() {
  const intents = new Map(); const requests = []; const idempotency = new Map();
  let loseCreate = false, loseConfirm = false, confirmationStatus = 'succeeded';
  const transport = async (url, options) => {
    const path = new URL(url).pathname; const params = new URLSearchParams(options.body);
    requests.push({ path, options, params });
    const respond = value => new Response(JSON.stringify(value));
    if (path === '/v1/balance') return respond({ object: 'balance', livemode: false });
    if (path === '/v1/payment_intents' && options.method === 'POST') {
      if (params.has('payment_method_types[]')) return new Response(JSON.stringify({ error: { type: 'invalid_request_error' } }), { status: 400 });
      const idem = options.headers['Idempotency-Key'];
      let intent = idempotency.get(idem);
      if (!intent) {
        const metadata = {};
        for (const [key, value] of params) if (key.startsWith('metadata[')) metadata[key.slice(9,-1)] = value;
        intent = { id: `pi_test_${intents.size+1}`, object: 'payment_intent', livemode: false, amount: Number(params.get('amount')),
          currency: params.get('currency'), amount_received: 0, status: 'requires_payment_method',
          capture_method: 'automatic', confirmation_method: 'manual', metadata, latest_charge: null };
        intents.set(intent.id, intent); idempotency.set(idem, intent);
      }
      if (loseCreate) { loseCreate = false; throw new Error('connection lost after creation'); }
      return respond(intent);
    }
    if (path === '/v1/payment_intents' && options.method === 'GET') return respond({ object: 'list', has_more: false, data: [...intents.values()] });
    const id = path.split('/')[3]; const intent = intents.get(id);
    if (!intent) return new Response('{}', { status: 404 });
    if (path.endsWith('/confirm')) {
      if (params.get('payment_method') === 'pm_card_visa_chargeDeclined') {
        intent.status = 'requires_payment_method';
        return new Response(JSON.stringify({ error: { payment_intent: intent } }), { status: 402 });
      }
      intent.status = confirmationStatus;
      if (intent.status === 'succeeded') { intent.amount_received = intent.amount; intent.latest_charge = `ch_test_${intents.size}`; }
      if (loseConfirm) { loseConfirm = false; throw new Error('response lost after acceptance'); }
    } else if (path.endsWith('/cancel')) {
      if (intent.status === 'succeeded') return new Response('{}', { status: 400 });
      intent.status = 'canceled'; intent.amount_received = 0;
    }
    return respond(intent);
  };
  return { transport, intents, requests,
    loseNextCreate: () => { loseCreate = true; }, loseNextConfirm: () => { loseConfirm = true; },
    setConfirmationStatus: status => { confirmationStatus = status; } };
}
