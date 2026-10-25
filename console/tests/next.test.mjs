import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';

const site = new URL('../../site/', import.meta.url);
const read = path => readFileSync(new URL(path, site), 'utf8');
const html = read('next/next.html');
const server = read('server.mjs');
const { blake3 } = await import(new URL('next/blake3.js', site).href);
const { createGateway, NORTHWIND_REQUESTS } = await import(new URL('next/engine.js', site).href);

test('the page says what Cosigna is, with one action and a heading per section', () => {
  assert.equal((html.match(/<h1[\s>]/g) || []).length, 1);
  assert.match(html, /<h1>Your agents’ <em>co-signer<\/em><br>for every payment<\/h1>/);
  for (const id of ['product', 'cases', 'try', 'evidence', 'faq']) assert.match(html, new RegExp(`id="${id}"`), `section #${id}`);
  // Every call to action is the same one: book a demo.
  const ctas = [...html.matchAll(/<a class="btn[^"]*" href="([^"]+)"[^>]*>([^<]+)/g)].filter(m => m[1].startsWith('mailto:'));
  assert.ok(ctas.length >= 4);
  for (const [, href, text] of ctas) { assert.match(href, /^mailto:hello@cosigna\.ai/); assert.match(text, /Book a demo|Get started/); }
  // No invented social proof.
  assert.doesNotMatch(html, /testimonial|trusted by|customers love|\$\d+M raised/i);
});

test('the page fits the site security policy', () => {
  assert.doesNotMatch(html, /\sstyle=/, 'inline style attributes are blocked by the CSP');
  assert.doesNotMatch(html, /<script(?![^>]*\ssrc=)[^>]*>/, 'inline scripts are blocked by the CSP');
  assert.doesNotMatch(html, /(src|href)="https?:\/\//, 'every asset is self-hosted');
  // Every local asset the page references has a route.
  for (const [, path] of html.matchAll(/(?:src|href)="(\/[^"#?]+)"/g)) assert.match(server, new RegExp(`\\['${path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}'`), `route for ${path}`);
  for (const file of ['next.js', 'site.js', 'demo.js', 'engine.js']) assert.doesNotMatch(read('next/' + file), /style="/, `${file} writes no inline style attributes`);
});

test('BLAKE3 in the page matches the reference implementation', () => {
  const vectors = {
    0: 'af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262',
    1: '2d3adedff11b61f14c886e35afa036736dcd87a74d27b5c1510225d0f592e213',
    64: '4eed7141ea4a5cd4b788606bd23f46e212af9cacebacdc7d1f4c6dc7f2511b98',
    1024: '42214739f095a406f3fc83deb889744ac00df831c10daa55189b5d121c855af7',
    1025: 'd00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444',
    3073: '7124b49501012f81cc7f11ca069ec9226cecb8a2c850cfe644e327d22d3e1cd3',
  };
  for (const [n, hex] of Object.entries(vectors)) assert.equal(blake3(Uint8Array.from({ length: +n }, (_, i) => i % 251)), hex, `length ${n}`);
});

test('the gateway model reproduces the real Northwind decisions and catches tampering', () => {
  const gw = createGateway();
  const reasons = NORTHWIND_REQUESTS.map(r => gw.request(r).decision.reason);
  assert.deepEqual(reasons, ['within_agent_auto_limit', 'within_agent_auto_limit', 'within_agent_auto_limit', 'budget_exhausted', 'over_per_operation_limit',
    'not_granted', 'within_agent_auto_limit', 'exceeds_refundable', 'within_agent_auto_limit', 'source_not_found']);
  assert.equal(gw.budget.spent_minor, 18298);
  // The hero's held example: a fresh ledger holds $450 until the exact digest is signed.
  const hero = createGateway(), op = hero.request(NORTHWIND_REQUESTS[3]);
  assert.equal(op.stage, 'held');
  assert.throws(() => hero.approve(op.id, '0'.repeat(64)));
  hero.approve(op.id, op.proposal.digest);
  assert.equal(op.stage, 'succeeded');
  assert.equal(gw.verify().ok, true);
  const target = gw.records.find(r => r.body.includes('"amount_minor":2499')).seq;
  gw.tamper(target, b => b.replace('"amount_minor":2499', '"amount_minor":249'));
  assert.deepEqual(gw.verify().problems.map(p => p.seq), [target]);
});
