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
test('an approver session is valid only while this console holds it, and never as the owner', async () => {
  const { remember, forget, signInAllowed, signInFailed } = await import('../server/approvers.mjs');
  const { cookie, session } = issueSession('Priya', Date.now(), 'approver');
  assert.equal(readSession(`${COOKIE}=${cookie}`), null, 'not signed in through /signin');
  remember(session.id, { key: 'approverkey_' + '0'.repeat(64), id: 'approver_x', name: 'Priya', currency: 'USD', max_approval_minor: 100 });
  const read = readSession(`${COOKIE}=${cookie}`);
  assert.equal(read.role, 'approver'); assert.equal(read.user, 'Priya');
  assert.equal(JSON.stringify(read).includes('approverkey_'), false, 'the key never travels in the cookie');
  // The signed name must match the approver this console holds for the session.
  remember(session.id, { key: 'approverkey_' + '0'.repeat(64), id: 'approver_x', name: 'Mallory', currency: 'USD', max_approval_minor: 100 });
  assert.equal(readSession(`${COOKIE}=${cookie}`), null);
  forget(session.id);
  assert.equal(readSession(`${COOKIE}=${cookie}`), null, 'sign-out or restart ends it');
  // Owner sessions are unchanged, including ones signed before roles existed.
  assert.equal(readSession(`${COOKIE}=${issueSession('fixture-approver').cookie}`).role, 'owner');
  // Online guessing is bounded: five failures a minute from one client.
  const start = 10_000_000;
  for (let i = 0; i < 5; i++) { assert.equal(signInAllowed(start + i), true); signInFailed(start + i); }
  assert.equal(signInAllowed(start + 10), false);
  assert.equal(signInAllowed(start + 61_000), true);
});
test('signing out ends the owner session on the server, not only in the browser', async () => {
  const { forget } = await import('../server/approvers.mjs');
  const { cookie, session } = issueSession('fixture-approver');
  const copied = `${COOKIE}=${cookie}`;
  assert.equal(readSession(copied).role, 'owner');
  forget(session.id);
  assert.equal(readSession(copied), null, 'a copied cookie stops working after sign-out');
  const request = new Request('http://127.0.0.1:3090/api/console', { headers: { cookie: copied } });
  assert.throws(() => requireRequest(request));
  // A session this process never issued is not accepted, even with a valid signature.
  const other = issueSession('fixture-approver');
  forget(other.session.id);
  assert.equal(readSession(`${COOKIE}=${other.cookie}`), null);
});
test('one client cannot lock every approver out of sign-in', async () => {
  const { reserveSignIn, signInAllowed, signInFailed, wellFormed } = await import('../server/approvers.mjs');
  const start = 20_000_000;
  // Malformed keys are refused without counting: they cannot be guesses at a real key.
  for (const junk of ['', 'x', 'approverkey_zz', null, 'approverkey_' + '0'.repeat(63)]) assert.equal(wellFormed(junk), false);
  assert.equal(wellFormed('approverkey_' + 'a'.repeat(64)), true);
  for (let i = 0; i < 5; i++) signInFailed(start + i, '198.51.100.9');
  assert.equal(signInAllowed(start + 10, '198.51.100.9'), false, 'the guessing client is limited');
  assert.equal(signInAllowed(start + 10, '198.51.100.10'), true, 'another approver is not');
  assert.equal(signInAllowed(start + 10, '198.51.100.11'), true);
  // Attempts in flight count at once, so parallel guesses cannot all pass; a success undoes it.
  const reserved = [];
  for (let i = 0; i < 5; i++) reserved.push(reserveSignIn('198.51.100.12', start + 20));
  assert.equal(reserveSignIn('198.51.100.12', start + 20), null);
  reserved[0]();
  assert.notEqual(reserveSignIn('198.51.100.12', start + 20), null);
});
