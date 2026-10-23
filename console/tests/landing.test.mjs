import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

function landing(withCanvas = false) {
  const dom = new JSDOM(readFileSync(new URL('../../site/index.html', import.meta.url), 'utf8'), { runScripts: 'outside-only', url: 'http://127.0.0.1:3091', pretendToBeVisual: true });
  Object.defineProperty(dom.window, 'matchMedia', { value: () => ({ matches: true, addEventListener() {} }) });
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
  return { dom, frames, draws: () => draws };
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
