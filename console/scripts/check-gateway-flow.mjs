import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import net from 'node:net';
import readline from 'node:readline';
import { DatabaseSync } from 'node:sqlite';

// Live check: a real Cosigna gateway with real held refunds, the production console in front of
// it, and decisions made over HTTP. Synthetic fixture rail only; loopback only; no real money.
const exe = process.platform === 'win32' ? '.exe' : '';
const binary = process.env.COSIGNA_AGENT_BIN || path.resolve('..', 'target', 'debug', `cosigna-agent${exe}`);
if (!existsSync(binary)) throw new Error(`Build the gateway first (cargo build -p cosigna-agent); missing ${binary}`);

async function freePort() {
  const reservation = net.createServer(); reservation.listen(0, '127.0.0.1'); await once(reservation, 'listening');
  const port = reservation.address().port; await new Promise(resolve => reservation.close(resolve)); return port;
}
const dir = mkdtempSync(path.join(tmpdir(), 'cosigna-console-gateway-'));
const children = [];
try {
  execFileSync(binary, ['init', '--data-dir', dir], { stdio: 'ignore' });
  const db = new DatabaseSync(path.join(dir, 'fixture-provider.sqlite'));
  db.prepare('INSERT INTO fixture_payments(id,currency,amount_minor,customer) VALUES (?,?,?,?)').run('pay_live_2045', 'USD', 45000, 'Farah Haddad');
  db.prepare('INSERT INTO fixture_payments(id,currency,amount_minor,customer) VALUES (?,?,?,?)').run('pay_live_2049', 'USD', 64000, 'Ben Larsen');
  db.close();
  const ownerToken = JSON.parse(readFileSync(path.join(dir, 'credentials.json'), 'utf8')).owner;
  const gatewayPort = await freePort();
  const gateway = spawn(binary, ['serve', '--data-dir', dir, '--port', String(gatewayPort)], { stdio: 'ignore', windowsHide: true });
  children.push(gateway);
  const gw = async (auth, method, route, body) => {
    const response = await fetch(`http://127.0.0.1:${gatewayPort}/gateway/v1${route}`, { method,
      headers: { Authorization: `Bearer ${auth}`, ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
    return response.json();
  };
  for (let i = 0; i < 100; i++) { try { await fetch(`http://127.0.0.1:${gatewayPort}/v1/health`); break; } catch { await new Promise(r => setTimeout(r, 50)); } }
  const now = Math.floor(Date.now() / 1000);
  const budget = (await gw(ownerToken, 'POST', '/budgets', { currency: 'USD', limit_minor: 200000, per_purchase_minor: 100000, merchants: ['northwind'], expires_at: now + 3600 })).data.id;
  const agent = (await gw(ownerToken, 'POST', '/agents', { name: 'refunds-bot', budget_id: budget, permissions: ['execute'], expires_at: now + 1800,
    operations: { kinds: ['refund'], rails: ['fixture'], auto_approve_minor: 10000 } })).data.token;
  const refund = (source, amount, key) => gw(agent, 'POST', '/operations', { budget_id: budget, kind: 'refund', rail: 'fixture', source,
    amount_minor: amount, currency: 'USD', reason: `Live check ${key}`, idempotency_key: key });
  const toApprove = (await refund('pay_live_2045', 45000, 'live-approve')).data;
  const toReject = (await refund('pay_live_2049', 64000, 'live-reject')).data;
  assert.equal(toApprove.stage, 'held'); assert.equal(toReject.stage, 'held');

  const consolePort = await freePort();
  const app = spawn(process.execPath, ['server.mjs'], { cwd: process.cwd(), windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, CONSOLE_FIXTURES: '', CONSOLE_PORT: String(consolePort), CONSOLE_USER: 'priya@northwind.test', CONSOLE_PROJECT: dir,
      CONSOLE_FABRIC: path.join(dir, 'no-engine-cli-in-this-check'), CONSOLE_GATEWAY_URL: `http://127.0.0.1:${gatewayPort}`, CONSOLE_GATEWAY_DIR: dir } });
  children.push(app);
  const lines = readline.createInterface({ input: app.stdout });
  const ready = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Console did not become ready.')), 20000);
    lines.on('line', line => { const url = line.match(/http:\/\/127\.0\.0\.1:\d+\/\?token=[a-f0-9]+/); if (url) { clearTimeout(timer); resolve(url[0]); } });
  });
  const origin = `http://127.0.0.1:${consolePort}`, api = origin + '/api/console';
  const login = await fetch(ready, { redirect: 'manual' });
  const cookie = login.headers.get('set-cookie').split(';')[0];
  const snapshot = async () => (await fetch(api, { headers: { cookie } })).json();
  const raw = await (await fetch(api, { headers: { cookie } })).text();
  assert.ok(!raw.includes(ownerToken), 'The owner credential must never reach the browser.');
  const data = JSON.parse(raw);
  assert.equal(data.gatewayConfigured, true); assert.equal(data.errors.operations, undefined);
  // The real gateway's exposure report passes the console contract: held refunds commit nothing yet.
  assert.equal(data.errors.exposure, undefined);
  assert.equal(data.exposure.currencies[0].without_a_person_minor, 200000);
  assert.equal(data.exposure.agents[0].name, 'refunds-bot'); assert.equal(data.exposure.agents[0].limited_by, 'budget');
  assert.equal(data.exposure.agents[0].if_case_required_minor, 50000);
  const held = data.operations.filter(op => op.data.stage === 'held');
  assert.equal(held.length, 2);
  const target = held.find(op => op.data.id === toApprove.id);
  assert.equal(target.amountLabel, '$450.00'); assert.equal(target.agentName, 'refunds-bot');
  const send = (action, headers = {}) => fetch(api, { method: 'POST', body: JSON.stringify(action),
    headers: { cookie, origin, 'x-csrf-token': data.csrf, 'content-type': 'application/json', ...headers } });
  const approve = { action: 'operation-decide', id: target.data.id, decision: 'approve', digest: target.proposal_digest };
  assert.equal((await send(approve, { 'x-csrf-token': 'wrong' })).status, 403, 'CSRF is enforced.');
  assert.equal((await send({ ...approve, digest: '0'.repeat(64) })).status, 409, 'A stale or tampered digest is refused.');
  const approved = await send(approve);
  assert.equal(approved.status, 200);
  assert.match((await approved.json()).message, /Recorded by the Cosigna gateway\. Approved and paid\./);
  assert.equal((await send(approve)).status, 409, 'A decided payment cannot be decided again.');
  const rejectTarget = held.find(op => op.data.id === toReject.id);
  const rejected = await send({ action: 'operation-decide', id: rejectTarget.data.id, decision: 'reject', digest: rejectTarget.proposal_digest });
  assert.equal(rejected.status, 200);
  const after = (await gw(ownerToken, 'GET', '/operations')).data;
  assert.equal(after.find(op => op.data.id === toApprove.id).data.stage, 'succeeded');
  assert.equal(after.find(op => op.data.id === toApprove.id).data.approved_by, 'owner');
  assert.equal(after.find(op => op.data.id === toReject.id).data.stage, 'rejected');
  assert.equal((await snapshot()).operations.filter(op => op.data.stage === 'held').length, 0);
  assert.equal((await snapshot()).exposure.currencies[0].without_a_person_minor, 200000 - 45000, 'Paid money leaves the bound.');
  lines.close();
  for (const child of children) child.kill();
  await new Promise(r => setTimeout(r, 300));
  execFileSync(binary, ['evidence', 'verify', '--data-dir', dir], { stdio: 'ignore' });
  console.log('Live gateway console flow passed: held payments listed, owner credential kept server-side, CSRF, stale-digest refusal, approve paid, reject recorded, no double decision, exposure reported, evidence intact.');
} finally {
  for (const child of children) child.kill();
  await new Promise(r => setTimeout(r, 300));
  try { rmSync(dir, { recursive: true, force: true }); } catch { /* Windows may hold files briefly */ }
}
