import 'server-only';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { z } from 'zod';
import { approvalSchema, approvalsSchema, agentsSchema, callsSchema, consentsSchema, declarationsSchema,
  mandatesSchema, verifySchema, type ConsoleAction, type Snapshot, type ActionResult } from './contracts';
import { buildCommand } from './commands';
import { approvalView } from './display';
import { redact, redactText } from './redact';

const execute = promisify(execFile);
const files = { approvals: 'inbox_approvals.json', calls: 'evidence_calls.json', agents: 'agent_list.json',
  mandates: 'inbox_mandates.json', verification: 'evidence_verify.json', consents: 'agentauth_consents.json', declarations: 'payments_list.json' };
const readArgs = { approvals: ['approvals', '--all'], calls: ['evidence', 'calls', '-n', '200'],
  agents: ['agent', 'list'], mandates: ['mandate', 'list'], verification: ['evidence', 'verify'], declarations: ['payments', 'list'] };
export const fixtureMode = () => process.env.CONSOLE_FIXTURES === '1';

function safeMessage(message: string) {
  let safe = redactText(message);
  for (const name of ['AGENTAUTH_API_KEY', 'CONSOLE_INTERNAL_KEY']) {
    const value = process.env[name];
    if (value) safe = safe.split(value).join('[redacted]');
  }
  return safe;
}
export async function runFabric(args: string[]): Promise<unknown> {
  const cwd = process.env.CONSOLE_PROJECT;
  if (!cwd || !path.isAbsolute(cwd)) throw new Error('Configure an absolute CONSOLE_PROJECT directory.');
  let stdout = '', failed = false, failureCode: number | string | undefined;
  try {
    const result = await execute(process.env.CONSOLE_FABRIC || 'cosigna', args, {
      cwd, shell: false, windowsHide: true, timeout: 20000, maxBuffer: 4 * 1024 * 1024,
    });
    stdout = result.stdout;
  } catch (error) {
    const failure = error as { stdout?: string; code?: number | string };
    if (!failure.stdout) throw new Error('Cosigna could not complete the command. Check its installation, project permissions and timeout.');
    stdout = failure.stdout;
    failed = true; failureCode = failure.code;
  }
  let value: unknown;
  try { value = JSON.parse(stdout); } catch { throw new Error('Cosigna returned invalid JSON. No response fields were inferred.'); }
  const failure = z.object({ schema: z.literal('fabric.error/v1'), error: z.object({ message: z.string() }) }).safeParse(value);
  if (failure.success) throw new Error(safeMessage(failure.data.error.message));
  if (failed && !(args[0] === 'evidence' && args[1] === 'verify' && failureCode === 5))
    throw new Error('Cosigna reported a failure. Refresh the record before attempting another decision.');
  return value;
}
function validate<T>(schema: z.ZodType<T>, data: unknown): T {
  const result = schema.safeParse(data);
  if (!result.success) throw new Error('Unsupported engine schema or invalid response. Update the console contract before continuing.');
  return result.data;
}
async function read(name: keyof typeof files) {
  if (fixtureMode()) return JSON.parse(await readFile(path.join(process.cwd(), 'fixtures', files[name]), 'utf8')) as unknown;
  if (name === 'consents') {
    const url = new URL(process.env.AGENTAUTH_URL || '');
    if (url.username || url.password || (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))))
      throw new Error('The consent service URL must use HTTPS or a loopback HTTP address.');
    if (!process.env.AGENTAUTH_API_KEY) throw new Error('Configure the server-side consent service key to load consents.');
    const response = await fetch(new URL('/v1/consents?limit=100&offset=0', url), {
      headers: { 'X-API-Key': process.env.AGENTAUTH_API_KEY }, cache: 'no-store', signal: AbortSignal.timeout(10000),
    });
    if (!response.ok) throw new Error(`Cosigna could not list consents (HTTP ${response.status}).`);
    return response.json() as Promise<unknown>;
  }
  return runFabric([...readArgs[name], '--json']);
}
type FixtureState = { approvals?: z.infer<typeof approvalsSchema>; agents?: z.infer<typeof agentsSchema>; mandates?: z.infer<typeof mandatesSchema> };
const shared = globalThis as typeof globalThis & { consoleFixtures?: Map<string, FixtureState> };
const states: Map<string, FixtureState> = shared.consoleFixtures ??= new Map<string, FixtureState>();
function state(session: string): FixtureState { let value = states.get(session); if (!value) { value = {}; states.set(session, value); } return value; }
async function approvals(session: string) {
  const value = fixtureMode() ? state(session).approvals : undefined;
  return value || validate(approvalsSchema, await read('approvals'));
}
export async function snapshot(session: { id: string; user: string; csrf: string }): Promise<Snapshot> {
  const errors: Record<string, string> = {};
  async function load<T>(name: keyof typeof files, schema: z.ZodType<T>): Promise<T | null> {
    try {
      const override = fixtureMode() && name in state(session.id) ? state(session.id)[name as keyof FixtureState] : undefined;
      return validate(schema, override || await read(name));
    } catch (error) { errors[name] = safeMessage(error instanceof Error ? error.message : 'Could not load engine data.'); return null; }
  }
  const [inbox, activity, agents, mandates, verification, consents, declarations] = await Promise.all([
    load('approvals', approvalsSchema), load('calls', callsSchema), load('agents', agentsSchema),
    load('mandates', mandatesSchema), load('verification', verifySchema), load('consents', consentsSchema), load('declarations', declarationsSchema),
  ]);
  const data: Snapshot = { user: session.user, csrf: session.csrf, fixtures: fixtureMode(), now: new Date().toISOString(),
    approvals: (inbox?.requests || []).map(r => approvalView(r, declarations?.payments || [], fixtureMode())),
    calls: activity?.calls || [], agents: agents?.agents || [], required: agents?.required ?? false,
    mandates: mandates?.mandates || [], verification, consents: consents?.consents || [], consentTotal: consents?.total || 0,
    errors, exportEnabled: fixtureMode() || !!process.env.CONSOLE_EXPORT_DIR };
  // Session/CSRF values are returned intentionally. Redact only engine-derived data.
  return { ...data, approvals: redact(data.approvals) as Snapshot['approvals'], calls: redact(data.calls) as Snapshot['calls'],
    agents: redact(data.agents) as Snapshot['agents'], mandates: redact(data.mandates) as Snapshot['mandates'],
    verification: redact(data.verification) as Snapshot['verification'], consents: redact(data.consents) as Snapshot['consents'] };
}
export async function perform(action: ConsoleAction, session: { id: string; user: string }): Promise<ActionResult> {
  if (session.user !== process.env.CONSOLE_USER) throw new Error('Console identity changed. Restart and sign in again.');
  if (action.action === 'approve') {
    const request = (await approvals(session.id)).requests.find(r => r.id === action.id);
    if (!request || request.status !== 'pending' || request.digest !== action.digest)
      throw new Error('This request changed or was already decided. Refresh the inbox before deciding.');
  }
  let out: string | undefined;
  if (action.action === 'evidence-export') {
    const root = process.env.CONSOLE_EXPORT_DIR;
    if (!fixtureMode() && (!root || !path.isAbsolute(root))) throw new Error('Configure an absolute CONSOLE_EXPORT_DIR before exporting.');
    out = path.join(root || path.join(process.cwd(), 'fixture-export'), `evidence-${Date.now()}-${randomBytes(4).toString('hex')}`);
  }
  const args = buildCommand(action, session.user, out);
  if (fixtureMode()) {
    const memory = state(session.id), now = new Date().toISOString();
    if (action.action === 'approve') {
      memory.approvals ||= await approvals(session.id);
      const request = memory.approvals.requests.find(r => r.id === action.id)!;
      request.status = action.decision === 'approve' ? 'approved' : 'rejected';
      request.approver = session.user; request.decided_at = now; request.note = action.note || null;
    }
    if (action.action === 'agent-add' || action.action === 'agent-revoke') {
      memory.agents ||= validate(agentsSchema, await read('agents'));
      if (action.action === 'agent-add') {
        if (memory.agents.agents.some(a => a.name === action.name && !a.revoked_at)) throw new Error('An active agent already has this name.');
        memory.agents.agents.push({ name: action.name, tools: action.tools, created_at: now, revoked_at: null });
      } else {
        const agent = memory.agents.agents.find(a => a.name === action.name && !a.revoked_at);
        if (!agent) throw new Error('No active agent with this name.');
        agent.revoked_at = now;
      }
    }
    if (action.action === 'mandate-grant' || action.action === 'mandate-revoke') {
      memory.mandates ||= validate(mandatesSchema, await read('mandates'));
      if (action.action === 'mandate-grant') memory.mandates.mandates.push({
        id: Math.max(0, ...memory.mandates.mandates.map(m => m.id)) + 1, granted_at: now, granted_by: session.user,
        revoked_at: null, terms: { title: action.title, tools: action.tools, agent: action.agent || null,
          expires: action.expires || null, max_per_call: action.maxPerCall || null,
          max_per_day: action.maxPerDay || null, max_calls_per_day: action.maxCallsPerDay || null }, uses: [],
      });
      else {
        const mandate = memory.mandates.mandates.find(m => m.id === action.id && !m.revoked_at);
        if (!mandate) throw new Error('This mandate is no longer active.');
        mandate.revoked_at = now;
      }
    }
    return { simulated: true, args: redact(args) as string[], message: 'Simulation only. No engine command ran and no money moved.',
      ...(action.action === 'agent-add' ? { token: `fixture-only-${randomBytes(24).toString('hex')}` } : {}),
      ...(out ? { out, message: 'Simulation only. No signed pack was created; a real fingerprint is unavailable.' } : {}) };
  }
  const value = await runFabric(args);
  const schemas = { approve: 'fabric.approve/v2', 'agent-add': 'fabric.agent.add/v1', 'agent-revoke': 'fabric.agent.revoke/v1',
    'mandate-grant': 'fabric.mandate.grant/v1', 'mandate-revoke': 'fabric.mandate.revoke/v1', 'evidence-export': 'fabric.evidence.export/v1' };
  validate(z.object({ schema: z.literal(schemas[action.action]) }), value);
  if (action.action === 'approve') {
    const receipt = validate(z.object({ requests: z.array(approvalSchema) }), value);
    const decided = receipt.requests.find(r => r.id === action.id && r.digest === action.digest);
    if (!decided || decided.status !== (action.decision === 'approve' ? 'approved' : 'rejected') || decided.approver !== session.user)
      throw new Error('The engine receipt did not match the requested decision. Refresh the record before retrying.');
  }
  const result: ActionResult = { simulated: false, args: redact(args) as string[], message: 'Recorded by the engine.' };
  if (action.action === 'agent-add') result.token = validate(z.object({ token: z.string().min(1) }), value).token;
  if (action.action === 'evidence-export') {
    const exported = validate(z.object({ out: z.string(), fingerprint: z.string() }), value);
    result.out = exported.out; result.fingerprint = exported.fingerprint;
    const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
    result.verifyCommand = `& cosigna evidence verify-pack ${quote(exported.out)} --key ${quote(exported.fingerprint)}`;
  }
  return result;
}
