import { useEffect, useMemo, useRef, useState } from 'react';
import { AppState, StyleSheet, Switch, Text, TextInput, View } from 'react-native';
import * as Crypto from 'expo-crypto';
import * as Speech from 'expo-speech';
import { LabApiError, LabClient, LAB_URL, LAB_IS_WIFI, budgetActive, hasAttempt, hostedConsent, labMoney, minorUnits, quoteApprovable,
  type LabState, type LabTaskResult } from '../domain/lab';
import { Button, Card, Copy, Screen, Title, colors, styles } from './ui';
import { LabVoice } from './lab-voice';
import { VoiceCheckout } from './voice-checkout';
import { clarificationRequest } from '../domain/voice-checkout';
import { loadDeviceSession, saveDeviceSession } from '../domain/device-session';

function Toggle({ label, value, onChange, disabled = false }: { label: string; value: boolean; onChange: (value: boolean) => void; disabled?: boolean }) {
  return <View style={local.toggle}><Text style={[styles.copy, { flex: 1 }]}>{label}</Text><Switch accessibilityLabel={label} value={value} onValueChange={onChange} disabled={disabled} trackColor={{ true: colors.accent }} /></View>;
}
export function OrderLabScreen({ goBack, focused = true, incomingRequest = null, incomingId }: {
  goBack: () => void; focused?: boolean; incomingRequest?: string | null; incomingId?: string;
}) {
  const [token, setToken] = useState(''), [code, setCode] = useState('');
  const [restoring, setRestoring] = useState(true), [handledAssistant, setHandledAssistant] = useState<string | null>(null);
  const assistantKey = JSON.stringify([incomingRequest, incomingId]);
  const assistantDraft = handledAssistant === assistantKey ? null : incomingRequest;
  const [voiceDraft, setVoiceDraft] = useState<string | null>(null), [voiceSignal, setVoiceSignal] = useState(0);
  const [snapshot, setSnapshot] = useState<LabState | null>(null), [result, setResult] = useState<LabTaskResult | null>(null);
  const [text, setText] = useState(''), [limit, setLimit] = useState('20.00'), [cap, setCap] = useState('10.00');
  const [busy, setBusy] = useState(false), [voiceBusy, setVoiceBusy] = useState(false), [message, setMessage] = useState('');
  const [voiceCheckoutBusy, setVoiceCheckoutBusy] = useState(false), [answer, setAnswer] = useState('');
  const [plannerConsent, setPlannerConsent] = useState(false), [voiceConsent, setVoiceConsent] = useState(false), [spoken, setSpoken] = useState(false);
  const [reviewDigest, setReviewDigest] = useState<string | null>(null), [mustRefresh, setMustRefresh] = useState(false);
  const [foreground, setForeground] = useState(true);
  const lock = useRef(false), key = useRef(''), alive = useRef(true);
  const client = useMemo(() => new LabClient(token), [token]);
  const task = result?.data ?? null;
  const provider = snapshot?.ai_provider ?? 'openai';
  const providerLabel = provider === 'groq' ? 'Groq' : 'OpenAI';
  const blocked = busy || voiceBusy || voiceCheckoutBusy;
  const active = budgetActive(snapshot?.balance ?? null);
  const unresolved = Boolean(snapshot?.balance?.reserved_minor || task?.stage === 'unknown' || task?.stage === 'submitting');
  const voiceEnabled = Boolean(voiceConsent && plannerConsent && spoken && snapshot?.voice_available && snapshot.mode !== 'fixtures' && active && !busy && !voiceBusy && !mustRefresh && !unresolved);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const saved = await loadDeviceSession();
        if (!saved || cancelled) return;
        const api = new LabClient(saved.token);
        const state = await api.call<LabState>('/state');
        const id = state.recent_tasks[0];
        const fresh = id ? await api.call<LabTaskResult>(`/tasks/${id}`) : null;
        if (!cancelled) { setToken(saved.token); setSnapshot(state); setResult(fresh); }
      } catch (error) {
        if (error instanceof LabApiError && error.status === 401) { try { await saveDeviceSession(null); } catch { /* Invalid token is still refused by the server. */ } }
        if (!cancelled) setMessage(error instanceof Error ? error.message : 'Could not restore your connection. Pair again.');
      } finally { if (!cancelled) setRestoring(false); }
    })();
    return () => { cancelled = true; };
  }, []);
  useEffect(() => {
    alive.current = true;
    const sub = AppState.addEventListener('change', state => {
      setForeground(state === 'active');
      if (state !== 'active') { setReviewDigest(null); void Speech.stop(); }
    });
    return () => { alive.current = false; sub.remove(); void Speech.stop(); };
  }, []);
  useEffect(() => { if (!focused) void Speech.stop(); }, [focused]);
  async function refresh(api = client, taskId = task?.id) {
    const state = await api.call<LabState>('/state');
    const id = taskId ?? state.recent_tasks[0];
    const fresh = id ? await api.call<LabTaskResult>(`/tasks/${id}`) : null;
    if (alive.current) {
      if (state.ai_provider !== snapshot?.ai_provider) { setVoiceConsent(false); setPlannerConsent(false); }
      setSnapshot(state); setResult(fresh); setAnswer(''); setMustRefresh(false); setReviewDigest(null);
    }
  }
  async function run(action: () => Promise<void>) {
    if (lock.current || voiceBusy || voiceCheckoutBusy) return;
    lock.current = true; setBusy(true); setMessage('');
    try { await action(); }
    catch (error) {
      if (alive.current) {
        setMessage(error instanceof Error ? error.message : 'The action failed. Refresh task status.');
        if (error instanceof LabApiError && error.status === 401) {
          setToken(''); setSnapshot(null); setResult(null); setVoiceConsent(false); setPlannerConsent(false); setSpoken(false);
          setReviewDigest(null); setText(''); key.current = ''; void Speech.stop();
          try { await saveDeviceSession(null); } catch { setMessage('Phone access expired. Could not clear encrypted storage; pair again or clear app data.'); }
        }
      }
    } finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  function changeText(value: string) { setText(value); key.current = ''; setReviewDigest(null); }
  function say(value: string) { if (alive.current && spoken && AppState.currentState === 'active' && focused) Speech.speak(value); }
  async function plan(request = text) {
    if (mustRefresh || unresolved) throw new Error('Resolve the existing task before requesting another purchase.');
    if (!request.trim()) throw new Error('Speak or type your order first.');
    if (snapshot?.mode !== 'fixtures' && !plannerConsent) throw new Error('Allow AI planning to send this request, or use the fixture server.');
    if (task?.stage === 'cancelled' || (task?.quote && task.quote.expires_at <= Date.now() / 1000)) key.current = '';
    key.current ||= Crypto.randomUUID();
    setReviewDigest(null);
    const planned = await client.call<LabTaskResult>('/plan', { request: request.trim(), idempotency_key: key.current }, plannerConsent ? hostedConsent(provider, 'planner') : undefined);
    setResult(planned); setAnswer(''); setMustRefresh(false);
    say(planned.data.clarification ?? (planned.data.quote ? `Your quote is ${labMoney(planned.data.quote.total_minor)}. Review it before approving.` : 'Your request has been updated.'));
  }
  async function approve() {
    if (!focused || !foreground || !result || !quoteApprovable(result) || reviewDigest !== result.approval_digest || mustRefresh) throw new Error('Review a current quote before approving.');
    // A dropped response never enables a second purchase. Fetch this durable task first.
    setMustRefresh(true); setReviewDigest(null);
    const updated = await client.call<LabTaskResult>('/approve', { task_id: result.data.id, quote_digest: result.approval_digest });
    setResult(updated);
    await refresh(client, updated.data.id);
    say(updated.data.stage === 'confirmed' ? 'The test merchant accepted your order. No real money moved.' : 'Check your order status before trying again.');
  }
  const order = snapshot?.orders.find(value => value.id === task?.receipt?.order_id);
  return <Screen>{process.env.EXPO_PUBLIC_COSIGNA_NATIVE_APP !== '1' && <Button title="Back to tasks" secondary onPress={goBack} />}
    <Title eyebrow="PHONE → COSIGNA → TEST MERCHANT">An order,{"\n"}with your permission.</Title>
    <Card><Text style={styles.status}>{LAB_IS_WIFI ? 'WI-FI' : 'USB'} DEVELOPMENT LAB · {snapshot?.payment_mode === 'stripe-test' ? 'STRIPE SANDBOX' : 'SIMULATED PAYMENTS'}</Text>
      <Copy>No real funds, food delivery or pickup. Google Assistant handoff requires the installed Android build and Google’s test preview. Swiggy and Zomato are not connected.</Copy></Card>
    {message ? <Text accessibilityRole="alert" style={styles.error}>{message}</Text> : null}
    {assistantDraft && <Card><Text style={styles.heading}>Request from another app</Text>
      <Copy>{assistantDraft}</Copy><Copy>This is a draft, not payment approval. Cosigna will find an offer and read its exact price before asking for a fresh confirmation.</Copy>
      <Button title="Continue assistant request by voice" disabled={!token || !voiceEnabled || blocked}
        onPress={() => { changeText(assistantDraft); setVoiceDraft(assistantDraft); setHandledAssistant(assistantKey); setVoiceSignal(value => value + 1); }} />
      <Copy>Connect and enable the voice and planning permissions below to continue.</Copy>
    </Card>}
    {!token ? <Card><Text style={styles.heading}>Pair your test phone</Text>
      <Copy>{LAB_IS_WIFI ? 'Use the same trusted Wi-Fi as the computer.' : 'Connect by USB.'} On the computer, start the mobile lab and create a one-use code. This phone will be able to view and manage that single-owner lab’s tasks and test allowance.</Copy>
      <Text style={styles.small}>{LAB_URL} · {LAB_IS_WIFI ? 'Trusted Wi-Fi test connection' : 'USB forwarding required'}</Text>
      <Copy>Enter the six-digit code shown on your computer. It expires after five minutes.</Copy>
      <TextInput accessibilityLabel="One-use pairing code" value={code} onChangeText={value => setCode(value.replace(/\D/g, '').slice(0, 6))} keyboardType="number-pad" autoCapitalize="none" autoCorrect={false} maxLength={6} style={local.input} editable={!busy} />
      {restoring && <Copy>Checking your saved connection…</Copy>}
      <Button title="Pair with computer" disabled={restoring || busy || !/^\d{6}$/.test(code.trim())} onPress={() => { void run(async () => {
        const paired = await new LabClient().call<{ token: string; expires_at: number }>('/pair', { code: code.trim() });
        await saveDeviceSession({ schema: 'cosigna.device-session/v1', token: paired.token, expiresAt: paired.expires_at });
        setToken(paired.token); setCode(''); await refresh(new LabClient(paired.token));
      }); }} />
      <Copy>Your scoped connection is saved in encrypted device storage until its 12-hour expiry or revocation. A server restart requires pairing again. Recording permissions and quote approvals are not saved. No model or payment API key belongs here.</Copy>
    </Card> : <>
      <Card><Text style={styles.heading}>Your data, before you share it</Text>
        <Copy>{snapshot?.privacy.planning ?? 'Loading the server’s data policy…'}</Copy>
        <Copy>Manual recordings go to {providerLabel} when you tap Stop and transcribe or Transcribe recording. Starting voice checkout allows automatic transcription and planning for that session. It asks for a new confirmation after reading the quote.</Copy>
        <Copy>{snapshot?.privacy.retention}</Copy><Copy>No location, contacts or voiceprint is requested. Provider-side retention is separate; this preview does not promise zero retention.</Copy>
        <Toggle label={`Allow audio transcription with ${providerLabel}`} value={voiceConsent} disabled={blocked} onChange={setVoiceConsent} />
        {snapshot?.mode !== 'fixtures' && <Toggle label={`Allow request and catalog to ${providerLabel} for planning`} value={plannerConsent} disabled={blocked} onChange={setPlannerConsent} />}
        <Toggle label="Read results aloud using device speech" value={spoken} disabled={blocked} onChange={value => { setSpoken(value); if (!value) void Speech.stop(); }} />
        <Button title="Refresh lab and current task" secondary disabled={blocked} onPress={() => { void run(() => refresh()); }} />
        <Button title="Disconnect phone" secondary disabled={blocked} onPress={() => { void run(async () => {
          await client.call('/disconnect', {}); setToken(''); setSnapshot(null); setResult(null); setText(''); setReviewDigest(null);
          setVoiceConsent(false); setPlannerConsent(false); setSpoken(false); key.current = ''; void Speech.stop();
          try { await saveDeviceSession(null); } catch { setMessage('Access revoked on the server. Could not clear encrypted storage; clear app data before sharing this phone.'); }
        }); }} />
        <Copy>Disconnecting revokes phone access. It does not cancel submitted orders or delete ledger history. The computer can also revoke access.</Copy>
      </Card>
      <Card><Text style={styles.heading}>Your test allowance</Text>
        {snapshot?.balance && <><Text style={styles.price}>{labMoney(snapshot.balance.available_minor)} available</Text>
          <Copy>{labMoney(snapshot.balance.spent_minor)} spent · {labMoney(snapshot.balance.reserved_minor)} reserved · {labMoney(snapshot.balance.budget.terms.per_purchase_minor)} per-order cap</Copy>
          <Text style={styles.small}>{active ? 'Active' : 'Revoked or expired'} · expires {new Date(snapshot.balance.budget.terms.expires_at * 1000).toLocaleTimeString()}</Text></>}
        {!active && <><TextInput accessibilityLabel="Test allowance in USD" value={limit} onChangeText={setLimit} keyboardType="decimal-pad" style={local.input} editable={!blocked} />
          <TextInput accessibilityLabel="Per-order limit in USD" value={cap} onChangeText={setCap} keyboardType="decimal-pad" style={local.input} editable={!blocked} />
          <Button title="Set test allowance" disabled={blocked || !snapshot} onPress={() => { void run(async () => {
            await client.call('/budget', { limit_minor: minorUnits(limit), per_purchase_minor: minorUnits(cap) }); await refresh();
          }); }} /></>}
        {active && <Button title="Revoke test allowance" secondary disabled={blocked} onPress={() => { void run(async () => { await client.call('/revoke-budget', {}); await refresh(); }); }} />}
        <Copy>This is a spending limit, not a funded wallet. Each purchase still requires your exact-quote approval. Revocation blocks new spending; unresolved payments still need reconciliation.</Copy>
      </Card>
      {focused && foreground && <Card><VoiceCheckout token={token} provider={provider} allowance={snapshot?.balance ?? null}
        enabled={voiceEnabled} initialRequest={voiceDraft} autoStartSignal={voiceSignal}
        initialTask={result} requestText={text} onBusy={setVoiceCheckoutBusy} onRequest={value => { changeText(value); setVoiceDraft(null); }}
        onTask={value => { setResult(value); setAnswer(''); }} onSubmitting={() => { setMustRefresh(true); setReviewDigest(null); }}
        onRefresh={id => refresh(client, id)} /></Card>}
      <Card><Text style={styles.heading}>What would you like?</Text>
        <Text style={styles.status}>{snapshot?.mode === 'fixtures' ? 'FIXTURE SCRIPT · NOT AI' : 'HOSTED AI PLANNER'}</Text>
        <TextInput accessibilityLabel="Your order request" value={text} onChangeText={changeText} multiline maxLength={4096} style={[local.input, { minHeight: 95 }]} editable={!blocked} placeholder="Order one oat latte" placeholderTextColor={colors.muted} />
        {snapshot?.mode === 'fixtures' && <><Copy>The offline planner accepts only the two exact examples below. Route planning and office locations are not available.</Copy>
          <Button title="Use oat latte example" secondary disabled={blocked} onPress={() => changeText('Order one oat latte')} />
          <Button title="Use black coffee example" secondary disabled={blocked} onPress={() => changeText('Order one black coffee')} /></>}
        {focused && foreground && !voiceCheckoutBusy && task?.stage !== 'needs_input' && <LabVoice token={token} provider={provider} enabled={Boolean(voiceConsent && snapshot?.voice_available && !busy)}
          disabledReason={!snapshot?.voice_available ? `Voice is not configured on the computer. Start the lab with its ${providerLabel} API key, then refresh.` : !voiceConsent ? `Turn on “Allow audio transcription with ${providerLabel}” above to enable the microphone.` : busy ? 'Wait for the current action to finish.' : undefined}
          onTranscript={value => { changeText(value); setMessage('Transcript ready. Edit it, then find a quote. No order has been placed.'); }}
          onError={setMessage} onBusy={setVoiceBusy} />}
        {!snapshot?.voice_available && <Copy>Hosted transcription is not configured on the computer. Typing and the exact fixture examples still work.</Copy>}
        {unresolved && <Copy>A purchase is reserved or unresolved. Recover it and check status before starting another request.</Copy>}
        <Button title="Find a quote" disabled={blocked || !active || mustRefresh || unresolved || (snapshot?.mode !== 'fixtures' && !plannerConsent)} onPress={() => { void run(plan); }} />
      </Card>
      {task && <Card><Text style={styles.heading}>{task.stage === 'quoted' ? 'Review your quote' : 'Your request'}</Text>
        <Text accessibilityLiveRegion="polite" style={styles.status}>{task.stage.replaceAll('_', ' ')}</Text>
        {task.clarification && <Copy>{task.clarification}</Copy>}
        {task.stage === 'needs_input' && <>
          <Text style={styles.heading}>Your answer</Text>
          <TextInput accessibilityLabel="Your answer" value={answer} onChangeText={setAnswer} multiline maxLength={1024}
            editable={!blocked} style={local.input} placeholder={snapshot?.mode === 'fixtures' ? 'Order one black coffee' : 'For example: Yes, one black coffee'} placeholderTextColor={colors.muted} />
          {focused && foreground && !voiceCheckoutBusy && <LabVoice token={token} provider={provider}
            enabled={Boolean(voiceConsent && snapshot?.voice_available && !busy)} startLabel="Speak your answer"
            disabledReason={`Enable audio transcription with ${providerLabel} to answer by voice.`}
            onTranscript={setAnswer} onBusy={setVoiceBusy} onError={setMessage} />}
          <Button title="Send answer" disabled={blocked || !answer.trim() || !active || mustRefresh || unresolved || (snapshot?.mode !== 'fixtures' && !plannerConsent)} onPress={() => { void run(async () => {
            if (!result) return;
            const request = snapshot?.mode === 'fixtures' ? answer.trim() : clarificationRequest(result, answer, text); key.current = ''; await plan(request);
          }); }} />
          <Copy>{snapshot?.mode === 'fixtures' ? 'Offline mode needs a complete supported request, such as Order one black coffee.' : 'Your reply includes the earlier request and the café’s question. A new quote still needs confirmation.'}</Copy>
        </>}
        {task.quote && <><Text style={styles.heading}>{task.quote.quantity} × {task.quote.offer.title}</Text><Text style={styles.price}>{labMoney(task.quote.total_minor)}</Text>
          <Copy>Seller: {task.quote.offer.merchant_id}</Copy><Copy>{task.quote.offer.fulfillment}</Copy><Copy>{task.quote.offer.terms}</Copy>
          <Text style={styles.small}>Quote expires {new Date(task.quote.expires_at * 1000).toLocaleTimeString()}</Text></>}
        {quoteApprovable(result) && !mustRefresh && active && <>
          {reviewDigest !== result?.approval_digest ? <Button title="Review this quote for approval" disabled={blocked} onPress={() => setReviewDigest(result?.approval_digest ?? null)} />
            : <><Copy>This button approves only the item, seller and total above. Voice checkout asks for its own fresh confirmation after reading the quote.</Copy>
              <Button title="Approve and place test order" disabled={blocked} onPress={() => { void run(approve); }} /></>}
        </>}
        {mustRefresh && <Copy>The submission response needs checking. Refresh this task before another action. Do not create a replacement order.</Copy>}
        {hasAttempt(task) && <Button title="Check payment and order status" secondary disabled={blocked} onPress={() => { void run(async () => {
          const updated = await client.call<LabTaskResult>('/reconcile', { task_id: task.id }); setResult(updated); await refresh(client, task.id);
        }); }} />}
        {['draft', 'needs_input', 'quoted', 'approved'].includes(task.stage) && <Button title="Cancel unsubmitted request" secondary disabled={blocked || mustRefresh} onPress={() => { void run(async () => {
          const updated = await client.call<LabTaskResult>('/cancel', { task_id: task.id }); setResult(updated); key.current = ''; await refresh(client, task.id);
        }); }} />}
        {['submitting', 'unknown'].includes(task.stage) && <Copy>The outcome is uncertain. Your budget stays reserved. Check status; do not place a replacement order.</Copy>}
        {task.stage === 'confirmed' && <Copy>Merchant accepted · {order?.state ?? 'refresh for preparation status'}. Payment confirmation and pickup readiness are separate.</Copy>}
        {task.receipt && <Text selectable style={styles.small}>Order: {task.receipt.order_id}</Text>}
        {order?.payment?.payment_intent_id && <Text selectable style={styles.small}>Stripe test PaymentIntent: {order.payment.payment_intent_id} · {order.payment.status}</Text>}
      </Card>}
      {Boolean(snapshot?.recent_tasks.length) && <Card><Text style={styles.heading}>Recover a previous request</Text><Copy>Stored on the computer. Recovery reads status; it does not submit another order.</Copy>
        {snapshot?.recent_tasks.slice(0, 8).map((id, index) => <Button key={id} title={`Open recent request ${index + 1}`} secondary disabled={blocked} onPress={() => { void run(() => refresh(client, id)); }} />)}
      </Card>}
    </>}
  </Screen>;
}
const local = StyleSheet.create({
  input: { color: colors.ink, backgroundColor: colors.bg, borderColor: colors.line, borderWidth: 1, borderRadius: 12, padding: 14, fontSize: 16, minHeight: 50 },
  toggle: { flexDirection: 'row', alignItems: 'center', gap: 14 },
});
