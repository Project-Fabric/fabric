jest.unmock('../src/domain/device-session');
import * as SecureStore from 'expo-secure-store';
import { loadDeviceSession, saveDeviceSession, type DeviceSession } from '../src/domain/device-session';

jest.mock('expo-secure-store', () => ({
  getItemAsync: jest.fn(), setItemAsync: jest.fn().mockResolvedValue(undefined), deleteItemAsync: jest.fn().mockResolvedValue(undefined),
  WHEN_UNLOCKED_THIS_DEVICE_ONLY: 'WHEN_UNLOCKED_THIS_DEVICE_ONLY',
}));
const session: DeviceSession = { schema: 'cosigna.device-session/v1', token: `mobile_${'a'.repeat(64)}`, expiresAt: 4_000_000_000 };
describe('encrypted test device session', () => {
  it('persists only the scoped session with device-only unlocked accessibility', async () => {
    await saveDeviceSession(session);
    expect(SecureStore.setItemAsync).toHaveBeenLastCalledWith('cosigna.paired-lab.v1', JSON.stringify(session), { keychainAccessible: 'WHEN_UNLOCKED_THIS_DEVICE_ONLY' });
    jest.mocked(SecureStore.getItemAsync).mockResolvedValue(JSON.stringify(session));
    await expect(loadDeviceSession()).resolves.toEqual(session);
  });
  it.each(['bad json', JSON.stringify({ ...session, schema: 'future/v2' }), JSON.stringify({ ...session, expiresAt: 1 }), JSON.stringify({ ...session, token: 'merchant_secret' })])('clears unusable sessions without attempting to use them: %s', async raw => {
    jest.mocked(SecureStore.getItemAsync).mockResolvedValue(raw);
    await expect(loadDeviceSession()).resolves.toBeNull();
    expect(SecureStore.deleteItemAsync).toHaveBeenCalledWith('cosigna.paired-lab.v1');
  });
  it('explicit disconnect deletes the stored connection', async () => {
    await saveDeviceSession(null);
    expect(SecureStore.deleteItemAsync).toHaveBeenCalledWith('cosigna.paired-lab.v1');
  });
});
