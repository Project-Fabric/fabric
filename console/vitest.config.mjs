import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('.', import.meta.url));
export default {
  root,
  plugins: [react()],
  resolve: { alias: {
    '@': fileURLToPath(new URL('./src', import.meta.url)),
    'server-only': fileURLToPath(new URL('./tests/server-only.ts', import.meta.url)),
  } },
  test: { environment: 'jsdom', setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.{ts,tsx}'], clearMocks: true, maxWorkers: 2, minWorkers: 1 },
};
