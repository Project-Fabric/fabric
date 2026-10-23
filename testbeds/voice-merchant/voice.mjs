import { LabError } from './store.mjs';
import { aiProvider } from './ai-providers.mjs';

const EXTENSIONS = { 'audio/webm': 'webm', 'audio/mp4': 'mp4', 'audio/wav': 'wav', 'audio/mpeg': 'mp3', 'audio/x-m4a': 'm4a' };
export async function transcribe(bytes, contentType, { apiKey, provider = 'openai', transport = fetch } = {}) {
  const selected = aiProvider(provider);
  const { label, keyEnv } = selected;
  if (!apiKey?.trim()) throw new LabError(503, 'voice_unconfigured', `Set ${keyEnv} on the server to use voice. You can still type a request.`);
  const mime = contentType?.split(';')[0].trim();
  const extension = EXTENSIONS[mime];
  if (!extension || bytes.length === 0 || bytes.length > 8 * 1024 * 1024) {
    throw new LabError(400, 'invalid_audio', 'Record a short audio clip (maximum 8 MiB).');
  }
  const form = new FormData();
  form.append('model', selected.speechModel);
  form.append('response_format', 'json');
  form.append('file', new Blob([bytes], { type: mime }), `request.${extension}`);
  let response;
  try {
    response = await transport(selected.speechUrl, {
      method: 'POST', headers: { Authorization: `Bearer ${apiKey}` }, body: form,
      redirect: 'error', signal: AbortSignal.timeout(30_000),
    });
  } catch { throw new LabError(502, 'voice_unavailable', 'Speech transcription is unavailable. Try again or type your request.'); }
  if (!response.ok) {
    // Return only our own status-specific guidance, never a provider body or credential.
    await response.body?.cancel().catch(() => {});
    const messages = {
      400: `${label} rejected this recording (400). Try recording again; check the server transcription configuration if it repeats.`,
      401: `${label} rejected the server API key (401). Set a valid ${keyEnv} and restart the phone lab.`,
      403: `This ${label} project cannot access transcription (403). Check the API key permissions and model access.`,
      404: `The configured transcription model is unavailable to this ${label} project (404). Check model access on the server.`,
      429: provider === 'groq' ? 'Groq rate or quota limit reached (429). Wait for the limit to reset or check your Groq plan limits.'
        : 'OpenAI blocked transcription because of a rate or quota limit (429). Check API credits and spending limits, then try again.',
    };
    throw new LabError(502, 'voice_unavailable', messages[response.status] ?? `Speech transcription is unavailable at ${label}. Try again shortly.`);
  }
  // Bound the response as well as the audio upload; never return provider error bodies.
  const reader = response.body.getReader();
  const chunks = []; let length = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    length += value.length;
    if (length > 64 * 1024) { await reader.cancel(); throw new LabError(502, 'invalid_transcript', 'Speech response was too large.'); }
    chunks.push(value);
  }
  let result;
  try { result = JSON.parse(Buffer.concat(chunks).toString('utf8')); }
  catch { throw new LabError(502, 'invalid_transcript', 'Speech response was invalid.'); }
  if (typeof result.text !== 'string' || !result.text.trim() || result.text.length > 4096) {
    throw new LabError(502, 'invalid_transcript', 'Speech was unclear. Please record again or type your request.');
  }
  return result.text.trim();
}
