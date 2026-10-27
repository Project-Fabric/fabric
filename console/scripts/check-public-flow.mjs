import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, readFileSync, rmSync, existsSync } from 'node:fs';
import http from 'node:http';
import { tmpdir } from 'node:os';
import path from 'node:path';
import net from 'node:net';
import readline from 'node:readline';
import { DatabaseSync } from 'node:sqlite';

// Live check of the server deployment (deploy/docker): the gateway behind a TLS proxy with
// --trust-local-clients, and the production console with CONSOLE_PUBLIC_ORIGIN and no Fabric
// project. Requests carry the Host and X-Forwarded-Proto a proxy sends. Synthetic fixture rail;
// loopback only; no real money. Run after `npm run build`.
const exe = process.platform === 'win32' ? '.exe' : '';
const binary = process.env.COSIGNA_AGENT_BIN || path.resolve('..', 'target', 'debug', `cosigna-agent${exe}`);
if (!existsSync(binary)) throw new Error(`Build the gateway first (cargo build -p cosigna-agent); missing ${binary}`);
const PUBLIC = 'cosigna.example.test:8443';
const ORIGIN = `https://${PUBLIC}`;

async function freePort() {
  const reservation = net.createServer(); reservation.listen(0, '127.0.0.1'); await once(reservation, 'listening');
  const port = reservation.address().port; await new Promise(resolve => reservation.close(resolve)); return port;
}
/** One request to 127.0.0.1:port with exactly these headers (fetch cannot set Host). */
function request(port, method, route, headers = {}, body) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, method, path: route, headers: { ...headers, ...(body ? { 'content-length': Buffer.byteLength(body) } : {}) } },
      res => { let text = ''; res.on('data', c => { text += c; }); res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, text })); });
    req.on('error', reject);
    if (body) req.write(body);
    req.end();
  });
}
const dir = mkdtempSync(path.join(tmpdir(), 'cosigna-console-public-'));
const children = [];
try {
  execFileSync(binary, ['init', '--data-dir', dir], { stdio: 'ignore' });
  const db = new DatabaseSync(path.join(dir, 'fixture-provider.sqlite'));
  db.prepare('INSERT INTO fixture_payments(id,currency,amount_minor,customer) VALUES (?,?,?,?)').run('pay_live_2045', 'USD', 45000, 'Farah Haddad');
  db.close();
  const ownerToken = JSON.parse(readFileSync(path.join(dir, 'credentials.json'), 'utf8')).owner;
  const gatewayPort = await freePort();
  children.push(spawn(binary, ['serve', '--data-dir', dir, '--listen', `127.0.0.1:${gatewayPort}`,
    '--public-host', 'cosigna.example.test', '--trust-local-clients'], { stdio: 'ignore', windowsHide: true }));
  const local = { host: `127.0.0.1:${gatewayPort}` };
  const gw = async (auth, method, route, body) => {
    const response = await request(gatewayPort, method, `/gateway/v1${route}`,
      { ...local, authorization: `Bearer ${auth}`, ...(body ? { 'content-type': 'application/json' } : {}) }, body && JSON.stringify(body));
    return JSON.parse(response.text);
  };
  for (let i = 0; i < 100; i++) { try { await request(gatewayPort, 'GET', '/v1/health', local); break; } catch { await new Promise(r => setTimeout(r, 50)); } }
  const now = Math.floor(Date.now() / 1000);
  const budget = (await gw(ownerToken, 'POST', '/budgets', { currency: 'USD', limit_minor: 200000, per_purchase_minor: 100000, merchants: ['northwind'], expires_at: now + 3600 })).data.id;
  const agent = (await gw(ownerToken, 'POST', '/agents', { name: 'refunds-bot', budget_id: budget, permissions: ['execute'], expires_at: now + 1800,
    operations: { kinds: ['refund'], rails: ['fixture'], auto_approve_minor: 10000 } })).data.token;
  const held = (await gw(agent, 'POST', '/operations', { budget_id: budget, kind: 'refund', rail: 'fixture', source: 'pay_live_2045',
    amount_minor: 45000, currency: 'USD', reason: 'Public check', idempotency_key: 'public' })).data;
  assert.equal(held.stage, 'held');

  const consolePort = await freePort();
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.startsWith('CONSOLE_')) delete env[name];
  const app = spawn(process.execPath, ['server.mjs'], { cwd: process.cwd(), windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...env, CONSOLE_PORT: String(consolePort), CONSOLE_USER: 'owner@northwind.test', CONSOLE_PUBLIC_ORIGIN: ORIGIN,
      CONSOLE_GATEWAY_URL: `http://127.0.0.1:${gatewayPort}`, CONSOLE_GATEWAY_DIR: dir } });
  children.push(app);
  app.stderr.resume();
  const lines = readline.createInterface({ input: app.stdout });
  const link = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Console did not become ready.')), 20000);
    lines.on('line', line => { const url = line.match(/https:\/\/cosigna\.example\.test:8443\/\?token=[a-f0-9]+/); if (url) { clearTimeout(timer); resolve(new URL(url[0])); } });
  });
  const proxied = { host: PUBLIC, 'x-forwarded-proto': 'https' };
  const route = link.pathname + link.search;
  // Not through the proxy, or under another name: refused, and the one-time link is not used up.
  assert.equal((await request(consolePort, 'GET', route, { host: PUBLIC })).status, 403, 'Plain HTTP is refused.');
  assert.equal((await request(consolePort, 'GET', route, { host: `127.0.0.1:${consolePort}`, 'x-forwarded-proto': 'https' })).status, 403, 'The local name is refused.');
  assert.equal((await request(consolePort, 'GET', route, { host: 'evil.example.test', 'x-forwarded-proto': 'https' })).status, 403, 'Another name is refused.');
  const login = await request(consolePort, 'GET', route, proxied);
  assert.equal(login.status, 303);
  const setCookie = String(login.headers['set-cookie']);
  assert.match(setCookie, /; Secure/, 'Session cookies are Secure behind HTTPS.');
  const cookie = setCookie.split(';')[0];

  const snapshot = await request(consolePort, 'GET', '/api/console', { ...proxied, cookie });
  assert.equal(snapshot.status, 200);
  assert.ok(!snapshot.text.includes(ownerToken), 'The owner credential never reaches the browser.');
  const data = JSON.parse(snapshot.text);
  // Gateway-only: no Fabric project, so its sections are absent rather than errors.
  assert.deepEqual(Object.keys(data.errors), [], JSON.stringify(data.errors));
  const target = data.operations.find(op => op.data.id === held.id);
  assert.equal(target.data.stage, 'held'); assert.equal(target.agentName, 'refunds-bot');

  const decide = origin => request(consolePort, 'POST', '/api/console', { ...proxied, cookie, origin, 'x-csrf-token': data.csrf, 'content-type': 'application/json' },
    JSON.stringify({ action: 'operation-decide', id: held.id, decision: 'approve', digest: target.proposal_digest }));
  assert.equal((await decide(`http://127.0.0.1:${consolePort}`)).status, 403, 'The local origin is not the public origin.');
  const approved = await decide(ORIGIN);
  assert.equal(approved.status, 200, approved.text);
  assert.equal((await gw(ownerToken, 'GET', `/operations/${held.id}`)).data.stage, 'succeeded');

  // Approver sign-in works the same way and its cookie is Secure too.
  const priya = (await gw(ownerToken, 'POST', '/approvers', { name: 'Priya', currency: 'USD', max_approval_minor: 50000 })).data.token;
  const signIn = await request(consolePort, 'POST', '/signin', { ...proxied, origin: ORIGIN, 'content-type': 'application/x-www-form-urlencoded' },
    new URLSearchParams({ key: priya }).toString());
  assert.equal(signIn.status, 303);
  assert.match(String(signIn.headers['set-cookie']), /; Secure/);
  lines.close();
  for (const child of children) child.kill();
  await new Promise(r => setTimeout(r, 300));
  execFileSync(binary, ['evidence', 'verify', '--data-dir', dir], { stdio: 'ignore' });
  console.log('Public console flow passed: proxy host and HTTPS required, Secure cookies, gateway-only snapshot without errors, decision through the trusted local gateway connection, approver sign-in, evidence intact.');
} finally {
  for (const child of children) child.kill();
  await new Promise(r => setTimeout(r, 300));
  try { rmSync(dir, { recursive: true, force: true }); } catch { /* Windows may hold files briefly */ }
}
