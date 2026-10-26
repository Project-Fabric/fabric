// Named-approver sign-in. An approver signs in with their own gateway credential; the console
// keeps it only in this process's memory for that session (never on disk, never in the
// browser) and uses it for that session's gateway calls. Restarting the console ends every
// approver session. Shared through globalThis so the HTTP wrapper and the compiled route
// handlers see one store.
const shared = globalThis;
const sessions = (shared.cosignaApproverSessions ??= new Map());
const failures = (shared.cosignaApproverFailures ??= []);
const KEY = /^approverkey_[0-9a-f]{64}$/;
const WINDOW = 60_000, MAX_FAILURES = 5;

export function remember(sessionId, approver) { sessions.set(sessionId, approver); }
export function recall(sessionId) { return sessions.get(sessionId) ?? null; }
export function forget(sessionId) { sessions.delete(sessionId); }

/** At most five failed sign-ins per minute for the whole console, so keys cannot be guessed online. */
export function signInAllowed(now = Date.now()) {
  while (failures.length && failures[0] < now - WINDOW) failures.shift();
  return failures.length < MAX_FAILURES;
}
export function signInFailed(now = Date.now()) { failures.push(now); }

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
