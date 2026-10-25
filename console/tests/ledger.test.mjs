import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

const html = readFileSync(new URL('../../site/ledger.html', import.meta.url), 'utf8');
const css = readFileSync(new URL('../../site/ledger.css', import.meta.url), 'utf8');

/** The landing page with app.js run. GSAP is not loaded here, which is also the no-motion path. */
function landing({ reduced = false } = {}) {
  const dom = new JSDOM(html, { runScripts: 'outside-only', url: 'http://127.0.0.1:3091', pretendToBeVisual: true });
  Object.defineProperty(dom.window, 'matchMedia', { value: query => ({ matches: reduced && query.includes('reduce'), addEventListener() {} }) });
  dom.window.eval(readFileSync(new URL('../../site/ledger.js', import.meta.url), 'utf8'));
  return dom;
}

test('the first screen says what Cosigna is, with one primary action and a heading on every section', () => {
  const dom = landing(), doc = dom.window.document;
  try {
    const h1s = doc.querySelectorAll('h1');
    assert.equal(h1s.length, 1);
    assert.match(h1s[0].textContent, /AI agents that move money/);
    const demo = doc.querySelector('.hero .button');
    assert.match(demo.textContent, /Book a demo/);
    assert.match(demo.getAttribute('href'), /^mailto:/);
    for (const section of doc.querySelectorAll('main section'))
      assert.ok(section.querySelector('h2'), `section ${section.className} needs a heading`);
    // The proof uses the real experiment numbers, not invented ones.
    assert.match(doc.querySelector('.statement').textContent, /\$1,168, \$1,168 and \$528/);
  } finally { dom.window.close(); }
});

test('the stage slider swaps the clause and the fragment, by pointer or keyboard', () => {
  const dom = landing(), doc = dom.window.document;
  try {
    const range = doc.getElementById('stage-range'), stages = [...doc.querySelectorAll('.stage')];
    assert.equal(doc.getElementById('clause').textContent, 'First, an agent asks.');
    assert.deepEqual(stages.map(s => s.classList.contains('active')), [true, false, false, false]);
    range.value = '2'; range.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    assert.equal(doc.getElementById('clause').textContent, 'Then your rules decide.');
    assert.deepEqual(stages.map(s => s.classList.contains('active')), [false, false, true, false]);
    assert.equal(doc.querySelector('.stage-names li.current').textContent, 'Decide');
    doc.querySelectorAll('.stage-names li')[3].click();
    assert.equal(range.value, '3');
    assert.equal(doc.getElementById('clause').textContent, 'Finally, a person signs.');
    assert.equal(range.type, 'range', 'a native range input keeps arrow-key control');
  } finally { dom.window.close(); }
});

test('without the motion library, or with reduced motion, nothing is hidden', () => {
  for (const reduced of [false, true]) {
    const dom = landing({ reduced }), doc = dom.window.document;
    try {
      assert.ok(doc.documentElement.classList.contains('js'));
      const hidden = [...doc.querySelectorAll('[style]')].filter(el => /opacity:\s*0|visibility:\s*hidden/.test(el.getAttribute('style')));
      assert.deepEqual(hidden, [], 'no element may be left invisible');
      assert.equal(doc.querySelectorAll('.strip .op.lit').length, 1, 'exactly one record is lit');
      assert.equal(doc.querySelectorAll('.row .panel.lit').length, 1, 'exactly one panel is lit');
      assert.ok(doc.getElementById('panel-name').textContent.length > 0);
    } finally { dom.window.close(); }
  }
});

test('the page fits the site security policy and honours reduced motion', () => {
  assert.doesNotMatch(html, /\sstyle=/, 'inline style attributes are blocked by the CSP');
  assert.doesNotMatch(html, /<script(?![^>]*\ssrc=)[^>]*>/, 'inline scripts are blocked by the CSP');
  assert.doesNotMatch(html, /https?:\/\/(?!127\.0\.0\.1)[^"']*\.(js|css|woff2?)/, 'every asset is self-hosted');
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  for (const src of html.matchAll(/<script src="([^"]+)"/g)) assert.match(src[1], /^\/(ledger\.js|assets\/vendor\/)/);
});
