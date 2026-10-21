import type { Approval, ApprovalView, Declaration } from './contracts';
export const field = (input: unknown, path: string): unknown => path.split('.').reduce<unknown>((value, key) =>
  value && typeof value === 'object' ? (value as Record<string, unknown>)[key] : undefined, input);
export function currencyPlaces(currency: string) {
  try { return new Intl.NumberFormat('en', { style: 'currency', currency }).resolvedOptions().maximumFractionDigits ?? 2; }
  catch { return 2; }
}
export function money(value: string | number, currency: string) {
  const amount = Number(value);
  if (!Number.isFinite(amount)) return 'Amount unavailable';
  try { return new Intl.NumberFormat('en-US', { style: 'currency', currency: currency.toUpperCase() }).format(amount); }
  catch { return `${value} ${currency}`; }
}
export function operation(input: unknown, target: string) {
  const opcode = field(input, 'stripe_api_operation_id');
  const source = typeof opcode === 'string' ? opcode : target.split('/').pop() || target;
  const labels: Record<string, string> = { PostRefunds: 'Refund', PostPayouts: 'Payout', PostTransfers: 'Transfer',
    PostPaymentIntents: 'Payment', PostCharges: 'Charge', issue_refund: 'Refund' };
  return labels[source] || source.replace(/_/g, ' ');
}
export function approvalView(request: Approval, declarations: Declaration[], fixtures: boolean): ApprovalView {
  const declaration = declarations.find(p => p.tool === request.target &&
    (p.when || []).every(condition => field(request.input, condition.field) === condition.equals));
  const fixtureRefund = fixtures && request.target === 'stripe/issue_refund';
  const amount = declaration ? field(request.input, declaration.amount) : fixtureRefund ? request.input.amount : undefined;
  const currency = declaration ? (declaration.currency ? field(request.input, declaration.currency) : null) || declaration.default_currency
    : fixtureRefund ? request.input.currency : undefined;
  let amountLabel = 'Amount unavailable';
  if ((typeof amount === 'number' || typeof amount === 'string') && typeof currency === 'string') {
    const major = Number(amount) / ((declaration?.minor_units || fixtureRefund) ? 10 ** currencyPlaces(currency) : 1);
    amountLabel = money(major, currency);
  }
  const other = request.input.parameters && typeof request.input.parameters === 'object' ? request.input.parameters as Record<string, unknown> : request.input;
  return { ...request, amountLabel, operation: operation(request.input, request.target),
    counterparty: String(other.payment_intent || other.merchant || other.merchant_id || 'Not supplied') };
}
export function waiting(at: string, now: string) {
  const minutes = Math.max(0, Math.floor((Date.parse(now) - Date.parse(at)) / 60000));
  if (!Number.isFinite(minutes)) return 'Time unavailable';
  if (minutes < 1) return 'Just now';
  if (minutes < 60) return `${minutes}m waiting`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)}h ${minutes % 60}m waiting`;
  return `${Math.floor(minutes / 1440)}d waiting`;
}
export function dateTime(at: string) { return new Date(at).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }) + ' UTC'; }
