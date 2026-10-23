'use client';
import { useEffect, useRef, useState, type FormEvent } from 'react';
import Link from 'next/link';
import { ArrowRight, Bot, Download, FilePenLine } from 'lucide-react';
import { agentTemplates, integrations } from '@/lib/catalog';
import { createAgentDraft, type AgentDraft } from '@/lib/agent-drafts';
import { DetailField, PageTitle, useConsole } from './console';

export function AutomationsScreen() {
  const { data } = useConsole(), [template, setTemplate] = useState<(typeof agentTemplates)[number] | null>(null);
  const [drafts, setDrafts] = useState<AgentDraft[]>([]), [error, setError] = useState('');
  const editor = useRef<HTMLFormElement>(null);
  useEffect(() => { if (template && editor.current) { editor.current.scrollIntoView({ block: 'nearest' }); editor.current.querySelector('input')?.focus({ preventScroll: true }); } }, [template]);
  if (!data) return null;
  const save = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); if (!template) return;
    const form = new FormData(event.currentTarget);
    try {
      const draft = createAgentDraft({ name: String(form.get('name') || ''), template: template.id, provider: String(form.get('provider') || ''),
        currency: String(form.get('currency') || 'USD'), perAction: String(form.get('perAction') || ''), perDay: String(form.get('perDay') || '') }, data.user);
      setDrafts(current => [...current, draft]); setTemplate(null); setError('');
    } catch (err) { setError(err instanceof Error && 'issues' in err ? (err as { issues: { message: string }[] }).issues[0].message : 'Check the draft fields.'); }
  };
  const download = (draft: AgentDraft) => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(draft, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a'); link.href = url; link.download = 'cosigna-agent-draft.json'; document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  return <><PageTitle eyebrow="GIVE THE WORK A ROLE. KEEP THE AUTHORITY." title="Cosigna agents" description="Shape a workflow for refunds, collections or reconciliation, with a person in control." />
    <div className="connection-intro panel"><Bot size={27} /><div><h2>Your agents start with boundaries.</h2><p>Agent execution is under development. You can configure and export a draft here. Drafts stay in this page until you leave; they do not run or connect an account.</p></div><Link href="/agents" className="text-link">Identity registry <ArrowRight size={14} /></Link></div>
    <div className="agent-template-grid">{agentTemplates.map(item => <section key={item.id} className="panel agent-template"><span className="template-symbol" aria-hidden="true">{item.initials}</span><span className="development-tag">Draft template</span><h2>{item.title}</h2><p>{item.description}</p><div className="tool-scopes">{item.tools.map(tool => <span key={tool}>{tool}</span>)}</div><button className="button secondary" onClick={() => { setTemplate(item); setError(''); }}>Configure draft <ArrowRight size={14} /></button></section>)}</div>
    {template && <form ref={editor} className="panel form-panel section-spaced" onSubmit={save} aria-label={`${template.title} draft`} key={template.id}><div className="section-heading"><h2>Configure {template.title.toLowerCase()}</h2><span className="development-tag">Not running</span></div><div className="form-grid"><label>Agent name<input name="name" required maxLength={64} defaultValue={template.title} /></label><label>Proposed provider<select name="provider" defaultValue="stripe">{integrations.map(p => <option key={p.id} value={p.id}>{p.name} · {p.stage}</option>)}</select></label>{template.writes && <><label>Currency<select name="currency">{['USD', 'EUR', 'GBP', 'INR'].map(c => <option key={c}>{c}</option>)}</select></label><label>Per-action limit<input name="perAction" inputMode="decimal" required placeholder="100.00" /></label><label>Daily limit<input name="perDay" inputMode="decimal" required placeholder="500.00" /></label></>}</div><p className="section-description section-spaced">{template.writes ? 'Every proposed refund requires human review in this draft. Limits do not grant authority.' : 'This template prepares analysis or drafts. It does not post entries, send messages or move money.'}</p>{error && <p role="alert" className="action-error">{error}</p>}<div className="form-actions"><button type="button" className="button secondary" onClick={() => setTemplate(null)}>Cancel</button><button className="button primary" type="submit"><FilePenLine size={14} />Save draft in this page</button></div></form>}
    {drafts.length > 0 && <section className="section-spaced" aria-label="Agent drafts"><div className="section-heading"><h2>Your agent drafts</h2><span className="small muted">Export to keep a copy</span></div><div className="card-grid">{drafts.map((draft, index) => <article className="panel" key={index}><div className="section-heading"><h3>{draft.name}</h3><span className="development-tag">Draft · not running</span></div><dl className="detail-grid"><DetailField label="Proposed provider">{integrations.find(p => p.id === draft.provider)?.name}</DetailField><DetailField label="Owner">{draft.owner}</DetailField><DetailField label="Permission">Human review before every write</DetailField>{draft.limits && <DetailField label="Draft limits">{draft.limits.currency} {draft.limits.per_action} / action · {draft.limits.per_day} / day</DetailField>}</dl><button className="button secondary section-spaced" onClick={() => download(draft)}><Download size={14} />Export draft</button></article>)}</div></section>}
  </>;
}
