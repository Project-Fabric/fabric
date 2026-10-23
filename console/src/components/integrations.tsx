'use client';
import Link from 'next/link';
import { useEffect, useRef, useState } from 'react';
import { ArrowRight, ArrowUpRight, Check, PlugZap } from 'lucide-react';
import { integrations, type Integration } from '@/lib/catalog';
import { PageTitle } from './console';

export function IntegrationsScreen() {
  const [category, setCategory] = useState('All'), [query, setQuery] = useState(''), [selected, setSelected] = useState<Integration | null>(null);
  const plan = useRef<HTMLElement>(null);
  useEffect(() => { if (selected && plan.current) { plan.current.focus({ preventScroll: true }); plan.current.scrollIntoView({ block: 'nearest' }); } }, [selected]);
  const rows = integrations.filter(p => (category === 'All' || p.category === category) && p.name.toLowerCase().includes(query.toLowerCase()));
  return <><PageTitle eyebrow="THE TOOLS BEHIND YOUR TRANSACTIONS" title="Integrations" description="Build your workspace around the payment and accounting tools your team uses." />
    <div className="connection-intro panel"><PlugZap size={27} /><div><h2>Start with a clear connection.</h2><p>Stripe has a validated test-mode workflow. Account connection flows and the other adapters are in development. This directory does not connect a live account.</p></div><Link href="/activity" className="button secondary">Recorded activity <ArrowUpRight size={14} /></Link></div>
    <div className="filters"><label>Find an integration<input value={query} onChange={e => setQuery(e.target.value)} placeholder="Search providers" /></label><label>Category<select value={category} onChange={e => setCategory(e.target.value)}>{['All', 'Payments', 'Accounting', 'Commerce'].map(c => <option key={c}>{c}</option>)}</select></label><span className="filter-count">{rows.length} providers</span></div>
    <div className="integration-grid">{rows.map(provider => <section key={provider.id} className="panel integration-card"><div className="section-heading"><span className={`provider-mark provider-${provider.id}`} aria-hidden="true">{provider.mark}</span><span className={`development-tag ${provider.id === 'stripe' ? 'validated' : ''}`}>{provider.stage}</span></div><span className="eyebrow">{provider.category}</span><h2>{provider.name}</h2><p>{provider.description}</p><button className="text-link" onClick={() => setSelected(provider)}>View connection plan <ArrowRight size={14} /></button></section>)}</div>
    {!rows.length && <div className="empty-state"><h2>No matching providers</h2><p>Try another name or category.</p></div>}
    {selected && <section ref={plan} tabIndex={-1} className="panel connection-plan section-spaced" aria-label={`${selected.name} connection plan`}><div className="section-heading"><div><span className="eyebrow">CONNECTION PLAN</span><h2>{selected.name}</h2></div><button className="button secondary" onClick={() => setSelected(null)}>Close plan</button></div><p>{selected.next}</p><h3>Access to scope before activation</h3><ul>{selected.access.map(item => <li key={item}><Check size={14} />{item}</li>)}</ul><div className="connection-steps"><span>01 · Authorize account</span><span>02 · Verify events</span><span>03 · Test limits</span><span>04 · Enable workflow</span></div><p>No credentials are collected by this preview. A draft is not an authorization to access your account.</p><div className="form-actions"><a className="button secondary" href={selected.docs} target="_blank" rel="noreferrer">Provider documentation <ArrowUpRight size={14} /></a><Link className="button primary" href="/automations">Explore Cosigna agents <ArrowRight size={14} /></Link></div></section>}
  </>;
}
