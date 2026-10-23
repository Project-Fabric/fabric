'use client';
import Link from 'next/link';
import { ArrowUpRight, Clock3, FileCheck2, ShieldCheck } from 'lucide-react';
import { Badge, DetailField, PageTitle, RefreshButton, useConsole } from './console';
import { money, waiting } from '@/lib/display';

export function OverviewScreen() {
  const { data } = useConsole(); if (!data) return null;
  const pending = data.approvals.filter(r => r.status === 'pending').sort((a, b) => a.requested_at.localeCompare(b.requested_at));
  const today = data.now.slice(0, 10), week = new Date(data.now); week.setUTCHours(0, 0, 0, 0); week.setUTCDate(week.getUTCDate() - (week.getUTCDay() + 6) % 7);
  const totals = (since: number) => {
    const byCurrency = new Map<string, number>();
    for (const c of data.calls) if (c.payment?.decision === 'ALLOW' && Date.parse(c.at) >= since)
      byCurrency.set(c.payment.currency, (byCurrency.get(c.payment.currency) || 0) + Number(c.payment.amount));
    return [...byCurrency].map(([currency, amount]) => money(amount, currency)).join(' + ') || 'No recorded payments';
  };
  const blocked = data.calls.filter(c => c.outcome === 'blocked').length;
  const healthy = data.verification?.chain.ok && !data.verification.chain.incomplete_tail && !data.verification.signed_decisions.problems.length && !data.verification.mandate_uses.problems.length && !data.verification.exceptions.length;
  return <><PageTitle eyebrow="YOUR AGENTS. YOUR AUTHORITY." title="Workspace overview" description="The decisions waiting on you, and the activity behind them."><RefreshButton /></PageTitle>
    <section className="panel workspace-launchpad"><div><span className="eyebrow">BUILD YOUR PAYMENT WORKSPACE</span><h2>Connect the tools. Shape the agents.</h2><p>Start with a connection plan and an agent draft, then keep every exception in view.</p></div><div><Link className="button primary" href="/integrations">Explore integrations <ArrowUpRight size={14} /></Link><Link className="button secondary" href="/automations">Configure an agent draft <ArrowUpRight size={14} /></Link></div></section>
    <Link href="/tasks" className="panel task-preview-link"><span><strong>Explore the personal assistant</strong><small>Shopping and travel, from a request to a simulated result.</small></span><ArrowUpRight size={18} /></Link>
    <div className="overview-grid"><Link className="panel metric-card" href="/approvals"><div><span>Waiting for approval</span><Clock3 size={18} /></div><strong>{data.errors.approvals ? 'Unavailable' : pending.length}</strong><p>{pending[0] ? `Oldest · ${waiting(pending[0].requested_at, data.now)}` : 'No requests waiting'}<ArrowUpRight size={14} /></p></Link>
    <Link className="panel metric-card" href="/activity"><div><span>Agent payments today</span><ShieldCheck size={18} /></div><strong className="money-metric">{data.errors.calls ? 'Unavailable' : totals(Date.parse(`${today}T00:00:00Z`))}</strong><p>ALLOW authorizations · UTC<ArrowUpRight size={14} /></p></Link>
    <Link className="panel metric-card" href="/activity"><div><span>Agent payments this week</span><ShieldCheck size={18} /></div><strong className="money-metric">{data.errors.calls ? 'Unavailable' : totals(week.getTime())}</strong><p>Monday onward · loaded activity<ArrowUpRight size={14} /></p></Link>
    <Link className="panel metric-card" href="/activity"><div><span>Blocked attempts</span><ShieldCheck size={18} /></div><strong>{data.errors.calls ? 'Unavailable' : blocked}</strong><p>Across loaded activity<ArrowUpRight size={14} /></p></Link></div>
    <Link className="panel overview-evidence" href="/evidence"><span className="empty-icon"><FileCheck2 size={23} /></span><div><h2>{data.verification ? healthy ? 'Your evidence passed verification' : 'Your evidence needs attention' : 'Evidence status unavailable'}</h2><p>{data.verification ? `${data.verification.chain.records} records · ${data.verification.deep ? 'deep' : 'standard'} verification` : data.errors.verification}</p></div><ArrowUpRight size={19} /></Link>
    <div className="section-heading section-spaced"><h2>Next decision</h2><Link className="text-link" href="/approvals">Open inbox <ArrowUpRight size={13} /></Link></div>
    {pending[0] ? <Link href={`/approvals/${pending[0].id}`} className="panel next-decision"><div><Badge value="pending" /><h3>{pending[0].operation} · {pending[0].amountLabel}</h3><p className="mono">{pending[0].counterparty}</p></div><dl><DetailField label="Agent">{pending[0].agent}</DetailField><DetailField label="Held by">{pending[0].rule}</DetailField></dl><ArrowUpRight size={18} /></Link> : <div className="panel"><h3>Nothing needs your decision</h3><p className="section-description">Open the activity timeline to review what your agents have done.</p></div>}
    <p className="section-description section-spaced">Totals summarize authorizations in up to 200 loaded calls, grouped by currency. They do not establish settlement or complete spend for the period.</p>
  </>;
}
