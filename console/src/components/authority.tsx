'use client';
import { useState, type FormEvent } from 'react';
import { Badge, ConfirmDialog, DetailField, Empty, Notice, PageTitle, SectionError, useConsole } from './console';
import { dateTime, money } from '@/lib/display';
import type { ActionResult, ConsoleAction } from '@/lib/contracts';

export function AuthorityScreen() {
  const { data, act } = useConsole(), [formOpen, setFormOpen] = useState(false), [action, setAction] = useState<ConsoleAction | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [result, setResult] = useState<ActionResult | null>(null);
  if (!data) return null;
  const day = data.now.slice(0, 10);
  function grant(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const fields = new FormData(event.currentTarget);
    const value = (name: string) => String(fields.get(name) || '').trim();
    setError(''); setAction({ action: 'mandate-grant', title: value('title'), tools: value('tools').split('\n').map(t => t.trim()).filter(Boolean),
      ...(value('agent') ? { agent: value('agent') } : {}), ...(value('perCall') ? { maxPerCall: value('perCall') } : {}),
      ...(value('perDay') ? { maxPerDay: value('perDay') } : {}), ...(value('calls') ? { maxCallsPerDay: Number(value('calls')) } : {}),
      ...(value('expires') ? { expires: value('expires') } : {}) });
  }
  const submit = async () => {
    if (!action || busy) return;
    setBusy(true); setError('');
    try { setResult(await act(action)); setAction(null); setFormOpen(false); }
    catch (err) { setError(err instanceof Error ? err.message : 'The engine refused this action.'); }
    finally { setBusy(false); }
  };
  return <><PageTitle eyebrow="DELEGATE WITH BOUNDARIES" title="Spending authority" description="Consents authorize payments. Mandates cover defined tool calls within their limits."><button className="button primary" onClick={() => setFormOpen(!formOpen)}>Grant mandate</button></PageTitle><Notice result={result} />
    <SectionError name="consents" /><SectionError name="mandates" />
    <div className="section-heading"><h2>Payment consents</h2><span className="small muted">{data.consents.length} loaded of {data.consentTotal}</span></div>
    <p className="section-description">Usage is the sum of ALLOW authorizations in the loaded activity. It is not a settlement balance.</p>
    <div className="card-grid">{data.consents.length ? data.consents.map(consent => {
      const spent = data.calls.filter(c => c.payment?.consent === consent.consent_id && c.payment.decision === 'ALLOW' && c.payment.currency.toUpperCase() === consent.constraints.currency.toUpperCase()).reduce((sum, c) => sum + Number(c.payment!.amount), 0);
      const expired = !!consent.expires_at && Date.parse(consent.expires_at) <= Date.parse(data.now);
      return <section className="panel authority-card" key={consent.consent_id}><div className="section-heading"><span className="card-label">PAYMENT CONSENT</span><Badge value={!consent.is_active ? 'revoked' : expired ? 'expired' : 'active'} /></div><h3>{consent.intent_description}</h3><div className="authority-limit">{money(consent.constraints.max_amount, consent.constraints.currency)}<span>total consent limit</span></div><dl className="detail-grid"><DetailField label="Recorded authorizations">{money(spent, consent.constraints.currency)}</DetailField><DetailField label="Use">{consent.scope.single_use ? 'Single use' : 'Multiple use'}</DetailField><DetailField label="Expires">{consent.expires_at ? dateTime(consent.expires_at) : 'No expiry recorded'}</DetailField><DetailField label="Principal">{consent.user_id}</DetailField></dl><code className="identifier">{consent.consent_id}</code></section>;
    }) : <Empty title="No consents available" />}</div>
    <div className="section-heading section-spaced"><h2>Tool mandates</h2><span className="small muted">Usage resets daily at 00:00 UTC</span></div>
    {formOpen && <form className="panel form-panel" onSubmit={grant}><h3>Grant bounded tool authority</h3><p>Amount limits use the engine’s units. No currency is recorded in mandate terms.</p><div className="form-grid"><label>Title<input name="title" required maxLength={200} placeholder="Small goodwill refunds" /></label><label>Agent (optional)<select name="agent"><option value="">Any agent</option>{data.agents.filter(a => !a.revoked_at).map(a => <option key={a.name}>{a.name}</option>)}</select></label><label className="span-two">Tool globs, one per line<textarea name="tools" required rows={2} placeholder="*/issue_refund" /></label><label>Maximum per call (optional)<input name="perCall" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,6})?" /></label><label>Maximum per UTC day (optional)<input name="perDay" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,6})?" /></label><label>Calls per UTC day (optional)<input name="calls" type="number" min={1} step={1} /></label><label>Expiry (optional)<input name="expires" type="date" /><small>Ends at the start of this date, UTC.</small></label></div><div className="form-actions"><button type="button" className="button secondary" onClick={() => setFormOpen(false)}>Cancel</button><button className="button primary" type="submit">Review mandate</button></div></form>}
    <div className="card-grid">{data.mandates.length ? data.mandates.map(mandate => {
      const uses = mandate.uses.filter(u => u.day === day), amount = uses.reduce((sum, u) => sum + (u.amount || 0), 0) / 1_000_000;
      const expired = !!mandate.terms.expires && mandate.terms.expires <= day;
      return <section className="panel authority-card" key={mandate.id}><div className="section-heading"><span className="card-label">MANDATE #{mandate.id}</span><Badge value={mandate.revoked_at ? 'revoked' : expired ? 'expired' : 'active'} /></div><h3>{mandate.terms.title}</h3><div className="tool-scopes">{mandate.terms.tools.map(tool => <code key={tool}>{tool}</code>)}</div><dl className="detail-grid"><DetailField label="Per call">{mandate.terms.max_per_call ? `${mandate.terms.max_per_call} units` : 'No limit'}</DetailField><DetailField label="Per UTC day">{mandate.terms.max_per_day ? `${mandate.terms.max_per_day} units` : 'No limit'}</DetailField><DetailField label="Today’s use">{amount.toLocaleString('en-US')} units · {uses.length} calls</DetailField><DetailField label="Calls per day">{mandate.terms.max_calls_per_day ?? 'No limit'}</DetailField><DetailField label="Agent">{mandate.terms.agent || 'Any agent'}</DetailField><DetailField label="Expiry">{mandate.terms.expires ? `${mandate.terms.expires} · 00:00 UTC` : 'No expiry'}</DetailField></dl>{!mandate.revoked_at && <button className="button secondary reject" onClick={() => { setError(''); setAction({ action: 'mandate-revoke', id: mandate.id }); }}>Revoke mandate</button>}</section>;
    }) : <Empty title="No mandates granted" />}</div>
    <ConfirmDialog open={!!action} onOpenChange={open => { if (!open) setAction(null); }} title={action?.action === 'mandate-revoke' ? 'Revoke this mandate?' : 'Grant this authority?'} description={`Recorded as ${data.user}. ${data.fixtures ? 'This is a simulation.' : 'The engine validates the terms and signing requirements.'}`} confirm={action?.action === 'mandate-revoke' ? 'Confirm revocation' : 'Confirm grant'} busy={busy} error={error} danger={action?.action === 'mandate-revoke'} onConfirm={() => void submit()}>
      {action?.action === 'mandate-grant' ? <dl className="confirmation-terms"><DetailField label="Title">{action.title}</DetailField><DetailField label="Tools">{action.tools.join(', ')}</DetailField><DetailField label="Agent">{action.agent || 'Any agent'}</DetailField><DetailField label="Per call / per day">{action.maxPerCall || 'Unlimited'} / {action.maxPerDay || 'Unlimited'} units</DetailField><DetailField label="Calls per day">{action.maxCallsPerDay || 'Unlimited'}</DetailField><DetailField label="Expiry">{action.expires ? `${action.expires}, start of day UTC` : 'No expiry'}</DetailField></dl> : <p>The engine will revoke mandate #{action?.action === 'mandate-revoke' ? action.id : ''}. Calls it covered will need another authority or a person’s approval.</p>}
    </ConfirmDialog>
  </>;
}
