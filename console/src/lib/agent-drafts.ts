import { z } from 'zod';

const amount = z.string().regex(/^\d{1,9}(\.\d{1,2})?$/, 'Use a positive amount with up to two decimal places.');
export const draftInputSchema = z.object({
  name: z.string().trim().min(1).max(64), template: z.enum(['refunds', 'collections', 'reconciliation']),
  provider: z.enum(['stripe', 'paypal', 'razorpay', 'adyen', 'square', 'quickbooks', 'xero', 'shopify']),
  currency: z.enum(['USD', 'EUR', 'GBP', 'INR']), perAction: z.string(), perDay: z.string(),
}).strict().superRefine((value, ctx) => {
  if (value.template !== 'refunds') return;
  const valid = amount.safeParse(value.perAction).success && amount.safeParse(value.perDay).success;
  if (!valid) { ctx.addIssue({ code: 'custom', message: 'Use positive limits with up to two decimal places.' }); return; }
  const cents = (text: string) => { const [whole, fraction = ''] = text.split('.'); return BigInt(whole) * BigInt(100) + BigInt(fraction.padEnd(2, '0')); };
  if (cents(value.perAction) <= BigInt(0) || cents(value.perDay) < cents(value.perAction))
    ctx.addIssue({ code: 'custom', message: 'The per-action limit must be positive and the daily limit must cover it.' });
});

export function createAgentDraft(input: unknown, owner: string) {
  const value = draftInputSchema.parse(input);
  return { schema: 'cosigna.agent-draft/v1' as const, name: value.name, template: value.template, provider: value.provider,
    owner, status: 'draft' as const, execution_enabled: false as const, approval: 'human_review_before_every_write' as const,
    limits: value.template === 'refunds' ? { currency: value.currency, per_action: value.perAction, per_day: value.perDay } : null };
}
export type AgentDraft = ReturnType<typeof createAgentDraft>;
