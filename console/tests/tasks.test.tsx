import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TasksScreen } from '../src/components/tasks';
import { advanceTask, beginTask, parseTaskBudget, taskExamples } from '../src/lib/task-fixtures';

describe('personal task preview', () => {
  it('requires two approval steps and completes without any network call', async () => {
    const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
    render(<TasksScreen />); const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Review proposal' }));
    await user.click(screen.getByRole('button', { name: 'Approve demo proposal' }));
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('INR 12,999.00');
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('region', { name: 'Execution simulation' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Approve demo proposal' }));
    await user.click(screen.getByRole('button', { name: 'Confirm demo approval' }));
    await user.click(screen.getByRole('button', { name: /simulate success/i }));
    expect(screen.getByRole('region', { name: 'Task result' })).toHaveTextContent('None. This is a simulation.');
    expect(fetch).not.toHaveBeenCalled();
  });
  it('blocks an affordable flight that fails requirements and blocks an over-budget offer', async () => {
    render(<TasksScreen />); const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /plan my flight to delhi/i }));
    await user.click(screen.getAllByRole('radio')[1]);
    expect(screen.getByRole('alert')).toHaveTextContent('Checked baggage is missing');
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeDisabled();
    await user.click(screen.getAllByRole('radio')[0]);
    await user.clear(screen.getByLabelText('Example budget (INR)'));
    await user.type(screen.getByLabelText('Example budget (INR)'), '10000');
    await user.click(screen.getByRole('button', { name: 'Apply demo budget' }));
    expect(screen.getByRole('alert')).toHaveTextContent('exceeds your task budget');
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeDisabled();
  });
  it('requires new approval after a price change and resolves an unknown outcome without resubmission', () => {
    let task = beginTask(taskExamples[0]);
    expect(advanceTask(task, { type: 'complete' })).toEqual(task);
    task = advanceTask(advanceTask(task, { type: 'review' }), { type: 'approve' });
    task = advanceTask(task, { type: 'price-change' });
    expect(task.approvedVersion).toBeNull();
    expect(advanceTask(task, { type: 'complete' })).toEqual(task);
    task = advanceTask(advanceTask(task, { type: 'review' }), { type: 'approve' });
    task = advanceTask(task, { type: 'unknown' });
    expect(advanceTask(task, { type: 'complete' })).toEqual(task);
    expect(advanceTask(task, { type: 'approve' })).toEqual(task);
    task = advanceTask(task, { type: 'reconcile' });
    expect(task.stage).toBe('completed');
    expect(task.events.at(-1)).toContain('No second order submitted');
    expect(advanceTask(task, { type: 'reconcile' })).toEqual(task);
  });
  it('declines, resets and changes examples without carrying approval between tasks', async () => {
    render(<TasksScreen />); const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Decline proposal' }));
    expect(screen.getByRole('region', { name: 'Task result' })).toHaveTextContent('No order submitted');
    await user.click(screen.getByRole('button', { name: 'Reset example' }));
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: 'Review proposal' }));
    await user.click(screen.getByRole('button', { name: /plan my flight to delhi/i }));
    expect(screen.queryByRole('region', { name: 'Proposal review' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Review proposal' })).toBeEnabled();
  });
  it('parses money exactly and refuses malformed or non-positive budgets', () => {
    expect(parseTaskBudget('12000.01')).toBe(1200001);
    for (const input of ['0', '-1', '1e4', '12.345', 'NaN', '', 'Infinity']) expect(parseTaskBudget(input)).toBeNull();
  });
});
