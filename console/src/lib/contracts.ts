import { z } from 'zod';

const string = z.string(), nullable = string.nullable().optional();
export const approvalSchema = z.object({
  id: z.number().int().positive(), digest: string, rule: string, reason: string,
  agent: string, session: nullable, tool: string, target: string, input: z.record(z.unknown()),
  requested_at: string, requested_os_user: nullable,
  status: z.enum(['pending', 'approved', 'rejected', 'used', 'expired']),
  approver: nullable, approver_os_user: nullable, decided_at: nullable, note: nullable,
}).passthrough();
export const approvalsSchema = z.object({ schema: z.literal('fabric.approvals/v1'), requests: z.array(approvalSchema) });
export const paymentSchema = z.object({ amount: string, currency: string, merchant: nullable,
  decision: z.enum(['ALLOW', 'DENY', 'STEP_UP', 'UNAVAILABLE']), authorization_code: nullable, consent: nullable, reason: nullable }).passthrough();
export const callsSchema = z.object({ schema: z.literal('fabric.evidence.calls/v1'), calls: z.array(z.object({
  record: z.number().int(), at: string, agent: string, session: nullable, tool: string,
  arguments: nullable, decision: z.enum(['allow', 'approve', 'deny', 'ask']), outcome: string,
  rule: nullable, result: nullable, payment: paymentSchema.nullable().optional(),
}).passthrough()) });
export const agentsSchema = z.object({ schema: z.literal('fabric.agent.list/v1'), required: z.boolean(),
  agents: z.array(z.object({ name: string, tools: z.array(string), created_at: string, revoked_at: nullable }).passthrough()) });
export const mandatesSchema = z.object({ schema: z.literal('fabric.mandate.list/v1'), mandates: z.array(z.object({
  id: z.number().int(), granted_at: string, granted_by: string, revoked_at: nullable,
  terms: z.object({ title: string, tools: z.array(string), agent: nullable, expires: nullable,
    max_per_call: nullable, max_per_day: nullable, max_calls_per_day: z.number().nullable().optional() }).passthrough(),
  uses: z.array(z.object({ day: string, amount: z.number().nullable(), tool: string, at_unix: z.number() }).passthrough()),
}).passthrough()) });
export const verifySchema = z.object({ schema: z.literal('fabric.evidence.verify/v1'), deep: z.boolean(),
  chain: z.object({ head: string.nullable(), ok: z.boolean(), records: z.number(), incomplete_tail: z.boolean(), problems: z.array(string) }),
  exceptions: z.array(z.unknown()), signed_decisions: z.object({ checked: z.number(), problems: z.array(string) }),
  mandate_uses: z.object({ checked: z.number(), problems: z.array(string) }),
});
export const consentsSchema = z.object({ consents: z.array(z.object({
  consent_id: string, user_id: string, intent_description: string,
  constraints: z.object({ max_amount: z.number(), currency: string }).passthrough(),
  scope: z.object({ single_use: z.boolean() }).passthrough(), is_active: z.boolean(),
  created_at: nullable, expires_at: nullable,
}).passthrough()), total: z.number(), limit: z.number(), offset: z.number() });
export const declarationsSchema = z.object({ schema: z.literal('fabric.payments.list/v1'),
  payments: z.array(z.object({ tool: string, amount: string, minor_units: z.boolean(),
    currency: nullable, default_currency: nullable, merchant: nullable,
    when: z.array(z.object({ field: string, equals: string })).optional(),
  }).passthrough()) });
// Cosigna gateway money operations (GET /gateway/v1/operations, schema cosigna.operation/v1).
const opId = z.string().regex(/^op_[0-9a-f]{64}$/);
const hex64 = z.string().regex(/^[0-9a-f]{64}$/);
export const gatewayOperationSchema = z.object({
  schema: z.literal('cosigna.operation/v1'), simulated_rail: z.boolean(), needs_owner: z.boolean(),
  proposal_digest: hex64.nullable(),
  data: z.object({
    id: opId, agent: string.nullable(), version: z.number().int(),
    request: z.object({ kind: z.enum(['refund', 'payout']), rail: string, source: nullable, payee: nullable,
      amount_minor: z.number().int().positive(), currency: string, reason: string, budget_id: string }).passthrough(),
    stage: z.enum(['held', 'approved', 'submitting', 'unknown', 'succeeded', 'failed', 'denied', 'rejected']),
    decision: z.object({ outcome: z.enum(['auto_approved', 'held', 'denied']), rule: string }),
    proposal: z.object({ facts: z.record(z.unknown()), expires_at: z.number(), amount_minor: z.number().int(),
      currency: string }).passthrough().nullable(),
    approved_by: nullable,
    // The first of two required approvals (gateway ledger version 8).
    first_approval: z.object({ by: z.enum(['owner', 'approver']), approver: string.optional(), digest: hex64, at: z.number() })
      .passthrough().nullable().optional(),
    receipt: z.object({ rail_reference: string, succeeded: z.boolean() }).passthrough().nullable(),
  }).passthrough(),
});
export const gatewayOperationsSchema = z.object({ schema: z.literal('cosigna.operation/v1'), data: z.array(gatewayOperationSchema) });
export const gatewayAgentsSchema = z.object({ data: z.array(z.object({ id: string, name: string }).passthrough()) }).passthrough();
// Worst-case exposure (GET /gateway/v1/exposure, schema cosigna.exposure/v1): the most agents can move without a person.
const minorAmount = z.number().int().nonnegative();
const limit = z.enum(['budget', 'cases', 'no_grant', 'agent_inactive', 'budget_inactive', 'no_automatic_room']);
export const gatewayExposureSchema = z.object({ schema: z.literal('cosigna.exposure/v1'), data: z.object({
  at: z.number().int(), notes: z.array(string),
  currencies: z.array(z.object({ currency: string, without_a_person_minor: minorAmount, approved_minor: minorAmount, open_cases_minor: minorAmount })),
  budgets: z.array(z.object({ budget_id: string, currency: string, available_minor: z.number().int(), approved_minor: minorAmount,
    exposure_minor: minorAmount, limited_by: limit, agents: z.array(string), until: z.number().int().optional() }).passthrough()),
  agents: z.array(z.object({ agent_id: string, name: string, budget_id: string, currency: string, exposure_minor: minorAmount,
    largest_single_minor: minorAmount, limited_by: limit, must_name_case: z.boolean(),
    if_case_required_minor: minorAmount.optional(), until: z.number().int().optional() }).passthrough()),
}) });
export const gatewayFixtureSchema = z.object({ operations: gatewayOperationsSchema, agents: gatewayAgentsSchema });
const id = z.number().int().positive().safe();
const text = z.string().trim().min(1).max(200).refine(value => !value.startsWith('-'), 'Values cannot begin with an option prefix.');
const name = z.string().regex(/^[a-z0-9][a-z0-9_-]{0,63}$/);
const money = z.string().regex(/^\d+(\.\d{1,6})?$/).optional();
export const actionSchema = z.discriminatedUnion('action', [
  z.object({ action: z.literal('approve'), id, decision: z.enum(['approve', 'reject']), note: z.string().max(2000).refine(value => !value.startsWith('-')), digest: text }).strict(),
  z.object({ action: z.literal('agent-add'), name, tools: z.array(text).min(1).max(20) }).strict(),
  z.object({ action: z.literal('agent-revoke'), name }).strict(),
  z.object({ action: z.literal('mandate-grant'), title: text, tools: z.array(text).min(1).max(20), agent: name.optional(),
    maxPerCall: money, maxPerDay: money, maxCallsPerDay: z.number().int().positive().optional(),
    expires: z.string().regex(/^\d{4}-\d{2}-\d{2}$/).optional() }).strict(),
  z.object({ action: z.literal('mandate-revoke'), id }).strict(),
  z.object({ action: z.literal('evidence-export') }).strict(),
  z.object({ action: z.literal('operation-decide'), id: opId, decision: z.enum(['approve', 'reject']), digest: hex64 }).strict(),
  z.object({ action: z.literal('operation-resolve'), id: opId }).strict(),
]);
export type Approval = z.infer<typeof approvalSchema>;
export type Call = z.infer<typeof callsSchema>['calls'][number];
export type Agent = z.infer<typeof agentsSchema>['agents'][number];
export type Mandate = z.infer<typeof mandatesSchema>['mandates'][number];
export type Consent = z.infer<typeof consentsSchema>['consents'][number];
export type Verification = z.infer<typeof verifySchema>;
export type Declaration = z.infer<typeof declarationsSchema>['payments'][number];
export type ConsoleAction = z.infer<typeof actionSchema>;
export type GatewayOperation = z.infer<typeof gatewayOperationSchema>;
export type Exposure = z.infer<typeof gatewayExposureSchema>['data'];
export type OperationView = GatewayOperation & { agentName: string; amountLabel: string; counterparty: string; kindLabel: string };
export type ApprovalView = Approval & { amountLabel: string; operation: string; counterparty: string };
export type Snapshot = { user: string; fixtures: boolean; now: string; csrf: string;
  approvals: ApprovalView[]; calls: Call[]; agents: Agent[]; required: boolean;
  mandates: Mandate[]; consents: Consent[]; consentTotal: number; verification: Verification | null;
  errors: Record<string, string>; exportEnabled: boolean;
  operations: OperationView[]; gatewayConfigured: boolean; exposure: Exposure | null };
export type ActionResult = { simulated: boolean; args: string[]; message: string; token?: string;
  fingerprint?: string; out?: string; verifyCommand?: string };
