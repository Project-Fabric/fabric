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
]);
export type Approval = z.infer<typeof approvalSchema>;
export type Call = z.infer<typeof callsSchema>['calls'][number];
export type Agent = z.infer<typeof agentsSchema>['agents'][number];
export type Mandate = z.infer<typeof mandatesSchema>['mandates'][number];
export type Consent = z.infer<typeof consentsSchema>['consents'][number];
export type Verification = z.infer<typeof verifySchema>;
export type Declaration = z.infer<typeof declarationsSchema>['payments'][number];
export type ConsoleAction = z.infer<typeof actionSchema>;
export type ApprovalView = Approval & { amountLabel: string; operation: string; counterparty: string };
export type Snapshot = { user: string; fixtures: boolean; now: string; csrf: string;
  approvals: ApprovalView[]; calls: Call[]; agents: Agent[]; required: boolean;
  mandates: Mandate[]; consents: Consent[]; consentTotal: number; verification: Verification | null;
  errors: Record<string, string>; exportEnabled: boolean };
export type ActionResult = { simulated: boolean; args: string[]; message: string; token?: string;
  fingerprint?: string; out?: string; verifyCommand?: string };
