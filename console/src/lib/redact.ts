const sensitive = /(?:secret|token|password|api.?key|authorization|signature|private.?key)/i;
const patterns = /(?:sk|rk)_(?:test|live)_[A-Za-z0-9_]+|fat_[A-Za-z0-9_-]+|aa_live_[A-Za-z0-9_-]+|(?:owner|planner|executor|agentkey|approverkey)_[0-9a-f]{64}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g;
export function redactText(value: string): string { return value.replace(patterns, '[redacted]'); }
export function redact(value: unknown): unknown {
  if (typeof value === 'string') {
    try { return JSON.stringify(redact(JSON.parse(value))); } catch { return redactText(value); }
  }
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) =>
    [k, k !== 'authorization_code' && sensitive.test(k) ? '[redacted]' : redact(v)]));
  return value;
}
