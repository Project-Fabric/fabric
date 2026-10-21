import test from 'node:test';
import assert from 'node:assert/strict';
import { COOKIE, issueSession, readSession, requireRequest } from '../server/security.mjs';
process.env.CONSOLE_INTERNAL_KEY = 'a'.repeat(128);
process.env.CONSOLE_USER = 'fixture-approver';
process.env.CONSOLE_ORIGIN = 'http://127.0.0.1:3090';
test('signed sessions expire and cannot be changed or used for another identity', () => {
  const { cookie } = issueSession('fixture-approver', 1000);
  assert.equal(readSession(`${COOKIE}=${cookie}`, 2000).user, 'fixture-approver');
  assert.equal(readSession(`${COOKIE}=${cookie}`, 1000 + 12 * 3600000 + 1), null);
  assert.equal(readSession(`${COOKIE}=${cookie}x`, 2000), null);
  assert.equal(readSession(`${COOKIE}=${issueSession('other-user').cookie}`), null);
});
test('mutations require the session, exact origin and per-session CSRF token', () => {
  const { cookie, session } = issueSession('fixture-approver');
  const request = headers => new Request('http://127.0.0.1:3090/api/console', { method: 'POST', headers });
  const valid = { cookie: `${COOKIE}=${cookie}`, origin: process.env.CONSOLE_ORIGIN, 'x-csrf-token': session.csrf };
  assert.equal(requireRequest(request(valid), true).user, 'fixture-approver');
  for (const invalid of [{ ...valid, cookie: '' }, { ...valid, origin: 'https://foreign.example' },
    { ...valid, 'x-csrf-token': 'wrong' }, { ...valid, origin: '' }]) assert.throws(() => requireRequest(request(invalid), true));
});
