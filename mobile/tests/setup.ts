import { AccessibilityInfo } from 'react-native';
import mockSafeAreaContext from 'react-native-safe-area-context/jest/mock';

jest.mock('react-native-safe-area-context', () => mockSafeAreaContext);
jest.mock('../src/domain/device-session', () => ({ loadDeviceSession: jest.fn().mockResolvedValue(null), saveDeviceSession: jest.fn().mockResolvedValue(undefined) }));
beforeEach(() => {
  const load = require('../src/domain/device-session').loadDeviceSession;
  if (jest.isMockFunction(load)) load.mockResolvedValue(null);
  jest.spyOn(AccessibilityInfo, 'isReduceMotionEnabled').mockResolvedValue(true);
  jest.spyOn(globalThis, 'fetch').mockImplementation(() => { throw new Error('Fixture screens must not call the network.'); });
});
afterEach(() => { jest.restoreAllMocks(); });
