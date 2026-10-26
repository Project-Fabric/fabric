import fs from 'node:fs';
import path from 'node:path';
import { approvalsSchema, agentsSchema, callsSchema, consentsSchema, declarationsSchema, gatewayExposureSchema, gatewayFixtureSchema, mandatesSchema, verifySchema, type Snapshot } from '../src/lib/contracts';
import { approvalView, operationViews } from '../src/lib/display';
export function fixture(name: string) { return JSON.parse(fs.readFileSync(path.join(process.cwd(), 'fixtures', name), 'utf8')); }
export function sample(): Snapshot {
  const agents = agentsSchema.parse(fixture('agent_list.json')), consents = consentsSchema.parse(fixture('agentauth_consents.json'));
  const declarations = declarationsSchema.parse(fixture('payments_list.json'));
  return { user: 'priya@northwind.test', csrf: 'synthetic-csrf', fixtures: true, now: '2026-10-21T13:00:00Z',
    approvals: approvalsSchema.parse(fixture('inbox_approvals.json')).requests.map(r => approvalView(r, declarations.payments, true)),
    calls: callsSchema.parse(fixture('evidence_calls.json')).calls, agents: agents.agents, required: agents.required,
    mandates: mandatesSchema.parse(fixture('inbox_mandates.json')).mandates, consents: consents.consents,
    consentTotal: consents.total, verification: verifySchema.parse(fixture('evidence_verify.json')), errors: {}, exportEnabled: true,
    operations: gateway(), gatewayConfigured: true, role: 'owner', approver: null,
    exposure: gatewayExposureSchema.parse(fixture('gateway_exposure.json')).data };
}
export function gateway() {
  const raw = gatewayFixtureSchema.parse(fixture('gateway_operations.json'));
  return operationViews(raw.operations.data, raw.agents.data);
}
