import test from 'node:test';
import assert from 'node:assert/strict';
import { request } from 'node:http';
import { createWifiRelay, privateIPv4 } from '../wifi-relay.mjs';

async function relayTest(run, transport = async () => new Response(JSON.stringify({ schema: 'cosigna.mobile-lab/v3', data: { ready: true } }))) {
  const server = createWifiRelay({ host: '192.168.0.181', netmask: '255.255.255.0', transport });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  const send = (path, { method = 'GET', headers = {}, body } = {}) => new Promise((resolve, reject) => {
    const req = request({ hostname: '127.0.0.1', port, path, method, headers: { Host: `192.168.0.181:${port}`, ...headers } }, res => {
      const chunks = []; res.on('data', chunk => chunks.push(chunk));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: JSON.parse(Buffer.concat(chunks)) }));
    });
    req.on('error', reject); req.end(body);
  });
  try { await run(send); } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

test('Wi-Fi relay forwards one native request with only scoped headers and fixed loopback destination', async () => {
  let calls = 0;
  await relayTest(async send => {
    const result = await send('/mobile/v3/plan', { method: 'POST', headers: { Authorization: 'Bearer synthetic-phone-session', 'Content-Type': 'application/json', 'X-Cosigna-Hosted-Consent': 'planner-groq-v1', 'X-Forwarded-Host': 'untrusted.example' }, body: '{"request":"Order one oat latte"}' });
    assert.equal(result.status, 200); assert.equal(result.body.data.ready, true);
    assert.equal(result.headers['cache-control'], 'no-store'); assert.equal(result.headers['set-cookie'], undefined);
  }, async (url, options) => {
    calls++; assert.equal(url, 'http://127.0.0.1:3097/mobile/v3/plan'); assert.equal(options.redirect, 'error');
    assert.deepEqual(Object.keys(options.headers).sort(), ['authorization', 'content-type', 'x-cosigna-hosted-consent']);
    assert.equal(options.headers.authorization, 'Bearer synthetic-phone-session');
    assert.equal(options.body.toString(), '{"request":"Order one oat latte"}');
    return new Response(JSON.stringify({ schema: 'cosigna.mobile-lab/v3', data: { ready: true } }), { headers: { 'set-cookie': 'private-staff-session' } });
  });
  assert.equal(calls, 1);
});

test('Wi-Fi relay rejects browser requests, wrong Host, staff, merchant, Rust and arbitrary routes', async () => {
  let calls = 0;
  await relayTest(async send => {
    for (const path of ['/', '/api/state', '/api/device-pairing', '/merchant/v1/orders', '/v1/tasks', '/gateway/v1/agents', '//untrusted.example', '/mobile/v3/state?token=x', '/mobile/v3/../api/state']) assert.equal((await send(path)).status, 403);
    for (const headers of [{ Origin: 'https://untrusted.example' }, { Cookie: 'staff=session' }, { 'Sec-Fetch-Site': 'none' }, { Host: 'untrusted.example' }]) assert.equal((await send('/mobile/v3/state', { headers })).status, 403);
    assert.equal((await send('/mobile/v3/transcribe')).status, 403);
  }, async () => { calls++; throw new Error('must not forward'); });
  assert.equal(calls, 0);
});

test('pairing/authentication errors survive the relay without automatic retries or sensitive errors', async () => {
  let calls = 0;
  await relayTest(async send => {
    const result = await send('/mobile/v3/state');
    assert.equal(result.status, 401); assert.equal(result.body.error.code, 'device_unpaired');
  }, async () => { calls++; return new Response(JSON.stringify({ schema: 'cosigna.mobile-lab/v3', error: { code: 'device_unpaired', message: 'Pair first.' } }), { status: 401 }); });
  assert.equal(calls, 1);
  calls = 0;
  await relayTest(async send => {
    const result = await send('/mobile/v3/approve', { method: 'POST', body: '{}' });
    assert.equal(result.status, 503); assert.equal(result.body.error.code, 'connection_lost');
    assert(!JSON.stringify(result.body).includes('synthetic-secret'));
  }, async () => { calls++; throw new Error('synthetic-secret'); });
  assert.equal(calls, 1);
});

test('relay bounds uploads and responses, rejects redirects/bad schemas and public bind addresses', async () => {
  assert(privateIPv4('10.0.0.2')); assert(!privateIPv4('8.8.8.8'));
  for (const host of ['0.0.0.0', '127.0.0.1', '8.8.8.8', 'example.test']) assert.throws(() => createWifiRelay({ host, netmask: '255.255.255.0' }), /private IPv4/);
  await relayTest(async send => assert.equal((await send('/mobile/v3/plan', { method: 'POST', body: 'x'.repeat(16_385) })).status, 413));
  await relayTest(async send => assert.equal((await send('/mobile/v3/state')).status, 503), async () => new Response(JSON.stringify({ schema: 'other/v1' })));
  await relayTest(async send => assert.equal((await send('/mobile/v3/state')).status, 503), async () => new Response('x'.repeat(1024 * 1024 + 1)));
});
