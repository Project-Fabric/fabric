import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { cp, mkdtemp, rm, symlink } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

// A real development boot with no preconfigured console user or mode. Use a
// temporary checkout so a user's running dev server and .next cache are untouched.
const source = fileURLToPath(new URL('..', import.meta.url));
const tempRoot = path.resolve(os.tmpdir());
const workspace = await mkdtemp(path.join(tempRoot, 'cosigna-dev-startup-'));
let child, lines;
try {
  for (const entry of ['src', 'server', 'fixtures', 'server.mjs', 'package.json', 'next.config.mjs',
    'tsconfig.json', 'next-env.d.ts', 'postcss.config.mjs', 'tailwind.config.ts'])
    await cp(path.join(source, entry), path.join(workspace, entry), { recursive: true });
  await symlink(path.join(source, 'node_modules'), path.join(workspace, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');
  const reservation = net.createServer();
  reservation.listen(0, '127.0.0.1'); await once(reservation, 'listening');
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  const env = { ...process.env, NODE_ENV: 'development', NEXT_TELEMETRY_DISABLED: '1' };
  for (const name of Object.keys(env)) if (name.startsWith('CONSOLE_')) delete env[name];
  env.CONSOLE_PORT = String(port);
  // If fixture mode is lost, any accidental engine read must fail the test.
  env.CONSOLE_FABRIC = path.join(workspace, 'no-engine-executable');
  child = spawn(process.execPath, ['server.mjs', '--dev', '--fixtures'], {
    cwd: workspace, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  });
  child.stderr.resume(); // Consume framework output without logging access credentials.
  lines = readline.createInterface({ input: child.stdout });
  const origin = `http://127.0.0.1:${port}`;
  let banner = '';
  const access = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Development server startup timed out.')), 120000);
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('exit', () => { clearTimeout(timer); reject(new Error('Development server exited during startup.')); });
    lines.on('line', line => {
      if (line.startsWith('Cosigna Console')) banner = line;
      if (line.startsWith('Open once')) {
        clearTimeout(timer);
        const match = line.match(/http:\/\/127\.0\.0\.1:\d+\/\?token=[a-f0-9]{64}$/);
        if (!match) reject(new Error('Development server printed an invalid access URL.'));
        else resolve(match[0]);
      }
    });
  });
  assert.ok(banner.includes('priya@northwind.test'), 'The default fixture user must survive development startup.');
  assert.equal(new URL(access).origin, origin);
  const request = (url, options = {}) => fetch(url, { ...options, signal: AbortSignal.timeout(120000) });
  assert.equal((await request(origin + '/tasks')).status, 401);
  const login = await request(access, { redirect: 'manual' });
  assert.equal(login.status, 303, 'The development server must issue a session.');
  const issued = login.headers.get('set-cookie');
  assert.match(issued, /HttpOnly/); assert.match(issued, /SameSite=Strict/);
  const cookie = issued.split(';')[0];
  assert.equal((await request(access, { redirect: 'manual' })).status, 403);
  const api = origin + '/api/console';
  const response = await request(api, { headers: { cookie } });
  assert.equal(response.status, 200, 'Compiled routes must accept the same session key.');
  const snapshot = await response.json();
  assert.equal(snapshot.fixtures, true); assert.equal(snapshot.user, 'priya@northwind.test');
  assert.deepEqual(snapshot.errors, {});
  const pending = snapshot.approvals.find(item => item.status === 'pending');
  const action = { action: 'approve', id: pending.id, digest: pending.digest, decision: 'reject', note: 'Development startup regression' };
  const headers = { cookie, origin, 'Content-Type': 'application/json', 'X-CSRF-Token': snapshot.csrf };
  assert.equal((await request(api, { method: 'POST', headers: { ...headers, origin: 'https://foreign.example' }, body: JSON.stringify(action) })).status, 403);
  const decision = await request(api, { method: 'POST', headers, body: JSON.stringify(action) });
  assert.equal(decision.status, 200); assert.equal((await decision.json()).simulated, true);
  assert.equal((await request(origin + '/tasks', { headers: { cookie } })).status, 200);
  assert.equal((await request(api, { headers: { cookie } })).status, 200, 'Session must survive subsequent page compilation.');
  console.log('Development startup passed: valid access URL, fixture defaults, one-time login, compiled API/session, CSRF, simulated decision and Tasks page.');
  if (process.argv.includes('--production')) {
    const exited = once(child, 'exit'); child.kill(); await exited; lines.close();
    const run = async (args, commandEnv) => {
      child = spawn(process.execPath, args, { cwd: workspace, env: commandEnv,
        stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
      child.stdout.resume(); child.stderr.resume();
      const [code] = await once(child, 'exit');
      assert.equal(code, 0, 'Isolated production verification failed.');
    };
    const productionEnv = { ...env, NODE_ENV: 'production' };
    await run([path.join(source, 'node_modules/next/dist/bin/next'), 'build'], productionEnv);
    await run([path.join(source, 'scripts/check-fixture-flow.mjs')], productionEnv);
    console.log('Isolated production build and both fixture HTTP approval flows passed.');
  }
} finally {
  lines?.close();
  if (child && child.exitCode === null) {
    const exited = once(child, 'exit'); child.kill(); await exited;
  }
  // Remove only the temporary checkout we created, never the linked dependencies.
  assert.equal(path.dirname(workspace), tempRoot);
  assert.ok(path.basename(workspace).startsWith('cosigna-dev-startup-'));
  await rm(path.join(workspace, 'node_modules'), { force: true, recursive: true });
  await rm(workspace, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
