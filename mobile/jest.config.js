module.exports = {
  preset: 'jest-expo/android',
  testMatch: ['**/tests/**/*.test.[jt]s?(x)'],
  setupFilesAfterEnv: ['./tests/setup.ts'],
  collectCoverageFrom: ['src/domain/**/*.ts', 'src/state/**/*.tsx', 'src/components/**/*screen.tsx'],
  clearMocks: true,
  testTimeout: 15000,
};
