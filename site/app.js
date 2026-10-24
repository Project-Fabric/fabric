/* Cosigna landing: the lit strip, the stage slider, the panel row, and the motion layer.
   Everything works without this script; it only adds lighting, the slider and motion. */
(function () {
  const doc = document;
  doc.documentElement.classList.add('js');
  const reduced = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  /** Lights whichever child of `row` sits nearest its centre; calls `onLit` when it changes. */
  function lightCentre(row, items, onLit) {
    if (!row || !items.length) return;
    let current = null;
    const update = () => {
      const box = row.getBoundingClientRect(), mid = box.left + box.width / 2;
      let best = items[0], distance = Infinity;
      for (const item of items) {
        const r = item.getBoundingClientRect(), d = Math.abs(r.left + r.width / 2 - mid);
        if (d < distance) { distance = d; best = item; }
      }
      if (best !== current) {
        items.forEach(item => item.classList.toggle('lit', item === best));
        current = best;
        if (onLit) onLit(best);
      }
    };
    let pending = false;
    row.addEventListener('scroll', () => { if (!pending) { pending = true; requestAnimationFrame(() => { pending = false; update(); }); } }, { passive: true });
    window.addEventListener('resize', update);
    update();
  }
  function centre(row, item) {
    if (row && item) row.scrollLeft = item.offsetLeft - (row.clientWidth - item.offsetWidth) / 2;
  }

  // Hero: start on the prompt-injection record, light the centred record.
  const strip = doc.getElementById('strip'), caption = doc.getElementById('strip-caption');
  if (strip) {
    const ops = Array.from(strip.querySelectorAll('.op'));
    centre(strip, doc.getElementById('injection'));
    lightCentre(strip, ops, lit => { if (caption) caption.hidden = lit.id !== 'injection'; });
  }

  // How it works: one slider drives the clause and the fragment.
  const range = doc.getElementById('stage-range'), clause = doc.getElementById('clause');
  const stages = Array.from(doc.querySelectorAll('.stage')), names = Array.from(doc.querySelectorAll('.stage-names li'));
  function showStage(index) {
    stages.forEach((stage, i) => stage.classList.toggle('active', i === index));
    names.forEach((name, i) => name.classList.toggle('current', i === index));
    if (clause && stages[index]) clause.textContent = stages[index].dataset.clause;
    const active = stages[index];
    if (active && window.gsap && !reduced) window.gsap.fromTo(active, { autoAlpha: 0, y: 12 }, { autoAlpha: 1, y: 0, duration: 0.2, ease: 'power2.out' });
  }
  if (range && stages.length) {
    range.addEventListener('input', () => showStage(Number(range.value)));
    names.forEach((name, i) => { name.addEventListener('click', () => { range.value = String(i); showStage(i); }); });
    showStage(Number(range.value) || 0);
  }

  // Showcase: the centred panel is lit; its name and two lines follow it.
  const row = doc.getElementById('row'), panelName = doc.getElementById('panel-name'), panelCopy = doc.getElementById('panel-copy');
  if (row) {
    const panels = Array.from(row.querySelectorAll('.panel'));
    centre(row, row.querySelector('.panel.lit'));
    lightCentre(row, panels, lit => {
      if (panelName) panelName.textContent = lit.dataset.name || '';
      if (panelCopy) panelCopy.textContent = lit.dataset.text || '';
    });
  }

  // Motion layer: GSAP + ScrollTrigger, transform and opacity only. Without GSAP, or with
  // reduced motion, nothing is hidden and nothing moves.
  const gsap = window.gsap, ScrollTrigger = window.ScrollTrigger;
  if (!gsap || reduced) return;
  if (ScrollTrigger) gsap.registerPlugin(ScrollTrigger);

  // The signature move: the strip slides in from the right and settles on the lit record.
  gsap.from('.op', { x: 120, autoAlpha: 0, duration: 0.9, ease: 'power3.out', stagger: 0.05 });
  gsap.from(['.hero-copy h1', '.hero-copy p', '.hero .button', '.corner.tl', '.corner.tr', '.caption'],
    { y: 18, autoAlpha: 0, duration: 0.7, ease: 'power2.out', stagger: 0.07, delay: 0.35 });

  if (!ScrollTrigger) return;
  // One entrance per section: heading, then text, then the picture, once.
  const groups = [
    ['.proof h2', '.proof .statement', '.proof .note'],
    ['.how h2', '.how .clause', '.how-ui', '.how-control'],
    ['.showcase h2', '.showcase .row', '.panel-text'],
    ['.figures h2', '.figures > dl', '.figures-panel'],
    ['.cta p', '.cta .button'],
  ];
  for (const parts of groups) {
    const trigger = doc.querySelector(parts[0]);
    if (!trigger) continue;
    gsap.from(parts, { y: 20, autoAlpha: 0, duration: 0.45, ease: 'power2.out', stagger: 0.07,
      scrollTrigger: { trigger, start: 'top 85%', once: true } });
  }
  // One moving picture: the lit record drifts as the hero leaves.
  gsap.to('#strip', { y: -60, ease: 'none', scrollTrigger: { trigger: '.hero', start: 'top top', end: 'bottom top', scrub: true } });
})();
