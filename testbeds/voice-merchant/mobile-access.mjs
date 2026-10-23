import { createHash, randomBytes, randomInt, timingSafeEqual } from 'node:crypto';
import { LabError, seconds } from './store.mjs';

const hash = value => createHash('sha256').update(value).digest();
// One trusted development phone, one local owner. Not a multi-user identity service.
// Codes and bearer tokens are never persisted; restart/revoke invalidates access.
export class MobileAccess {
  constructor(now = seconds) { this.now = now; }
  issue() {
    const code = randomInt(0, 1_000_000).toString().padStart(6, '0');
    this.pending = { hash: hash(code), expires: this.now() + 300, failures: 0 };
    return { code, expires_at: this.pending.expires };
  }
  pair(code) {
    const pending = this.pending;
    if (!pending || pending.expires <= this.now()) {
      this.pending = null;
      throw new LabError(403, 'pairing_expired', 'Create a new pairing code on the computer.');
    }
    if (typeof code !== 'string' || !/^\d{6}$/.test(code) || !timingSafeEqual(hash(code), pending.hash)) {
      if (++pending.failures >= 5) this.pending = null;
      throw new LabError(403, 'pairing_invalid', 'Pairing code is invalid. Check the code on the computer.');
    }
    this.pending = null;
    const token = `mobile_${randomBytes(32).toString('hex')}`;
    this.session = { hash: hash(token), expires: this.now() + 12 * 3600 };
    return { token, expires_at: this.session.expires };
  }
  authorize(authorization) {
    const token = authorization?.startsWith('Bearer ') ? authorization.slice(7) : '';
    if (!this.session || this.session.expires <= this.now() || !/^mobile_[a-f0-9]{64}$/.test(token)
        || !timingSafeEqual(hash(token), this.session.hash)) {
      throw new LabError(401, 'device_unpaired', 'Pair this phone again on the computer.');
    }
  }
  revoke() { this.pending = null; this.session = null; }
}
