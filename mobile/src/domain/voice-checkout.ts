import { quoteApprovable, type LabTaskResult } from './lab';

export function voiceCommand(text: string): 'confirm' | 'cancel' | null {
  const words = text.toLowerCase().replace(/[.,!?]/g, '').replace(/\s+/g, ' ').trim();
  if (words === 'i confirm' || /^confirm (?:(?:my|the|this) )?order$/.test(words)) return 'confirm';
  if (/^cancel(?:(?: my| the| this)? order)?$/.test(words)) return 'cancel';
  return null;
}

export function clarificationRequest(result: LabTaskResult, answer: string, fallback = '') {
  if (result.data.stage !== 'needs_input' || !answer.trim()) throw new Error('Answer the question before continuing.');
  const original = result.data.input?.request ?? fallback;
  if (!original.trim()) throw new Error('Enter the complete updated order in Your order request above.');
  const request = `${original}\nMerchant question: ${result.data.clarification ?? 'Please clarify your order.'}\nYour answer: ${answer.trim()}`;
  if (request.length > 4096) throw new Error('This conversation is too long. Enter the complete updated order above instead.');
  return request;
}

export interface SpokenQuote { taskId: string; digest: string; expiresAt: number }
export function spokenQuote(result: LabTaskResult): SpokenQuote {
  if (!quoteApprovable(result)) throw new Error('The quote expired or changed. Find a new quote before confirming.');
  return { taskId: result.data.id, digest: result.approval_digest!, expiresAt: result.data.quote!.expires_at };
}
export function matchesSpokenQuote(result: LabTaskResult, quote: SpokenQuote) {
  return quoteApprovable(result) && result.data.id === quote.taskId && result.approval_digest === quote.digest
    && quote.expiresAt > Date.now() / 1000;
}
