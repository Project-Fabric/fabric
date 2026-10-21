'use client';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import { Activity, ArrowDownLeft, ArrowLeft, ArrowUpRight, Check, ChevronRight, CircleHelp, Clock3,
  FileCheck2, Inbox, LayoutDashboard, LockKeyhole, Moon, RefreshCw, ShieldCheck, Sun, Users, X } from 'lucide-react';
import * as Dialog from '@radix-ui/react-dialog';
import type { Snapshot, ConsoleAction, ActionResult } from '@/lib/contracts';

type Context = { data: Snapshot | null; error: string; refresh: () => Promise<void>;
  act: (action: ConsoleAction) => Promise<ActionResult> };
const ConsoleContext = createContext<Context | null>(null);
export function useConsole() { const context = useContext(ConsoleContext); if (!context) throw new Error('Missing console provider.'); return context; }
export function ConsoleProvider({ children }: { children: ReactNode }) {
  const [data, setData] = useState<Snapshot | null>(null), [error, setError] = useState('');
  const refresh = useCallback(async () => {
    try {
      const response = await fetch('/api/console', { cache: 'no-store' });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || 'Could not load the console.');
      setData(result); setError('');
    } catch (err) { setError(err instanceof Error ? err.message : 'Could not connect to the local console.'); }
  }, []);
  useEffect(() => { void refresh(); const timer = setInterval(() => void refresh(), 30000); return () => clearInterval(timer); }, [refresh]);
  const act = async (action: ConsoleAction) => {
    if (!data || error) throw new Error('Refresh the console before making a decision.');
    const response = await fetch('/api/console', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': data.csrf }, body: JSON.stringify(action) });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || 'The engine refused this action.');
    await refresh(); return result as ActionResult;
  };
  return <ConsoleContext.Provider value={{ data, error, refresh, act }}>{children}</ConsoleContext.Provider>;
}
const links = [
  { href: '/', label: 'Overview', icon: LayoutDashboard }, { href: '/approvals', label: 'Approvals', icon: Inbox },
  { href: '/activity', label: 'Activity', icon: Activity }, { href: '/authority', label: 'Authority', icon: LockKeyhole },
  { href: '/agents', label: 'Agents', icon: Users }, { href: '/evidence', label: 'Evidence', icon: FileCheck2 },
];
export function Shell({ children }: { children: ReactNode }) {
  const { data, error, refresh } = useConsole(), pathname = usePathname();
  const [light, setLight] = useState(false);
  useEffect(() => { const enabled = localStorage.getItem('console-theme') === 'light'; setLight(enabled); document.documentElement.dataset.theme = enabled ? 'light' : 'dark'; }, []);
  const toggle = () => { const next = !light; setLight(next); document.documentElement.dataset.theme = next ? 'light' : 'dark'; localStorage.setItem('console-theme', next ? 'light' : 'dark'); };
  const pending = data?.approvals.filter(a => a.status === 'pending').length || 0;
  return <div className="app-shell">
    <a className="skip-link" href="#main">Skip to content</a>
    <aside className="sidebar">
      <Link className="brand" href="/approvals" aria-label="AgentAuth approvals"><span className="brand-mark"><ShieldCheck size={21} /></span>AgentAuth<span className="brand-tag">CONSOLE</span></Link>
      <div className="workspace"><span className="workspace-avatar">N</span><span><strong>{data?.fixtures ? 'Northwind Supplies' : 'Local workspace'}</strong><small>Finance operations</small></span><ChevronRight size={14} className="muted" /></div>
      <div className="nav-caption">WORKSPACE</div>
      <nav aria-label="Main navigation">{links.map(({ href, label, icon: Icon }) => {
        const active = href === '/' ? pathname === '/' : pathname.startsWith(href);
        return <Link key={href} href={href} className={`nav-item ${active ? 'active' : ''}`} aria-current={active ? 'page' : undefined}><Icon size={18} /><span>{label}</span>{href === '/approvals' && pending > 0 && <span className="nav-count">{pending}</span>}</Link>;
      })}</nav>
      <div className="sidebar-footer"><span className="status-dot" />Local connection<span className="local-lock"><LockKeyhole size={13} />Single user</span></div>
    </aside>
    <div className="main-column">
      <header className="topbar"><div className="breadcrumb">Workspace <span>/</span> <strong>{links.find(l => l.href !== '/' && pathname.startsWith(l.href))?.label || 'Overview'}</strong></div><div className="header-actions">
        {data && <span className={`mode-badge ${data.fixtures ? 'fixture' : ''}`}><span className="status-dot" />{data.fixtures ? 'Fixtures mode' : 'Live mode'}</span>}
        <button className="icon-button" onClick={toggle} aria-label={`Switch to ${light ? 'dark' : 'light'} mode`}>{light ? <Moon size={17} /> : <Sun size={17} />}</button>
        <span className="identity" title={data?.user} aria-label={`Console user: ${data?.user || 'Connecting'}`}><span className="user-avatar" aria-hidden="true">{data?.user?.slice(0, 1).toUpperCase() || '·'}</span><span>{data?.user || 'Connecting…'}</span></span>
      </div></header>
      <main id="main">{data?.fixtures && <div className="fixture-banner"><CircleHelp size={15} /><span><strong>Fixtures mode.</strong> Decisions are simulated. No engine commands run and no money moves.</span></div>}
        {error ? <div className="error-panel" role="alert"><strong>Connection needs attention</strong><p>{error}</p><button className="button secondary" onClick={() => void refresh()}>Try again</button></div>
          : data ? children : <div className="loading" role="status"><RefreshCw size={21} className="spin" />Loading your workspace…</div>}
      </main>
      <footer className="page-footer"><span><ShieldCheck size={13} /> Enforced by the engine. Decisions belong to people.</span><span>AgentAuth Console · v0.1</span></footer>
    </div>
  </div>;
}
export function PageTitle({ eyebrow, title, description, children }: { eyebrow?: string; title: string; description: string; children?: ReactNode }) {
  return <div className="page-heading"><div>{eyebrow && <div className="eyebrow">{eyebrow}</div>}<h1>{title}</h1><p>{description}</p></div><div className="heading-actions">{children}</div></div>;
}
export function Badge({ value }: { value: string }) {
  const green = ['approved', 'used', 'allow', 'ALLOW', 'ok', 'active'].includes(value);
  const red = ['rejected', 'deny', 'DENY', 'blocked', 'error', 'UNAVAILABLE', 'revoked'].includes(value);
  const amber = ['pending', 'approve', 'held', 'STEP_UP', 'waiting'].includes(value);
  return <span className={`badge ${green ? 'green' : red ? 'red' : amber ? 'amber' : ''}`}><span className="badge-dot" />{value === 'pending' ? 'Awaiting approval' : value.replace(/_/g, ' ')}</span>;
}
export function OperationIcon({ blocked = false }: { blocked?: boolean }) { return <span className={`operation-icon ${blocked ? 'blocked' : ''}`}>{blocked ? <X size={18} /> : <ArrowDownLeft size={18} />}</span>; }
export function Empty({ title, children }: { title: string; children?: ReactNode }) { return <div className="empty-state"><span className="empty-icon"><Inbox size={24} /></span><h3>{title}</h3><p>{children || 'There are no requests in this view.'}</p></div>; }
export function SectionError({ name }: { name: string }) { const { data } = useConsole(); return data?.errors[name] ? <div className="error-panel" role="alert"><strong>Could not load {name}</strong><p>{data.errors[name]}</p></div> : null; }
export function RefreshButton() { const { refresh } = useConsole(); const [busy, setBusy] = useState(false); return <button className="button secondary" disabled={busy} onClick={async () => { setBusy(true); await refresh(); setBusy(false); }}><RefreshCw size={15} className={busy ? 'spin' : ''} />Refresh</button>; }
export function DetailField({ label, children, mono = false }: { label: string; children: ReactNode; mono?: boolean }) { return <div className="detail-field"><dt>{label}</dt><dd className={mono ? 'mono' : ''}>{children}</dd></div>; }
export function JsonBlock({ value }: { value: unknown }) { return <pre className="json-block"><code>{JSON.stringify(value, null, 2)}</code></pre>; }
export function Notice({ result }: { result: ActionResult | null }) { return result && <div className="notice" role="status"><Check size={17} /><div><strong>{result.simulated ? 'Simulation complete' : 'Decision recorded'}</strong><p>{result.message}</p>{result.simulated && <details><summary>What would have run</summary><code className="command">{JSON.stringify(['fabric', ...result.args])}</code></details>}</div></div>; }
export function ConfirmDialog({ open, onOpenChange, title, description, children, confirm, busy, error, onConfirm, danger = false }: {
  open: boolean; onOpenChange: (open: boolean) => void; title: string; description: string; children?: ReactNode;
  confirm: string; busy: boolean; error: string; onConfirm: () => void; danger?: boolean;
}) {
  return <Dialog.Root open={open} onOpenChange={value => { if (!busy) onOpenChange(value); }}><Dialog.Portal><Dialog.Overlay className="modal-overlay" /><Dialog.Content className="modal" onOpenAutoFocus={event => { event.preventDefault(); const cancel = document.getElementById('confirm-cancel'); cancel?.focus(); }}>
    <span className={`confirmation-icon ${danger ? 'danger' : ''}`}>{danger ? <X size={23} /> : <ShieldCheck size={23} />}</span>
    <Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description>
    <div className="confirmation-body">{children}</div>{error && <p role="alert" className="action-error">{error}</p>}
    <div className="modal-actions"><Dialog.Close asChild><button id="confirm-cancel" className="button secondary" disabled={busy}>Cancel</button></Dialog.Close><button className={`button ${danger ? 'danger-button' : 'primary'}`} disabled={busy} onClick={onConfirm}>{busy ? 'Recording…' : confirm}</button></div>
  </Dialog.Content></Dialog.Portal></Dialog.Root>;
}
export { ArrowLeft, ArrowUpRight, Check, Clock3, ShieldCheck, X };
