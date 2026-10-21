'use client';
import { useState } from 'react';
import { Badge, Empty, JsonBlock, OperationIcon, PageTitle, RefreshButton, SectionError, useConsole } from './console';
import { dateTime, field, money, operation } from '@/lib/display';

export function ActivityScreen() {
  const { data } = useConsole(), [agent, setAgent] = useState('all'), [outcome, setOutcome] = useState('all'), [payments, setPayments] = useState(false);
  if (!data) return null;
  const rows = data.calls.filter(c => (agent === 'all' || c.agent === agent) && (outcome === 'all' || c.outcome === outcome) && (!payments || !!c.payment))
    .sort((a, b) => Date.parse(b.at) - Date.parse(a.at) || b.record - a.record);
  return <><PageTitle eyebrow="THE RECORD OF WHAT HAPPENED" title="Agent activity" description="Every tool call, its decision and its outcome. Read directly from the engine."><RefreshButton /></PageTitle><SectionError name="calls" />
    <div className="filters"><label>Agent<select value={agent} onChange={e => setAgent(e.target.value)}><option value="all">All agents</option>{Array.from(new Set(data.calls.map(c => c.agent))).map(name => <option key={name}>{name}</option>)}</select></label><label>Outcome<select value={outcome} onChange={e => setOutcome(e.target.value)}><option value="all">All outcomes</option>{Array.from(new Set(data.calls.map(c => c.outcome))).map(value => <option key={value}>{value}</option>)}</select></label><label className="checkbox-label"><input type="checkbox" checked={payments} onChange={e => setPayments(e.target.checked)} />Payments only</label><span className="filter-count">{rows.length} calls · up to 200 loaded</span></div>
    <div className="timeline">{rows.length ? rows.map(call => {
      let input: unknown = {}; try { input = JSON.parse(call.arguments || '{}'); } catch { /* The raw value remains visible below. */ }
      const blocked = call.outcome === 'blocked' || call.decision === 'deny';
      const opcode = field(input, 'stripe_api_operation_id');
      return <article className={`activity-card ${blocked ? 'blocked-card' : ''}`} key={`${call.record}-${call.session}`}>
        <div className="activity-top"><OperationIcon blocked={blocked} /><div className="activity-title"><strong>{operation(input, call.tool)}{call.payment && <span className="activity-amount">{money(call.payment.amount, call.payment.currency)}</span>}</strong><p>{call.agent} <span>·</span> {dateTime(call.at)}</p></div><Badge value={call.outcome} /></div>
        <div className="activity-metadata"><code>{call.tool}</code>{typeof opcode === 'string' && <code>{opcode}</code>}<span>Decision <Badge value={call.decision} /></span>{call.payment && <span>AgentAuth <Badge value={call.payment.decision} /></span>}</div>
        {call.payment?.reason && <p className="payment-reason">{call.payment.reason}</p>}
        {call.payment?.authorization_code && <div className="authorization"><span>Authorization</span><code>{call.payment.authorization_code}</code></div>}
        {!call.payment && blocked && <p className="payment-reason">{call.result || 'The gateway refused this call.'}</p>}
        <details><summary>View record #{call.record}</summary><dl className="detail-grid"><div className="detail-field"><dt>Rule</dt><dd>{call.rule || 'No holding rule'}</dd></div><div className="detail-field"><dt>Session</dt><dd className="mono">{call.session || 'Not recorded'}</dd></div></dl><h3 className="record-subtitle">Tool input</h3><JsonBlock value={input} /><h3 className="record-subtitle">Recorded result</h3><pre className="json-block">{call.result || 'No response recorded'}</pre></details>
      </article>;
    }) : <Empty title="No activity matches these filters" />}</div>
  </>;
}
