import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MobileAccess } from '../mobile-access.mjs';

test('pairing is one use, expires, locks after failures and replaces the previous phone', () => {
  let now = 100; const access = new MobileAccess(() => now);
  const code = access.issue(); assert.match(code.code, /^\d{6}$/); now += 300;
  assert.throws(() => access.pair(code.code), { code: 'pairing_expired' });
  const pending = access.issue();
  const wrong = ((Number(pending.code) + 1) % 1_000_000).toString().padStart(6, '0');
  for (let i = 0; i < 5; i++) assert.throws(() => access.pair(wrong), { code: 'pairing_invalid' });
  assert.throws(() => access.pair(pending.code), { code: 'pairing_expired' });
  const first = access.pair(access.issue().code);
  access.authorize(`Bearer ${first.token}`);
  assert.throws(() => access.pair(pending.code));
  const second = access.pair(access.issue().code);
  assert.throws(() => access.authorize(`Bearer ${first.token}`), { status: 401 });
  access.authorize(`Bearer ${second.token}`);
  assert.throws(() => access.authorize(`Bearer ${second.token}x`), { status: 401 });
  now += 12 * 3600;
  assert.throws(() => access.authorize(`Bearer ${second.token}`), { status: 401 });
  const third = access.pair(access.issue().code); access.revoke();
  assert.throws(() => access.authorize(`Bearer ${third.token}`), { status: 401 });
});
