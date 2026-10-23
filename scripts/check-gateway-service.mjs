// Two real processes, temporary fixture data, no model/processor calls or secret output.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { createServer } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';

const binary = resolve(process.argv[2] ?? `target/debug/cosigna-agent${process.platform === 'win32' ? '.exe' : ''}`);
const directory = await mkdtemp(join(tmpdir(), 'cosigna-service-check-'));
const children = new Set();
function start(args) {
  const child = spawn(binary, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  // Drain output without logging it; never print the private credential file.
  child.stdout.resume();
  child.stderr.resume();
  const done = new Promise(resolveDone => {
    child.once('error', () => resolveDone({ error: true }));
    child.once('close', (code, signal) => resolveDone({ code, signal }));
  });
  const managed = { child, done };
  children.add(managed);
  return managed;
}
async function stop(process) {
  process.child.kill();
  await process.done;
  children.delete(process);
}
async function until(check, message) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await delay(100);
  }
  throw new Error(message);
}
try {
  const initialized = await start(['init', '--data-dir', directory]).done;
  assert.equal(initialized.code, 0, 'fixture profile initialization failed');
  const credentials = JSON.parse(await readFile(join(directory, 'credentials.json'), 'utf8'));
  const listener = createServer();
  await new Promise((ok, fail) => listener.listen(0, '127.0.0.1', ok).once('error', fail));
  const port = listener.address().port;
  await new Promise(ok => listener.close(ok));
  const base = `http://127.0.0.1:${port}`;
  const api = start(['serve', '--data-dir', directory, '--port', String(port), '--planner', 'fixtures']);
  let worker = start(['worker', '--data-dir', directory, '--planner', 'fixtures']);
  await until(async () => {
    try { return (await fetch(`${base}/v1/health`, { signal: AbortSignal.timeout(1000) })).ok; }
    catch { return false; }
  }, 'fixture API did not become reachable');
  async function call(path, body, token = credentials.owner) {
    const response = await fetch(`${base}/gateway/v1${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      signal: AbortSignal.timeout(3000),
    });
    assert.equal(response.status, 200, 'unexpected fixture gateway status');
    return response.json();
  }
  const expires = Math.floor(Date.now() / 1000) + 3600;
  const budget = (await call('/budgets', { currency: 'USD', limit_minor: 1000, per_purchase_minor: 1000, merchants: ['fixture-cafe'], expires_at: expires })).data;
  const agent = (await call('/agents', { name: 'service-check', budget_id: budget.id, permissions: ['plan', 'execute'], expires_at: expires - 60 })).data;
  const task = (await call('/tasks', { budget_id: budget.id, request: 'Order one oat latte', idempotency_key: 'service-check' }, agent.token)).data;
  const taskPath = `/pipelines/${task.id}`;
  await call(`${taskPath}/worker`, { action: 'enroll' });
  const quoted = await until(async () => {
    const view = await call(taskPath);
    return view.status === 'awaiting_approval' ? view : null;
  }, 'worker did not quote the enrolled draft');
  assert.equal((await call(`/budgets/${budget.id}`)).data.spent_minor, 0);
  // Stop at approval waiting, then commit owner approval while the worker is down.
  // The replacement process must discover that ledger transition without a new enqueue.
  await stop(worker);
  await call(`/tasks/${task.id}/approve`, { quote_digest: quoted.approval_digest });
  worker = start(['worker', '--data-dir', directory, '--planner', 'fixtures']);
  const confirmed = await until(async () => {
    const view = await call(taskPath);
    return view.status === 'confirmed' ? view : null;
  }, 'restarted worker did not execute the approved task');
  assert.equal((await call(`${taskPath}/worker`)).data.state, 'done');
  const balance = (await call(`/budgets/${budget.id}`)).data;
  assert.equal(balance.spent_minor, 700);
  assert.equal(balance.reserved_minor, 0);
  await stop(worker);
  const check = start(['worker', '--data-dir', directory, '--planner', 'fixtures', '--once']);
  assert.equal((await check.done).code, 0);
  assert.ok(confirmed.data.attempt_key);
  assert.equal((await call(taskPath)).data.attempt_key, confirmed.data.attempt_key);
  assert.equal((await call(`/budgets/${budget.id}`)).data.spent_minor, 700);
  await stop(api);
  console.log('Gateway service check passed: API + worker, explicit approval, restart discovery, confirmed fixture purchase, terminal replay. No real funds or model calls.');
} finally {
  for (const managed of children) {
    managed.child.kill();
    await managed.done;
  }
  const target = resolve(directory);
  assert.equal(dirname(target), resolve(tmpdir()));
  assert.ok(basename(target).startsWith('cosigna-service-check-'));
  await rm(target, { recursive: true, force: true });
}
