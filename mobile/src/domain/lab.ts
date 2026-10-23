// Only an explicit private-LAN sandbox relay may replace the loopback default.
export function labUrl(value?: string) {
  if (!value || value === 'http://127.0.0.1:3097') return 'http://127.0.0.1:3097';
  const match = /^http:\/\/(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}):3099$/.exec(value);
  const parts = match?.[1].split('.');
  const octets = parts?.map(Number);
  if (!parts || !octets || parts.some((part, index) => String(octets[index]) !== part || octets[index] > 255)
    || !(octets[0] === 10 || (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) || (octets[0] === 192 && octets[1] === 168))) {
    throw new Error('Use the configured private Wi-Fi sandbox relay on port 3099, or the default USB endpoint.');
  }
  return value;
}
export const LAB_URL = labUrl(process.env.EXPO_PUBLIC_COSIGNA_LAB_URL);
export const LAB_IS_WIFI = LAB_URL !== 'http://127.0.0.1:3097';
export const MOBILE_SCHEMA = 'cosigna.mobile-lab/v3';
export type AiProvider = 'openai' | 'groq';
export type HostedConsent = 'planner-v1' | 'voice-v1' | 'planner-groq-v1' | 'voice-groq-v1';
export function hostedConsent(provider: AiProvider, operation: 'planner' | 'voice'): HostedConsent {
  return provider === 'groq' ? `${operation}-groq-v1` : `${operation}-v1`;
}
export type LabStage = 'draft' | 'needs_input' | 'quoted' | 'approved' | 'submitting' | 'unknown' | 'confirmed' | 'declined' | 'cancelled';
export interface LabQuote {
  quantity: number; total_minor: number; expires_at: number;
  offer: { title: string; merchant_id: string; currency: string; fulfillment: string; terms: string };
}
export interface LabTask {
  id: string; stage: LabStage; clarification: string | null; quote: LabQuote | null;
  input?: { request: string };
  receipt: { order_id: string; total_minor: number; currency: string } | null;
}
export interface LabTaskResult { data: LabTask; approval_digest: string | null }
export interface LabBalance {
  available_minor: number; reserved_minor: number; spent_minor: number;
  budget: { revoked: boolean; terms: { expires_at: number; per_purchase_minor: number; limit_minor: number; currency: string } };
}
export interface LabOrder {
  id: string; state: string; quote: LabQuote;
  payment?: { payment_intent_id: string | null; status: string };
}
export interface LabState {
  mode: 'fixtures' | 'openai' | 'groq'; ai_provider: AiProvider; voice_available: boolean; payment_mode: 'simulated' | 'stripe-test';
  balance: LabBalance | null; recent_tasks: string[]; orders: LabOrder[];
  privacy: { audio: string; planning: string; retention: string; access: string };
}
export function minorUnits(input: string) {
  if (!/^\d+(\.\d{1,2})?$/.test(input.trim())) throw new Error('Enter a positive USD amount with up to two decimal places.');
  const [whole, fraction = ''] = input.trim().split('.');
  const value = Number(whole) * 100 + Number(fraction.padEnd(2, '0'));
  if (!Number.isSafeInteger(value) || value < 1 || value > 100_000) throw new Error('Use an amount between $0.01 and $1,000.00.');
  return value;
}
export const labMoney = (minor: number) => `$${(minor / 100).toFixed(2)} USD`;
export const budgetActive = (balance: LabBalance | null) => Boolean(balance && !balance.budget.revoked && balance.budget.terms.expires_at > Date.now() / 1000);
export const hasAttempt = (task: LabTask | null) => Boolean(task && ['submitting', 'unknown', 'confirmed', 'declined'].includes(task.stage));
export function quoteApprovable(result: LabTaskResult | null, now = Date.now()) {
  const quote = result?.data.quote;
  return Boolean(result && ['quoted', 'approved'].includes(result.data.stage) && /^[a-f0-9]{64}$/.test(result.approval_digest ?? '')
    && quote && quote.offer.currency === 'USD' && Number.isSafeInteger(quote.total_minor) && quote.total_minor > 0 && quote.expires_at > now / 1000);
}

export class LabApiError extends Error {
  constructor(message: string, public code: string, public status: number) { super(message); }
}
interface RequestOptions { method: string; headers: Record<string, string>; body?: string | Blob; signal: AbortSignal; redirect: 'error' }
export type LabTransport = (url: string, options: RequestOptions) => Promise<{ ok: boolean; status: number; json: () => Promise<unknown> }>;
export class LabClient {
  constructor(private token = '', private transport: LabTransport = fetch) {}
  async call<T>(path: string, body?: object, consent?: HostedConsent): Promise<T> {
    return this.send(path, body === undefined ? undefined : JSON.stringify(body), 'application/json', consent);
  }
  async send<T>(path: string, body?: string | Blob, type = 'application/json', consent?: string): Promise<T> {
    if (!/^\/[a-z0-9_/-]+$/.test(path)) throw new Error('Invalid lab endpoint.');
    let response;
    try {
      response = await this.transport(`${LAB_URL}/mobile/v3${path}`, {
        method: body === undefined ? 'GET' : 'POST', redirect: 'error', signal: AbortSignal.timeout(65_000),
        headers: { ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
          ...(body === undefined ? {} : { 'Content-Type': type }), ...(consent ? { 'X-Cosigna-Hosted-Consent': consent } : {}) }, body,
      });
    } catch { throw new LabApiError('Connection lost. Refresh this task before trying another purchase. Check your Wi-Fi connection or USB forwarding and keep the computer lab running.', 'connection_lost', 0); }
    let value: { schema?: string; error?: { message?: string; code?: string }; data?: T };
    try { value = await response.json() as typeof value; }
    catch { throw new LabApiError('The lab returned an invalid response. Refresh task status.', 'invalid_response', response.status); }
    if (value.schema !== MOBILE_SCHEMA) throw new LabApiError('This server does not support the mobile test API.', 'invalid_schema', response.status);
    if (!response.ok) throw new LabApiError(value.error?.message ?? 'The request failed. Refresh task status.', value.error?.code ?? 'request_failed', response.status);
    if (value.data === undefined) throw new LabApiError('Missing server result. Refresh task status.', 'invalid_response', response.status);
    return value.data;
  }
}
