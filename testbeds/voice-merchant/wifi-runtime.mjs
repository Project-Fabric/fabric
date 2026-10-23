import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { networkInterfaces, homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { launch } from './dev.mjs';
import { loadStripeTestKey } from './stripe.mjs';
import { inspectExistingLab } from './android-runtime.mjs';
import { createWifiRelay } from './wifi-relay.mjs';

export async function runWifiRuntime({ host }) {
  const connection = Object.values(networkInterfaces()).flat().find(value => value.address === host && value.family === 'IPv4' && !value.internal);
  if (!connection) throw new Error('Select this computer’s Wi-Fi IPv4 address.');
  const relay = createWifiRelay({ host, netmask: connection.netmask });
  let ownedLab; let metro;
  const close = async () => {
    if (metro?.exitCode === null) metro.kill();
    relay.closeAllConnections();
    if (relay.listening) await new Promise(resolve => relay.close(resolve));
    if (ownedLab) await ownedLab.close();
  };
  try {
    let reachable = false;
    try { await fetch('http://127.0.0.1:3097', { signal: AbortSignal.timeout(2000) }); reachable = true; } catch { /* no running backend */ }
    if (reachable) { await inspectExistingLab('http://127.0.0.1:3097', 'groq'); console.log('Reusing the Groq / Stripe sandbox backend.'); }
    else {
      const dataRoot = process.env.LOCALAPPDATA ?? join(homedir(), '.local', 'share');
      ownedLab = await launch({ directory: join(dataRoot, 'Cosigna', 'voice-merchant-lab', 'mobile-stripe-v1'), mode: 'groq',
        mobile: true, paymentMode: 'stripe-test', stripeKey: await loadStripeTestKey(), port: 3097, agentPort: 3098 });
    }
    await new Promise((resolve, reject) => { relay.once('error', reject); relay.listen(3099, host, resolve); });
    const mobile = fileURLToPath(new URL('../../mobile/', import.meta.url));
    const env = { ...process.env, EXPO_PUBLIC_COSIGNA_LAB_URL: `http://${host}:3099`, REACT_NATIVE_PACKAGER_HOSTNAME: host };
    for (const name of Object.keys(env)) if (/^(COSIGNA_|STRIPE_)/.test(name)) delete env[name];
    console.log(`Wi-Fi phone API: http://${host}:3099 · paired mobile routes only · Stripe sandbox`);
    console.log('Desktop pairing: http://127.0.0.1:3097 — create a fresh six-digit code there.');
    console.log('Use trusted Wi-Fi. This HTTP development transport is not a public deployment. Ctrl+C closes the Wi-Fi relay and Expo.');
    if (process.platform === 'win32') await promisify(execFile)('powershell.exe', ['-NoProfile', '-Command', "Start-Process 'http://127.0.0.1:3097/'"], { windowsHide: true });
    metro = spawn(process.execPath, [join(mobile, 'node_modules', 'expo', 'bin', 'cli'), 'start', '--go', '--lan', '--port', '8081'],
      { cwd: mobile, env, windowsHide: true, stdio: 'inherit' });
    await new Promise((resolve, reject) => {
      const stop = () => { cleanup(); resolve(); };
      const cleanup = () => { process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop); };
      process.once('SIGINT', stop); process.once('SIGTERM', stop);
      metro.once('error', error => { cleanup(); reject(error); });
      metro.once('exit', code => { cleanup(); code === 0 || code === null ? resolve() : reject(new Error('Expo stopped unsuccessfully.')); });
    });
  } finally { await close(); }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const host = process.argv[process.argv.indexOf('--host') + 1];
  try { await runWifiRuntime({ host }); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
