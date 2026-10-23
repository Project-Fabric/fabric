// Synthetic UI scenarios only. Nothing in this module grants real authority.
export type TaskStage = 'research' | 'review' | 'approved' | 'unknown' | 'completed' | 'rejected';
export type TaskOption = { id: string; title: string; provider: string; total: number; details: string[]; mismatch?: string };
export type TaskExample = { id: string; kind: 'shopping' | 'travel'; title: string; request: string; budget: number;
  constraints: string[]; options: TaskOption[]; outcome: string };
export type TaskState = { example: TaskExample; stage: TaskStage; budget: number; selected: string;
  quoteVersion: number; priceIncrease: number; approvedVersion: number | null; events: string[] };
export const taskExamples: TaskExample[] = [
  { id: 'headphones', kind: 'shopping', title: 'Find my next headphones', budget: 1500000,
    request: 'Find wireless headphones under INR 15,000, with noise cancellation and delivery to my saved home address within three days.',
    constraints: ['New, wireless, with noise cancellation', 'Delivery within three days', 'Saved home address (demo)', 'No substitutions or recurring charges'],
    options: [
      { id: 'studio', title: 'Studio One headphones', provider: 'Example Electronics', total: 1299900, details: ['Black · new · noise cancellation', 'Delivery in two days · returns within 7 days', 'INR 12,499 item + INR 500 tax · free delivery'] },
      { id: 'air', title: 'Air Lite headphones', provider: 'Example Audio', total: 899900, details: ['White · new · wireless', 'Delivery tomorrow · returns within 7 days', 'INR 8,499 item + INR 500 tax · free delivery'], mismatch: 'No noise cancellation. This does not meet your request.' },
    ], outcome: 'Demo order recorded. Delivery is simulated for two days from now.' },
  { id: 'delhi', kind: 'travel', title: 'Plan my flight to Delhi', budget: 1200000,
    request: 'Book a one-way flight from Bengaluru to Delhi on 12 November 2026, arriving before noon, under INR 12,000 including a checked bag.',
    constraints: ['BLR → DEL · 12 November 2026 · one adult', 'Nonstop · arrival before noon, local time', 'Economy · one 15 kg checked bag', 'Traveler: Demo Passenger · no substitutions'],
    options: [
      { id: 'morning', title: 'Morning nonstop', provider: 'Example Airways', total: 1085000, details: ['07:10 BLR → 10:00 DEL · 2h 50m', '15 kg checked bag included · economy', 'INR 9,350 fare + INR 1,500 taxes and fees', 'Non-refundable · changes cost INR 3,000 plus fare difference'] },
      { id: 'basic', title: 'Early basic fare', provider: 'Example Air', total: 840000, details: ['06:00 BLR → 08:50 DEL · nonstop', 'Cabin bag only · economy', 'INR 7,000 fare + INR 1,400 taxes and fees', 'Non-refundable · changes unavailable'], mismatch: 'Checked baggage is missing. This does not meet your request.' },
    ], outcome: 'Demo booking confirmed for Demo Passenger. No ticket has been issued.' },
];
export function beginTask(example: TaskExample): TaskState {
  return { example, stage: 'research', budget: example.budget, selected: example.options[0].id,
    quoteVersion: 1, priceIncrease: 0, approvedVersion: null, events: ['Example request loaded. All offers and sources are synthetic.'] };
}
export function selectedOption(task: TaskState) { return task.example.options.find(option => option.id === task.selected)!; }
export function taskTotal(task: TaskState) { return selectedOption(task).total + task.priceIncrease; }
export function taskBlocker(task: TaskState): string | null {
  if (selectedOption(task).mismatch) return selectedOption(task).mismatch!;
  if (taskTotal(task) > task.budget) return 'The all-in total exceeds your task budget.';
  return null;
}
export function parseTaskBudget(value: string): number | null {
  if (!/^\d{1,7}(\.\d{1,2})?$/.test(value)) return null;
  const [whole, fraction = ''] = value.split('.');
  const minor = Number(whole) * 100 + Number(fraction.padEnd(2, '0'));
  return minor > 0 ? minor : null;
}
export function taskMoney(minor: number) { return `INR ${(minor / 100).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`; }
export type TaskAction = { type: 'select'; id: string } | { type: 'budget'; amount: number } |
  { type: 'review' | 'edit' | 'approve' | 'reject' | 'price-change' | 'complete' | 'unknown' | 'reconcile' };
export function advanceTask(task: TaskState, action: TaskAction): TaskState {
  const record = (changes: Partial<TaskState>, event: string) => ({ ...task, ...changes, events: [...task.events, event] });
  if (action.type === 'select' && task.stage === 'research' && task.example.options.some(o => o.id === action.id))
    return { ...task, selected: action.id, quoteVersion: task.quoteVersion + 1, approvedVersion: null };
  if (action.type === 'budget' && task.stage === 'research' && Number.isSafeInteger(action.amount) && action.amount > 0)
    return record({ budget: action.amount, approvedVersion: null }, 'Example task budget updated. No real spending permission granted.');
  if (action.type === 'review' && task.stage === 'research' && !taskBlocker(task)) return { ...task, stage: 'review' };
  if (action.type === 'edit' && task.stage === 'review') return { ...task, stage: 'research' };
  if (action.type === 'approve' && task.stage === 'review' && !taskBlocker(task))
    return record({ stage: 'approved', approvedVersion: task.quoteVersion }, `Simulated approval of quote ${task.quoteVersion}: ${taskMoney(taskTotal(task))}.`);
  if (action.type === 'reject' && (task.stage === 'research' || task.stage === 'review'))
    return record({ stage: 'rejected', approvedVersion: null }, 'Proposal declined. No order submitted.');
  if (action.type === 'price-change' && task.stage === 'approved')
    return record({ stage: 'research', quoteVersion: task.quoteVersion + 1, priceIncrease: task.priceIncrease + 50000, approvedVersion: null }, 'Demo quote increased by INR 500.00. Previous approval cleared; review required again.');
  if ((action.type === 'complete' || action.type === 'unknown') && task.stage === 'approved' && task.approvedVersion === task.quoteVersion && !taskBlocker(task))
    return action.type === 'complete' ? record({ stage: 'completed' }, task.example.outcome) : record({ stage: 'unknown' }, 'Demo provider timed out. Outcome unknown; budget remains reserved in this simulation.');
  if (action.type === 'reconcile' && task.stage === 'unknown')
    return record({ stage: 'completed' }, 'Simulated status lookup found the original order. No second order submitted.');
  return task;
}
