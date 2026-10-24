import { createServer } from 'node:http';
import { randomBytes, createHash, timingSafeEqual } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { SCHEMA, LabError, object, seconds } from './store.mjs';
import { transcribe } from './voice.mjs';
import { verifyStripeEvent } from './stripe.mjs';
import { MobileAccess } from './mobile-access.mjs';
import { providerForMode } from './ai-providers.mjs';

const PUBLIC = new URL('./public/', import.meta.url);
function equal(a, b) {
  return typeof a === 'string' && typeof b === 'string'
    && timingSafeEqual(createHash('sha256').update(a).digest(), createHash('sha256').update(b).digest());
}
async function readBody(request, max) {
  if (Number(request.headers['content-length']) > max) throw new LabError(413, 'body_too_large', 'Request body is too large.');
  const chunks = []; let length = 0;
  for await (const chunk of request) {
    length += chunk.length;
    if (length > max) throw new LabError(413, 'body_too_large', 'Request body is too large.');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}
async function readJson(request) {
  if (request.headers['content-type']?.split(';')[0] !== 'application/json') throw new LabError(400, 'invalid_request', 'Send application/json.');
  try { return JSON.parse((await readBody(request, 16_384)).toString('utf8')); }
  catch (error) { if (error instanceof LabError) throw error; throw new LabError(400, 'invalid_json', 'Invalid JSON.'); }
}
function json(response, status, value) {
  response.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'Connection': 'close' });
  response.end(JSON.stringify(value));
}
function isTaskId(id) { return typeof id === 'string' && /^task_[a-f0-9]{64}$/.test(id); }

export function createLabServer(config) {
  const { store } = config;
  const session = randomBytes(32).toString('hex');
  const csrf = randomBytes(32).toString('hex');
  const mobileAccess = new MobileAccess();
  const server = createServer(async (request, response) => {
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('Referrer-Policy', 'no-referrer');
    response.setHeader('X-Frame-Options', 'DENY');
    response.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; media-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'");
    response.setHeader('Permissions-Policy', 'microphone=(self), camera=(), geolocation=()');
    const port = server.address()?.port;
    const cookieName = `cosigna_lab_${port}`;
    const origin = `http://127.0.0.1:${port}`;
    let native = false;
    try {
      const provider = providerForMode(config.mode);
      if (request.headers.host !== `127.0.0.1:${port}` || !request.url.startsWith('/') || request.url.startsWith('//')) {
        throw new LabError(403, 'forbidden', 'Use the local lab address.');
      }
      const url = new URL(request.url, origin);
      let path = url.pathname;
      native = path.startsWith('/mobile/');
      if (native) {
        if (!config.mobile) throw new LabError(404, 'not_found', 'Mobile testing is not enabled.');
        if (request.headers.origin || request.headers['sec-fetch-site'] || request.headers.cookie) {
          throw new LabError(403, 'forbidden', 'Use the paired native application.');
        }
        if (request.method === 'POST' && path === '/mobile/v3/pair') {
          const body = await readJson(request); object(body, ['code']);
          json(response, 200, { schema: 'cosigna.mobile-lab/v3', data: mobileAccess.pair(body.code) }); return;
        }
        mobileAccess.authorize(request.headers.authorization);
        if (request.method === 'POST' && path === '/mobile/v3/disconnect') {
          object(await readJson(request), []); mobileAccess.revoke();
          json(response, 200, { schema: 'cosigna.mobile-lab/v3', data: { disconnected: true } }); return;
        }
        const action = path.slice('/mobile/v3/'.length);
        const allowed = request.method === 'GET' ? /^(state|tasks\/task_[a-f0-9]{64})$/
          : request.method === 'POST' ? /^(budget|revoke-budget|plan|approve|reconcile|cancel|transcribe)$/ : /$a/;
        if (!path.startsWith('/mobile/v3/') || !allowed.test(action)) throw new LabError(404, 'not_found', 'Mobile endpoint not found.');
        if ((action === 'plan' && config.mode !== 'fixtures' && request.headers['x-cosigna-hosted-consent'] !== provider.planningConsent)
            || (action === 'transcribe' && request.headers['x-cosigna-hosted-consent'] !== provider.voiceConsent)) {
          throw new LabError(403, 'consent_required', 'Allow this hosted processing before sending your request.');
        }
        path = `/api/${action}`;
      }
      if (request.method === 'POST' && path === '/stripe/webhook') {
        if (store.paymentMode !== 'stripe-test' || !config.webhookSecret) throw new LabError(503, 'webhook_unconfigured', 'Stripe test webhooks are not configured.');
        const event = verifyStripeEvent(await readBody(request, 64 * 1024), request.headers['stripe-signature'], config.webhookSecret);
        json(response, 200, { received: await store.webhook(event) }); return;
      }
      if (path.startsWith('/merchant/')) {
        if (request.headers.origin || !equal(request.headers.authorization, `Bearer ${store.token}`)) throw new LabError(403, 'forbidden', 'Merchant authentication required.');
        if (path !== '/merchant/v1/info' && request.headers['x-cosigna-merchant-instance'] !== store.instance) {
          throw new LabError(409, 'merchant_instance_changed', 'Merchant identity does not match.');
        }
        let data;
        if (request.method === 'GET' && path === '/merchant/v1/info') data = { instance_id: store.instance, mode: 'test' };
        else if (request.method === 'GET' && path === '/merchant/v1/catalog') data = store.catalog();
        else if (request.method === 'GET' && /^\/merchant\/v1\/attempts\/[A-Za-z0-9_]{1,128}$/.test(path)) data = await store.lookup(path.split('/').at(-1));
        else if (request.method === 'POST' && /^\/merchant\/v1\/attempts\/[A-Za-z0-9_]{1,128}\/void$/.test(path)) data = await store.void(path.split('/').at(-2), await readJson(request));
        else if (request.method === 'POST' && path === '/merchant/v1/orders') {
          const input = await readJson(request);
          data = await store.submit(input);
          // Drop the response only after the store transaction has returned/committed.
          if (input.scenario === 'timeout_after_accept' && data.status === 'observed') { request.socket.destroy(); return; }
        } else throw new LabError(404, 'not_found', 'Merchant endpoint not found.');
        json(response, 200, { schema: SCHEMA, data }); return;
      }
      if (request.headers.origin && request.headers.origin !== origin) throw new LabError(403, 'forbidden', 'Cross-origin requests are not allowed.');
      if (request.headers['sec-fetch-site'] === 'cross-site') throw new LabError(403, 'forbidden', 'Cross-site requests are not allowed.');
      const assets = { '/': ['index.html', 'text/html; charset=utf-8'], '/app.js': ['app.js', 'text/javascript; charset=utf-8'], '/styles.css': ['styles.css', 'text/css; charset=utf-8'] };
      if (request.method === 'GET' && Object.hasOwn(assets, path)) {
        const [name, type] = assets[path];
        if (path === '/') response.setHeader('Set-Cookie', `${cookieName}=${session}; HttpOnly; SameSite=Strict; Path=/`);
        response.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-store' });
        response.end(await readFile(fileURLToPath(new URL(name, PUBLIC)))); return;
      }
      const cookie = request.headers.cookie?.split(';').map(value => value.trim()).find(value => value.startsWith(`${cookieName}=`))?.slice(cookieName.length + 1);
      if (!native && !equal(cookie, session)) throw new LabError(403, 'forbidden', 'Open the lab page first.');
      if (!native && request.method === 'POST' && (!equal(request.headers['x-cosigna-csrf'], csrf) || request.headers.origin !== origin)) {
        throw new LabError(403, 'forbidden', 'Refresh the lab page before trying again.');
      }
      if (!config.credentials) throw new LabError(503, 'starting', 'Cosigna is starting. Refresh in a moment.');
      const agent = async (role, method, path, body) => {
        let result;
        try {
          result = await fetch(`${config.agentUrl}${path}`, { method,
            headers: { Authorization: `Bearer ${config.credentials[role]}`, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
            body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(45_000), redirect: 'error' });
        } catch { throw new LabError(503, 'agent_unavailable', 'Cosigna is unavailable. Refresh task status before attempting another order.'); }
        let value;
        try { value = await result.json(); } catch { throw new LabError(502, 'agent_invalid', 'Cosigna returned an invalid response.'); }
        if (!result.ok) throw new LabError(result.status, value.error?.code ?? 'agent_error', value.error?.message ?? 'Cosigna could not complete the request.');
        if (value.schema !== 'cosigna.agent/v1') throw new LabError(502, 'agent_invalid', 'Unsupported Cosigna API version.');
        return value;
      };
      const requireTask = id => {
        if (!isTaskId(id) || !store.ownsTask(id)) throw new LabError(404, 'not_found', 'Task not found in this lab.');
      };
      let data;
      if (!native && request.method === 'POST' && path === '/api/device-pairing') {
        object(await readJson(request), []);
        if (!config.mobile) throw new LabError(404, 'not_found', 'Start the mobile lab first.');
        data = mobileAccess.issue();
      } else if (!native && request.method === 'POST' && path === '/api/revoke-devices') {
        object(await readJson(request), []); mobileAccess.revoke(); data = { revoked: true };
      } else if (request.method === 'GET' && path === '/api/state') {
        const budgetId = store.get('budget_id');
        const balance = budgetId ? (await agent('owner', 'GET', `/v1/budgets/${budgetId}`)).data : null;
        data = { csrf, mode: config.mode, ai_provider: provider.id, voice_available: Boolean(config.apiKey?.trim()), simulated_payment: true,
          products: store.products(), orders: store.orders(), recent_tasks: store.tasks(), balance };
        if (store.paymentMode === 'stripe-test') {
          data.payment_mode = 'stripe-test'; data.payments = store.payments(); data.webhooks_configured = Boolean(config.webhookSecret);
        }
        if (native) {
          delete data.csrf;
          data.payment_mode = store.paymentMode;
          data.privacy = { audio: `Sent to ${provider.label} only with your permission; this server does not save the recording.`,
            planning: config.mode !== 'fixtures' ? `Request and test catalog are sent to ${provider.label} with your permission.` : 'Fixture script; no request is sent to a model.',
            retention: 'Submitted text, quotes and payment records remain in the local test ledger. Disconnecting does not delete them.',
            access: 'This phone can view and manage this single-owner lab. No location, contacts or voiceprints are requested.' };
        }
      } else if (request.method === 'GET' && /^\/api\/tasks\/task_[a-f0-9]{64}$/.test(path)) {
        const id = path.split('/').at(-1); requireTask(id);
        data = await agent('planner', 'GET', `/v1/tasks/${id}`);
      } else if (request.method === 'POST' && path === '/api/budget') {
        const body = await readJson(request); object(body, ['limit_minor', 'per_purchase_minor']);
        if (!Number.isSafeInteger(body.limit_minor) || body.limit_minor < 1 || body.limit_minor > 100_000
            || !Number.isSafeInteger(body.per_purchase_minor) || body.per_purchase_minor < 1 || body.per_purchase_minor > body.limit_minor) {
          throw new LabError(400, 'invalid_budget', 'Use a positive test budget up to $1,000 and a per-order cap within it.');
        }
        const existing = store.get('budget_id');
        if (existing) {
          const old = (await agent('owner', 'GET', `/v1/budgets/${existing}`)).data.budget;
          if (!old.revoked && old.terms.expires_at > seconds()) throw new LabError(409, 'budget_active', 'This test budget is already active. Continue using it until it expires.');
        }
        data = await agent('owner', 'POST', '/v1/budgets', { ...body, currency: 'USD', merchants: ['cosigna-test-cafe'], expires_at: seconds() + 3600 });
        store.set('budget_id', data.data.id);
      } else if (native && request.method === 'POST' && path === '/api/revoke-budget') {
        object(await readJson(request), []);
        const id = store.get('budget_id');
        if (!id) throw new LabError(409, 'budget_required', 'No test allowance exists.');
        data = await agent('owner', 'POST', `/v1/budgets/${id}/revoke`, {});
      } else if (request.method === 'POST' && path === '/api/plan') {
        const body = await readJson(request); object(body, ['request', 'idempotency_key']);
        if (typeof body.request !== 'string' || !body.request.trim() || body.request.length > 4096
            || typeof body.idempotency_key !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(body.idempotency_key)) throw new LabError(400, 'invalid_request', 'Enter a short request.');
        const budgetId = store.get('budget_id');
        if (!budgetId) throw new LabError(409, 'budget_required', 'Set your test budget first.');
        const task = await agent('planner', 'POST', '/v1/tasks', { budget_id: budgetId, request: body.request,
          idempotency_key: body.idempotency_key, required_features: [] });
        store.rememberTask(task.data.id, body.idempotency_key);
        data = await agent('planner', 'POST', `/v1/tasks/${task.data.id}/plan`, {});
      } else if (request.method === 'POST' && path === '/api/approve') {
        const body = await readJson(request); object(body, ['task_id', 'quote_digest']); requireTask(body.task_id);
        if (typeof body.quote_digest !== 'string' || !/^[a-f0-9]{64}$/.test(body.quote_digest)) throw new LabError(400, 'invalid_request', 'Review a fresh quote first.');
        // The browser supplies an explicit owner click. The model never calls this route.
        const current = (await agent('owner', 'GET', `/v1/tasks/${body.task_id}`)).data;
        if (['submitting', 'unknown', 'confirmed', 'declined'].includes(current.stage)) {
          data = await agent('owner', 'GET', `/v1/tasks/${body.task_id}`);
        } else {
          await agent('owner', 'POST', `/v1/tasks/${body.task_id}/approve`, { quote_digest: body.quote_digest });
          data = await agent('executor', 'POST', `/v1/tasks/${body.task_id}/execute`, {});
        }
      } else if (request.method === 'POST' && path === '/api/reconcile') {
        const body = await readJson(request); object(body, ['task_id']); requireTask(body.task_id);
        data = await agent('planner', 'POST', `/v1/tasks/${body.task_id}/reconcile`, {});
      } else if (request.method === 'POST' && path === '/api/cancel') {
        const body = await readJson(request); object(body, ['task_id']); requireTask(body.task_id);
        data = await agent('owner', 'POST', `/v1/tasks/${body.task_id}/cancel`, {});
      } else if (request.method === 'POST' && path === '/api/fulfill') {
        const body = await readJson(request); object(body, ['order_id', 'state']);
        if (typeof body.order_id !== 'string' || body.order_id.length > 100) throw new LabError(400, 'invalid_request', 'Invalid order ID.');
        store.fulfill(body.order_id, body.state); data = { orders: store.orders() };
      } else if (request.method === 'POST' && path === '/api/transcribe') {
        const bytes = await readBody(request, 8 * 1024 * 1024);
        const text = await (config.transcriber ?? transcribe)(bytes, request.headers['content-type'], { apiKey: config.apiKey, provider: provider.id });
        data = { text };
      } else throw new LabError(404, 'not_found', 'Endpoint not found.');
      json(response, 200, { schema: native ? 'cosigna.mobile-lab/v3' : 'cosigna.voice-lab/v3', data });
    } catch (error) {
      const known = error instanceof LabError;
      json(response, known ? error.status : 500, { schema: native ? 'cosigna.mobile-lab/v3' : 'cosigna.voice-lab/v3', error: {
        code: known ? error.code : 'internal_error', message: known ? error.message : 'The lab could not complete the request.' } });
    }
  });
  server.requestTimeout = 70_000;
  server.headersTimeout = 5000;
  server.on('clientError', (_error, socket) => socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n'));
  return server;
}
