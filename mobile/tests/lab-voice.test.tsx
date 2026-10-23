import { AppState } from 'react-native';
import { act, fireEvent, render, screen } from '@testing-library/react-native';
import { AudioModule, type RecordingStatus } from 'expo-audio';
import { LabVoice } from '../src/components/lab-voice';

const originalAppState = Object.getOwnPropertyDescriptor(AppState, 'currentState')!;
const mockRecorder = {
  uri: 'file:///synthetic-cache/order.m4a' as string | null, isRecording: false,
  prepareToRecordAsync: jest.fn().mockResolvedValue(undefined),
  record: jest.fn(() => { mockRecorder.isRecording = true; }),
  stop: jest.fn(async () => { mockRecorder.isRecording = false; }),
  getStatus: jest.fn(() => ({ metering: -70 })),
};
let mockRecordingStatus: ((status: RecordingStatus) => void) | undefined;
const mockDelete = jest.fn();
const mockUpload = jest.fn();
jest.mock('expo-audio', () => ({
  useAudioRecorder: (_options: unknown, listener: (status: RecordingStatus) => void) => { mockRecordingStatus = listener; return mockRecorder; }, RecordingPresets: { HIGH_QUALITY: {} },
  AudioModule: { requestRecordingPermissionsAsync: jest.fn() }, setAudioModeAsync: jest.fn().mockResolvedValue(undefined),
}));
jest.mock('expo-file-system', () => ({ File: class { exists = true; size = 100; delete = mockDelete; } }));
jest.mock('expo/fetch', () => ({ fetch: (...args: unknown[]) => mockUpload(...args) }));
beforeEach(() => {
  jest.clearAllMocks(); mockRecorder.isRecording = false; mockRecorder.uri = 'file:///synthetic-cache/order.m4a';
  mockRecorder.getStatus.mockReset().mockReturnValue({ metering: -70 });
  Object.defineProperty(AppState, 'currentState', { value: 'active', writable: true, configurable: true });
  mockRecorder.stop.mockImplementation(async () => { mockRecorder.isRecording = false; });
  jest.mocked(AudioModule.requestRecordingPermissionsAsync).mockResolvedValue({ granted: true } as Awaited<ReturnType<typeof AudioModule.requestRecordingPermissionsAsync>>);
  mockUpload.mockResolvedValue({ ok: true, status: 200, json: async () => ({ schema: 'cosigna.mobile-lab/v3', data: { text: 'Order one oat latte' } }) });
});
afterEach(() => { Object.defineProperty(AppState, 'currentState', originalAppState); jest.useRealTimers(); });
const props = () => ({ token: 'synthetic-token', enabled: true, onTranscript: jest.fn(), onBusy: jest.fn(), onError: jest.fn() });
describe('native recording boundary', () => {
  it('requests permission only after pressing record and uploads only after explicit transcription', async () => {
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    expect(AudioModule.requestRecordingPermissionsAsync).not.toHaveBeenCalled();
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    expect(mockRecorder.record).toHaveBeenCalledWith({ forDuration: 20 }); expect(mockUpload).not.toHaveBeenCalled();
    await fireEvent.press(screen.getByRole('button', { name: 'Stop and transcribe' }));
    expect(mockUpload).toHaveBeenCalledTimes(1);
    expect(mockUpload.mock.calls[0][0]).toBe('http://127.0.0.1:3097/mobile/v3/transcribe');
    expect(mockUpload.mock.calls[0][1].headers['X-Cosigna-Hosted-Consent']).toBe('voice-v1');
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte'); expect(mockDelete).toHaveBeenCalled();
  });
  it('denied permission leaves typed input usable without recording or uploading', async () => {
    jest.mocked(AudioModule.requestRecordingPermissionsAsync).mockResolvedValue({ granted: false } as Awaited<ReturnType<typeof AudioModule.requestRecordingPermissionsAsync>>);
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    expect(mockRecorder.record).not.toHaveBeenCalled(); expect(mockUpload).not.toHaveBeenCalled();
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('denied')); expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
  });
  it('uses Groq-specific consent for a Groq recording', async () => {
    const callbacks = props(); await render(<LabVoice {...callbacks} provider="groq" />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await fireEvent.press(screen.getByRole('button', { name: 'Stop and transcribe' }));
    expect(mockUpload.mock.calls[0][1].headers['X-Cosigna-Hosted-Consent']).toBe('voice-groq-v1');
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte');
  });
  it('does not send an existing recording to a different provider', async () => {
    const callbacks = props(); await render(<LabVoice {...callbacks} provider="openai" />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await screen.rerender(<LabVoice {...callbacks} provider="groq" />);
    await fireEvent.press(screen.getByRole('button', { name: 'Stop and transcribe' }));
    expect(mockUpload).not.toHaveBeenCalled();
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('provider changed'));
  });
  it('voice checkout starts after the prompt and sends once after measured speech and a pause', async () => {
    jest.useFakeTimers();
    mockRecorder.getStatus.mockReturnValueOnce({ metering: -20 }).mockReturnValueOnce({ metering: -20 }).mockReturnValueOnce({ metering: -20 }).mockReturnValue({ metering: -70 });
    const callbacks = props(); await render(<LabVoice {...callbacks} provider="groq" autoTranscribe autoStartSignal={1} />);
    expect(mockRecorder.record).not.toHaveBeenCalled();
    await act(async () => { jest.advanceTimersByTime(500); });
    expect(mockRecorder.record).toHaveBeenCalledWith({ forDuration: 12 });
    await act(async () => { jest.advanceTimersByTime(2600); });
    expect(mockUpload).toHaveBeenCalledTimes(1);
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte', { heardSpeech: true });
  });
  it('keeps the file URI captured before Android stops and resets the recorder', async () => {
    mockRecorder.stop.mockImplementation(async () => { mockRecorder.isRecording = false; mockRecorder.uri = null; });
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await fireEvent.press(screen.getByRole('button', { name: 'Stop and transcribe' }));
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte');
    expect(mockUpload).toHaveBeenCalledTimes(1);
  });
  it('finishes automatically at 12 seconds when Android omits its completion event', async () => {
    jest.useFakeTimers();
    mockRecorder.getStatus.mockReturnValue({ metering: -20 });
    const callbacks = props(); await render(<LabVoice {...callbacks} autoTranscribe autoStartSignal={1} />);
    await act(async () => { jest.advanceTimersByTime(500); });
    await act(async () => { jest.advanceTimersByTime(12_000); });
    expect(mockUpload).toHaveBeenCalledTimes(1);
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte', { heardSpeech: true });
    await act(async () => { jest.advanceTimersByTime(12_000); });
    expect(mockUpload).toHaveBeenCalledTimes(1);
  });
  it('stops a silent automatic turn without uploading, planning or repeating a question', async () => {
    jest.useFakeTimers();
    const callbacks = props(); await render(<LabVoice {...callbacks} autoTranscribe autoStartSignal={1} />);
    await act(async () => { jest.advanceTimersByTime(500); });
    expect(screen.getByText(/No microphone signal detected yet/)).toBeVisible();
    await act(async () => { jest.advanceTimersByTime(12_000); });
    expect(mockUpload).not.toHaveBeenCalled(); expect(callbacks.onTranscript).not.toHaveBeenCalled();
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('No microphone signal was detected'));
    expect(mockDelete).toHaveBeenCalled(); expect(mockRecorder.isRecording).toBe(false);
    expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
  });
  it('rejects a silent automatic recording even if Android reports completion first', async () => {
    jest.useFakeTimers();
    const callbacks = props(); await render(<LabVoice {...callbacks} autoTranscribe autoStartSignal={1} />);
    await act(async () => { jest.advanceTimersByTime(500); });
    await act(async () => mockRecordingStatus?.({ id: 'synthetic', isFinished: true, hasError: false, error: null, url: 'file:///synthetic-cache/completed.m4a' }));
    expect(mockUpload).not.toHaveBeenCalled(); expect(callbacks.onTranscript).not.toHaveBeenCalled();
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('No microphone signal was detected'));
    expect(mockDelete).toHaveBeenCalled();
  });
  it('handles a released recorder during metering without uploading', async () => {
    jest.useFakeTimers();
    const callbacks = props(); await render(<LabVoice {...callbacks} autoTranscribe autoStartSignal={1} />);
    await act(async () => { jest.advanceTimersByTime(500); });
    mockRecorder.getStatus.mockImplementation(() => { throw new Error('Synthetic released recorder'); });
    await act(async () => { jest.advanceTimersByTime(200); });
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('microphone stopped unexpectedly'));
    expect(mockUpload).not.toHaveBeenCalled();
  });
  it('offers a finished recording for transcription after the native duration limit without uploading automatically', async () => {
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    mockRecorder.isRecording = false; mockRecorder.uri = null;
    await act(async () => mockRecordingStatus?.({ id: 'synthetic', isFinished: true, hasError: false, error: null, url: 'file:///synthetic-cache/completed.m4a' }));
    expect(mockUpload).not.toHaveBeenCalled();
    await fireEvent.press(screen.getByRole('button', { name: 'Transcribe recording' }));
    expect(callbacks.onTranscript).toHaveBeenCalledWith('Order one oat latte');
  });
  it('shows a missing recording error next to the microphone rather than silently doing nothing', async () => {
    mockRecorder.uri = null;
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await fireEvent.press(screen.getByRole('button', { name: 'Stop and transcribe' }));
    expect(screen.getByRole('alert')).toHaveTextContent(/did not produce an audio recording/);
    expect(mockUpload).not.toHaveBeenCalled(); expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
  });
  it('explains a disabled recording control before requesting microphone permission', async () => {
    await render(<LabVoice {...props()} enabled={false} disabledReason="Allow transcription in the settings above." />);
    expect(screen.getByRole('button', { name: 'Speak your order' })).toBeDisabled();
    expect(screen.getByText('Allow transcription in the settings above.')).toBeVisible();
    expect(AudioModule.requestRecordingPermissionsAsync).not.toHaveBeenCalled();
  });
  it('refuses to start capture when microphone permission resolves with the app in the background', async () => {
    jest.mocked(AudioModule.requestRecordingPermissionsAsync).mockImplementation(async () => {
      AppState.currentState = 'background';
      return { granted: true } as Awaited<ReturnType<typeof AudioModule.requestRecordingPermissionsAsync>>;
    });
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    expect(mockRecorder.record).not.toHaveBeenCalled(); expect(mockUpload).not.toHaveBeenCalled();
    expect(callbacks.onError).toHaveBeenCalledWith(expect.stringContaining('Return to Cosigna'));
  });
  it('discard and background stop capture, clean the temporary file and never send audio', async () => {
    const listener = jest.spyOn(AppState, 'addEventListener'); const callbacks = props();
    await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await fireEvent.press(screen.getByRole('button', { name: 'Discard recording' }));
    expect(mockDelete).toHaveBeenCalled(); expect(mockUpload).not.toHaveBeenCalled();
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    await act(async () => { await listener.mock.calls.at(-1)![1]('background'); });
    expect(mockRecorder.isRecording).toBe(false); expect(mockUpload).not.toHaveBeenCalled(); expect(callbacks.onBusy).toHaveBeenLastCalledWith(false);
  });
  it('unmount still deletes the cached file when Expo has already released the native recorder', async () => {
    const callbacks = props(); await render(<LabVoice {...callbacks} />);
    await fireEvent.press(screen.getByRole('button', { name: 'Speak your order' }));
    const original = Object.getOwnPropertyDescriptor(mockRecorder, 'isRecording')!;
    Object.defineProperty(mockRecorder, 'isRecording', { configurable: true, get() { throw new Error('Synthetic released native recorder'); } });
    try { await screen.unmount(); }
    finally { Object.defineProperty(mockRecorder, 'isRecording', original); }
    expect(mockDelete).toHaveBeenCalled(); expect(mockUpload).not.toHaveBeenCalled();
  });
});
