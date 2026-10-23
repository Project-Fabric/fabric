import { advanceTask, beginTask, parseTaskBudget, taskBlocker, taskExamples, taskTotal, type TaskState } from '../src/domain/task-fixtures';
import { initialTasks, tasksReducer } from '../src/state/tasks';

const approve = (task: TaskState) => {
  const review = advanceTask(task, { type: 'review' });
  return advanceTask(review, { type: 'approve', quoteVersion: review.quoteVersion });
};
describe('mobile fixture task rules', () => {
  it.each(taskExamples)('requires exact approval before completing $kind', example => {
    const task = beginTask(example);
    expect(advanceTask(task, { type: 'complete' })).toBe(task);
    expect(advanceTask(task, { type: 'approve', quoteVersion: task.quoteVersion })).toBe(task);
    const approved = approve(task);
    expect(approved.stage).toBe('approved');
    const completed = advanceTask(approved, { type: 'complete' });
    expect(completed.stage).toBe('completed');
    expect(advanceTask(completed, { type: 'complete' })).toBe(completed);
  });
  it.each(taskExamples)('blocks an affordable $kind option that misses a requirement', example => {
    const task = advanceTask(beginTask(example), { type: 'select', id: example.options[1].id });
    expect(taskTotal(task)).toBeLessThan(task.budget);
    expect(taskBlocker(task)).toBe(example.options[1].mismatch);
    expect(advanceTask(task, { type: 'review' })).toBe(task);
  });
  it('applies exact money limits, including the boundary', () => {
    let task = beginTask(taskExamples[0]);
    task = advanceTask(task, { type: 'budget', amount: taskTotal(task) - 1 });
    expect(taskBlocker(task)).toContain('exceeds');
    task = advanceTask(task, { type: 'budget', amount: taskTotal(task) });
    expect(taskBlocker(task)).toBeNull();
    expect(approve(task).stage).toBe('approved');
  });
  it('invalidates approval after a price change and rejects a stale confirmation', () => {
    let task = approve(beginTask(taskExamples[0]));
    const oldVersion = task.quoteVersion;
    task = advanceTask(task, { type: 'price-change' });
    expect(task.approvedVersion).toBeNull();
    expect(taskTotal(task)).toBe(1349900);
    expect(advanceTask(task, { type: 'complete' })).toBe(task);
    task = advanceTask(task, { type: 'review' });
    expect(advanceTask(task, { type: 'approve', quoteVersion: oldVersion })).toBe(task);
    expect(advanceTask(task, { type: 'approve', quoteVersion: task.quoteVersion }).stage).toBe('approved');
  });
  it('holds an unknown outcome until reconciliation, without a second submit', () => {
    const task = advanceTask(approve(beginTask(taskExamples[1])), { type: 'unknown' });
    for (const action of ['complete', 'review', 'reject', 'price-change'] as const)
      expect(advanceTask(task, { type: action })).toBe(task);
    expect(advanceTask(task, { type: 'reconcile' }).events.at(-1)).toContain('No second order');
  });
  it('rejects invalid inputs and treats unknown task IDs as unavailable', () => {
    const task = beginTask(taskExamples[0]);
    for (const amount of [NaN, Infinity, -1, 0, 1.1]) expect(advanceTask(task, { type: 'budget', amount })).toBe(task);
    expect(advanceTask(task, { type: 'select', id: 'untrusted-id' })).toBe(task);
    const tasks = initialTasks();
    expect(tasksReducer(tasks, { id: '__proto__', action: { type: 'review' } })).toBe(tasks);
  });
  it('keeps authority scoped to each task and resets both', () => {
    const tasks = initialTasks();
    const next = tasksReducer(tasks, { id: 'headphones', action: { type: 'review' } });
    expect(next.delhi).toBe(tasks.delhi);
    expect(next.headphones.stage).toBe('review');
    expect(tasksReducer(next, { reset: true })).toEqual(initialTasks());
  });
  it('declining a proposal cannot lead to completion', () => {
    const task = advanceTask(beginTask(taskExamples[0]), { type: 'reject' });
    expect(approve(task)).toBe(task);
    expect(advanceTask(task, { type: 'complete' })).toBe(task);
  });
  it('parses decimal input without floating point rounding', () => {
    expect(parseTaskBudget('0.01')).toBe(1);
    expect(parseTaskBudget('12999.90')).toBe(1299990);
    for (const value of ['', '0', '-1', '1e4', '12.345', 'Infinity', '1,000', ' 10']) expect(parseTaskBudget(value)).toBeNull();
  });
});
