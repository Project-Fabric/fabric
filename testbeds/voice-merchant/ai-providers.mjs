// Provider destinations are fixed; requests and model output cannot select URLs or keys.
const openai = Object.freeze({ id: 'openai', label: 'OpenAI', keyEnv: 'COSIGNA_OPENAI_API_KEY',
  model: 'gpt-6.1-sol', speechModel: 'gpt-transcribe',
  speechUrl: 'https://api.openai.com/v1/audio/transcriptions',
  planningConsent: 'planner-v1', voiceConsent: 'voice-v1' });
const groq = Object.freeze({ id: 'groq', label: 'Groq', keyEnv: 'COSIGNA_GROQ_API_KEY',
  model: 'openai/gpt-oss-120b', speechModel: 'whisper-large-v3-turbo',
  speechUrl: 'https://api.groq.com/openai/v1/audio/transcriptions',
  planningConsent: 'planner-groq-v1', voiceConsent: 'voice-groq-v1' });

export function aiProvider(id) {
  if (id === 'openai') return openai;
  if (id === 'groq') return groq;
  throw new Error('Unsupported AI provider. Choose openai or groq.');
}
export function providerForMode(mode) {
  if (mode === 'fixtures') return openai;
  return aiProvider(mode);
}
