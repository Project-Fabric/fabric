import { AppState } from 'react-native';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react-native';
import * as Speech from 'expo-speech';
import { VoiceCheckout } from '../src/components/voice-checkout';
import type { LabBalance, LabTaskResult } from '../src/domain/lab';

let mockVoice: { onTranscript: (text: string, audio: { heardSpeech: boolean }) => void } | null = null;
jest.mock('../src/components/lab-voice', () => {
  const { Text } = require('react-native');
  return { LabVoice: (props: typeof mockVoice) => { mockVoice = props; return <Text>Voice listener</Text>; } };
});
jest.mock('expo-speech', () => ({ speak: jest.fn(), stop: jest.fn().mockResolvedValue(undefined), getAvailableVoicesAsync: jest.fn() }));
jest.mock('expo-audio', () => ({ setAudioModeAsync: jest.fn().mockResolvedValue(undefined) }));
jest.mock('expo-crypto', () => ({ randomUUID: () => 'synthetic-voice-request' }));

const originalState = Object.getOwnPropertyDescriptor(AppState, 'currentState')!;
const balance = { available_minor: 2000, spent_minor: 0, reserved_minor: 0,
  budget: { revoked: false, terms: { expires_at: 4_000_000_000, currency: 'USD', limit_minor: 2000, per_purchase_minor: 1000 } } } satisfies LabBalance;
const quoted: LabTaskResult = { approval_digest: 'a'.repeat(64), data: { id: 'task_test', stage: 'quoted', clarification: null, input: { request: 'Order one black coffee' }, receipt: null,
  quote: { quantity: 1, total_minor: 450, expires_at: 4_000_000_000, offer: { title: 'Black coffee', merchant_id: 'test-cafe', currency: 'USD', fulfillment: 'Pickup counter.', terms: 'Test payment.' } } } };
let network: jest.SpyInstance;
beforeEach(() => {
  mockVoice = null;
  jest.mocked(Speech.getAvailableVoicesAsync).mockResolvedValue([{ identifier: 'synthetic-english', language: 'en-US', name: 'Test', quality: Speech.VoiceQuality?.Default } as Speech.Voice]);
  Object.defineProperty(AppState, 'currentState', { value: 'active', writable: true, configurable: true });
  network = jest.spyOn(globalThis, 'fetch').mockImplementation(async (url, options) => {
    const path = String(url).split('/mobile/v3')[1];
    const data = path === '/approve' ? { ...quoted, data: { ...quoted.data, stage: 'confirmed', receipt: { order_id: 'test_order', total_minor: 450, currency: 'USD' } } }
      : path === '/cancel' ? { ...quoted, data: { ...quoted.data, stage: 'cancelled' } }
        : { ...quoted, data: { ...quoted.data, input: { request: JSON.parse(options!.body as string).request } } };
    return { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data }) } as Response;
  });
});
afterEach(() => { Object.defineProperty(AppState, 'currentState', originalState); jest.useRealTimers(); });
const props = (initialTask: LabTaskResult | null = null) => ({ token: 'synthetic-token', provider: 'groq' as const, enabled: true, allowance: balance,
  initialTask, requestText: '', onBusy: jest.fn(), onRequest: jest.fn(), onTask: jest.fn(), onSubmitting: jest.fn(), onRefresh: jest.fn().mockResolvedValue(undefined) });
async function start(initialTask: LabTaskResult | null = null) {
  const callbacks = props(initialTask); await render(<VoiceCheckout {...callbacks} />);
  await fireEvent.press(screen.getByRole('button', { name: 'Start voice checkout' }));
  await waitFor(() => expect(Speech.speak).toHaveBeenCalled()); return callbacks;
}
async function finishPrompt() {
  const options = jest.mocked(Speech.speak).mock.calls.at(-1)![1] as { onDone: () => void };
  await act(async () => { options.onDone(); });
}
async function say(text: string, heardSpeech = true) {
  await act(async () => { mockVoice!.onTranscript(text, { heardSpeech }); });
}
const approvals = () => network.mock.calls.filter(([url]) => String(url).endsWith('/approve'));

describe('spoken test checkout', () => {
  it('handles a rejected native voice lookup without opening the microphone or approving', async () => {
    jest.mocked(Speech.getAvailableVoicesAsync).mockRejectedValue(new Error("Call to ExpoSpeech.getVoices rejected: not bound to TTS engine"));
    const callbacks = props(quoted);
    await render(<VoiceCheckout {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Start voice checkout' }));
    await waitFor(() => expect(screen.getByText(/Close and reopen Cosigna/)).toBeVisible());
    expect(screen.queryByText(/Call to ExpoSpeech/)).toBeNull();
    expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
    expect(Speech.speak).not.toHaveBeenCalled(); expect(mockVoice).toBeNull();
    expect(approvals()).toHaveLength(0);
  });
  it('ignores a native voice lookup rejection after the user stops checkout', async () => {
    let reject!: (error: Error) => void;
    jest.mocked(Speech.getAvailableVoicesAsync).mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
    await render(<VoiceCheckout {...props(quoted)} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Start voice checkout' }));
    await waitFor(() => expect(Speech.getAvailableVoicesAsync).toHaveBeenCalled());
    await fireEvent.press(screen.getByRole('button', { name: 'Stop voice checkout' }));
    await act(async () => { reject(new Error('Synthetic late engine failure')); });
    expect(screen.getByText(/Voice checkout stopped. Submitted payments/)).toBeVisible();
    expect(Speech.speak).not.toHaveBeenCalled(); expect(mockVoice).toBeNull();
    expect(approvals()).toHaveLength(0);
  });
  it('stops an unresponsive speech engine instead of waiting forever or arming approval', async () => {
    jest.useFakeTimers();
    const callbacks = await start(quoted);
    const delayed = jest.mocked(Speech.speak).mock.calls.at(-1)![1]!;
    await act(async () => { jest.advanceTimersByTime(8_000); });
    expect(screen.getByText(/enable Speech Services by Google/i)).toBeVisible();
    expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
    await act(async () => { delayed.onDone?.(); });
    expect(screen.queryByText('Voice listener')).toBeNull();
    expect(approvals()).toHaveLength(0);
  });
  it('stops when speech starts but never finishes reading the quote', async () => {
    jest.useFakeTimers();
    await start(quoted);
    await act(async () => { jest.mocked(Speech.speak).mock.calls.at(-1)![1]!.onStart?.(); jest.advanceTimersByTime(120_000); });
    expect(screen.getByText(/enable Speech Services by Google/i)).toBeVisible();
    expect(approvals()).toHaveLength(0); expect(mockVoice).toBeNull();
  });
  it('bounds a pending native voice lookup without speaking or approving later', async () => {
    jest.useFakeTimers();
    let resolve!: (voices: Speech.Voice[]) => void;
    jest.mocked(Speech.getAvailableVoicesAsync).mockImplementation(() => new Promise(done => { resolve = done; }));
    await render(<VoiceCheckout {...props(quoted)} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Start voice checkout' }));
    await act(async () => { jest.advanceTimersByTime(8_000); resolve([{ language: 'en-US' } as Speech.Voice]); });
    expect(screen.getByText(/enable Speech Services by Google/i)).toBeVisible();
    expect(Speech.speak).not.toHaveBeenCalled(); expect(approvals()).toHaveLength(0);
  });
  it('explains a missing English voice without opening the microphone', async () => {
    jest.mocked(Speech.getAvailableVoicesAsync).mockResolvedValue([]);
    await render(<VoiceCheckout {...props()} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Start voice checkout' }));
    await waitFor(() => expect(screen.getByText(/enable Speech Services by Google/i)).toBeVisible());
    expect(Speech.speak).not.toHaveBeenCalled(); expect(mockVoice).toBeNull();
  });
  it.each(['Confirm my order.', 'I confirm.'])('plans a spoken order and approves once only from a fresh post-quote response: %s', async confirmation => {
    const callbacks = await start();
    expect(mockVoice).toBeNull();
    await finishPrompt(); await say('Order one black coffee and confirm my order');
    await waitFor(() => expect(Speech.speak).toHaveBeenCalledTimes(2));
    expect(approvals()).toHaveLength(0);
    expect(jest.mocked(Speech.speak).mock.calls[1][0]).toContain('$4.50 USD');
    await finishPrompt();
    const listener = mockVoice!;
    await act(async () => { listener.onTranscript(confirmation, { heardSpeech: true }); listener.onTranscript(confirmation, { heardSpeech: true }); });
    await waitFor(() => expect(callbacks.onRefresh).toHaveBeenCalledWith('task_test'));
    expect(approvals()).toHaveLength(1);
    expect(JSON.parse(approvals()[0][1]!.body as string)).toEqual({ task_id: 'task_test', quote_digest: quoted.approval_digest });
    expect(callbacks.onSubmitting).toHaveBeenCalledTimes(1);
  });
  it.each(['yes', 'do not confirm my order', 'confirm my order and add a croissant', 'I confirm if it is under five dollars', 'I confirm, add a croissant', 'I do not confirm'])('does not approve an ambiguous or changed response: %s', async text => {
    await start(quoted); await finishPrompt(); await say(text);
    expect(approvals()).toHaveLength(0); expect(screen.getByText(/No order placed/)).toBeVisible();
  });
  it('requires measured microphone activity for confirmation', async () => {
    await start(quoted); await finishPrompt(); await say('Confirm my order', false);
    expect(approvals()).toHaveLength(0); expect(screen.getByText(/Could not verify microphone activity/)).toBeVisible();
  });
  it('clears voice authority on background and ignores late transcripts', async () => {
    const listener = jest.spyOn(AppState, 'addEventListener');
    await start(quoted); await finishPrompt();
    const voice = mockVoice!;
    await act(async () => { listener.mock.calls[0][1]('background'); voice.onTranscript('Confirm my order', { heardSpeech: true }); });
    expect(approvals()).toHaveLength(0);
  });
  it('blocks a quote that expires after being spoken', async () => {
    const result = { ...quoted, data: { ...quoted.data, quote: { ...quoted.data.quote!, expires_at: Math.floor(Date.now() / 1000) + 30 } } };
    await start(result); await finishPrompt();
    jest.spyOn(Date, 'now').mockReturnValue((result.data.quote.expires_at + 1) * 1000);
    await say('Confirm my order'); expect(approvals()).toHaveLength(0);
    expect(screen.getByText(/quote or allowance changed/)).toBeVisible();
  });
  it('keeps the original request and merchant question when answering a clarification', async () => {
    await start({ ...quoted, approval_digest: null, data: { ...quoted.data, quote: null, stage: 'needs_input', input: { request: 'Order one black tea' }, clarification: 'Would you like black coffee instead?' } });
    await finishPrompt(); await say('Yes, one black coffee');
    await waitFor(() => expect(network).toHaveBeenCalled());
    const body = JSON.parse(network.mock.calls[0][1]!.body as string);
    expect(body.request).toContain('Order one black tea'); expect(body.request).toContain('Would you like black coffee instead?'); expect(body.request).toContain('Yes, one black coffee');
    expect(approvals()).toHaveLength(0);
  });
  it('pauses instead of repeatedly speaking the same unanswered clarification', async () => {
    const unclear: LabTaskResult = { ...quoted, approval_digest: null, data: { ...quoted.data, quote: null, stage: 'needs_input', clarification: 'Which item and quantity would you like?' } };
    network.mockResolvedValue({ ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: unclear }) });
    await start(unclear); await finishPrompt(); await say('Unclear microphone noise');
    await waitFor(() => expect(screen.getByText(/Voice checkout paused because the request is still unclear/)).toBeVisible());
    expect(Speech.speak).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('Voice listener')).toBeNull(); expect(approvals()).toHaveLength(0);
  });
  it('a lost submission response requires status recovery and never automatically retries', async () => {
    const callbacks = await start(quoted); await finishPrompt();
    network.mockRejectedValue(new Error('Synthetic lost response. Check status.'));
    await say('Confirm my order');
    await waitFor(() => expect(screen.getByText(/Connection lost. Refresh this task/)).toBeVisible());
    expect(callbacks.onSubmitting).toHaveBeenCalledTimes(1); expect(approvals()).toHaveLength(1);
  });
  it('a spoken cancellation cancels the unsubmitted request without an approval', async () => {
    await start(quoted); await finishPrompt(); await say('Cancel my order');
    await waitFor(() => expect(screen.getByText(/Request cancelled/)).toBeVisible());
    expect(approvals()).toHaveLength(0);
    expect(network.mock.calls.filter(([url]) => String(url).endsWith('/cancel'))).toHaveLength(1);
  });
});
