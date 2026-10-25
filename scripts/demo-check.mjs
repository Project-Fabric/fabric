// Pre-demo check: run before a meeting. It plays the demo's story against the real gateway in a
// temporary profile and checks every piece the demo shows, so a failure appears here and not on
// stage. Synthetic fixture rail only, loopback only; no model calls, no real money, no secret
// output. Usage, from the repository root: node scripts/demo-check.mjs
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer, connect } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(import.meta.dirname, '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const binary = join(root, 'target', 'debug', `cosigna-agent${exe}`);
const results = [];
const children = [];
let directory;

function record(ok, name, detail = '') {
  results.push({ ok, name });
  console.log(`${ok === true ? 'PASS' : ok === null ? 'NOTE' : 'FAIL'}  ${name}${detail ? ` - ${detail}` : ''}`);
}
async function check(name, run) {
  try {
    const detail = await run();
    record(true, name, typeof detail === 'string' ? detail : '');
  } catch (error) {
    record(false, name, error.message);
  }
}
function must(condition, message) {
  if (!condition) throw new Error(message);
}
const run = (command, args, options = {}) =>
  spawnSync(command, args, { cwd: root, encoding: 'utf8', windowsHide: true, ...options });
async function freePort() {
  const server = createServer().listen(0, '127.0.0.1');
  await new Promise(r => server.once('listening', r));
  const { port } = server.address();
  await new Promise(r => server.close(r));
  return port;
}
const inUse = port => new Promise(done => {
  const socket = connect(port, '127.0.0.1');
  socket.once('connect', () => { socket.destroy(); done(true); });
  socket.once('error', () => done(false));
});
async function waitFor(url, deadline = 20_000) {
  const end = Date.now() + deadline;
  while (Date.now() < end) {
    try { await fetch(url); return; } catch { await delay(100); }
  }
  throw new Error(`${url} did not start`);
}
function start(command, args, env = {}) {
  const child = spawn(command, args, { cwd: root, stdio: 'ignore', windowsHide: true, env: { ...process.env, ...env } });
  children.push(child);
  return child;
}

try {
  console.log('Cosigna pre-demo check\n');

  await check('Tools: cargo, node 22+, python with blake3', () => {
    must(run('cargo', ['--version']).status === 0, 'cargo not found');
    must(Number(process.versions.node.split('.')[0]) >= 22, `node ${process.versions.node}; need 22 or later`);
    const python = run('python', ['-I', '-c', 'import blake3']);
    must(python.status === 0, 'python with the blake3 package not found (python -m pip install blake3)');
    return `node ${process.versions.node}`;
  });

  await check('Gateway builds', () => {
    const build = run('cargo', ['build', '-q', '-p', 'cosigna-agent']);
    must(build.status === 0 && existsSync(binary), 'cargo build -p cosigna-agent failed');
  });

  // The demo's story, against the real binary over HTTP.
  directory = mkdtempSync(join(tmpdir(), 'cosigna-demo-check-'));
  must(run(binary, ['init', '--data-dir', directory]).status === 0, 'gateway init failed');
  const owner = `Bearer ${JSON.parse(readFileSync(join(directory, 'credentials.json'), 'utf8')).owner}`;
  const port = await freePort();
  start(binary, ['serve', '--data-dir', directory, '--port', String(port)]);
  const base = `http://127.0.0.1:${port}`;
  await waitFor(`${base}/v1/health`);
  const call = async (auth, method, path, body) => {
    const response = await fetch(`${base}/gateway/v1${path}`, { method,
      headers: { Authorization: auth, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined });
    return response.json();
  };
  const now = Math.floor(Date.now() / 1000);
  const budget = (await call(owner, 'POST', '/budgets', { currency: 'USD', limit_minor: 100000,
    per_purchase_minor: 50000, merchants: ['fixture-cafe'], expires_at: now + 86400 })).data.id;
  const bot = `Bearer ${(await call(owner, 'POST', '/agents', { name: 'refunds-bot', budget_id: budget,
    permissions: ['execute'], expires_at: now + 86400,
    operations: { kinds: ['refund'], rails: ['fixture'], auto_approve_minor: 10000 } })).data.token}`;
  const refund = (source, amount, key, caseId) => call(bot, 'POST', '/operations', { budget_id: budget,
    kind: 'refund', rail: 'fixture', source, amount_minor: amount, currency: 'USD',
    reason: 'Demo check', idempotency_key: key, ...(caseId ? { case_id: caseId } : {}) });
  const outcome = op => `${op.data.stage}/${op.data.decision.rule}`;
  const refunded = payment => {
    const db = new DatabaseSync(join(directory, 'fixture-provider.sqlite'), { readOnly: true });
    try { return db.prepare('SELECT refunded_minor FROM fixture_payments WHERE id=?').get(payment).refunded_minor; }
    finally { db.close(); }
  };

  await check('Routine refund within the limit pays on its own', async () => {
    const op = await refund('pay_fixture_2041', 2000, 'routine', 'case_fixture_2041');
    must(outcome(op) === 'succeeded/within_agent_auto_limit', outcome(op));
    must(refunded('pay_fixture_2041') === 2000, 'payment not refunded');
  });
  await check('Prompt-injected $640 refund is denied', async () => {
    const op = await refund('pay_fixture_2049', 64000, 'injection');
    must(outcome(op) === 'denied/over_per_operation_limit', outcome(op));
  });
  await check('Wrong amount inside the limit waits for a person (case binding)', async () => {
    const op = await refund('pay_fixture_2047', 4500, 'wrong-amount', 'case_fixture_2047');
    must(outcome(op) === 'held/exceeds_case_owed', outcome(op));
    must(refunded('pay_fixture_2047') === 0, 'money moved');
  });
  await check('Large refund is held, then paid once after exact approval', async () => {
    const held = await refund('pay_fixture_2045', 45000, 'large', 'case_fixture_2045');
    must(outcome(held) === 'held/needs_owner_approval', outcome(held));
    const forged = await call(owner, 'POST', `/operations/${held.data.id}/approve`, { proposal_digest: '0'.repeat(64) });
    must(forged.error, 'a wrong digest was accepted');
    const paid = await call(owner, 'POST', `/operations/${held.data.id}/approve`, { proposal_digest: held.proposal_digest });
    must(paid.data.stage === 'succeeded', paid.data?.stage ?? paid.error?.message);
    const again = await refund('pay_fixture_2045', 45000, 'large', 'case_fixture_2045');
    must(again.data.id === held.data.id && refunded('pay_fixture_2045') === 45000, 'a retry paid twice');
  });
  await check('Exposure report answers', async () => {
    const report = await call(owner, 'GET', '/exposure');
    must(report.schema === 'cosigna.exposure/v1', 'unexpected schema');
    return `$${(report.data.currencies[0].without_a_person_minor / 100).toFixed(2)} can move without a person`;
  });
  await check('Evidence verifies in the gateway and in the independent verifier', () => {
    must(run(binary, ['evidence', 'verify', '--data-dir', directory]).status === 0, 'gateway verify failed');
    const out = join(directory, 'evidence.jsonl');
    must(run(binary, ['evidence', 'export', '--data-dir', directory, '--out', out]).status === 0, 'export failed');
    const verify = run('python', ['-I', join(root, 'tools', 'evidence-verifier', 'verify_evidence.py'), out, '--require-hashes']);
    must(verify.status === 0, 'independent verifier found a problem');
  });

  await check('Console is built and its live gateway flow passes', () => {
    const consoleDir = join(root, 'console');
    must(existsSync(join(consoleDir, 'node_modules')), 'run: cd console && npm ci');
    must(existsSync(join(consoleDir, '.next', 'BUILD_ID')), 'run: cd console && npm run build');
    const flow = run(process.execPath, ['scripts/check-gateway-flow.mjs'], { cwd: consoleDir });
    must(flow.status === 0, 'console live flow failed (cd console && npm run test:gateway)');
  });
  await check('Landing page serves /next', async () => {
    const sitePort = await freePort();
    start(process.execPath, ['site/server.mjs'], { SITE_PORT: String(sitePort) });
    await waitFor(`http://127.0.0.1:${sitePort}/`);
    const page = await fetch(`http://127.0.0.1:${sitePort}/next`);
    must(page.status === 200, `HTTP ${page.status}`);
  });

  // Things only you can set up; reported, never printed.
  const key = process.env.COSIGNA_STRIPE_TEST_SECRET_KEY;
  if (!key) record(null, 'Stripe test key not set in this terminal (only needed for the Stripe part of the demo)');
  else record(/^(sk|rk)_test_/.test(key) ? true : false, 'Stripe key is a test-mode key', /^(sk|rk)_test_/.test(key) ? '' : 'it is not a test key: do not use it');
  for (const [port, name] of [[3092, 'gateway'], [3090, 'console'], [3091, 'landing page']])
    if (await inUse(port)) record(null, `Port ${port} (${name}) is already in use; stop the old process or pick another port`);
} catch (error) {
  record(false, 'Check stopped', error.message);
} finally {
  for (const child of children) child.kill();
  await delay(300);
  if (directory) rmSync(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
const failed = results.filter(r => r.ok === false).length;
console.log(failed ? `\n${failed} check(s) failed. Fix these before the demo.` : '\nReady for the demo.');
process.exitCode = failed ? 1 : 0;
