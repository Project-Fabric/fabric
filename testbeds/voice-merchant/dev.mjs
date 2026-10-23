import { spawn } from 'node:child_process';
import { readFile, mkdir, copyFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { MerchantStore } from './store.mjs';
import { createLabServer } from './server.mjs';
import { StripeTestProcessor, loadStripeTestKey } from './stripe.mjs';
import { StripeMerchantStore } from './stripe-store.mjs';
import { providerForMode } from './ai-providers.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
export const agentBuildDirectory = join(root, 'target', 'voice-merchant-build');
export const agentBinary = join(agentBuildDirectory, 'debug', process.platform === 'win32' ? 'cosigna-agent.exe' : 'cosigna-agent');
function command(program, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { cwd: root, windowsHide: true, stdio: 'inherit', ...options });
    child.once('error', reject);
    child.once('exit', code => code === 0 ? resolve() : reject(new Error(`${program} exited unsuccessfully.`)));
  });
}

export async function launch({ directory, mode = 'openai', port = 3093, agentPort = 3094,
  apiKey = process.env[providerForMode(mode).keyEnv], build = true, transcriber, quiet = false,
  paymentMode = 'simulated', stripeKey, stripeTransport, mobile = false, webhookSecret = process.env.COSIGNA_STRIPE_WEBHOOK_SECRET } = {}) {
  if (Number(process.versions.node.split('.')[0]) < 24) throw new Error('Node.js 24 or newer is required.');
  const provider = providerForMode(mode);
  if (mode !== 'fixtures' && !apiKey?.trim()) throw new Error(`Set ${provider.keyEnv} in this terminal, or use npm run dev:fixtures for a typed, offline test.`);
  if (!directory || !Number.isInteger(port) || port < 1 || !Number.isInteger(agentPort) || agentPort < 1 || port === agentPort) throw new Error('Invalid development configuration.');
  if (build) await command('cargo', ['build', '-p', 'cosigna-agent', '--target-dir', agentBuildDirectory]);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  // Run a profile-local copy so Windows never locks the build artifact during later builds.
  const binary = join(directory, process.platform === 'win32' ? 'agent-runtime.exe' : 'agent-runtime');
  await copyFile(agentBinary, binary);
  let store;
  if (paymentMode === 'stripe-test') {
    const processor = new StripeTestProcessor(stripeKey, { transport: stripeTransport });
    await processor.checkAccess();
    store = new StripeMerchantStore(join(directory, 'merchant.sqlite'), processor);
  } else store = new MerchantStore(join(directory, 'merchant.sqlite'));
  const config = { store, mode, apiKey, transcriber, mobile, webhookSecret, agentUrl: `http://127.0.0.1:${agentPort}` };
  const server = createLabServer(config);
  let child;
  let closing = false;
  const close = async () => {
    if (closing) return; closing = true;
    if (child?.pid && child.exitCode === null) {
      const exited = new Promise(resolve => child.once('exit', resolve));
      child.kill(); await exited;
    }
    server.closeAllConnections();
    if (server.listening) await new Promise(resolve => server.close(resolve));
    store.close();
  };
  try {
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
    const agentDirectory = join(directory, 'agent');
    const env = { ...process.env, COSIGNA_TEST_MERCHANT_TOKEN: store.token };
    delete env.COSIGNA_OPENAI_API_KEY;
    delete env.COSIGNA_GROQ_API_KEY;
    if (apiKey) env[provider.keyEnv] = apiKey;
    const args = ['--data-dir', agentDirectory, '--test-merchant-url', `http://127.0.0.1:${port}`];
    await command(binary, ['init', ...args], { env, stdio: quiet ? 'ignore' : 'inherit' });
    config.credentials = JSON.parse(await readFile(join(agentDirectory, 'credentials.json'), 'utf8'));
    // This alias/profile is kept separate from the previous 3092 fixture service.
    child = spawn(binary, ['serve', ...args, '--port', String(agentPort), '--planner', mode],
      { cwd: root, windowsHide: true, env, stdio: quiet ? 'ignore' : 'inherit' });
    let startupError;
    child.once('error', error => { startupError = error; });
    child.once('exit', () => { if (!closing && !quiet) console.error('Cosigna stopped. Restart the lab; do not recreate its data.'); });
    let ready = false;
    for (let i = 0; i < 100; i++) {
      if (startupError || child.exitCode !== null) throw new Error('Cosigna could not start. Check the service port and data directory.');
      try { ready = (await fetch(`${config.agentUrl}/v1/health`, { signal: AbortSignal.timeout(500) })).ok; } catch { /* startup */ }
      if (ready) break;
      await delay(100);
    }
    if (!ready) throw new Error('Cosigna did not become ready.');
    // Verify the service actually uses this lab's role credentials, not a different process
    // that happened to own the port. Pinning also protects merchant identity across restarts.
    const check = await fetch(`${config.agentUrl}/v1/catalog`, { headers: { Authorization: `Bearer ${config.credentials.planner}` }, signal: AbortSignal.timeout(15_000) });
    if (!check.ok || !(await check.json()).data.every(offer => offer.merchant_id === 'cosigna-test-cafe')) throw new Error('The agent port belongs to a different service.');
    return { server, store, config, close, url: `http://127.0.0.1:${port}` };
  } catch (error) { await close(); throw error; }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const fixtures = process.argv.includes('--fixtures');
  const stripe = process.argv.includes('--stripe');
  const mobile = process.argv.includes('--mobile');
  const groq = process.argv.includes('--groq');
  const dataRoot = process.env.LOCALAPPDATA ?? (process.platform === 'darwin' ? join(homedir(), 'Library', 'Application Support') : join(homedir(), '.local', 'share'));
  const directory = join(dataRoot, 'Cosigna', 'voice-merchant-lab', mobile ? (stripe ? 'mobile-stripe-v1' : 'mobile-v1') : stripe ? 'stripe-v1' : 'v1');
  try {
    if (fixtures && groq) throw new Error('Choose --fixtures or --groq; these are separate planner modes.');
    const mode = fixtures ? 'fixtures' : groq ? 'groq' : 'openai';
    const provider = providerForMode(mode);
    const lab = await launch({ directory, mode, paymentMode: stripe ? 'stripe-test' : 'simulated',
      stripeKey: stripe ? await loadStripeTestKey() : undefined, mobile, port: mobile ? 3097 : stripe ? 3095 : 3093, agentPort: mobile ? 3098 : stripe ? 3096 : 3094 });
    console.log(`\nCosigna voice commerce lab: ${lab.url}\nMerchant: independent local API · payment: ${stripe ? 'STRIPE SANDBOX (no real funds)' : 'SIMULATED (no processor)'}\nPlanner: ${fixtures ? 'fixture script (not AI)' : `${provider.label} / ${provider.model}`} · voice: ${lab.config.apiKey ? `${provider.label} / ${provider.speechModel}` : 'not configured; typing available'}\nData: ${directory}\n`);
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => { await lab.close(); process.exit(0); });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
