import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { once } from 'node:events';
import { request } from 'node:http';

test('Cosigna pages and linked assets are served through the local allowlist', async () => {
  const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = listener.address().port; await new Promise(resolve => listener.close(resolve));
  const child = spawn(process.execPath, ['server.mjs'], {
    cwd: new URL('..', import.meta.url),
    env: { SITE_PORT: String(port), ...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}) },
    stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  });
  const exited = new Promise(resolve => { child.once('error', resolve); child.once('close', resolve); });
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Local site did not become ready.')), 10_000);
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.stdout.on('data', bytes => { if (bytes.toString().includes('Cosigna landing preview:')) { clearTimeout(timer); resolve(); } });
      child.stderr.resume();
    });
    const base = `http://127.0.0.1:${port}`, assets = new Set();
    for (const route of ['/', '/about', '/architecture', '/benchmarks']) {
      const response = await fetch(base + route, { signal: AbortSignal.timeout(3000) });
      assert.equal(response.status, 200);
      assert.match(response.headers.get('content-security-policy'), /frame-ancestors 'none'/);
      const html = await response.text();
      assert.match(html, /<title>[^<]*Cosigna[^<]*<\/title>/);
      assert.doesNotMatch(html, /\bAgentAuth\b|\bFabric\b/);
      for (const match of html.matchAll(/(?:src|href)="(\/[^"#]*)"/g)) assets.add(match[1]);
    }
    for (const route of assets) {
      const response = await fetch(base + route, { signal: AbortSignal.timeout(3000) });
      assert.equal(response.status, 200, `Broken local link or asset: ${route}`);
    }
    for (const route of ['/.env', '/credentials.json', '/README.md', '/docs/COSIGNA_UNIFICATION.md']) {
      assert.equal((await fetch(base + route)).status, 404);
    }
    assert.equal((await fetch(base, { method: 'POST' })).status, 405);
    // Fetch normalizes its Host header; use the raw HTTP client for rebinding coverage.
    const wrongHost = await new Promise((resolve, reject) => {
      const req = request(base, { headers: { Host: 'untrusted.example' }, timeout: 3000 }, res => { res.resume(); resolve(res.statusCode); });
      req.once('error', reject); req.once('timeout', () => req.destroy(new Error('Host-check timeout'))); req.end();
    });
    assert.equal(wrongHost, 403);
  } finally {
    child.kill(); await exited;
  }
});
