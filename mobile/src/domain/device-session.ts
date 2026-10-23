import * as SecureStore from 'expo-secure-store';

const KEY = 'cosigna.paired-lab.v1';
export interface DeviceSession {
  schema: 'cosigna.device-session/v1'; token: string; expiresAt: number;
}
// Only the scoped test session persists, never model/payment/merchant credentials,
// task text, quote approval, or permission to record automatically.
let writes: Promise<unknown> = Promise.resolve();
export function saveDeviceSession(session: DeviceSession | null): Promise<void> {
  const next = writes.catch(() => {}).then(() => session
    ? SecureStore.setItemAsync(KEY, JSON.stringify(session), { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY })
    : SecureStore.deleteItemAsync(KEY));
  writes = next;
  return next;
}
export async function loadDeviceSession(): Promise<DeviceSession | null> {
  await writes.catch(() => {});
  const raw = await SecureStore.getItemAsync(KEY);
  if (!raw) return null;
  let value: DeviceSession;
  try { value = JSON.parse(raw); } catch { await saveDeviceSession(null); return null; }
  if (value?.schema !== 'cosigna.device-session/v1' || !/^mobile_[a-f0-9]{64}$/.test(value.token)
      || !Number.isSafeInteger(value.expiresAt) || value.expiresAt <= Date.now() / 1000) {
    await saveDeviceSession(null); return null;
  }
  return value;
}
