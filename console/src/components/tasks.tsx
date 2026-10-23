'use client';
import { useState } from 'react';
import { ArrowRight, Check, CircleHelp, Plane, Search, ShoppingBag } from 'lucide-react';
import { advanceTask, beginTask, parseTaskBudget, selectedOption, taskBlocker, taskExamples, taskMoney, taskTotal, type TaskExample } from '@/lib/task-fixtures';
import { ConfirmDialog, PageTitle } from './console';

const labels = { research: 'Compare options', review: 'Review proposal', approved: 'Approved in demo', unknown: 'Outcome unknown', completed: 'Demo completed', rejected: 'Declined' };
export function TasksScreen() {
  const [example, setExample] = useState(taskExamples[0]);
  return <><PageTitle eyebrow="FROM A REQUEST TO A RESULT" title="Your tasks" description="Shopping, travel, and the details in between. See how Cosigna could carry a task through." />
    <div className="task-demo-note"><CircleHelp size={18} /><p><strong>Interactive preview.</strong> Every offer, identity, price and outcome here is fictional. No searches, account connections or purchases run. Progress resets when you leave this page.</p></div>
    <div className="task-example-picker" role="group" aria-label="Example tasks">{taskExamples.map(item => <button key={item.id} aria-pressed={example.id === item.id} onClick={() => setExample(item)}>
      {item.kind === 'shopping' ? <ShoppingBag size={20} /> : <Plane size={20} />}<span><small>{item.kind === 'shopping' ? 'SHOPPING' : 'TRAVEL'}</small><strong>{item.title}</strong></span><ArrowRight size={16} />
    </button>)}</div>
    <TaskWorkspace key={example.id} example={example} />
  </>;
}
function TaskWorkspace({ example }: { example: TaskExample }) {
  const [task, setTask] = useState(() => beginTask(example));
  const [budget, setBudget] = useState(String(example.budget / 100)), [error, setError] = useState('');
  const [confirm, setConfirm] = useState(false);
  const option = selectedOption(task), blocker = taskBlocker(task), total = taskTotal(task);
  const update = (action: Parameters<typeof advanceTask>[1]) => setTask(current => advanceTask(current, action));
  const stages = ['Research', 'Approval', 'Execution', 'Result'];
  const progress = task.stage === 'research' ? 0 : task.stage === 'review' || task.stage === 'rejected' ? 1 : task.stage === 'completed' ? 3 : 2;
  const reset = () => { setTask(beginTask(example)); setBudget(String(example.budget / 100)); setError(''); setConfirm(false); };
  return <div className="task-workspace">
    <section className="task-main" aria-label="Task workspace">
      <div className="panel task-request"><span className="eyebrow">EXAMPLE REQUEST</span><h2>{example.request}</h2><div className="task-status-line"><span className="development-tag">{labels[task.stage]}</span><button className="text-link" onClick={reset}>Reset example</button></div></div>
      <ol className="task-progress" aria-label="Task progress">{stages.map((stage, i) => <li key={stage} aria-current={i === progress ? 'step' : undefined} className={i <= progress ? 'reached' : ''}><span>{i < progress ? <Check size={12} /> : i + 1}</span>{stage}</li>)}</ol>
      {task.stage === 'research' ? <section aria-label="Example offers"><div className="section-heading"><h2>Two options, with the tradeoffs visible.</h2></div><p className="task-caption"><Search size={14} />Synthetic offers for this demo. No live availability checked.</p>
        <fieldset className="task-options"><legend className="sr-only">Choose an example offer</legend>{example.options.map(item => <label className={`panel task-option ${item.id === task.selected ? 'selected' : ''}`} key={item.id}>
          <div className="task-option-head"><input type="radio" name="offer" value={item.id} checked={task.selected === item.id} onChange={() => update({ type: 'select', id: item.id })} /><div><strong>{item.title}</strong><p>{item.provider} · fictional</p></div><strong>{taskMoney(item.total + task.priceIncrease)}</strong></div>
          <ul>{item.details.map(detail => <li key={detail}>{detail}</li>)}</ul>{task.priceIncrease > 0 && <p className="task-change">Updated quote adds {taskMoney(task.priceIncrease)} to the listed breakdown.</p>}{item.mismatch ? <p className="task-mismatch">{item.mismatch}</p> : <p className="task-match">Matches the example requirements</p>}
        </label>)}</fieldset>
        {blocker && <p className="task-alert" role="alert">{blocker}</p>}
        <div className="task-actions"><button className="button primary" disabled={!!blocker || budget !== String(task.budget / 100)} onClick={() => update({ type: 'review' })}>Review proposal <ArrowRight size={14} /></button><button className="button secondary" onClick={() => update({ type: 'reject' })}>Decline proposal</button></div>
      </section> : task.stage === 'review' ? <section className="panel task-review" aria-label="Proposal review"><span className="eyebrow">YOUR DECISION</span><h2>Review the exact proposal.</h2><p>{option.title} · {option.provider}</p><strong className="task-price">{taskMoney(total)}</strong><p>All-in demo total · quote {task.quoteVersion}</p><ul>{option.details.map(detail => <li key={detail}>{detail}</li>)}</ul>{task.priceIncrease > 0 && <p>Includes a quote increase of {taskMoney(task.priceIncrease)}.</p>}<p>Approval covers this option once. A change to the quote needs another review.</p><div className="task-actions"><button className="button primary" onClick={() => setConfirm(true)}>Approve demo proposal</button><button className="button secondary" onClick={() => update({ type: 'edit' })}>Back to options</button><button className="button secondary" onClick={() => update({ type: 'reject' })}>Decline proposal</button></div></section>
      : task.stage === 'approved' ? <section className="panel task-review" aria-label="Execution simulation"><span className="eyebrow">APPROVAL RECORDED IN THIS PAGE</span><h2>Explore what happens next.</h2><p>Choose a simulated provider response for {option.title}. Nothing has been submitted to a vendor.</p><div className="task-scenarios"><button onClick={() => update({ type: 'complete' })}><Check size={18} /><strong>Simulate success</strong><span>Receive a demo confirmation.</span></button><button onClick={() => update({ type: 'price-change' })}><ArrowRight size={18} /><strong>Simulate price change</strong><span>Add INR 500 and require new approval.</span></button><button onClick={() => update({ type: 'unknown' })}><CircleHelp size={18} /><strong>Simulate timeout</strong><span>Keep the outcome unresolved.</span></button></div></section>
      : <section className="panel task-review" aria-label="Task result"><span className="eyebrow">{labels[task.stage]}</span><h2>{task.stage === 'unknown' ? 'Check the original request first.' : task.stage === 'rejected' ? 'You chose not to proceed.' : 'A result you can inspect.'}</h2><p>{task.events[task.events.length - 1]}</p>{task.stage === 'unknown' ? <><p>Retrying could create a duplicate purchase. The simulated reservation stays in place until the original outcome is resolved.</p><button className="button primary" onClick={() => update({ type: 'reconcile' })}>Simulate status lookup</button></> : task.stage === 'completed' ? <dl className="task-result-details"><div><dt>Example confirmation</dt><dd>DEMO-{example.id.toUpperCase()}-001</dd></div><div><dt>Recorded demo amount</dt><dd>{taskMoney(total)}</dd></div><div><dt>Real order or ticket</dt><dd>None. This is a simulation.</dd></div></dl> : null}</section>}
      <section className="panel task-history" aria-label="Demo activity"><h2>Task activity</h2><ol>{task.events.map((event, index) => <li key={index}><span>{String(index + 1).padStart(2, '0')}</span>{event}</li>)}</ol></section>
    </section>
    <aside className="panel task-boundaries" aria-label="Task boundaries"><span className="eyebrow">YOU SET THE BOUNDARIES</span><h2>Permission for this task</h2><p>Exact approval before every simulated purchase.</p><form onSubmit={event => { event.preventDefault(); const amount = parseTaskBudget(budget); if (amount === null) { setError('Enter a positive INR amount with up to two decimal places.'); return; } update({ type: 'budget', amount }); setBudget(String(amount / 100)); setError(''); }}><label htmlFor="task-budget">Example budget (INR)</label><input id="task-budget" inputMode="decimal" value={budget} disabled={task.stage !== 'research'} onChange={event => { setBudget(event.target.value); setError(''); }} />{error && <p className="action-error" role="alert">{error}</p>}{task.stage === 'research' && <button className="button secondary" type="submit">Apply demo budget</button>}</form>
      <dl className="task-budget-summary"><div><dt>Task limit</dt><dd>{taskMoney(task.budget)}</dd></div><div><dt>{task.stage === 'completed' ? 'Recorded in demo' : ['approved', 'unknown'].includes(task.stage) ? 'Reserved in demo' : 'Proposed total'}</dt><dd>{taskMoney(total)}</dd></div></dl>
      <h3>Requirements</h3><ul>{example.constraints.map(constraint => <li key={constraint}>{constraint}</li>)}</ul><h3>Access used</h3><p>Example data only. No location, identity documents, payment details or connected accounts.</p>
    </aside>
    <ConfirmDialog open={confirm} onOpenChange={setConfirm} title="Confirm simulated approval" description="This records a decision only in this preview. It does not authorize a real purchase." confirm="Confirm demo approval" busy={false} error="" onConfirm={() => { update({ type: 'approve' }); setConfirm(false); }}><p>{option.title} from {option.provider}</p><p><strong>{taskMoney(total)}</strong> · quote {task.quoteVersion} · one purchase</p><ul>{example.constraints.map(constraint => <li key={constraint}>{constraint}</li>)}</ul></ConfirmDialog>
  </div>;
}
