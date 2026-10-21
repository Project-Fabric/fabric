'use client';
import { useState, type FormEvent } from 'react';
import { Copy, KeyRound, Users } from 'lucide-react';
import { Badge, ConfirmDialog, DetailField, Empty, Notice, PageTitle, SectionError, useConsole } from './console';
import { dateTime } from '@/lib/display';
import type { ConsoleAction, ActionResult } from '@/lib/contracts';

export function AgentsScreen() {
  const { data, act } = useConsole(), [formOpen, setFormOpen] = useState(false), [action, setAction] = useState<ConsoleAction | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [result, setResult] = useState<ActionResult | null>(null);
  const [token, setToken] = useState<string | null>(null), [copied, setCopied] = useState(false);
  if (!data) return null;
  const register = (event: FormEvent<HTMLFormElement>) => { event.preventDefault(); const form = new FormData(event.currentTarget); setError('');
    setAction({ action: 'agent-add', name: String(form.get('name') || ''), tools: String(form.get('tools') || '').split('\n').map(t => t.trim()).filter(Boolean) }); };
  const submit = async () => {
    if (!action || busy) return;
    setBusy(true); setError('');
    try { const response = await act(action); setToken(response.token || null); setCopied(false);
      const visible = { ...response }; delete visible.token; setResult(visible); setAction(null); setFormOpen(false); }
    catch (err) { setError(err instanceof Error ? err.message : 'The engine refused this action.'); }
    finally { setBusy(false); }
  };
  return <><PageTitle eyebrow="IDENTITY BEFORE AUTHORITY" title="Agent registry" description="Know which agent acted. Give each identity only the tools it needs."><button className="button primary" onClick={() => setFormOpen(!formOpen)}>Register agent</button></PageTitle><SectionError name="agents" /><Notice result={result} />
    <div className="registry-banner"><Users size={18} /><div><strong>{data.required ? 'Agent tokens are required' : 'Agent tokens are optional'}</strong><p>{data.required ? 'The gateway refuses calls without an active, valid identity token.' : 'Unauthenticated clients may be recorded by their self-reported names.'}</p></div></div>
    {token && <section className="panel token-panel" role="status"><KeyRound size={22} /><h2>{data.fixtures ? 'Simulated token · not a credential' : 'Copy this token now'}</h2><p>This is its only display. Save it in the agent’s secure configuration. Closing this card removes it from this screen; the console does not persist it.</p><code className="command">{token}</code><div className="form-actions"><button className="button secondary" onClick={() => { setToken(null); setCopied(false); }}>I saved it · close</button><button className="button primary" onClick={async () => { try { await navigator.clipboard.writeText(token); setCopied(true); } catch { setCopied(false); } }}><Copy size={14} />{copied ? 'Copied' : 'Copy token'}</button></div></section>}
    {formOpen && <form className="panel form-panel" onSubmit={register}><h3>Register an agent identity</h3><div className="form-grid"><label>Agent name<input name="name" aria-label="Agent name" aria-describedby="agent-name-hint" pattern="[a-z0-9][a-z0-9_-]{0,63}" maxLength={64} required placeholder="refunds-bot" /><small id="agent-name-hint">Lowercase letters, numbers, hyphens and underscores.</small></label><label>Tool scopes, one per line<textarea name="tools" required rows={3} placeholder="stripe/*" /></label></div><div className="form-actions"><button className="button secondary" type="button" onClick={() => setFormOpen(false)}>Cancel</button><button className="button primary" type="submit">Review registration</button></div></form>}
    <div className="card-grid">{data.agents.length ? [...data.agents].sort((a, b) => Number(!!a.revoked_at) - Number(!!b.revoked_at)).map((agent, index) => <section className="panel agent-card" key={`${agent.name}-${agent.created_at}-${index}`}><div className="section-heading"><div className="agent-card-name"><span className="agent-avatar">{agent.name.slice(0, 1).toUpperCase()}</span><h3>{agent.name}</h3></div><Badge value={agent.revoked_at ? 'revoked' : 'active'} /></div><div className="tool-scopes">{agent.tools.length ? agent.tools.map(tool => <code key={tool}>{tool}</code>) : <span className="muted small">All tools · no scopes recorded</span>}</div><dl className="detail-grid"><DetailField label="Registered">{dateTime(agent.created_at)}</DetailField><DetailField label="Revoked">{agent.revoked_at ? dateTime(agent.revoked_at) : 'Not revoked'}</DetailField></dl>{!agent.revoked_at && <button className="button secondary reject" onClick={() => { setError(''); setAction({ action: 'agent-revoke', name: agent.name }); }}>Revoke agent</button>}</section>) : <Empty title="No agents registered" />}</div>
    <ConfirmDialog open={!!action} onOpenChange={open => { if (!open) setAction(null); }} title={action?.action === 'agent-add' ? 'Register this agent?' : 'Revoke this identity?'} description={`${data.fixtures ? 'Simulated' : 'Recorded'} in this project as ${data.user}.`} confirm={action?.action === 'agent-add' ? 'Confirm registration' : 'Confirm revocation'} danger={action?.action === 'agent-revoke'} busy={busy} error={error} onConfirm={() => void submit()}>
      <dl><DetailField label="Agent">{action && 'name' in action ? action.name : ''}</DetailField>{action?.action === 'agent-add' && <DetailField label="Scopes">{action.tools.join(', ')}</DetailField>}</dl><p>{action?.action === 'agent-add' ? 'The token will be displayed once after registration.' : 'The gateway will refuse this identity’s revoked token.'}</p>
    </ConfirmDialog>
  </>;
}
