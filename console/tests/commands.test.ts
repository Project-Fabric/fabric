import { describe, expect, it, vi } from 'vitest';
import path from 'node:path';
import { buildCommand } from '../src/lib/commands';
import { actionSchema, approvalsSchema } from '../src/lib/contracts';
import { approvalView } from '../src/lib/display';
import { redact } from '../src/lib/redact';
import { fixture } from './fixtures';

describe('engine command boundary', () => {
  it('keeps the exact approval command and treats a shell-like note as one argument', () => {
    const note = 'Verified; $(echo injected) & another-command "quoted"\nsecond line';
    const action = actionSchema.parse({ action: 'approve', id: 1, digest: 'fixture-digest', decision: 'approve', note });
    expect(buildCommand(action, 'priya@northwind.test')).toEqual(['approve', '1', '--yes', '--as', 'priya@northwind.test', '--note', note, '--json']);
  });
  it('adds only the reject flag for a rejected request', () => {
    expect(buildCommand({ action: 'approve', id: 1, digest: 'fixture', decision: 'reject', note: '' }, 'priya'))
      .toEqual(['approve', '1', '--yes', '--as', 'priya', '--reject', '--json']);
  });
  it('rejects unknown actions, impersonation fields, invalid ids and CLI option injection', () => {
    for (const input of [{ action: 'secret-set', name: 'key' },
      { action: 'agent-add', name: '--signed', tools: ['stripe/*'] },
      { action: 'mandate-grant', title: '--signed=foreign.json', tools: ['stripe/*'] },
      { action: 'agent-add', name: 'bot', tools: ['--signed=foreign.json'] },
      { action: 'approve', id: -1, digest: 'fixture', note: '', decision: 'approve' },
      { action: 'approve', id: 1, digest: 'fixture', note: '', decision: 'approve', user: 'someone-else' }])
      expect(actionSchema.safeParse(input).success).toBe(false);
  });
  it('refuses unknown schema versions', () => {
    const data = fixture('inbox_approvals.json'); data.schema = 'fabric.approvals/v999';
    expect(approvalsSchema.safeParse(data).success).toBe(false);
  });
  it('does not guess units for an undeclared live approval', () => {
    const request = approvalsSchema.parse(fixture('inbox_approvals.json')).requests[2];
    expect(approvalView(request, [], false).amountLabel).toBe('Amount unavailable');
    expect(approvalView(request, [], true).amountLabel).toBe('$450.00');
  });
  it('redacts nested and serialized secrets but preserves an evidence authorization code', () => {
    const secret = ['sk', 'test', 'syntheticCredential123'].join('_');
    const data = redact({ api_key: 'synthetic-private-value', input: JSON.stringify({ delegation_token: 'private', reason: secret }),
      payment: { authorization_code: 'recorded-reference' } });
    expect(JSON.stringify(data)).not.toContain(secret); expect(JSON.stringify(data)).not.toContain('synthetic-private-value');
    expect(JSON.stringify(data)).not.toContain('"private"'); expect(JSON.stringify(data)).toContain('recorded-reference');
  });
});

const mocks = vi.hoisted(() => ({ execute: vi.fn() }));
vi.mock('node:child_process', async importOriginal => {
  const actual = await importOriginal<typeof import('node:child_process')>();
  const execFile = Object.assign(() => {}, { [Symbol.for('nodejs.util.promisify.custom')]: mocks.execute });
  return { ...actual, execFile, default: { ...actual, execFile } };
});
describe('server execution', () => {
  it('uses execFile with shell disabled, configured cwd, JSON arguments and unmodified agent markers', async () => {
    const cwd = path.resolve('tests/fixture-project');
    process.env.CONSOLE_PROJECT = cwd; process.env.CONSOLE_FABRIC = 'fabric-fixture';
    process.env.CODEX_SANDBOX = 'fixture-marker';
    mocks.execute.mockResolvedValue({ stdout: '{"schema":"fabric.approve/v2"}', stderr: '' });
    const { runFabric } = await import('../src/lib/engine');
    const args = buildCommand({ action: 'approve', id: 1, digest: 'fixture', decision: 'approve', note: 'safe & literal' }, 'priya');
    await runFabric(args);
    expect(mocks.execute).toHaveBeenCalledWith('fabric-fixture', args, expect.objectContaining({ shell: false, cwd, windowsHide: true }));
    expect(process.env.CODEX_SANDBOX).toBe('fixture-marker');
    delete process.env.CODEX_SANDBOX;
  });
  it('surfaces the engine refusal instead of retrying without safeguards', async () => {
    mocks.execute.mockRejectedValue({ stdout: JSON.stringify({ schema: 'fabric.error/v1', error: { message: 'Approvals must be made by a person.' } }) });
    const { runFabric } = await import('../src/lib/engine');
    await expect(runFabric(['approve', '1', '--yes', '--json'])).rejects.toThrow('Approvals must be made by a person.');
  });
});
