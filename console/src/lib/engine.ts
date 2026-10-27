import 'server-only';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { randomBytes } from 'node:crypto';
import { z } from 'zod';
import { approvalSchema, approvalsSchema, agentsSchema, callsSchema, consentsSchema, declarationsSchema,
  gatewayAgentsSchema, gatewayExposureSchema, gatewayFixtureSchema, timelineSchema, gatewayOperationSchema, gatewayOperationsSchema,
  mandatesSchema, verifySchema, type ConsoleAction, type Snapshot, type ActionResult, type Exposure, type GatewayOperation, type Timeline } from './contracts';
import { buildCommand } from './commands';
import { approvalView, operationViews } from './display';
import { redact, redactText } from './redact';
import { recall } from '../../server/approvers.mjs';

/** A signed-in console session: the configured owner, or a named approver signed in with their own key. */
type Session = { id: string; user: string; csrf: string; role?: 'owner' | 'approver' };
const isApprover = (session: { role?: string }) => session.role === 'approver';

const execute = promisify(execFile);
const files = { approvals: 'inbox_approvals.json', calls: 'evidence_calls.json', agents: 'agent_list.json',
  mandates: 'inbox_mandates.json', verification: 'evidence_verify.json', consents: 'agentauth_consents.json', declarations: 'payments_list.json' };
const readArgs = { approvals: ['approvals', '--all'], calls: ['evidence', 'calls', '-n', '200'],
  agents: ['agent', 'list'], mandates: ['mandate', 'list'], verification: ['evidence', 'verify'], declarations: ['payments', 'list'] };
export const fixtureMode = () => process.env.CONSOLE_FIXTURES === '1';
/** A server deployment may have a gateway and no Fabric project: its sections are then left out, not errors. */
export const fabricConfigured = () => fixtureMode() || !!process.env.CONSOLE_PROJECT;

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
type GatewayData = { operations: GatewayOperation[]; agents: { id: string; name: string }[] };
type FixtureState = { approvals?: z.infer<typeof approvalsSchema>; agents?: z.infer<typeof agentsSchema>; mandates?: z.infer<typeof mandatesSchema>; gateway?: GatewayData };

export const gatewayConfigured = () => fixtureMode() || !!process.env.CONSOLE_GATEWAY_URL;
/** Call to the Cosigna gateway as this session's person: the owner credential (read here, server-side) or the
 * approver's own key (held in server memory for the session). Neither is ever returned to the browser. */
async function gatewayCall(session: { id: string; role?: string }, method: 'GET' | 'POST', route: string, body?: unknown): Promise<unknown> {
  let base: URL;
  try { base = new URL(process.env.CONSOLE_GATEWAY_URL || ''); } catch { throw new Error('Configure CONSOLE_GATEWAY_URL as http://127.0.0.1:<port>.'); }
  if (base.protocol !== 'http:' || base.hostname !== '127.0.0.1' || !base.port || base.username || base.password)
    throw new Error('CONSOLE_GATEWAY_URL must be a loopback address: http://127.0.0.1:<port>.');
  let credential: string;
  if (isApprover(session)) {
    const approver = recall(session.id) as { key: string } | null;
    if (!approver) throw new Error('Your approver session ended. Sign in again.');
    credential = approver.key;
  } else {
    const dir = process.env.CONSOLE_GATEWAY_DIR;
    if (!dir || !path.isAbsolute(dir)) throw new Error('Configure an absolute CONSOLE_GATEWAY_DIR (the gateway profile holding credentials.json).');
    try { credential = z.object({ owner: z.string().regex(/^owner_[0-9a-f]{64}$/) }).passthrough().parse(JSON.parse(await readFile(path.join(dir, 'credentials.json'), 'utf8'))).owner; }
    catch { throw new Error('Could not read the gateway owner credential from CONSOLE_GATEWAY_DIR.'); }
  }
  const response = await fetch(new URL(`/gateway/v1${route}`, base), { method, cache: 'no-store', redirect: 'error',
    signal: AbortSignal.timeout(30000), body: body === undefined ? undefined : JSON.stringify(body),
    headers: { Authorization: `Bearer ${credential}`, ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) } });
  const value: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const message = z.object({ error: z.object({ message: z.string() }) }).safeParse(value);
    throw new Error(safeMessage(message.success ? `The gateway refused this: ${message.data.error.message}` : `The gateway refused this (HTTP ${response.status}).`));
  }
  return value;
}
async function gateway(session: { id: string; role?: string }): Promise<GatewayData> {
  if (fixtureMode()) {
    const memory = state(session.id);
    if (!memory.gateway) {
      const raw = gatewayFixtureSchema.parse(JSON.parse(await readFile(path.join(process.cwd(), 'fixtures', 'gateway_operations.json'), 'utf8')));
      memory.gateway = { operations: raw.operations.data, agents: raw.agents.data };
    }
    return memory.gateway;
  }
  const [operations, agents] = await Promise.all([gatewayCall(session, 'GET', '/operations'), gatewayCall(session, 'GET', '/agents')]);
  return { operations: validate(gatewayOperationsSchema, operations).data, agents: validate(gatewayAgentsSchema, agents).data };
}
/** What the owner's agents can move without a person. Read-only; fixtures use a report captured from the gateway. */
async function exposure(session: { id: string; role?: string }): Promise<Exposure> {
  const value = fixtureMode()
    ? JSON.parse(await readFile(path.join(process.cwd(), 'fixtures', 'gateway_exposure.json'), 'utf8')) as unknown
    : await gatewayCall(session, 'GET', '/exposure');
  return validate(gatewayExposureSchema, value).data;
}
const shared = globalThis as typeof globalThis & { consoleFixtures?: Map<string, FixtureState> };
const states: Map<string, FixtureState> = shared.consoleFixtures ??= new Map<string, FixtureState>();
function state(session: string): FixtureState { let value = states.get(session); if (!value) { value = {}; states.set(session, value); } return value; }
async function approvals(session: string) {
  const value = fixtureMode() ? state(session).approvals : undefined;
  return value || validate(approvalsSchema, await read('approvals'));
}
export async function snapshot(session: Session): Promise<Snapshot> {
  if (isApprover(session)) return approverSnapshot(session);
  const errors: Record<string, string> = {};
  async function load<T>(name: keyof typeof files, schema: z.ZodType<T>): Promise<T | null> {
    if (!fabricConfigured()) return null;
    try {
      const override = fixtureMode() && name in state(session.id) ? state(session.id)[name as keyof FixtureState] : undefined;
      return validate(schema, override || await read(name));
    } catch (error) { errors[name] = safeMessage(error instanceof Error ? error.message : 'Could not load engine data.'); return null; }
  }
  const [inbox, activity, agents, mandates, verification, consents, declarations] = await Promise.all([
    load('approvals', approvalsSchema), load('calls', callsSchema), load('agents', agentsSchema),
    load('mandates', mandatesSchema), load('verification', verifySchema), load('consents', consentsSchema), load('declarations', declarationsSchema),
  ]);
  let operations: Snapshot['operations'] = [], exposed: Exposure | null = null;
  if (gatewayConfigured()) {
    try { const value = await gateway(session); operations = operationViews(value.operations, value.agents); }
    catch (error) { errors.operations = safeMessage(error instanceof Error ? error.message : 'Could not load gateway operations.'); }
    try { exposed = await exposure(session); }
    catch (error) { errors.exposure = safeMessage(error instanceof Error ? error.message : 'Could not load what agents can pay without you.'); }
  }
  const data: Snapshot = { user: session.user, csrf: session.csrf, fixtures: fixtureMode(), now: new Date().toISOString(),
    role: 'owner', approver: null, operations, gatewayConfigured: gatewayConfigured(), exposure: exposed,
    approvals: (inbox?.requests || []).map(r => approvalView(r, declarations?.payments || [], fixtureMode())),
    calls: activity?.calls || [], agents: agents?.agents || [], required: agents?.required ?? false,
    mandates: mandates?.mandates || [], verification, consents: consents?.consents || [], consentTotal: consents?.total || 0,
    errors, exportEnabled: fixtureMode() || !!process.env.CONSOLE_EXPORT_DIR };
  // Session/CSRF values are returned intentionally. Redact only engine-derived data.
  return { ...data, approvals: redact(data.approvals) as Snapshot['approvals'], calls: redact(data.calls) as Snapshot['calls'],
    agents: redact(data.agents) as Snapshot['agents'], mandates: redact(data.mandates) as Snapshot['mandates'],
    verification: redact(data.verification) as Snapshot['verification'], consents: redact(data.consents) as Snapshot['consents'],
    operations: redact(data.operations) as Snapshot['operations'], exposure: redact(data.exposure) as Snapshot['exposure'] };
}

/** One payment's story from the gateway's evidence, as this session's person. Needs a live gateway. */
export async function timeline(session: Session, id: string): Promise<Timeline> {
  if (!/^op_[0-9a-f]{64}$/.test(id)) throw new Error('Unknown payment.');
  if (fixtureMode()) throw new Error('Payment timelines are read from a live Cosigna gateway; fixtures mode has none.');
  return redact(validate(timelineSchema, await gatewayCall(session, 'GET', `/operations/${id}/timeline`)).data) as Timeline;
}

/** A named approver sees only the agent payments their gateway key can read and decide; no local engine data. */
async function approverSnapshot(session: Session): Promise<Snapshot> {
  const errors: Record<string, string> = {};
  const approver = recall(session.id) as { name: string; currency: string; max_approval_minor: number } | null;
  let operations: Snapshot['operations'] = [];
  try { const value = await gateway(session); operations = operationViews(value.operations, value.agents); }
  catch (error) { errors.operations = safeMessage(error instanceof Error ? error.message : 'Could not load gateway operations.'); }
  return { user: session.user, csrf: session.csrf, fixtures: false, now: new Date().toISOString(), role: 'approver',
    approver: approver && { name: approver.name, currency: approver.currency, max_approval_minor: approver.max_approval_minor },
    operations: redact(operations) as Snapshot['operations'], gatewayConfigured: true, exposure: null,
    approvals: [], calls: [], agents: [], required: false, mandates: [], verification: null, consents: [], consentTotal: 0,
    errors, exportEnabled: false };
}

/** Decides or settles a gateway operation after re-checking its current state, then checks the gateway's result. */
async function performOperation(action: Extract<ConsoleAction, { action: 'operation-decide' | 'operation-resolve' }>,
  session: Session): Promise<ActionResult> {
  const current = (await gateway(session)).operations.find(op => op.data.id === action.id);
  if (action.action === 'operation-decide' && (!current || current.data.stage !== 'held' || current.proposal_digest !== action.digest))
    throw new Error('This payment changed or was already decided. Refresh the inbox before deciding.');
  if (action.action === 'operation-resolve' && (!current || !['unknown', 'submitting'].includes(current.data.stage)))
    throw new Error('This payment is no longer waiting to be settled. Refresh the inbox.');
  const route = action.action === 'operation-resolve' ? 'resolve' : action.decision;
  const args = [`POST /gateway/v1/operations/${action.id}/${route}`];
  if (fixtureMode()) {
    const op = current!;
    if (route === 'approve') {
      op.data.stage = 'succeeded'; op.data.approved_by = 'owner';
      op.data.receipt = { rail_reference: 're_fixture_simulated', succeeded: true };
    } else if (route === 'reject') op.data.stage = 'rejected';
    else op.data.stage = 'failed';
    op.needs_owner = false;
    return { simulated: true, args, message: 'Simulation only. No gateway call was made and no money moved.' };
  }
  const value = validate(gatewayOperationSchema, await gatewayCall(session, 'POST', `/operations/${action.id}/${route}`,
    action.action === 'operation-decide' && route === 'approve' ? { proposal_digest: action.digest } : {}));
  const stage = value.data.stage;
  // Above a budget's two-person threshold this approval may be the first of two: it stays held.
  const as = isApprover(session) ? 'approver' : 'owner';
  const firstOfTwo = route === 'approve' && stage === 'held' && value.data.first_approval?.by === as;
  if (value.data.id !== action.id || (route === 'approve' && !firstOfTwo && value.data.approved_by !== as) || (route === 'reject' && stage !== 'rejected'))
    throw new Error('The gateway result did not match the requested decision. Refresh the record before retrying.');
  if (firstOfTwo) return { simulated: false, args,
    message: 'Recorded by the Cosigna gateway as the first of two approvals. A second person must approve before it is paid.' };
  const outcome: Record<string, string> = { succeeded: 'Approved and paid.', failed: 'The payment system refused it; no money moved and the hold was released.',
    unknown: 'Approved; the payment result is not known yet. It stays held until settled.', submitting: 'Approved; the payment is being submitted.',
    rejected: 'Rejected. No money moved.' };
  return { simulated: false, args, message: `Recorded by the Cosigna gateway. ${outcome[stage] || ''}`.trim() };
}
export async function perform(action: ConsoleAction, session: Session): Promise<ActionResult> {
  if (isApprover(session)) {
    // A named approver may only decide agent payments; settling and local engine actions stay with the owner.
    if (action.action !== 'operation-decide') throw new Error('Approvers can only approve or reject agent payments.');
    return performOperation(action, session);
  }
  if (session.user !== process.env.CONSOLE_USER) throw new Error('Console identity changed. Restart and sign in again.');
  if (action.action === 'operation-decide' || action.action === 'operation-resolve') return performOperation(action, session);
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
