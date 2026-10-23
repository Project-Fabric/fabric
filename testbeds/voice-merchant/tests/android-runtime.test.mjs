import { test } from 'node:test';
import assert from 'node:assert/strict';
import { inspectExistingLab } from '../android-runtime.mjs';
function transport({ mode = 'groq', schema = 'cosigna.voice-lab/v3', mobile = true, payment = 'stripe-test' } = {}) {
  const requests = [];
  const call = async (url, options) => {
    requests.push({ url, options });
    if (url.endsWith('/mobile/v3/state')) return { status: mobile ? 401 : 404, json: async () => ({ schema: 'cosigna.mobile-lab/v3', error: { code: mobile ? 'device_unpaired' : 'not_found' } }) };
    if (url.endsWith('/api/state')) return { ok: true, json: async () => ({ schema, data: { mode, payment_mode: payment } }) };
    return { ok: true, headers: new Headers({ 'set-cookie': 'synthetic_session=fixture; HttpOnly' }) };
  };
  return { call, requests };
}
test('runtime only reuses a matching paired mobile sandbox and performs no mutation', async () => {
  const { call, requests } = transport();
  await inspectExistingLab('http://127.0.0.1:3097', 'groq', call);
  assert(requests.every(({ options }) => !options.method && !options.body));
  assert(!requests.some(({ url }) => url.includes('/approve') || url.includes('/device-pairing')));
});
test('runtime refuses incompatible servers rather than taking over their ports or profiles', async () => {
  for (const value of [{ mode: 'openai' }, { schema: 'other/v1' }, { mobile: false }, { payment: 'simulated' }]) {
    await assert.rejects(inspectExistingLab('http://127.0.0.1:3097', 'groq', transport(value).call));
  }
});
