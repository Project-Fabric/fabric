import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { recall } from './approvers.mjs';

export const COOKIE = 'agentauth_session';
export function equal(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string') return false;
  const left = Buffer.from(a), right = Buffer.from(b);
  return left.length === right.length && timingSafeEqual(left, right);
}
function key() {
  const value = process.env.CONSOLE_INTERNAL_KEY;
  if (!value || value.length < 64) throw new Error('Start the console with npm run dev or npm start.');
  return value;
}
/** `role` is 'owner' (the configured console user) or 'approver' (a named approver signed in with their key). */
export function issueSession(user, now = Date.now(), role = 'owner') {
  const session = { id: randomBytes(24).toString('hex'), user, role,
    csrf: randomBytes(32).toString('hex'), expires: now + 12 * 60 * 60 * 1000 };
  const payload = Buffer.from(JSON.stringify(session)).toString('base64url');
  const signature = createHmac('sha256', key()).update(payload).digest('base64url');
  return { session, cookie: `${payload}.${signature}` };
}
export function readSession(cookieHeader, now = Date.now()) {
  const cookie = (cookieHeader || '').split(';').map(x => x.trim()).find(x => x.startsWith(`${COOKIE}=`))?.slice(COOKIE.length + 1);
  if (!cookie || cookie.length > 4096) return null;
  const [payload, signature, extra] = cookie.split('.');
  if (!payload || !signature || extra) return null;
  const expected = createHmac('sha256', key()).update(payload).digest('base64url');
  if (!equal(expected, signature)) return null;
  try {
    const value = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'));
    if (typeof value.id !== 'string' || typeof value.user !== 'string' || typeof value.csrf !== 'string'
      || typeof value.expires !== 'number' || value.expires <= now) return null;
    // Sessions signed before roles existed are the owner's.
    const role = value.role ?? 'owner';
    if (role === 'owner') return value.user === process.env.CONSOLE_USER ? { ...value, role } : null;
    // An approver session lives only while this process holds its key (sign-out or restart ends it).
    if (role === 'approver') return recall(value.id)?.name === value.user ? value : null;
    return null;
  } catch { return null; }
}
export function requireRequest(request, mutation = false) {
  const session = readSession(request.headers.get('cookie'));
  if (!session) throw new Error('Session expired. Sign in again.');
  if (mutation) {
    if (request.headers.get('origin') !== process.env.CONSOLE_ORIGIN ||
        !equal(request.headers.get('x-csrf-token'), session.csrf)) throw new Error('Request rejected: session or CSRF check failed.');
  }
  return session;
}
