import { test } from 'node:test';
import assert from 'node:assert/strict';
import { providerForMode } from '../ai-providers.mjs';
import { launch } from '../dev.mjs';
import { agentBuildDirectory } from '../dev.mjs';
import { createServer } from 'node:net';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

test('Groq requires its own key before building or opening a development profile', async () => {
  await assert.rejects(launch({ mode: 'groq', apiKey: '', build: false }), /COSIGNA_GROQ_API_KEY/);
  assert.throws(() => providerForMode('unknown'), /Unsupported AI provider/);
  assert.equal(providerForMode('fixtures').id, 'openai');
  assert.notEqual(providerForMode('groq').voiceConsent, providerForMode('openai').voiceConsent);
  assert.notEqual(providerForMode('groq').planningConsent, providerForMode('openai').planningConsent);
});

test('Groq startup passes its own key to the real Rust planner and advertises Groq without external inference', async () => {
  const freePort = async () => {
    const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
  };
  await promisify(execFile)('cargo', ['build', '-p', 'cosigna-agent', '--target-dir', agentBuildDirectory], { cwd: new URL('../../../', import.meta.url), windowsHide: true });
  const directory = await mkdtemp(join(tmpdir(), 'cosigna-groq-startup-'));
  let lab;
  try {
    lab = await launch({ directory, mode: 'groq', apiKey: 'synthetic-groq-key-not-for-network', mobile: true,
      port: await freePort(), agentPort: await freePort(), build: false, quiet: true });
    const page = await fetch(lab.url); const cookie = page.headers.get('set-cookie').split(';')[0];
    const response = await fetch(`${lab.url}/api/state`, { headers: { Cookie: cookie } });
    const value = await response.json();
    assert.equal(response.status, 200); assert.equal(value.schema, 'cosigna.voice-lab/v3');
    assert.equal(value.data.mode, 'groq'); assert.equal(value.data.ai_provider, 'groq');
    assert.equal(value.data.voice_available, true);
    assert(!JSON.stringify(value).includes('synthetic-groq-key-not-for-network'));
    assert.equal((await fetch(`${lab.config.agentUrl}/v1/health`)).status, 200);
  } finally {
    if (lab) await lab.close();
    await rm(directory, { recursive: true, force: true });
  }
});
