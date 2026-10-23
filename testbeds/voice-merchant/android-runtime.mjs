import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { join } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { launch } from './dev.mjs';
import { loadStripeTestKey } from './stripe.mjs';

// This manages one development stack, never a public/production endpoint.
export async function inspectExistingLab(url, mode, transport = fetch) {
  const page = await transport(url, { signal: AbortSignal.timeout(2_000), redirect: 'error' });
  const cookie = page.headers.get('set-cookie')?.split(';')[0];
  if (!page.ok || !cookie) throw new Error('Port 3097 belongs to an incompatible service. Leave it running and resolve the port conflict.');
  const response = await transport(`${url}/api/state`, { headers: { Cookie: cookie }, signal: AbortSignal.timeout(5_000), redirect: 'error' });
  const value = await response.json();
  if (!response.ok || value.schema !== 'cosigna.voice-lab/v3' || value.data?.mode !== mode
      || value.data?.payment_mode !== 'stripe-test') throw new Error('The existing lab has a different mode or is not ready. Stop it in its own terminal before switching modes.');
  // The endpoint must also be mobile-enabled, not just an unrelated browser lab.
  const mobile = await transport(`${url}/mobile/v3/state`, { signal: AbortSignal.timeout(2_000), redirect: 'error' });
  const boundary = await mobile.json();
  if (mobile.status !== 401 || boundary.schema !== 'cosigna.mobile-lab/v3' || boundary.error?.code !== 'device_unpaired') {
    throw new Error('The service on 3097 does not expose the expected paired mobile API.');
  }
}

export async function runAndroidRuntime({ adb, device, fixtures = false }) {
  if (!adb || !device || !/^[A-Za-z0-9_.:-]+$/.test(device)) throw new Error('Pass a valid ADB executable and device serial.');
  const url = 'http://127.0.0.1:3097', mode = fixtures ? 'fixtures' : 'groq';
  let ownedLab;
  // Probe connection separately. A reachable incompatible service must never be replaced.
  let reachable = false;
  try { await fetch(url, { signal: AbortSignal.timeout(2_000) }); reachable = true; } catch { /* no lab */ }
  if (reachable) { await inspectExistingLab(url, mode); console.log('Reusing the running Cosigna test backend.'); }
  else {
    const dataRoot = process.env.LOCALAPPDATA ?? join(homedir(), '.local', 'share');
    ownedLab = await launch({ directory: join(dataRoot, 'Cosigna', 'voice-merchant-lab', 'mobile-stripe-v1'), mode,
      mobile: true, paymentMode: 'stripe-test', stripeKey: await loadStripeTestKey(), port: 3097, agentPort: 3098 });
    console.log('Cosigna test backend started: voice gateway, Rust transaction service and Stripe sandbox.');
  }
  const execute = promisify(execFile);
  const close = async () => { if (ownedLab) await ownedLab.close(); };
  try {
    await execute(adb, ['-s', device, 'shell', 'am', 'start', '-a', 'android.intent.action.VIEW', '-d',
      'cosigna-preview://order-lab', '-n', 'com.cosigna.preview/.MainActivity'], { windowsHide: true });
    // Open the private desktop pairing page without putting its session/code into logs.
    if (process.platform === 'win32') await execute('powershell.exe', ['-NoProfile', '-Command', `Start-Process '${url}/'`], { windowsHide: true });
    console.log(`Cosigna is open on ${device}. UI is bundled in the installed APK; no Metro or Expo Go needed.`);
    console.log('Pair once from the desktop page if needed. Your scoped phone session is saved encrypted until expiry/revocation.');
    console.log('Test merchant and Stripe sandbox only. Google Assistant account registration/preview is a separate required setup.');
    if (!ownedLab) return; // Never assume lifecycle ownership of an existing user's server.
    console.log('This window manages the test backend. Ctrl+C stops only the backend started here.');
    await new Promise(resolve => {
      const stop = () => { process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop); resolve(); };
      process.once('SIGINT', stop); process.once('SIGTERM', stop);
    });
  } finally { await close(); }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2), value = name => args[args.indexOf(name) + 1];
  try { await runAndroidRuntime({ adb: args.includes('--adb') ? value('--adb') : null,
    device: args.includes('--device') ? value('--device') : null, fixtures: args.includes('--fixtures') }); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
