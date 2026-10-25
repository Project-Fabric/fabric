// BLAKE3 (hash mode, 32-byte output), after the reference implementation in the BLAKE3 spec.
// Small and dependency-free so the demo's evidence chain uses the same hash as the gateway.
// Checked against the Python `blake3` package by shared/blake3.test.mjs.
const IV = new Uint32Array([0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
const PERM = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];
const CHUNK_START = 1, CHUNK_END = 2, PARENT = 4, ROOT = 8;
const BLOCK = 64, CHUNK = 1024;

const rotr = (x, n) => (x >>> n) | (x << (32 - n));

function g(s, a, b, c, d, x, y) {
  s[a] = (s[a] + s[b] + x) | 0; s[d] = rotr(s[d] ^ s[a], 16);
  s[c] = (s[c] + s[d]) | 0; s[b] = rotr(s[b] ^ s[c], 12);
  s[a] = (s[a] + s[b] + y) | 0; s[d] = rotr(s[d] ^ s[a], 8);
  s[c] = (s[c] + s[d]) | 0; s[b] = rotr(s[b] ^ s[c], 7);
}

/** One compression; returns all 16 output words. */
function compress(cv, block, counter, len, flags) {
  const s = new Uint32Array(16);
  s.set(cv, 0); s.set(IV.subarray(0, 4), 8);
  s[12] = counter >>> 0; s[13] = Math.floor(counter / 0x100000000) >>> 0; s[14] = len; s[15] = flags;
  let m = Uint32Array.from(block);
  for (let r = 0; r < 7; r++) {
    g(s, 0, 4, 8, 12, m[0], m[1]); g(s, 1, 5, 9, 13, m[2], m[3]);
    g(s, 2, 6, 10, 14, m[4], m[5]); g(s, 3, 7, 11, 15, m[6], m[7]);
    g(s, 0, 5, 10, 15, m[8], m[9]); g(s, 1, 6, 11, 12, m[10], m[11]);
    g(s, 2, 7, 8, 13, m[12], m[13]); g(s, 3, 4, 9, 14, m[14], m[15]);
    if (r < 6) m = Uint32Array.from(PERM, i => m[i]);
  }
  for (let i = 0; i < 8; i++) { s[i] ^= s[i + 8]; s[i + 8] ^= cv[i]; }
  return s;
}

function words(bytes, start, len) {
  const w = new Uint32Array(16);
  for (let i = 0; i < len; i++) w[i >> 2] |= bytes[start + i] << (8 * (i & 3));
  return w;
}

/** The pending output of a chunk or parent: compress it with or without ROOT. */
function chunkOutput(bytes, start, end, counter) {
  let cv = IV, at = start;
  const blocks = Math.max(1, Math.ceil((end - start) / BLOCK));
  for (let b = 0; b < blocks - 1; b++, at += BLOCK)
    cv = compress(cv, words(bytes, at, BLOCK), counter, BLOCK, b === 0 ? CHUNK_START : 0).subarray(0, 8);
  const len = end - at;
  return { cv, block: words(bytes, at, len), counter, len, flags: (blocks === 1 ? CHUNK_START : 0) | CHUNK_END };
}
const parentOutput = (left, right) => {
  const block = new Uint32Array(16); block.set(left, 0); block.set(right, 8);
  return { cv: IV, block, counter: 0, len: BLOCK, flags: PARENT };
};
const chain = o => compress(o.cv, o.block, o.counter, o.len, o.flags).subarray(0, 8);

/** BLAKE3 of `input` (string as UTF-8, or bytes) as 64 lowercase hex characters. */
export function blake3(input) {
  const bytes = typeof input === 'string' ? new TextEncoder().encode(input) : input;
  const chunks = Math.max(1, Math.ceil(bytes.length / CHUNK));
  const stack = [];
  for (let c = 0; c < chunks - 1; c++) {
    let cv = chain(chunkOutput(bytes, c * CHUNK, (c + 1) * CHUNK, c));
    for (let total = c + 1; (total & 1) === 0; total >>= 1) cv = chain(parentOutput(stack.pop(), cv));
    stack.push(cv);
  }
  let out = chunkOutput(bytes, (chunks - 1) * CHUNK, bytes.length, chunks - 1);
  while (stack.length) out = parentOutput(stack.pop(), chain(out));
  const root = compress(out.cv, out.block, out.counter, out.len, out.flags | ROOT).subarray(0, 8);
  let hex = '';
  for (const w of root) for (let i = 0; i < 4; i++) hex += ((w >>> (8 * i)) & 255).toString(16).padStart(2, '0');
  return hex;
}
