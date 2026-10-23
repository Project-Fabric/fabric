import { useEffect, useMemo, useRef, useState } from 'react';
import { AppState, Text } from 'react-native';
import * as Crypto from 'expo-crypto';
import * as Speech from 'expo-speech';
import { setAudioModeAsync } from 'expo-audio';
import { LabClient, budgetActive, hostedConsent, labMoney, quoteApprovable, type AiProvider, type LabBalance, type LabTaskResult } from '../domain/lab';
import { clarificationRequest, matchesSpokenQuote, spokenQuote, voiceCommand, type SpokenQuote } from '../domain/voice-checkout';
import { LabVoice } from './lab-voice';
import { Button, Copy, styles } from './ui';

interface Props {
  token: string; provider: AiProvider; enabled: boolean; allowance: LabBalance | null;
  initialTask: LabTaskResult | null; requestText: string;
  initialRequest?: string | null; autoStartSignal?: number;
  onBusy: (busy: boolean) => void; onRequest: (text: string) => void; onTask: (task: LabTaskResult) => void;
  onSubmitting: () => void; onRefresh: (id: string) => Promise<void>;
}
type Turn = 'order' | 'answer' | 'confirm';

export function VoiceCheckout(props: Props) {
  const client = useMemo(() => new LabClient(props.token), [props.token]);
  const callbacks = useRef(props);
  useEffect(() => { callbacks.current = props; });
  const alive = useRef(true), active = useRef(false), generation = useRef(0), listening = useRef(false);
  const latest = useRef<LabTaskResult | null>(null), armed = useRef<SpokenQuote | null>(null), turn = useRef<Turn>('order');
  const promptGeneration = useRef(0);
  const clarification = useRef({ count: 0, question: '' });
  const speechTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [running, setRunning] = useState(false), [listen, setListen] = useState(false), [signal, setSignal] = useState(0);
  const [status, setStatus] = useState(''), [transcript, setTranscript] = useState('');
  function current(run: number) { return alive.current && active.current && generation.current === run; }
  function clearSpeechTimer() { if (speechTimer.current) clearTimeout(speechTimer.current); speechTimer.current = null; }
  function end(message: string, read = false) {
    clearSpeechTimer();
    active.current = false; listening.current = false; armed.current = null; generation.current++;
    const stoppedAt = generation.current;
    void Speech.stop().then(() => { if (read && alive.current && !active.current && generation.current === stoppedAt && AppState.currentState === 'active') Speech.speak(message, { language: 'en-US' }); }).catch(() => {});
    callbacks.current.onBusy(false);
    if (alive.current) { setRunning(false); setListen(false); setStatus(message); }
  }
  const endRef = useRef(end);
  useEffect(() => { endRef.current = end; });
  useEffect(() => {
    alive.current = true;
    const sub = AppState.addEventListener('change', state => { if (state !== 'active' && active.current) endRef.current('Voice checkout stopped. Check the current task before continuing.'); });
    return () => { alive.current = false; sub.remove(); endRef.current(''); };
  }, []);
  async function prompt(text: string, next: Turn, run: number, quote: SpokenQuote | null = null) {
    const promptId = ++promptGeneration.current;
    listening.current = false; setListen(false); armed.current = null;
    setStatus('Preparing the spoken prompt. Wait for Listening before speaking.');
    const speechFailure = 'The device speech engine is unavailable or could not finish speaking. No new order was submitted. Close and reopen Cosigna (or Expo Go if using the preview). On Android, enable Speech Services by Google and an English voice in Settings > Text-to-speech output. You can also review manually.';
    const watchSpeech = (ms: number) => {
      clearSpeechTimer();
      speechTimer.current = setTimeout(() => { if (current(run) && promptGeneration.current === promptId) end(speechFailure); }, ms);
    };
    // Native TTS can remain pending forever when Android's speech engine is disabled.
    // A timeout stops the session; it must never stand in for a completed quote.
    watchSpeech(8_000);
    let voices: Speech.Voice[];
    try {
      await Speech.stop();
      if (!current(run)) return;
      voices = await Speech.getAvailableVoicesAsync();
    } catch {
      if (current(run)) end(speechFailure);
      return;
    }
    if (!current(run)) return;
    const voice = voices.find(value => value.language === 'en-US') ?? voices.find(value => /^en[-_]/i.test(value.language));
    if (!voice) { end(speechFailure); return; }
    await setAudioModeAsync({ allowsRecording: false, playsInSilentMode: true });
    if (!current(run)) return;
    setStatus(text);
    Speech.speak(text, {
      language: voice.language,
      onStart: () => { if (current(run) && promptGeneration.current === promptId) watchSpeech(Math.min(120_000, Math.max(20_000, text.length * 125 + 10_000))); },
      onDone: () => {
        if (!current(run) || promptGeneration.current !== promptId) return;
        clearSpeechTimer();
        // Confirmation is armed only after the complete quote has finished speaking.
        armed.current = quote; turn.current = next; listening.current = true;
        setStatus(next === 'confirm' ? 'Listening next: say I confirm, or cancel.' : 'Listening next: speak your reply when the microphone is ready.');
        setListen(true); setSignal(value => value + 1);
      },
      onStopped: () => { if (current(run) && promptGeneration.current === promptId) end('The spoken review was interrupted. Start voice checkout again or review manually.'); },
      onError: () => { if (current(run) && promptGeneration.current === promptId) end(speechFailure); },
    });
  }
  async function review(result: LabTaskResult, run: number) {
    if (result.data.stage === 'needs_input') {
      const question = result.data.clarification ?? 'Please clarify your order.';
      const normalized = question.trim().toLowerCase();
      if (clarification.current.question === normalized || clarification.current.count >= 3) {
        end('Voice checkout paused because the request is still unclear. Check your microphone and edit Your answer on screen, or start voice checkout again. No new order was submitted.');
        return;
      }
      clarification.current = { count: clarification.current.count + 1, question: normalized };
      await prompt(question, 'answer', run);
    } else if (quoteApprovable(result)) {
      const quote = result.data.quote!;
      await prompt(`${quote.quantity} ${quote.offer.title}. Total ${labMoney(quote.total_minor)}. Seller ${quote.offer.merchant_id}. ${quote.offer.fulfillment} ${quote.offer.terms} After I finish speaking, say I confirm, or say cancel.`, 'confirm', run, spokenQuote(result));
    } else end('This request needs checking. Use the current task controls before starting another order.');
  }
  async function plan(request: string, run: number) {
    setStatus('Finding an offer and checking your test allowance…');
    callbacks.current.onRequest(request);
    const result = await client.call<LabTaskResult>('/plan', { request, idempotency_key: Crypto.randomUUID() }, hostedConsent(props.provider, 'planner'));
    if (!current(run)) return;
    latest.current = result; callbacks.current.onTask(result);
    await review(result, run);
  }
  async function receive(text: string, audio?: { heardSpeech: boolean }) {
    if (!active.current || !listening.current) return;
    listening.current = false; setListen(false); setTranscript(text);
    const run = generation.current;
    try {
      const command = voiceCommand(text);
      if (command === 'cancel') {
        const task = latest.current?.data;
        if (task && ['draft', 'needs_input', 'quoted', 'approved'].includes(task.stage)) {
          const result = await client.call<LabTaskResult>('/cancel', { task_id: task.id });
          if (!current(run)) return;
          callbacks.current.onTask(result);
        }
        end('Request cancelled. No new purchase was submitted.', true); return;
      }
      if (turn.current === 'confirm') {
        const result = latest.current, quote = armed.current;
        armed.current = null;
        if (command !== 'confirm') { end('No order placed. To approve, start voice checkout again and say I confirm after the quote.'); return; }
        if (!audio?.heardSpeech) throw new Error('Could not verify microphone activity for that confirmation. Review and approve manually.');
        if (!result || !quote || !matchesSpokenQuote(result, quote) || !budgetActive(callbacks.current.allowance)
          || !callbacks.current.enabled || AppState.currentState !== 'active') throw new Error('The quote or allowance changed. Review a current quote again.');
        // Consume the spoken quote before any asynchronous call; replays cannot submit again.
        callbacks.current.onSubmitting(); setStatus('Submitting the approved test order…');
        const updated = await client.call<LabTaskResult>('/approve', { task_id: quote.taskId, quote_digest: quote.digest });
        if (!current(run)) return;
        latest.current = updated; callbacks.current.onTask(updated);
        await callbacks.current.onRefresh(updated.data.id);
        if (!current(run)) return;
        end(updated.data.stage === 'confirmed' ? 'Your test order is confirmed. No real money moved.' : 'The payment outcome needs checking. Use Check payment and order status before trying again.', true);
      } else if (turn.current === 'answer') {
        if (!latest.current) throw new Error('Open the request again before answering.');
        await plan(clarificationRequest(latest.current, text, callbacks.current.requestText), run);
      } else await plan(text.trim(), run);
    } catch (error) { if (current(run)) end(error instanceof Error ? error.message : 'Voice checkout stopped. Check the current task.'); }
  }
  async function start() {
    if (!props.enabled || active.current) return;
    if (AppState.currentState !== 'active') { setStatus('Return to Cosigna before starting voice checkout.'); return; }
    active.current = true; const run = ++generation.current;
    clarification.current = { count: 0, question: '' };
    latest.current = props.initialTask; armed.current = null; setRunning(true); setTranscript(''); props.onBusy(true);
    try {
      if (props.initialRequest?.trim()) await plan(props.initialRequest.trim(), run);
      else if (latest.current && (latest.current.data.stage === 'needs_input' || quoteApprovable(latest.current))) await review(latest.current, run);
      else await prompt('What would you like to order? Speak after I finish.', 'order', run);
    } catch (error) { if (current(run)) end(error instanceof Error ? error.message : 'Could not start voice checkout.'); }
  }
  const startRef = useRef(start);
  useEffect(() => { startRef.current = start; });
  useEffect(() => { if (props.autoStartSignal) void startRef.current(); }, [props.autoStartSignal]);
  return <>
    <Text style={styles.heading}>Voice checkout</Text>
    <Copy>Tap once to begin. Each spoken turn is sent automatically after a pause or 12 seconds. Cosigna reads the quote, then listens for “I confirm”, “confirm my order” or “cancel”. Test payments only; voice does not verify identity.</Copy>
    {!running && <Button title="Start voice checkout" disabled={!props.enabled} onPress={() => { void start(); }} />}
    {!props.enabled && !running && <Copy>Enable audio transcription, AI planning and spoken results above, and set an active allowance. Resolve any pending payment first.</Copy>}
    {status ? <Text accessibilityLiveRegion="polite" style={styles.status}>{status}</Text> : null}
    {transcript ? <Copy>You said: {transcript}</Copy> : null}
    {running && listen && <LabVoice token={props.token} provider={props.provider} enabled autoTranscribe autoStartSignal={signal}
      startLabel="Speak your reply" onBusy={() => {}} onTranscript={(text, audio) => { void receive(text, audio); }} onError={message => end(message)} />}
    {running && <Button title="Stop voice checkout" secondary onPress={() => end('Voice checkout stopped. Submitted payments still need a status check.')} />}
  </>;
}
