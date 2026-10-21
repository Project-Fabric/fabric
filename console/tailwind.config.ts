import type { Config } from 'tailwindcss';
export default {
  content: ['./src/**/*.{ts,tsx}'], darkMode: 'class',
  theme: { extend: { colors: { accent: '#0071e3' } } }, plugins: [],
} satisfies Config;
