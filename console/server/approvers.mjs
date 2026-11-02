// Named-approver sign-in. An approver signs in with their own gateway credential; the console
// keeps it only in this process's memory for that session (never on disk, never in the
// browser) and uses it for that session's gateway calls. Restarting the console ends every
// approver session. Shared through globalThis so the HTTP wrapper and the compiled route
// handlers see one store.
const shared = globalThis;
const sessions = (shared.cosignaApproverSessions ??= new Map());
const owners = (shared.cosignaOwnerSessions ??= new Map());
const failures = (shared.cosignaApproverFailures ??= new Map());
const KEY = /^approverkey_[0-9a-f]{64}$/;
const WINDOW = 60_000, MAX_FAILURES = 5, MAX_FAILURES_TOTAL = 200;

export function remember(sessionId, approver) { sessions.set(sessionId, approver); }
export function recall(sessionId) { return sessions.get(sessionId) ?? null; }
/** Ends a session of either kind: sign-out, or a restart, which empties both stores. */
export function forget(sessionId) { sessions.delete(sessionId); owners.delete(sessionId); }

/** The owner's sessions also live only in this process, so signing out really ends them. */
export function rememberOwner(sessionId, expires, now = Date.now()) {
  for (const [id, until] of owners) if (until <= now) owners.delete(id);
  owners.set(sessionId, expires);
}
export function ownerActive(sessionId, now = Date.now()) { return (owners.get(sessionId) ?? 0) > now; }

/** Only a key of the right shape can be a guess at a real key. Anything else is refused without counting. */
export function wellFormed(key) { return typeof key === 'string' && KEY.test(key); }

function recent(client, now) {
  const times = failures.get(client) ?? [];
  while (times.length && times[0] < now - WINDOW) times.shift();
  if (times.length) failures.set(client, times); else failures.delete(client);
  return times;
}
function total(now) {
  let count = 0;
  for (const client of [...failures.keys()]) count += recent(client, now).length;
  return count;
}
/**
 * Online guessing limit: at most five failed sign-ins per minute from one client, with a much
 * higher ceiling for the whole console as a backstop. One client's failures do not lock out
 * other approvers.
 */
export function signInAllowed(now = Date.now(), client = 'local') {
  return recent(client, now).length < MAX_FAILURES && total(now) < MAX_FAILURES_TOTAL;
}
export function signInFailed(now = Date.now(), client = 'local') {
  const times = recent(client, now);
  times.push(now);
  failures.set(client, times);
}
/**
 * Counts an attempt as failed before it is checked, so parallel attempts cannot all pass the
 * limit while they wait for the gateway. Returns a function that undoes it on success.
 */
export function reserveSignIn(client = 'local', now = Date.now()) {
  if (!signInAllowed(now, client)) return null;
  signInFailed(now, client);
  return () => {
    const times = failures.get(client);
    const at = times?.lastIndexOf(now) ?? -1;
    if (at >= 0) times.splice(at, 1);
  };
}

/** The gateway's loopback address, as the engine requires it. */
export function gatewayBase() {
  let base;
  try { base = new URL(process.env.CONSOLE_GATEWAY_URL || ''); } catch { return null; }
  if (base.protocol !== 'http:' || base.hostname !== '127.0.0.1' || !base.port || base.username || base.password) return null;
  return base;
}

/** Asks the gateway who this key belongs to. Returns the approver, or null if it is not a valid, active key. */
export async function identify(key) {
  if (typeof key !== 'string' || !KEY.test(key)) return null;
  const base = gatewayBase();
  if (!base) return null;
  const response = await fetch(new URL('/gateway/v1/approvers/me', base), { cache: 'no-store', redirect: 'error',
    signal: AbortSignal.timeout(10_000), headers: { Authorization: `Bearer ${key}` } }).catch(() => null);
  if (!response?.ok) return null;
  const value = await response.json().catch(() => null);
  const data = value?.schema === 'cosigna.approver/v1' ? value.data : null;
  if (!data || typeof data.id !== 'string' || typeof data.name !== 'string' || data.revoked !== false
      || typeof data.currency !== 'string' || !Number.isInteger(data.max_approval_minor)) return null;
  return { key, id: data.id, name: data.name, currency: data.currency, max_approval_minor: data.max_approval_minor };
}
