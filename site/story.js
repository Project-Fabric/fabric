// View switches and scroll change illustrations only. No transaction authority or I/O.
(() => {
  const views = [...document.querySelectorAll('[data-preview-view]')];
  if (!views.length) return;
  const demo = document.querySelector('#purchase-demo');
  const record = document.querySelector('#example-record');
  function select(view, focus = false) {
    for (const button of views) {
      const selected = button.dataset.previewView === view;
      button.setAttribute('aria-selected', String(selected)); button.tabIndex = selected ? 0 : -1;
      document.getElementById(button.getAttribute('aria-controls')).hidden = !selected;
      if (selected && focus) button.focus();
    }
  }
  for (const [index, button] of views.entries()) {
    button.addEventListener('click', () => select(button.dataset.previewView));
    button.addEventListener('keydown', event => {
      if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? views.length - 1 : (index + 1) % views.length;
        select(views[next].dataset.previewView, true);
      }
    });
  }
  function updateRecord() {
    record.textContent = JSON.stringify({ illustrative: true, merchant: 'fixture-cafe',
      currency: 'USD', amount_minor: demo.dataset.scenario === 'over' ? 2800 : 700,
      per_order_limit_minor: 1000, state: demo.dataset.state,
      owner_approval: demo.dataset.state === 'confirmed' ? 'example_recorded' : demo.dataset.state === 'replay' ? 'assumed_prior_approval' : 'not_recorded',
      network_requests: 0 }, null, 2);
  }
  new MutationObserver(updateRecord).observe(demo, { attributes: true, attributeFilter: ['data-state', 'data-scenario'] });
  updateRecord();
})();

(() => {
  const story = document.querySelector('#authority-story');
  if (!story) return;
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  const chapters = [...story.querySelectorAll('[data-chapter]')];
  const plates = [...story.querySelectorAll('[data-layer]')];
  const mode = story.querySelector('[data-story-mode]');
  const titles = ['A request, with a purpose.', 'Boundaries, before action.', 'Your decision. The exact quote.', 'An outcome you can trace.'];
  const descriptions = [
    'The agent proposes a purchase. Its request carries intent, never payment authority.',
    'Code checks the merchant, currency, expiry, and spending limits outside the model.',
    'The owner reviews the actual terms. Only that exact approval can reserve the budget.',
    'A durable attempt precedes execution. An uncertain result stays held for reconciliation.'
  ];
  let stage = 0, manual = false, frame = 0;
  function select(index) {
    stage = Math.max(0, Math.min(3, index)); story.dataset.stage = String(stage);
    for (const [i, button] of chapters.entries()) button.setAttribute('aria-pressed', String(i === stage));
    for (const [i, plate] of plates.entries()) { plate.classList.toggle('is-current', i === stage); plate.classList.toggle('is-past', i < stage); }
    story.querySelector('#story-title').textContent = titles[stage];
    story.querySelector('#story-description').textContent = descriptions[stage];
    story.querySelector('#story-count').textContent = `0${stage + 1} / 04`;
  }
  function canScrub() { return !reduced.matches && !document.body.classList.contains('motion-paused') && window.innerWidth > 900 && window.innerHeight >= 650; }
  function update() {
    frame = 0;
    if (!canScrub() || manual || document.hidden) return;
    const rect = story.getBoundingClientRect();
    const travel = Math.max(1, rect.height - window.innerHeight);
    const progress = Math.max(0, Math.min(1, (88 - rect.top) / travel));
    select(Math.min(3, Math.floor(progress * 4)));
  }
  function schedule() { if (!frame && canScrub() && !manual && !document.hidden) frame = requestAnimationFrame(update); }
  function syncMode() {
    const capable = canScrub(); story.classList.toggle('is-scrubbing', capable && !manual);
    mode.hidden = !capable; mode.textContent = manual ? 'Follow the scroll ↓' : 'Use chapter controls ↗';
    mode.setAttribute('aria-pressed', String(manual));
    cancelAnimationFrame(frame); frame = 0; if (capable && !manual) schedule();
  }
  for (const button of chapters) button.addEventListener('click', () => { manual = true; select(Number(button.dataset.chapter)); syncMode(); });
  mode.addEventListener('click', () => { manual = !manual; syncMode(); });
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', syncMode);
  reduced.addEventListener('change', syncMode);
  document.addEventListener('visibilitychange', syncMode);
  new MutationObserver(syncMode).observe(document.body, { attributes: true, attributeFilter: ['class'] });
  select(0); syncMode();
})();
