'use client';
import Link from 'next/link';
import { useState } from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import { ArrowLeft, ArrowUpRight, Clock3, ConfirmDialog, DetailField, Empty, JsonBlock, Notice, OperationIcon,
  PageTitle, Badge, RefreshButton, SectionError, useConsole } from './console';
import type { ActionResult } from '@/lib/contracts';
import { dateTime, waiting } from '@/lib/display';

export function ApprovalInbox() {
  const { data } = useConsole(), [tab, setTab] = useState('pending');
  if (!data) return null;
  const pending = data.approvals.filter(r => r.status === 'pending');
  const rows = data.approvals.filter(r => tab === 'all' || r.status === tab || tab === 'approved' && r.status === 'used')
    .sort((a, b) => Number(b.status === 'pending') - Number(a.status === 'pending') || Date.parse(b.requested_at) - Date.parse(a.requested_at) || b.id - a.id);
  const oldest = [...pending].sort((a, b) => a.requested_at.localeCompare(b.requested_at))[0];
  return <>
    <PageTitle eyebrow="HUMAN DECISIONS. CLEAR AUTHORITY." title="Approval inbox" description="A clear decision for every exception. Review the request, then decide."><RefreshButton /></PageTitle>
    <SectionError name="approvals" />
    <div className="inbox-summary"><span className="summary-icon"><Clock3 size={21} /></span><div><strong>{pending.length} {pending.length === 1 ? 'request needs' : 'requests need'} your decision</strong><p>{oldest ? `Oldest request · ${waiting(oldest.requested_at, data.now)}` : 'You’re all caught up. New requests will appear here.'}</p></div><span className="summary-tag">{data.fixtures ? 'Sample workspace' : 'This project'}</span></div>
    <Tabs.Root value={tab} onValueChange={setTab}>
      <div className="inbox-toolbar"><Tabs.List className="tabs" aria-label="Approval status">{[['pending', 'Pending'], ['approved', 'Approved'], ['rejected', 'Rejected'], ['all', 'All requests']].map(([value, label]) => <Tabs.Trigger key={value} value={value} className="tab">{label}{value === 'pending' && <span>{pending.length}</span>}</Tabs.Trigger>)}</Tabs.List><span className="table-caption">Newest first</span></div>
      <Tabs.Content value={tab} className="inbox-panel" key={tab}>
        <div className="approval-table-head" aria-hidden="true"><span>REQUEST</span><span>AGENT</span><span>WAITING / DECIDED</span><span>STATUS</span><span /></div>
        {rows.length ? <ul className="approval-list">{rows.map(request => <li key={request.id}><Link href={`/approvals/${request.id}`} className="approval-row" aria-label={`Review ${request.operation.toLowerCase()} of ${request.amountLabel}, request ${request.id}`}>
          <div className="request-cell"><OperationIcon /><div><div className="request-title"><strong>{request.operation}</strong><span className="amount">{request.amountLabel}</span></div><span className="counterparty mono">{request.counterparty}</span><span className="rule-label">{request.rule}</span></div></div>
          <div className="agent-cell"><span className="agent-avatar">{request.agent.slice(0, 1).toUpperCase()}</span><span>{request.agent}<small>{request.target}</small></span></div>
          <div className="time-cell"><span>{request.status === 'pending' ? waiting(request.requested_at, data.now) : request.decided_at ? dateTime(request.decided_at) : 'Not recorded'}</span><small>Request #{request.id.toString().padStart(3, '0')}</small></div>
          <Badge value={request.status} /><ArrowUpRight size={17} className="row-arrow" />
        </Link></li>)}</ul> : <Empty title={tab === 'pending' ? 'Nothing waiting on you' : 'No requests in this view'}>{tab === 'pending' ? 'Your agents can continue within their authority. Exceptions will appear here.' : 'Decisions matching this status will appear here.'}</Empty>}
      </Tabs.Content>
    </Tabs.Root>
    <div className="inbox-help"><span className="help-line" /><p>Approvals are permissions for an exact request.<br /><span>The engine records your decision; the agent must retry the action.</span></p></div>
  </>;
}

export function ApprovalDetail({ id }: { id: string }) {
  const { data, act } = useConsole(), [note, setNote] = useState(''), [decision, setDecision] = useState<'approve' | 'reject' | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [result, setResult] = useState<ActionResult | null>(null);
  if (!data) return null;
  const request = data.approvals.find(r => String(r.id) === id);
  if (!request) return <><Link className="back-link" href="/approvals"><ArrowLeft size={15} />Approval inbox</Link><SectionError name="approvals" /><Empty title="Request not found">Refresh the inbox to see the requests available in this project.</Empty></>;
  const input = request.input.parameters && typeof request.input.parameters === 'object' ? request.input.parameters as Record<string, unknown> : request.input;
  const pending = request.status === 'pending';
  const submit = async () => {
    if (!decision || busy) return;
    setBusy(true); setError('');
    try { const response = await act({ action: 'approve', id: request.id, digest: request.digest, decision, note }); setResult(response); setDecision(null); }
    catch (err) { setError(err instanceof Error ? err.message : 'The engine refused this decision.'); }
    finally { setBusy(false); }
  };
  return <>
    <Link className="back-link" href="/approvals"><ArrowLeft size={15} />Approval inbox</Link>
    <PageTitle eyebrow={`REQUEST #${request.id.toString().padStart(3, '0')}`} title={`${request.operation} request`} description={pending ? 'Review the exact request before granting permission.' : 'This request has a recorded decision.'}><Badge value={request.status} /></PageTitle>
    <Notice result={result} />
    <div className="detail-layout"><div className="detail-main">
      <section className="panel request-overview"><div className="request-hero"><OperationIcon /><span>{request.operation}</span><strong>{request.amountLabel}</strong></div><div className="payee"><span>COUNTERPARTY</span><strong className="mono">{request.counterparty}</strong></div>
        <dl className="detail-grid"><DetailField label="Agent">{request.agent}</DetailField><DetailField label="Time waiting">{pending ? waiting(request.requested_at, data.now) : 'Decision recorded'}</DetailField><DetailField label="Tool" mono>{request.target}</DetailField><DetailField label="Held by rule" mono>{request.rule}</DetailField></dl>
      </section>
      <section className="panel"><h2>Why this was requested</h2><p className="agent-reason">{typeof input.reason === 'string' ? input.reason : 'The agent supplied no reason in the tool input.'}</p><div className="context-note"><Clock3 size={16} /><p>{request.reason}</p></div></section>
      <section className="panel"><div className="section-heading"><h2>Exact tool input</h2><span className="small muted">Secrets redacted</span></div><JsonBlock value={request.input} /></section>
      <section className="panel"><h2>Request record</h2><dl className="detail-grid"><DetailField label="Requested by">{request.requested_os_user || 'Not recorded'}</DetailField><DetailField label="Requested at">{dateTime(request.requested_at)}</DetailField><DetailField label="Session" mono>{request.session || 'Not recorded'}</DetailField><DetailField label="Request digest" mono>{request.digest}</DetailField>{request.approver && <DetailField label="Decided by">{request.approver}</DetailField>}{request.decided_at && <DetailField label="Decided at">{dateTime(request.decided_at)}</DetailField>}{request.note && <DetailField label="Decision note">{request.note}</DetailField>}</dl></section>
    </div><aside className="decision-panel panel"><span className="eyebrow">YOUR DECISION</span><h2>{pending ? 'Ready for your review' : 'Decision complete'}</h2><p>{pending ? 'Your decision is attributed to you and bound to this exact request.' : 'Return to the inbox to review other requests.'}</p><div className="decision-identity"><span className="user-avatar">{data.user.slice(0, 1).toUpperCase()}</span><span>Deciding as<strong>{data.user}</strong></span></div>
      {pending && <><label className="field-label" htmlFor="decision-note">Decision note <span>(optional)</span></label><textarea id="decision-note" rows={4} maxLength={2000} value={note} onChange={e => setNote(e.target.value)} placeholder="Add context for the record…" />
      {request.amountLabel === 'Amount unavailable' && <p className="action-error">Amount units could not be determined. Approval is unavailable; review the raw input and console questions.</p>}
      <button className="button primary full" disabled={!!data.errors.approvals || request.amountLabel === 'Amount unavailable'} onClick={() => { setError(''); setDecision('approve'); }}>Review approval<ArrowUpRight size={16} /></button><button className="button secondary full reject" onClick={() => { setError(''); setDecision('reject'); }}>Reject request</button><p className="decision-footnote">You’ll confirm before anything is recorded.</p></>}
    </aside></div>
    <ConfirmDialog open={!!decision} onOpenChange={open => { if (!open) setDecision(null); }} title={decision === 'reject' ? 'Reject this request?' : 'Confirm your approval'} description={`${data.fixtures ? 'Simulate' : 'Record'} this decision as ${data.user}.`} confirm={decision === 'reject' ? 'Confirm rejection' : 'Confirm approval'} danger={decision === 'reject'} busy={busy} error={error} onConfirm={() => void submit()}>
      <div className="confirmation-request"><span>{request.operation}</span><strong>{request.amountLabel}</strong><code>{request.counterparty}</code></div><dl><DetailField label="Record">{decision === 'reject' ? 'Reject' : 'Approve'} {request.operation.toLowerCase()} of {request.amountLabel} to {request.counterparty} as {data.user}</DetailField><DetailField label="Note">{note || 'No note'}</DetailField><DetailField label="Tool">{request.target}</DetailField></dl><details><summary>Review exact input again</summary><JsonBlock value={request.input} /></details>
    </ConfirmDialog>
  </>;
}
