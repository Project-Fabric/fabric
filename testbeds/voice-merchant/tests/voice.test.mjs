import test from 'node:test';
import assert from 'node:assert/strict';
import { transcribe } from '../voice.mjs';

test('recording uses the hosted multipart transcription boundary without exposing the key', async () => {
  const bytes = Buffer.from('synthetic-audio-for-wire-contract'); let called = false;
  const text = await transcribe(bytes, 'audio/webm;codecs=opus', { apiKey: 'test-key', transport: async (url, options) => {
    called = true; assert.equal(url, 'https://api.openai.com/v1/audio/transcriptions');
    assert.equal(options.headers.Authorization, 'Bearer test-key'); assert.equal(options.redirect, 'error');
    assert.equal(options.body.get('model'), 'gpt-transcribe');
    assert.equal(options.body.get('file').name, 'request.webm');
    assert.deepEqual(Buffer.from(await options.body.get('file').arrayBuffer()), bytes);
    return new Response(JSON.stringify({ text: 'Order one oat latte' }));
  } });
  assert(called); assert.equal(text, 'Order one oat latte');
});

test('missing credentials, invalid audio and bad provider responses never become requests', async () => {
  const bytes = Buffer.from('synthetic'); const options = { apiKey: 'test-key', transport: async () => { throw new Error('must not run'); } };
  await assert.rejects(transcribe(bytes, 'audio/webm'), error => error.code === 'voice_unconfigured');
  await assert.rejects(transcribe(bytes, 'text/plain', options), error => error.code === 'invalid_audio');
  await assert.rejects(transcribe(Buffer.alloc(0), 'audio/webm', options), error => error.code === 'invalid_audio');
  await assert.rejects(transcribe(bytes, 'audio/webm', { apiKey: 'test-key', transport: async () => new Response('private provider body', { status: 401 }) }), error => error.code === 'voice_unavailable' && !error.message.includes('private'));
  for (const body of ['not JSON', '{}', JSON.stringify({ text: '' }), JSON.stringify({ text: 'x'.repeat(4097) }), 'x'.repeat(65 * 1024)]) {
    await assert.rejects(transcribe(bytes, 'audio/webm', { apiKey: 'test-key', transport: async () => new Response(body) }), error => ['invalid_transcript'].includes(error.code));
  }
});

test('provider failures give actionable guidance without returning provider secrets or bodies', async () => {
  for (const [status, guidance] of [[400, 'recording'], [401, 'API key'], [403, 'permissions'], [404, 'model'], [429, 'credits'], [500, 'shortly']]) {
    await assert.rejects(transcribe(Buffer.from('synthetic'), 'audio/mp4', { apiKey: 'synthetic-private-key',
      transport: async () => new Response('private provider body', { status }) }),
    error => error.code === 'voice_unavailable' && error.message.includes(guidance)
      && !error.message.includes('private') && !error.message.includes('synthetic'));
  }
});

test('Groq transcription uses only its fixed endpoint, model and credential without falling back', async () => {
  let calls = 0;
  const text = await transcribe(Buffer.from('synthetic'), 'audio/mp4', { apiKey: 'synthetic-groq-key', provider: 'groq', transport: async (url, options) => {
    calls++; assert.equal(url, 'https://api.groq.com/openai/v1/audio/transcriptions');
    assert.equal(options.headers.Authorization, 'Bearer synthetic-groq-key');
    assert.equal(options.body.get('model'), 'whisper-large-v3-turbo');
    assert.equal(options.body.get('response_format'), 'json');
    assert.equal(options.redirect, 'error');
    return new Response(JSON.stringify({ text: 'Order one oat latte' }));
  } });
  assert.equal(text, 'Order one oat latte'); assert.equal(calls, 1);
  await assert.rejects(transcribe(Buffer.from('synthetic'), 'audio/mp4', { apiKey: 'synthetic-groq-key', provider: 'groq',
    transport: async () => { calls++; return new Response('private body', { status: 429 }); } }),
    error => error.message.includes('Groq') && error.message.includes('limit') && !error.message.includes('private'));
  assert.equal(calls, 2);
  await assert.rejects(transcribe(Buffer.from('synthetic'), 'audio/mp4', { provider: 'groq' }), error => error.message.includes('COSIGNA_GROQ_API_KEY'));
  await assert.rejects(transcribe(Buffer.from('synthetic'), 'audio/mp4', { provider: 'unknown', apiKey: 'synthetic-key',
    transport: async () => { throw new Error('must not contact an unknown provider'); } }), /Unsupported AI provider/);
});
