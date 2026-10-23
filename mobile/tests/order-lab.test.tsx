import { AppState } from 'react-native';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react-native';
import { OrderLabScreen } from '../src/components/order-lab-screen';
import { loadDeviceSession, saveDeviceSession } from '../src/domain/device-session';

jest.mock('expo-speech', () => ({ speak: jest.fn(), stop: jest.fn().mockResolvedValue(undefined), getAvailableVoicesAsync: jest.fn().mockResolvedValue([{ language: 'en-US' }]) }));
jest.mock('expo-audio', () => ({ setAudioModeAsync: jest.fn().mockResolvedValue(undefined) }));
jest.mock('expo-crypto', () => ({ randomUUID: () => 'synthetic-request-key' }));
jest.mock('../src/components/lab-voice', () => {
  const { Button } = require('react-native');
  return { LabVoice: ({ enabled, onTranscript }: { enabled: boolean; onTranscript: (text: string) => void }) =>
    <Button title="Synthetic voice transcript" accessibilityLabel="Synthetic voice transcript" disabled={!enabled} onPress={() => onTranscript('Order one oat latte')} /> };
});

const state = {
  mode: 'fixtures', ai_provider: 'openai', voice_available: true, payment_mode: 'stripe-test', recent_tasks: [], orders: [],
  privacy: { planning: 'Fixture script; no model.', retention: 'Text remains in the local test ledger.' },
  balance: { available_minor: 2000, reserved_minor: 0, spent_minor: 0,
    budget: { revoked: false, terms: { expires_at: 4_000_000_000, per_purchase_minor: 1000, limit_minor: 2000, currency: 'USD' } } },
};
const task = { id: `task_${'a'.repeat(64)}`, stage: 'quoted', receipt: null, clarification: null,
  quote: { quantity: 1, total_minor: 700, expires_at: 4_000_000_000,
    offer: { title: 'Oat latte', merchant_id: 'cosigna-test-cafe', currency: 'USD', fulfillment: 'Test counter A.', terms: 'No real funds.' } } };
const quoted = { data: task, approval_digest: 'b'.repeat(64) };
let fetchMock: jest.SpyInstance;
beforeEach(() => {
  fetchMock = jest.spyOn(globalThis, 'fetch').mockImplementation(async url => {
    const path = String(url).split('/mobile/v3')[1];
    let data: unknown = {};
    if (path === '/pair') data = { token: 'synthetic-token', expires_at: 4_000_000_000 };
    if (path === '/state') data = state;
    if (path === '/plan') data = quoted;
    if (path.startsWith('/tasks/')) data = quoted;
    if (path === '/approve') throw new Error('synthetic connection loss');
    return { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data }) } as Response;
  });
});
const press = async (name: string) => fireEvent.press(screen.getByRole('button', { name }));
async function pair() {
  await render(<OrderLabScreen goBack={jest.fn()} />);
  await fireEvent.changeText(screen.getByLabelText('One-use pairing code'), '012345');
  await press('Pair with computer');
}
describe('paired purchase UI', () => {
  it('does not connect or ask for microphone access on initial render', async () => {
    await render(<OrderLabScreen goBack={jest.fn()} />);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(screen.getByText(/Google Assistant handoff requires the installed Android build/)).toBeVisible();
  });
  it('restores a scoped session, checks server state and retains fresh processing consent', async () => {
    jest.mocked(loadDeviceSession).mockResolvedValue({ schema: 'cosigna.device-session/v1', token: 'synthetic-token', expiresAt: 4_000_000_000 });
    await render(<OrderLabScreen goBack={jest.fn()} />);
    await waitFor(() => expect(screen.getByText('Your test allowance')).toBeVisible());
    expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/pair'))).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'Synthetic voice transcript' })).toBeDisabled();
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
  });
  it('an external confirmation string is only a draft and cannot trigger network, microphone or approval', async () => {
    await render(<OrderLabScreen goBack={jest.fn()} incomingRequest="I confirm" incomingId="synthetic-invocation" />);
    expect(screen.getByText('I confirm')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Continue assistant request by voice' })).toBeDisabled();
    expect(fetchMock).not.toHaveBeenCalled();
  });
  it('an assistant draft reaches only Cosigna planning after explicit voice-session consent, never approval', async () => {
    const appState = Object.getOwnPropertyDescriptor(AppState, 'currentState')!;
    Object.defineProperty(AppState, 'currentState', { value: 'active', configurable: true });
    try {
      const original = fetchMock.getMockImplementation()!;
      fetchMock.mockImplementation(async (url, options) => String(url).endsWith('/state')
        ? { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { ...state, mode: 'groq', ai_provider: 'groq' } }) } as Response
        : original(url, options));
      await render(<OrderLabScreen goBack={jest.fn()} incomingRequest="Order one black coffee" />);
      await fireEvent.changeText(screen.getByLabelText('One-use pairing code'), '012345'); await press('Pair with computer');
      await fireEvent(screen.getByLabelText('Allow audio transcription with Groq'), 'valueChange', true);
      await fireEvent(screen.getByLabelText('Allow request and catalog to Groq for planning'), 'valueChange', true);
      await fireEvent(screen.getByLabelText('Read results aloud using device speech'), 'valueChange', true);
      await press('Continue assistant request by voice');
      await waitFor(() => expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/plan'))).toBe(true));
      const request = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/plan'))!;
      expect(JSON.parse(request[1]!.body as string).request).toBe('Order one black coffee');
      expect(request[1]!.headers).toEqual(expect.objectContaining({ 'X-Cosigna-Hosted-Consent': 'planner-groq-v1' }));
      expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
    } finally { Object.defineProperty(AppState, 'currentState', appState); }
  });
  it('saves only the scoped expiring phone connection after pairing', async () => {
    await pair();
    expect(saveDeviceSession).toHaveBeenCalledWith({ schema: 'cosigna.device-session/v1', token: 'synthetic-token', expiresAt: 4_000_000_000 });
  });
  it('preserves a leading zero in the six-digit pairing code', async () => {
    await pair();
    const request = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/pair'));
    expect(JSON.parse(request![1]!.body as string)).toEqual({ code: '012345' });
  });
  it('can transcribe before an allowance is set while purchase planning stays disabled', async () => {
    fetchMock.mockImplementation(async url => ({ ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3',
      data: String(url).endsWith('/pair') ? { token: 'synthetic-token' } : { ...state, balance: null } }) }) as Response);
    await pair();
    await fireEvent(screen.getByLabelText('Allow audio transcription with OpenAI'), 'valueChange', true);
    expect(screen.getByRole('button', { name: 'Synthetic voice transcript' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Find a quote' })).toBeDisabled();
  });
  it('names Groq in both permissions and sends only Groq planning consent after opt-in', async () => {
    const original = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (url, options) => String(url).endsWith('/state')
      ? { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { ...state, mode: 'groq', ai_provider: 'groq' } }) } as Response
      : original(url, options));
    await pair();
    expect(screen.queryByLabelText('Allow audio transcription with OpenAI')).toBeNull();
    expect(screen.getByRole('button', { name: 'Find a quote' })).toBeDisabled();
    await fireEvent(screen.getByLabelText('Allow audio transcription with Groq'), 'valueChange', true);
    await press('Synthetic voice transcript');
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/plan'))).toBe(false);
    await fireEvent(screen.getByLabelText('Allow request and catalog to Groq for planning'), 'valueChange', true);
    await press('Find a quote');
    const request = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/plan'));
    expect(request![1]!.headers).toEqual(expect.objectContaining({ 'X-Cosigna-Hosted-Consent': 'planner-groq-v1' }));
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
  });
  it('requires transcription permission, lets the user edit text and never plans or pays from speech alone', async () => {
    await pair();
    expect(screen.getByRole('button', { name: 'Synthetic voice transcript' })).toBeDisabled();
    await fireEvent(screen.getByLabelText('Allow audio transcription with OpenAI'), 'valueChange', true);
    await press('Synthetic voice transcript');
    expect(screen.getByLabelText('Your order request')).toHaveDisplayValue('Order one oat latte');
    expect(fetchMock.mock.calls.some(([url]) => /\/(plan|approve)$/.test(String(url)))).toBe(false);
    await press('Find a quote');
    expect(screen.queryByRole('button', { name: 'Approve and place test order' })).toBeNull();
    await press('Review this quote for approval');
    expect(screen.getByRole('button', { name: 'Approve and place test order' })).toBeVisible();
  });
  it('a lost approval response locks spending until the durable task is refreshed', async () => {
    await pair(); await press('Use oat latte example'); await press('Find a quote'); await press('Review this quote for approval');
    await press('Approve and place test order');
    expect(screen.getByText(/submission response needs checking/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Find a quote' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Approve and place test order' })).toBeNull();
    expect(fetchMock.mock.calls.filter(([url]) => String(url).endsWith('/approve'))).toHaveLength(1);
    await press('Refresh lab and current task');
    expect(screen.getByRole('button', { name: 'Review this quote for approval' })).toBeVisible();
  });
  it('shows an inline answer box and sends the earlier request, question and reply for a new quote', async () => {
    const original = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (url, options) => String(url).endsWith('/state')
      ? { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { ...state, mode: 'groq', ai_provider: 'groq', recent_tasks: [task.id] } }) } as Response
      : String(url).includes('/tasks/') ? { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { approval_digest: null,
        data: { ...task, stage: 'needs_input', quote: null, input: { request: 'Order one black tea' }, clarification: 'Would you like black coffee instead?' } } }) } as Response
        : original(url, options));
    await pair();
    expect(screen.getByLabelText('Your answer')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Send answer' })).toBeDisabled();
    await fireEvent(screen.getByLabelText('Allow request and catalog to Groq for planning'), 'valueChange', true);
    await fireEvent.changeText(screen.getByLabelText('Your answer'), 'Yes, one black coffee');
    await press('Send answer');
    const call = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/plan'));
    const request = JSON.parse(call![1]!.body as string).request;
    expect(request).toContain('Order one black tea'); expect(request).toContain('Would you like black coffee instead?');
    expect(request).toContain('Yes, one black coffee');
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
  });
  it('backgrounding clears the unfinished quote approval', async () => {
    const listener = jest.spyOn(AppState, 'addEventListener');
    await pair(); await press('Use oat latte example'); await press('Find a quote'); await press('Review this quote for approval');
    const callback = listener.mock.calls.find(([event]) => event === 'change')![1];
    await act(async () => callback('background'));
    expect(screen.queryByRole('button', { name: 'Approve and place test order' })).toBeNull();
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
  });
  it('a recovered unknown outcome blocks new quotes and offers lookup rather than resubmission', async () => {
    await pair(); await press('Use oat latte example');
    fetchMock.mockImplementation(async url => {
      const path = String(url).split('/mobile/v3')[1];
      const data = path === '/state' ? { ...state, recent_tasks: [task.id], balance: { ...state.balance, reserved_minor: 700, available_minor: 1300 } }
        : { ...quoted, data: { ...task, stage: 'unknown' } };
      return { ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data }) } as Response;
    });
    await press('Refresh lab and current task');
    expect(screen.getByRole('button', { name: 'Find a quote' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Check payment and order status' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Approve and place test order' })).toBeNull();
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/approve'))).toBe(false);
  });
});
