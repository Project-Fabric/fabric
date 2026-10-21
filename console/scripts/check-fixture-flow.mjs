import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import net from 'node:net';
import readline from 'node:readline';

// Exercise the actual production HTTP/session boundary. All actions are fixture no-ops.
async function server() {
  const reservation = net.createServer(); reservation.listen(0, '127.0.0.1');
  await once(reservation, 'listening');
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  const child = spawn(process.execPath, ['server.mjs', '--fixtures'], {
    cwd: process.cwd(), env: { ...process.env, CONSOLE_PORT: String(port), CONSOLE_USER: 'priya@northwind.test' },
    stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  });
  const lines = readline.createInterface({ input: child.stdout });
  let ready;
  try {
    ready = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Fixture server did not become ready.')), 15000);
      child.once('exit', () => { clearTimeout(timer); reject(new Error('Fixture server exited before becoming ready.')); });
      lines.on('line', line => {
        const url = line.match(/http:\/\/127\.0\.0\.1:\d+\/\?token=[a-f0-9]+/);
        if (url) { clearTimeout(timer); resolve(url[0]); }
      });
    });
  } catch (error) { child.kill(); lines.close(); throw error; }
  return { child, lines, origin: `http://127.0.0.1:${port}`, ready };
}
async function check(decision) {
  const app = await server();
  const api = app.origin + '/api/console';
  try {
    assert.equal((await fetch(api)).status, 401, 'Anonymous API access must fail.');
    assert.equal((await fetch(app.origin + '/approvals')).status, 401, 'Anonymous pages must fail.');
    const login = await fetch(app.ready, { redirect: 'manual' });
    assert.equal(login.status, 303);
    const issued = login.headers.get('set-cookie');
    assert.match(issued, /HttpOnly/i); assert.match(issued, /SameSite=Strict/i);
    assert.equal(login.headers.get('location'), '/approvals');
    const cookie = issued.split(';')[0];
    assert.equal((await fetch(app.ready, { redirect: 'manual' })).status, 403, 'Access links must be one-time.');
    const load = async () => {
      const response = await fetch(api, { headers: { cookie } }); assert.equal(response.status, 200); return response.json();
    };
    const data = await load();
    assert.deepEqual(data.errors, {});
    const pending = data.approvals.find(request => request.status === 'pending');
    assert.equal(pending.amountLabel, '$450.00');
    const payload = { action: 'approve', id: pending.id, digest: pending.digest, decision, note: 'Verified ticket; literal $(text) & note' };
    const send = (body, extra = {}) => fetch(api, { method: 'POST', headers: {
      cookie, origin: app.origin, 'Content-Type': 'application/json', 'X-CSRF-Token': data.csrf, ...extra,
    }, body: JSON.stringify(body) });
    assert.equal((await send(payload, { 'X-CSRF-Token': '' })).status, 403);
    assert.equal((await send(payload, { origin: 'https://foreign.example' })).status, 403);
    assert.equal((await send({ ...payload, user: 'another-identity' })).status, 400);
    const response = await send(payload); assert.equal(response.status, 200);
    const receipt = await response.json(); assert.equal(receipt.simulated, true);
    assert.deepEqual(receipt.args, ['approve', '1', '--yes', '--as', data.user,
      ...(decision === 'reject' ? ['--reject'] : []), '--note', payload.note, '--json']);
    assert.equal((await load()).approvals.find(request => request.id === pending.id).status, decision === 'approve' ? 'approved' : 'rejected');
    assert.equal((await send(payload)).status, 409, 'A decided request cannot be submitted twice.');
    if (decision === 'approve') {
      const registered = await send({ action: 'agent-add', name: 'fixture-check-bot', tools: ['stripe/*'] });
      assert.equal(registered.status, 200); const registration = await registered.json();
      assert.ok(registration.token.startsWith('fixture-only-'));
      assert.ok(!JSON.stringify(await load()).includes(registration.token), 'One-time agent tokens must not reappear.');
      assert.equal((await send({ action: 'agent-revoke', name: 'fixture-check-bot' })).status, 200);
      assert.equal((await send({ action: 'mandate-grant', title: 'Fixture flow', tools: ['stripe/*'], maxPerCall: '25' })).status, 200);
      const granted = (await load()).mandates.find(m => m.terms.title === 'Fixture flow');
      assert.equal((await send({ action: 'mandate-revoke', id: granted.id })).status, 200);
      const exported = await (await send({ action: 'evidence-export' })).json();
      assert.equal(exported.simulated, true); assert.equal(exported.fingerprint, undefined);
      for (const route of ['/', '/approvals', '/approvals/1', '/activity', '/authority', '/agents', '/evidence'])
        assert.equal((await fetch(app.origin + route, { headers: { cookie } })).status, 200, route);
    }
    console.log(`Production fixture HTTP flow passed: ${decision}, session/CSRF, attribution, stale-decision refusal${decision === 'approve' ? ', agent/mandate simulations and all routes' : ''}.`);
  } finally {
    app.lines.close(); app.child.kill();
    await Promise.race([once(app.child, 'exit'), new Promise(resolve => setTimeout(resolve, 3000))]);
  }
}
await check('approve');
await check('reject');
