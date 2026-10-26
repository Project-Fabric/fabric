import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ConsoleProvider } from '../src/components/console';
import { ApprovalDetail, ApprovalInbox } from '../src/components/approvals';
import { ActivityScreen } from '../src/components/activity';
import { AuthorityScreen } from '../src/components/authority';
import { AgentsScreen } from '../src/components/agents';
import { EvidenceScreen } from '../src/components/evidence';
import { OverviewScreen } from '../src/components/overview';
import { sample } from './fixtures';

function mount(component: React.ReactNode) {
  const data = sample(), submitted: unknown[] = [];
  vi.stubGlobal('fetch', vi.fn(async (_url: string, options?: RequestInit) => {
    if (options?.method === 'POST') {
      const action = JSON.parse(options.body as string); submitted.push(action);
      if (action.action === 'approve') { const request = data.approvals.find(a => a.id === action.id)!;
        request.status = action.decision === 'approve' ? 'approved' : 'rejected'; request.note = action.note; request.approver = data.user; }
      if (action.action === 'operation-decide' || action.action === 'operation-resolve') {
        const op = data.operations.find(o => o.data.id === action.id)!;
        op.data.stage = action.action === 'operation-resolve' ? 'failed' : action.decision === 'approve' ? 'succeeded' : 'rejected';
      }
      return { ok: true, json: async () => ({ simulated: true, args: ['approve', '1', '--yes'], message: 'Simulation only.' }) };
    }
    return { ok: true, json: async () => structuredClone(data) };
  }));
  render(<ConsoleProvider>{component}</ConsoleProvider>);
  return { data, submitted };
}
describe('fixture approval inbox', () => {
  it('shows pending first with amount, counterparty, agent and holding rule; tabs show other decisions', async () => {
    mount(<ApprovalInbox />);
    expect(await screen.findByRole('heading', { name: 'Approval inbox' })).toBeVisible();
    expect(screen.getByRole('link', { name: /review refund of \$450.00/i })).toHaveAttribute('href', '/approvals/1');
    expect(screen.getByText('pi_3Northwind2045')).toBeVisible(); expect(screen.getByText('finance-ops-postings')).toBeVisible();
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Rejected' }));
    fireEvent.click(screen.getByRole('tab', { name: 'Rejected' }));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Rejected' })).toHaveAttribute('aria-selected', 'true'));
    expect(screen.getByRole('link', { name: /review refund of \$640.00/i })).toBeVisible();
  });
  it.each(['approve', 'reject'] as const)('requires a separate confirmation, records the note, and updates after %s', async decision => {
    const { submitted } = mount(<><ApprovalDetail id="1" /><ApprovalInbox /></>);
    await screen.findByRole('heading', { name: 'Refund request' });
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/decision note/i), 'Verified ticket and amount');
    await user.click(screen.getByRole('button', { name: decision === 'approve' ? /review approval/i : /reject request/i }));
    expect(submitted).toHaveLength(0);
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getAllByText(/as priya@northwind.test/)[0]).toBeVisible();
    await user.click(within(dialog).getByRole('button', { name: decision === 'approve' ? 'Confirm approval' : 'Confirm rejection' }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0]).toMatchObject({ action: 'approve', id: 1, decision, note: 'Verified ticket and amount' });
    expect(await screen.findByText('Nothing waiting on you')).toBeVisible();
    expect(screen.getByText('Simulation complete')).toBeVisible();
  });
  it('canceling a confirmation records nothing', async () => {
    const { submitted } = mount(<ApprovalDetail id="1" />); await screen.findByRole('heading', { name: 'Refund request' });
    const user = userEvent.setup(); await user.click(screen.getByRole('button', { name: /review approval/i }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }));
    expect(submitted).toHaveLength(0); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});
describe('Cosigna gateway payments in the inbox', () => {
  it('lists only payments that need a person, with the verified facts and the agent', async () => {
    mount(<ApprovalInbox />);
    const section = within(await screen.findByRole('region', { name: 'Agent payments' }));
    expect(section.getByText('$450.00')).toBeVisible();
    expect(section.getByText('$120.00')).toBeVisible();
    expect(section.queryByText('$20.00')).not.toBeInTheDocument();
    expect(section.queryByText('$640.00')).not.toBeInTheDocument();
    expect(section.getByText('Customer cancelled order #2045 before it shipped.')).toBeVisible();
    expect(section.getAllByText(/refunds-bot · Above the agent’s automatic limit/)).toHaveLength(2);
    expect(section.getByText(/Paid \$450.00 · refunded so far \$0.00 · refundable \$450.00 · customer Farah Haddad/)).toBeVisible();
  });
  it.each(['approve', 'reject'] as const)('%s needs a separate confirmation bound to the exact proposal', async decision => {
    const { data, submitted } = mount(<ApprovalInbox />);
    const section = within(await screen.findByRole('region', { name: 'Agent payments' }));
    const user = userEvent.setup();
    const row = section.getByText('Customer cancelled order #2045 before it shipped.').closest('li')!;
    await user.click(within(row).getByRole('button', { name: decision === 'approve' ? /review payment/i : 'Reject' }));
    expect(submitted).toHaveLength(0);
    const dialog = screen.getByRole('dialog');
    const op = data.operations.find(o => o.data.request.source === 'pay_nw_2045')!;
    if (decision === 'approve') expect(within(dialog).getByText(op.proposal_digest!)).toBeVisible();
    await user.click(within(dialog).getByRole('button', { name: decision === 'approve' ? 'Approve and pay' : 'Confirm rejection' }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0]).toEqual({ action: 'operation-decide', id: op.data.id, decision, digest: op.proposal_digest });
    await waitFor(() => expect(section.getAllByRole('listitem')).toHaveLength(1));
  });
  it('an unknown result offers settlement through the payment system, not approval', async () => {
    const data = sample(), op = data.operations.find(o => o.data.request.source === 'pay_nw_2041')!;
    op.data.stage = 'unknown';
    const submitted: unknown[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, options?: RequestInit) => {
      if (options?.method === 'POST') { submitted.push(JSON.parse(options.body as string)); return { ok: true, json: async () => ({ simulated: true, args: [], message: 'Simulation only.' }) }; }
      return { ok: true, json: async () => structuredClone(data) };
    }));
    render(<ConsoleProvider><ApprovalInbox /></ConsoleProvider>);
    const section = within(await screen.findByRole('region', { name: 'Agent payments' }));
    const row = section.getByText(/Result unknown/).closest('li')!;
    expect(within(row).queryByRole('button', { name: /review payment/i })).not.toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(within(row).getByRole('button', { name: 'Settle' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Settle through the payment system' }));
    await waitFor(() => expect(submitted).toEqual([{ action: 'operation-resolve', id: op.data.id }]));
  });
});
describe('why a payment is waiting', () => {
  it('names the reason for each hold, including a first of two approvals', async () => {
    const data = sample();
    const [two, theCase] = data.operations.filter(o => o.data.stage === 'held');
    two.data.decision = { outcome: 'held', rule: 'needs_two_approvals' };
    two.data.first_approval = { by: 'approver', approver: 'approver_x', digest: 'a'.repeat(64), at: 1 };
    theCase.data.decision = { outcome: 'held', rule: 'exceeds_case_owed' };
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => structuredClone(data) })));
    render(<ConsoleProvider><ApprovalInbox /></ConsoleProvider>);
    const section = within(await screen.findByRole('region', { name: 'Agent payments' }));
    expect(section.getByText(/Approved once by an approver; needs a second, different person/)).toBeVisible();
    expect(section.getByText(/More than the customer’s case says is owed/)).toBeVisible();
  });
});
describe('what agents can pay without the owner', () => {
  it('shows the worst case from the captured gateway report, what sets each limit, and the case what-if', async () => {
    mount(<ApprovalInbox />);
    const section = within(await screen.findByRole('region', { name: 'What agents can pay without you' }));
    expect(section.getByText('$1,980.00', { selector: 'strong' })).toBeVisible();
    expect(section.getByText(/open cases owe \$500.00/)).toBeVisible();
    const bot = section.getByText('refunds-bot').closest('li')!;
    expect(within(bot).getByText(/Up to \$100.00 each · limited by its budget/)).toBeVisible();
    expect(within(bot).getByText('If it had to name a case for each refund: $500.00')).toBeVisible();
    const desk = section.getByText('returns-desk').closest('li')!;
    expect(within(desk).getByText(/limited by what open cases owe/)).toBeVisible();
    expect(within(desk).queryByText(/If it had to name a case/)).not.toBeInTheDocument();
    expect(section.getByText(/An upper bound/)).toBeVisible();
  });
  it('says so when the report cannot be loaded, without inventing a figure', async () => {
    const data = sample(); data.exposure = null; data.errors.exposure = 'The gateway refused this (HTTP 500).';
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => structuredClone(data) })));
    render(<ConsoleProvider><ApprovalInbox /></ConsoleProvider>);
    const section = within(await screen.findByRole('region', { name: 'What agents can pay without you' }));
    expect(section.getByText(/HTTP 500/)).toBeVisible();
    expect(section.queryByText(/\$/)).not.toBeInTheDocument();
  });
});
describe('remaining screens with captured engine fixtures', () => {
  it('activity isolates the three blocked payments', async () => {
    mount(<ActivityScreen />); await screen.findByRole('heading', { name: 'Agent activity' });
    await userEvent.selectOptions(screen.getByLabelText('Outcome'), 'blocked');
    expect(screen.getByText(/3 calls/)).toBeVisible(); expect(screen.getByText(/budget_exhausted/, { selector: 'p' })).toBeVisible();
  });
  it('authority shows consent usage and scales mandate micro-units without inventing currency', async () => {
    mount(<AuthorityScreen />); await screen.findByRole('heading', { name: 'Spending authority' });
    expect(screen.getByText('Small goodwill refunds')).toBeVisible(); expect(screen.getByText('65 units · 2 calls')).toBeVisible();
    expect(screen.getByText(/5 loaded of 9369/)).toBeVisible();
  });
  it('agents show active and revoked identities and require confirmation for registration', async () => {
    const { submitted } = mount(<AgentsScreen />); await screen.findByRole('heading', { name: 'Agent registry' });
    expect(screen.getByText('Agent tokens are required')).toBeVisible(); expect(screen.getAllByText('refunds-bot')).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'Register agent' }));
    await userEvent.type(screen.getByLabelText('Agent name'), 'new-bot'); await userEvent.type(screen.getByLabelText('Tool scopes, one per line'), 'stripe/*');
    await userEvent.click(screen.getByRole('button', { name: 'Review registration' }));
    expect(screen.getByRole('dialog')).toBeVisible(); expect(submitted).toHaveLength(0);
  });
  it('evidence exposes the verifier limits and does not invent signatures', async () => {
    mount(<EvidenceScreen />); expect(await screen.findByText('Evidence intact: 236 records')).toBeVisible();
    expect(screen.getByText('No · standard verification')).toBeVisible();
    expect(screen.getByText('Signed decisions checked').nextElementSibling).toHaveTextContent('0');
  });
  it('overview links to decisions and labels bounded authorization totals', async () => {
    mount(<OverviewScreen />); await screen.findByRole('heading', { name: 'Workspace overview' });
    expect(screen.getByRole('link', { name: /waiting for approval/i })).toHaveAttribute('href', '/approvals');
    expect(screen.getByText(/Totals summarize authorizations/)).toBeVisible();
  });
});
