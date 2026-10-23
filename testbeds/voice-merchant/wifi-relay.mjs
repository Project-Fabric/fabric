import { createServer } from 'node:http';
import { isIP } from 'node:net';

export function privateIPv4(value) {
  if (isIP(value) !== 4) return false;
  const [a, b] = value.split('.').map(Number);
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
}
const number = value => value.split('.').reduce((result, octet) => ((result << 8) | Number(octet)) >>> 0, 0);
function sameSubnet(remote, host, netmask) {
  remote = remote?.replace(/^::ffff:/, '');
  return remote === '127.0.0.1' || (isIP(remote ?? '') === 4 && (number(remote) & number(netmask)) === (number(host) & number(netmask)));
}
function json(response, status, code, message) {
  response.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
  response.end(JSON.stringify({ schema: 'cosigna.mobile-lab/v3', error: { code, message } }));
}

// Opt-in trusted-LAN transport for the existing paired sandbox API. No browser,
// merchant staff, Rust, credential-management or arbitrary proxy routes exist.
export function createWifiRelay({ host, netmask, transport = fetch }) {
  if (!privateIPv4(host) || isIP(netmask) !== 4 || number(netmask) < number('255.255.0.0')) {
    throw new Error('Use a private IPv4 interface with a /16 or smaller local subnet.');
  }
  const server = createServer(async (request, response) => {
    const address = `${host}:${server.address().port}`;
    try {
      if (request.headers.host !== address || !sameSubnet(request.socket.remoteAddress, host, netmask)) {
        json(response, 403, 'forbidden', 'Use the configured local Wi-Fi address.'); return;
      }
      if (request.method === 'GET' && request.url === '/health') {
        response.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
        response.end(JSON.stringify({ schema: 'cosigna.wifi-relay/v1', transport: 'trusted-lan', payment_mode: 'stripe-test' })); return;
      }
      const read = request.method === 'GET' && /^\/mobile\/v3\/(state|tasks\/task_[a-f0-9]{64})$/.test(request.url);
      const write = request.method === 'POST' && /^\/mobile\/v3\/(pair|disconnect|budget|revoke-budget|plan|approve|reconcile|cancel|transcribe)$/.test(request.url);
      if ((!read && !write) || ['origin', 'cookie', 'sec-fetch-site'].some(header => request.headers[header] !== undefined)) {
        json(response, 403, 'forbidden', 'Only the paired native sandbox API is available over Wi-Fi.'); return;
      }
      const max = request.url.endsWith('/transcribe') ? 8 * 1024 * 1024 : 16_384;
      if (Number(request.headers['content-length']) > max) { json(response, 413, 'body_too_large', 'Request body is too large.'); request.resume(); return; }
      const chunks = []; let length = 0;
      for await (const chunk of request) {
        length += chunk.length;
        if (length > max) { json(response, 413, 'body_too_large', 'Request body is too large.'); return; }
        chunks.push(chunk);
      }
      const headers = {};
      for (const name of ['authorization', 'content-type', 'x-cosigna-hosted-consent']) {
        if (typeof request.headers[name] === 'string') headers[name] = request.headers[name];
      }
      // Exactly one request to a fixed loopback destination; no redirect or retry.
      // Host is supplied by fetch, not copied from the LAN caller.
      const result = await transport(`http://127.0.0.1:3097${request.url}`, {
        method: request.method, headers, redirect: 'error', signal: AbortSignal.timeout(65_000),
        ...(request.method === 'POST' ? { body: Buffer.concat(chunks) } : {}),
      });
      const reader = result.body.getReader(); const reply = []; let size = 0;
      for (;;) {
        const { done, value } = await reader.read(); if (done) break;
        size += value.length;
        if (size > 1024 * 1024) { await reader.cancel(); throw new Error('Response too large'); }
        reply.push(value);
      }
      const bytes = Buffer.concat(reply); const envelope = JSON.parse(bytes.toString('utf8'));
      if (envelope.schema !== 'cosigna.mobile-lab/v3') throw new Error('Unexpected upstream schema');
      response.writeHead(result.status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
      response.end(bytes);
    } catch {
      if (!response.headersSent) json(response, 503, 'connection_lost', 'The local test backend is unavailable. Check task status before trying another purchase.');
      else response.end();
    }
  });
  server.headersTimeout = 10_000; server.requestTimeout = 20_000; server.maxHeadersCount = 40;
  return server;
}
