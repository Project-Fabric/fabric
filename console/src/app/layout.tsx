import type { Metadata } from 'next';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import './globals.css';
import './workspace.css';
import './tasks.css';
import { ConsoleProvider, Shell } from '@/components/console';
export const metadata: Metadata = { title: 'Cosigna Console', description: 'Spending authority and human decisions for financial agents.', robots: { index: false, follow: false } };
export default function Layout({ children }: { children: React.ReactNode }) {
  return <html lang="en" data-theme="dark"><body><ConsoleProvider><Shell>{children}</Shell></ConsoleProvider></body></html>;
}
