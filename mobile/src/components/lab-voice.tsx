import { useEffect, useRef, useState } from 'react';
import { AppState, Platform, Text } from 'react-native';
import { AudioModule, RecordingPresets, setAudioModeAsync, useAudioRecorder, type RecordingStatus } from 'expo-audio';
import { File } from 'expo-file-system';
import { fetch as expoFetch } from 'expo/fetch';
import { Button, Copy, styles } from './ui';
import { LabClient, hostedConsent, type AiProvider, type LabTransport } from '../domain/lab';

export function LabVoice({ token, enabled, provider = 'openai', disabledReason, startLabel = 'Speak your order', autoStartSignal = 0, autoTranscribe = false, onTranscript, onBusy, onError }: {
  token: string; enabled: boolean; provider?: AiProvider; disabledReason?: string; startLabel?: string;
  autoStartSignal?: number; autoTranscribe?: boolean;
  onTranscript: (text: string, audio?: { heardSpeech: boolean }) => void; onBusy: (value: boolean) => void; onError: (message: string) => void;
}) {
  const statusHandler = useRef<(status: RecordingStatus) => void>(() => {});
  const startRef = useRef<() => Promise<void>>(async () => {}), finishRef = useRef<(send: boolean) => Promise<void>>(async () => {});
  const recorder = useAudioRecorder({ ...RecordingPresets.HIGH_QUALITY, isMeteringEnabled: autoTranscribe }, status => statusHandler.current(status));
  const [recording, setRecording] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [hasInput, setHasInput] = useState(false);
  const [ready, setReady] = useState(false), [notice, setNotice] = useState(''), [error, setError] = useState('');
  const completedUri = useRef<string | null>(null), requestingPermission = useRef(false);
  const lastUri = useRef<string | null>(null);
  const recordingProvider = useRef(provider);
  const meterTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const durationTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const levels = useRef({ speechSamples: 0, lastSpeech: 0, started: 0 });
  const running = useRef(false), processing = useRef(false), alive = useRef(true), generation = useRef(0);
  const callbacks = useRef({ onTranscript, onBusy, onError });
  useEffect(() => { callbacks.current = { onTranscript, onBusy, onError }; }, [onTranscript, onBusy, onError]);
  const cleanFile = (uri: string | null) => { if (uri) { const file = new File(uri); if (file.exists) file.delete(); } };
  const stopMeter = () => {
    if (meterTimer.current) clearInterval(meterTimer.current); meterTimer.current = null;
    if (durationTimer.current) clearTimeout(durationTimer.current); durationTimer.current = null;
  };
  const reportError = (message: string) => { if (alive.current) { setError(message); setNotice(''); callbacks.current.onError(message); } };
  const dispose = async () => {
    generation.current++;
    stopMeter();
    const uri = completedUri.current ?? lastUri.current;
    running.current = false;
    // Expo may already have released the shared native object during unmount. Always
    // attempt file cleanup using the cached URI without reading a released recorder.
    try { if (recorder.isRecording) await recorder.stop(); }
    catch { if (alive.current) reportError('Recording stop could not be confirmed. Clear Cosigna app cache before sharing this phone.'); }
    finally {
      try { cleanFile(uri); } catch { reportError('Recording cleanup could not be confirmed. Clear Cosigna app cache.'); }
      completedUri.current = null; lastUri.current = null; callbacks.current.onBusy(false);
      if (alive.current) { setRecording(false); setUploading(false); setReady(false); setNotice(''); }
    }
  };
  const disposeRef = useRef(dispose);
  useEffect(() => { disposeRef.current = dispose; });
  useEffect(() => {
    alive.current = true;
    const sub = AppState.addEventListener('change', state => { if (state !== 'active' && !requestingPermission.current) void disposeRef.current(); });
    return () => { alive.current = false; sub.remove(); void disposeRef.current(); };
  }, []);
  useEffect(() => {
    statusHandler.current = status => {
      if (!alive.current || !running.current || processing.current) return;
      if (status.hasError) { reportError('The microphone could not finish recording. Try again or check microphone access in phone settings.'); void disposeRef.current(); }
      else if (status.isFinished) {
        completedUri.current = status.url ?? recorder.uri;
        if (autoTranscribe) void finishRef.current(true);
        else { setRecording(false); setReady(true); setNotice('Recording finished. Tap Transcribe recording to send it, or discard it.'); }
      }
    };
  });
  async function finish(send: boolean) {
    if (!running.current || processing.current) return;
    processing.current = true;
    stopMeter();
    running.current = false;
    const run = generation.current;
    let recordedUri: string | null = completedUri.current ?? lastUri.current;
    setRecording(false); setReady(false); setUploading(send); setError('');
    setNotice(send ? 'Sending your recording for transcription…' : 'Recording discarded.');
    try {
      recordedUri ??= recorder.uri;
      if (recorder.isRecording) await recorder.stop();
      recordedUri ??= completedUri.current ?? recorder.uri;
      if (send && alive.current && run === generation.current) {
        // Silent emulator/phone input can produce invented Whisper transcripts.
        // Do not send a silent automatic turn into another planner/question loop.
        if (autoTranscribe && levels.current.speechSamples < 3) throw new Error('No microphone signal was detected. Voice checkout stopped without sending this recording. Check microphone access. In the Android emulator, enable host microphone input in Extended controls > Microphone, then try again.');
        if (recordingProvider.current !== provider) throw new Error('The voice provider changed. Record again after allowing the new provider.');
        if (!recordedUri) throw new Error('The phone did not produce an audio recording. Try again and check microphone permission.');
        const file = new File(recordedUri);
        if (!file.size || file.size > 8 * 1024 * 1024) throw new Error('Recording was empty or too large. Try a shorter request.');
        const result = await new LabClient(token, expoFetch as LabTransport).send<{ text: string }>('/transcribe', file, 'audio/mp4', hostedConsent(provider, 'voice'));
        if (alive.current && run === generation.current) {
          setNotice('Transcript ready.');
          if (autoTranscribe) callbacks.current.onTranscript(result.text, { heardSpeech: levels.current.speechSamples >= 3 });
          else callbacks.current.onTranscript(result.text);
        }
      }
    } catch (error) { if (alive.current && run === generation.current) reportError(error instanceof Error ? error.message : 'Recording failed. Type your request instead.'); }
    finally {
      processing.current = false;
      completedUri.current = null;
      lastUri.current = null;
      try { cleanFile(recordedUri); } catch { reportError('Could not remove the recording. Clear Cosigna app cache.'); }
      if (alive.current && run === generation.current) { setUploading(false); callbacks.current.onBusy(false); }
    }
  }
  async function start() {
    if (!enabled || running.current || processing.current || uploading || Platform.OS === 'web') return;
    processing.current = true;
    recordingProvider.current = provider;
    setError(''); setHasInput(false); setNotice('Requesting microphone access…'); completedUri.current = null;
    running.current = true; callbacks.current.onBusy(true);
    const run = ++generation.current;
    try {
      requestingPermission.current = true;
      const permission = await AudioModule.requestRecordingPermissionsAsync();
      requestingPermission.current = false;
      if (!permission.granted) throw new Error('Microphone permission was denied. You can type, or enable it in phone settings.');
      if (!alive.current || run !== generation.current) return;
      if (AppState.currentState && AppState.currentState !== 'active') throw new Error('Return to Cosigna and tap Speak your order again.');
      await setAudioModeAsync({ allowsRecording: true, playsInSilentMode: true, allowsBackgroundRecording: false });
      await recorder.prepareToRecordAsync();
      lastUri.current = recorder.uri;
      if (!alive.current || run !== generation.current) { await disposeRef.current(); return; }
      recorder.record({ forDuration: autoTranscribe ? 12 : 20 }); setRecording(true);
      setNotice(autoTranscribe ? 'Listening. Pause when finished; your audio will be sent automatically.' : 'Listening. Tap Stop and transcribe when you finish speaking.');
      if (autoTranscribe) {
        levels.current = { speechSamples: 0, lastSpeech: 0, started: Date.now() };
        // Some Android recorders omit the native duration-completed event.
        durationTimer.current = setTimeout(() => { void finishRef.current(true); }, 12_000);
        meterTimer.current = setInterval(() => {
          if (!running.current || processing.current) return;
          let status;
          try { status = recorder.getStatus(); }
          catch { reportError('The microphone stopped unexpectedly. Start voice checkout again.'); void disposeRef.current(); return; }
          if (typeof status.metering === 'number' && status.metering > -45) {
            levels.current.speechSamples++; levels.current.lastSpeech = Date.now();
            if (levels.current.speechSamples === 3) setHasInput(true);
          }
          if (levels.current.speechSamples >= 3 && Date.now() - levels.current.lastSpeech > 1200 && Date.now() - levels.current.started > 1800) void finishRef.current(true);
        }, 200);
      }
    } catch (error) {
      reportError(error instanceof Error ? error.message : 'Microphone unavailable.');
      await disposeRef.current();
    } finally { requestingPermission.current = false; processing.current = false; }
  }
  useEffect(() => { startRef.current = start; finishRef.current = finish; });
  useEffect(() => {
    if (!autoStartSignal) return;
    // Wait for the preceding spoken prompt/audio tail before opening the microphone.
    const timer = setTimeout(() => { void startRef.current(); }, 500);
    return () => clearTimeout(timer);
  }, [autoStartSignal]);
  return <>
    <Button title={recording ? 'Stop and transcribe' : ready ? 'Transcribe recording' : uploading ? 'Transcribing…' : startLabel}
      disabled={uploading || (!recording && !ready && (!enabled || Platform.OS === 'web'))} onPress={() => { if (recording || ready) void finish(true); else void start(); }} />
    {!enabled && !recording && !ready && !uploading && <Text style={styles.status}>{disabledReason ?? 'Voice is unavailable. Check transcription settings above.'}</Text>}
    {notice ? <Text accessibilityLiveRegion="polite" style={styles.status}>{notice}</Text> : null}
    {recording && autoTranscribe && <Text accessibilityLiveRegion="polite" style={styles.status}>{hasInput ? 'Microphone signal detected.' : 'No microphone signal detected yet. Speak when ready.'}</Text>}
    {error ? <Text accessibilityRole="alert" style={styles.error}>{error}</Text> : null}
    {(recording || ready) && <><Copy>{autoTranscribe ? 'This voice session sends each turn after a pause or 12 seconds. Payment still needs a fresh spoken confirmation after the quote.' : 'Up to 20 seconds. Transcription starts only when you tap Stop and transcribe or Transcribe recording.'}</Copy><Button title="Discard recording" secondary onPress={() => { void finish(false); }} /></>}
    <Copy>The app attempts to delete its temporary recording after transcription or discard. Submitted text remains in the computer’s transaction ledger. Force-closing the app may leave a cache file.</Copy>
  </>;
}
