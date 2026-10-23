import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

function landing(withCanvas = false, options = {}) {
  const dom = new JSDOM(readFileSync(new URL(`../../site/${options.page || 'index'}.html`, import.meta.url), 'utf8'), { runScripts: 'outside-only', url: 'http://127.0.0.1:3091', pretendToBeVisual: true });
  Object.defineProperty(dom.window, 'matchMedia', { value: () => ({ matches: options.reduced ?? true, addEventListener() {} }) });
  let requests = 0, timerId = 0;
  dom.window.fetch = () => { requests++; throw new Error('The landing illustration must not submit requests.'); };
  const timers = new Map();
  if (options.captureTimers) {
    dom.window.setTimeout = callback => { timers.set(++timerId, callback); return timerId; };
    dom.window.clearTimeout = id => timers.delete(id);
  }
  const frames = new Map(); let id = 0, draws = 0;
  const finite = (...values) => values.forEach(value => assert.ok(Number.isFinite(value), 'Canvas coordinates must be finite.'));
  const context = { clearRect() { draws++; }, setTransform: finite, beginPath() {}, moveTo: finite, lineTo: finite, stroke() {}, arc: finite, fill() {} };
  Object.defineProperty(dom.window.HTMLCanvasElement.prototype, 'getContext', { value: () => withCanvas ? context : null });
  Object.defineProperty(dom.window.HTMLCanvasElement.prototype, 'getBoundingClientRect', { value: () => ({ width: 800, height: 600 }) });
  dom.window.requestAnimationFrame = callback => { frames.set(++id, callback); return id; };
  dom.window.cancelAnimationFrame = frame => frames.delete(frame);
  Object.defineProperty(dom.window.HTMLDialogElement.prototype, 'showModal', { value: function () { this.setAttribute('open', ''); } });
  Object.defineProperty(dom.window.HTMLDialogElement.prototype, 'close', { value: function () { this.removeAttribute('open'); } });
  dom.window.eval(readFileSync(new URL('../../site/app.js', import.meta.url), 'utf8'));
  return { dom, frames, timers, requests: () => requests, draws: () => draws };
}

test('landing workflow supports mouse and keyboard selection without executing a transaction', () => {
  const { dom } = landing(), doc = dom.window.document;
  try {
    const collections = doc.querySelector('#tab-collections'); collections.click();
    assert.match(doc.querySelector('#workflow-title').textContent, /follow-up/);
    assert.equal(collections.getAttribute('aria-selected'), 'true');
    collections.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    assert.equal(doc.querySelector('#tab-reconciliation').getAttribute('aria-selected'), 'true');
    assert.match(doc.querySelector('#workflow-note').textContent, /no accounting or payment/);
  } finally { dom.window.close(); }
});

test('landing provides dashboard guidance with an accessible closeable dialog', () => {
  const { dom } = landing(), doc = dom.window.document;
  try {
    doc.querySelector('[data-open-dashboard]').click();
    assert.equal(doc.querySelector('#dashboard-dialog').hasAttribute('open'), true);
    assert.match(doc.querySelector('#dashboard-command').textContent, /npm run dev/);
    doc.querySelector('.dialog-close').click();
    assert.equal(doc.querySelector('#dashboard-dialog').hasAttribute('open'), false);
  } finally { dom.window.close(); }
});

test('wireframe starts still for reduced motion and supports an explicit resume and pause', () => {
  const preview = landing(true), { dom, frames } = preview;
  try {
    const toggle = dom.window.document.querySelector('#motion-toggle');
    assert.equal(toggle.getAttribute('aria-pressed'), 'true'); assert.equal(frames.size, 0);
    const before = preview.draws(); toggle.click(); assert.equal(frames.size, 1);
    const [id, callback] = frames.entries().next().value; frames.delete(id); callback(1000);
    assert.ok(preview.draws() > before); assert.equal(frames.size, 1);
    toggle.click(); assert.equal(toggle.getAttribute('aria-pressed'), 'true'); assert.equal(frames.size, 0);
  } finally { dom.window.close(); }
});

test('purchase illustration requires an explicit action and never submits a request', () => {
  const p = landing(), doc = p.dom.window.document;
  try {
    const card = doc.querySelector('#purchase-demo'), approve = doc.querySelector('#demo-approve');
    assert.equal(card.dataset.state, 'waiting');
    assert.match(doc.querySelector('#demo-detail').textContent, /budget alone/);
    approve.click();
    assert.equal(card.dataset.state, 'confirmed');
    assert.equal(approve.disabled, true);
    assert.match(doc.querySelector('#demo-footnote').textContent, /No order was submitted/);
    doc.querySelector('[data-scenario="over"]').click();
    assert.equal(card.dataset.state, 'blocked'); assert.equal(approve.disabled, true);
    assert.equal(doc.querySelector('#demo-amount').textContent, '$28.00');
    approve.click(); assert.equal(card.dataset.state, 'blocked');
    doc.querySelector('[data-scenario="replay"]').click();
    assert.equal(card.dataset.state, 'replay'); assert.equal(approve.disabled, true);
    assert.match(doc.querySelector('#demo-detail').textContent, /prior approved purchase/);
    assert.equal(p.requests(), 0);
  } finally { p.dom.window.close(); }
});

test('a stale illustrative confirmation cannot complete a newly selected scenario', () => {
  const p = landing(false, { reduced: false, captureTimers: true }), doc = p.dom.window.document;
  try {
    const approve = doc.querySelector('#demo-approve'), card = doc.querySelector('#purchase-demo');
    approve.click(); const oldCompletion = [...p.timers.values()][0];
    doc.querySelector('[data-scenario="over"]').click();
    assert.equal(p.timers.size, 0);
    doc.querySelector('[data-scenario="within"]').click(); approve.click();
    oldCompletion(); assert.equal(card.dataset.state, 'pending');
    [...p.timers.values()][0](); assert.equal(card.dataset.state, 'confirmed');
    assert.equal(p.requests(), 0);
  } finally { p.dom.window.close(); }
});

test('navigation opens, closes on Escape, and restores focus', () => {
  const { dom } = landing(), doc = dom.window.document;
  try {
    const toggle = doc.querySelector('.menu-toggle'), nav = doc.querySelector('#primary-navigation');
    toggle.click(); assert.equal(toggle.getAttribute('aria-expanded'), 'true'); assert.ok(nav.classList.contains('is-open'));
    doc.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    assert.equal(toggle.getAttribute('aria-expanded'), 'false'); assert.equal(doc.activeElement, toggle);
    toggle.click(); nav.querySelector('a').click(); assert.equal(toggle.getAttribute('aria-expanded'), 'false');
  } finally { dom.window.close(); }
});

test('About, Architecture and Benchmarks use the shared navigation without a hero canvas', () => {
  for (const page of ['about', 'architecture', 'benchmarks']) {
    const { dom } = landing(false, { page }), doc = dom.window.document;
    try {
      assert.equal(doc.querySelectorAll('h1').length, 1);
      assert.equal(doc.querySelector('[aria-current="page"]').getAttribute('href'), `/${page}`);
      assert.equal(doc.querySelector('#authority-field'), null);
      assert.doesNotMatch(doc.body.textContent, /\bFabric\b|\bAgentAuth\b/);
      doc.querySelector('[data-open-dashboard]').click(); assert.ok(doc.querySelector('#dashboard-dialog').hasAttribute('open'));
    } finally { dom.window.close(); }
  }
});
