const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const tabs = [...document.querySelectorAll('[data-workflow]')];
const panel = document.querySelector('#workflow-panel');
const workflows = {
  refunds: { role: 'REFUNDS AGENT', title: 'A refund that needs a second look.', description: 'A $450 request reaches an agent with a $100 per-action boundary.',
    steps: [['Read the transaction', 'Find the original payment and the customer’s request.'], ['Check the authority', 'The amount is above the agent’s delegated limit.'], ['Ask a person to decide', 'Review the reason and the exact amount before authorizing a retry.']],
    outcome: 'NEEDS REVIEW', note: 'This example does not execute a refund.' },
  collections: { role: 'COLLECTIONS ASSISTANT', title: 'A thoughtful follow-up, ready for review.', description: 'Two overdue invoices become two proposed follow-up drafts.',
    steps: [['Read the invoice context', 'Gather the balance, due date and recorded customer context.'], ['Prepare the follow-up', 'Draft a reminder for your team to review.'], ['Keep the person in the loop', 'The draft waits for a person; the agent sends no messages.']],
    outcome: 'DRAFT ONLY', note: 'This example sends no emails or customer messages.' },
  reconciliation: { role: 'RECONCILIATION AGENT', title: 'Find the difference before it becomes a distraction.', description: 'A sample payout differs from the expected transaction total by $25.',
    steps: [['Compare the records', 'Read the transaction and payout data.'], ['Flag the difference', 'Surface the $25 discrepancy with the relevant context.'], ['Prepare a review', 'A person investigates; no balancing entry is posted.']],
    outcome: 'FLAGGED FOR REVIEW', note: 'This example changes no accounting or payment records.' },
};
const tabLayout = window.matchMedia('(max-width: 60rem)');
function orientTabs() { document.querySelector('[role=tablist]').setAttribute('aria-orientation', tabLayout.matches ? 'horizontal' : 'vertical'); }
orientTabs(); tabLayout.addEventListener('change', orientTabs);
function selectWorkflow(id, focus = false) {
  const workflow = workflows[id]; if (!workflow) return;
  for (const tab of tabs) { const selected = tab.dataset.workflow === id; tab.setAttribute('aria-selected', String(selected)); tab.tabIndex = selected ? 0 : -1; if (focus && selected) tab.focus({ preventScroll: true }); }
  panel.setAttribute('aria-labelledby', `tab-${id}`);
  for (const [key, value] of [['role', workflow.role], ['title', workflow.title], ['description', workflow.description], ['outcome', workflow.outcome], ['note', workflow.note]]) document.querySelector(`#workflow-${key}`).textContent = value;
  for (const [i, name] of ['one', 'two', 'three'].entries()) { document.querySelector(`#step-${name}-title`).textContent = workflow.steps[i][0]; document.querySelector(`#step-${name}-body`).textContent = workflow.steps[i][1]; }
}
for (const [index, tab] of tabs.entries()) {
  tab.addEventListener('click', () => selectWorkflow(tab.dataset.workflow));
  tab.addEventListener('keydown', event => {
    const offsets = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    if (event.key in offsets) { event.preventDefault(); selectWorkflow(tabs[(index + offsets[event.key] + tabs.length) % tabs.length].dataset.workflow, true); }
    if (event.key === 'Home' || event.key === 'End') { event.preventDefault(); selectWorkflow(tabs[event.key === 'Home' ? 0 : tabs.length - 1].dataset.workflow, true); }
  });
}
for (const button of document.querySelectorAll('[data-show-workflow]')) button.addEventListener('click', () => { selectWorkflow(button.dataset.showWorkflow, true); document.querySelector('#workspace').scrollIntoView({ behavior: reducedMotion.matches ? 'instant' : 'smooth' }); });

const dialog = document.querySelector('#dashboard-dialog');
for (const button of document.querySelectorAll('[data-open-dashboard]')) button.addEventListener('click', () => dialog.showModal());
dialog.querySelector('.dialog-close').addEventListener('click', () => dialog.close());
dialog.addEventListener('click', event => { if (event.target === dialog) { const rect = dialog.getBoundingClientRect(); if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close(); } });
document.querySelector('#copy-command').addEventListener('click', async () => {
  try { await navigator.clipboard.writeText(document.querySelector('#dashboard-command').textContent); document.querySelector('#copy-status').textContent = 'Commands copied. Run them in your terminal.'; }
  catch { document.querySelector('#copy-status').textContent = 'Select and copy the commands above.'; }
});

if (!reducedMotion.matches && 'IntersectionObserver' in window) {
  // Content remains visible; no reveal depends on script execution.
  const observer = new IntersectionObserver(entries => { for (const entry of entries) if (entry.isIntersecting) { entry.target.classList.add('is-visible'); observer.unobserve(entry.target); } }, { threshold: 0.07 });
  for (const element of document.querySelectorAll('.reveal')) observer.observe(element);
}

// The gate: agents send payment requests along lanes to a wireframe gate.
// Inside the limit they pass to the ledger; above it they wait in clay until a
// person approves (a tick) or declines (a cross). Every landing adds a sheet.
const gateScene = (() => {
  const TAU = Math.PI * 2;
  const R = 1.48, r = .13, AGENTS = [[-2.6, -1.5, -.35], [-2.75, 0, .25], [-2.6, 1.5, -.1]];
  const NAMES = ['Refunds', 'Collections', 'Reconciliation'];
  const LEDGER = [2.2, -.55, 0], SHEETS = 9, GAP = .15, SHEET = .58;
  const EVERY = .85, TRAVEL = 2.1, CHECK = .35, REVIEW = 2.4, DECLINE = 1.7, FALL = .9, OUT = 1.4;
  const hash = n => { const s = Math.sin(n * 127.1 + 311.7) * 43758.5453; return s - Math.floor(s); };
  const ease = t => t < .5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
  const mix = (a, b, k) => a.map((v, i) => v + (b[i] - v) * k);
  const quad = (a, c, b, s) => { const q = 1 - s; return a.map((v, i) => q * q * v + 2 * q * s * c[i] + s * s * b[i]); };
  const slot = i => [0, (i - 1) * .42, 0];
  const lanePoint = (i, s) => { const a = AGENTS[i]; return quad(a, [-1.25, a[1] * .4, a[2] * .3], slot(i), s); };
  const outPoint = (s, i = 1) => quad(slot(i), [1.15, -.5, 0], [LEDGER[0], LEDGER[1] - .02, LEDGER[2]], s);
  const torus = [];
  for (let m = 0; m < 16; m++) { const v = m / 16 * TAU, p = []; for (let s = 0; s <= 80; s++) { const u = s / 80 * TAU; p.push([r * Math.sin(v), (R + r * Math.cos(v)) * Math.cos(u), (R + r * Math.cos(v)) * Math.sin(u)]); } torus.push(p); }
  for (let s = 0; s < 44; s++) { const u = s / 44 * TAU, p = []; for (let m = 0; m <= 18; m++) { const v = m / 18 * TAU; p.push([r * Math.sin(v), (R + r * Math.cos(v)) * Math.cos(u), (R + r * Math.cos(v)) * Math.sin(u)]); } torus.push(p); }
  const rim = []; for (let s = 0; s <= 64; s++) { const u = s / 64 * TAU; rim.push([0, (R - r * 1.4) * Math.cos(u), (R - r * 1.4) * Math.sin(u)]); }
  const kindOf = n => { const h = hash(n); return h < .6 ? 'pass' : h < .88 ? 'held' : 'declined'; };
  const laneOf = n => Math.floor(hash(n + 7.3) * 3);
  const atGate = n => (kindOf(n) === 'pass' ? CHECK : kindOf(n) === 'held' ? REVIEW : DECLINE);
  const landsAt = n => n * EVERY + TRAVEL + atGate(n) + OUT;
  // Where request n is at time t (seconds since its birth), or null when gone.
  function state(n, age) {
    const kind = kindOf(n), lane = laneOf(n), wait = atGate(n);
    if (age < 0) return null;
    if (age < TRAVEL) return { p: lanePoint(lane, ease(age / TRAVEL)), phase: 'travel', kind };
    if (age < TRAVEL + wait) return { p: slot(lane), phase: 'gate', kind, k: (age - TRAVEL) / wait };
    if (kind === 'declined') { const k = (age - TRAVEL - wait) / FALL; return k < 1 ? { p: [0, slot(lane)[1] + k * k * 1.4, 0], phase: 'fall', kind, k } : null; }
    const k = (age - TRAVEL - wait) / OUT;
    return k < 1 ? { p: outPoint(ease(k), lane), phase: 'out', kind, k } : null;
  }
  const poly = (ctx, pts) => { ctx.beginPath(); pts.forEach((p, i) => i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])); ctx.stroke(); };
  const dash = (ctx, pattern, offset) => { if (ctx.setLineDash) { ctx.setLineDash(pattern); ctx.lineDashOffset = offset; } };
  const disc = (ctx, p, rad, color, alpha) => { ctx.globalAlpha = alpha; ctx.fillStyle = color; ctx.beginPath(); ctx.arc(p[0], p[1], rad, 0, TAU); ctx.fill(); };
  const ring = (ctx, p, rad, color, alpha, from = 0, to = TAU) => { ctx.globalAlpha = alpha; ctx.strokeStyle = color; ctx.beginPath(); ctx.arc(p[0], p[1], rad, from, to); ctx.stroke(); };

  return { still: 9.4, draw(ctx, w, h, t, c) {
    const scale = Math.min(w / (w < 600 ? 6.2 : 5.15), h / 4.3), yaw = -.42 + Math.sin(t * .11) * .07, pitch = .27 + Math.sin(t * .08) * .03;
    const cY = Math.cos(yaw), sY = Math.sin(yaw), cP = Math.cos(pitch), sP = Math.sin(pitch), cx = w * (w < 600 ? .49 : .475), cy = h * .46;
    const cam = ([x, y, z]) => { const X = x * cY - z * sY; let Z = x * sY + z * cY; const Y = y * cP - Z * sP; Z = y * sP + Z * cP; const k = 8 / (8 + Z); return [cx + X * scale * k, cy + Y * scale * k, Z]; };
    ctx.lineWidth = .6; ctx.lineCap = 'round';

    // Floor grid, fading toward the edges.
    for (let x = -3.2; x <= 3.21; x += .4) { const a = cam([x, 2.2, -1.6]), b = cam([x, 2.2, 1.6]); ctx.strokeStyle = c.paper; ctx.globalAlpha = .07 * (1 - Math.abs(x) / 3.4); poly(ctx, [a, b]); }
    for (let z = -1.6; z <= 1.61; z += .4) { ctx.globalAlpha = .05; const pts = []; for (let x = -3.2; x <= 3.21; x += .4) pts.push(cam([x, 2.2, z])); poly(ctx, pts); }

    // Lanes in and out, dashes flowing toward the ledger.
    ctx.strokeStyle = c.paper; dash(ctx, [2, 6], -t * 16);
    AGENTS.forEach((_, i) => { const pts = []; for (let s = 0; s <= 40; s++) pts.push(cam(lanePoint(i, s / 40))); ctx.globalAlpha = .26; poly(ctx, pts); });
    for (let i = 0; i < 3; i++) { const pts = []; for (let s = 0; s <= 40; s++) pts.push(cam(outPoint(s / 40, i))); ctx.globalAlpha = i === 1 ? .26 : .12; poly(ctx, pts); }
    dash(ctx, [], 0);

    // Requests alive now.
    const live = [], first = Math.floor((t - 9) / EVERY), last = Math.floor(t / EVERY);
    for (let n = Math.max(first, 0); n <= last; n++) { const s = state(n, t - n * EVERY); if (s) live.push([n, s]); }
    const atGateNow = live.filter(([, s]) => s.phase === 'gate');

    // Gate, back half first so requests pass through it.
    const gateGlow = atGateNow.some(([, s]) => s.kind !== 'pass') ? c.clay : atGateNow.length ? c.paper : null;
    const half = front => { for (const path of torus) { const q = path.map(cam); for (let i = 1; i < q.length; i++) { const z = (q[i][2] + q[i - 1][2]) / 2; if ((z < 0) !== front) continue; ctx.globalAlpha = front ? .3 : .11; ctx.strokeStyle = c.paper; ctx.beginPath(); ctx.moveTo(q[i - 1][0], q[i - 1][1]); ctx.lineTo(q[i][0], q[i][1]); ctx.stroke(); } } };
    half(false);
    if (gateGlow) { ctx.lineWidth = 1.4; ctx.strokeStyle = gateGlow; ctx.globalAlpha = .55; poly(ctx, rim.map(cam)); ctx.lineWidth = .6; }

    // Ledger: sheets slide down as each request lands.
    let lastLanding = -Infinity; for (let n = last; n >= Math.max(first - 4, 0); n--) { if (kindOf(n) !== 'declined') { const at = landsAt(n); if (at <= t && at > lastLanding) lastLanding = at; } }
    const slide = Math.min(1, (t - lastLanding) / .7), shift = ease(slide);
    for (let i = 0; i <= SHEETS; i++) {
      const y = LEDGER[1] + (i - 1 + shift) * GAP, alpha = i === 0 ? shift : i === SHEETS ? 1 - shift : 1;
      const corners = [[-SHEET, -SHEET], [SHEET, -SHEET], [SHEET, SHEET], [-SHEET, SHEET], [-SHEET, -SHEET]].map(([dx, dz]) => cam([LEDGER[0] + dx, y, LEDGER[2] + dz * .8]));
      ctx.strokeStyle = c.paper; ctx.globalAlpha = alpha * (i <= 1 ? .5 : .32 - i * .025); poly(ctx, corners);
    }
    if (slide < 1) { const top = cam([LEDGER[0], LEDGER[1] - GAP + shift * GAP, 0]); ring(ctx, top, 6 + slide * 34, c.paper, .35 * (1 - slide)); }

    // Agents: rotating octahedra with their names.
    AGENTS.forEach((a, i) => {
      const spin = t * .5 + i * 1.7, s = .19;
      const v = [[s, 0, 0], [-s, 0, 0], [0, s * 1.2, 0], [0, -s * 1.2, 0], [0, 0, s], [0, 0, -s]].map(([x, y, z]) => cam([a[0] + x * Math.cos(spin) - z * Math.sin(spin), a[1] + y, a[2] + x * Math.sin(spin) + z * Math.cos(spin)]));
      ctx.strokeStyle = c.clay; ctx.globalAlpha = .7;
      for (const [p, q] of [[0, 2], [0, 3], [0, 4], [0, 5], [1, 2], [1, 3], [1, 4], [1, 5], [2, 4], [4, 3], [3, 5], [5, 2]]) poly(ctx, [v[p], v[q]]);
      const at = cam(a); disc(ctx, at, 1.6, c.clay, .9);
      if (ctx.fillText) { ctx.globalAlpha = .62; ctx.fillStyle = c.paper; ctx.font = `12px ${c.body}`; ctx.fillText(NAMES[i], at[0] + 24, at[1] + 4); }
    });

    // Requests, with short trails, and what happens to them at the gate.
    for (const [n, s] of live) {
      const born = n * EVERY, trail = [];
      for (let k = 0; k < 7; k++) { const q = state(n, t - born - k * .035); if (!q || q.phase !== s.phase) break; trail.push(cam(q.p)); }
      const here = cam(s.p);
      if (s.phase === 'gate' && s.kind !== 'pass') {
        const pulse = .5 + .5 * Math.sin(t * 5);
        ring(ctx, here, 11 + pulse * 3, c.clay, .25 + pulse * .15);
        ctx.lineWidth = 1.4; ring(ctx, here, 8, c.clay, .9, -Math.PI / 2, -Math.PI / 2 + TAU * Math.min(1, s.k / .8)); ctx.lineWidth = .6;
        disc(ctx, here, 3.2, c.clay, 1);
        if (s.k > .8) {
          ctx.lineWidth = 1.5; ctx.strokeStyle = c.paper; ctx.globalAlpha = Math.min(1, (s.k - .8) / .1);
          if (s.kind === 'held') poly(ctx, [[here[0] + 12, here[1] - 1], [here[0] + 15, here[1] + 2], [here[0] + 21, here[1] - 5]]);
          else { poly(ctx, [[here[0] + 12, here[1] - 5], [here[0] + 19, here[1] + 2]]); poly(ctx, [[here[0] + 19, here[1] - 5], [here[0] + 12, here[1] + 2]]); }
          ctx.lineWidth = .6;
        }
        continue;
      }
      const color = s.phase === 'fall' ? c.clay : c.paper, fade = s.phase === 'fall' ? 1 - s.k : 1;
      if (trail.length > 1) { ctx.lineWidth = 1.6; ctx.strokeStyle = color; ctx.globalAlpha = .35 * fade; poly(ctx, trail); ctx.lineWidth = .6; }
      disc(ctx, here, s.phase === 'gate' ? 3 : 2.5, color, .95 * fade);
      if (s.phase === 'gate') ring(ctx, here, 6 + s.k * 10, c.paper, .5 * (1 - s.k));
    }
    half(true);

    const intent = cam([AGENTS[0][0], AGENTS[0][1] - .5, AGENTS[0][2]]), authority = cam([0, -R - .22, 0]), record = cam([LEDGER[0] - .2, LEDGER[1] - .36, 0]);
    return [
      { at: cam([AGENTS[0][0], AGENTS[0][1] - .26, AGENTS[0][2]]), label: [intent[0] - 8, intent[1] - 34] },
      { at: authority, label: [authority[0] - 20, authority[1] - 44] },
      { at: cam([LEDGER[0] - .2, LEDGER[1] - .14, 0]), label: [record[0] - 10, record[1] - 40] },
    ];
  } };
})();

// Runs the gate on #authority-field: paused for reduced motion, when hidden,
// or when the visitor presses the motion control.
const canvas = document.querySelector('#authority-field');
const context = canvas.getContext('2d');
const motionToggle = document.querySelector('#motion-toggle');
const scene = gateScene;
if (context && scene) {
  let width = 0, height = 0, time = 0, lastTime = 0, frame = 0;
  let paused = reducedMotion.matches;
  const labels = [...document.querySelectorAll('.art-coordinate')];
  for (const label of labels) { label.style.left = '0'; label.style.top = '0'; label.style.right = 'auto'; label.style.bottom = 'auto'; label.style.whiteSpace = 'nowrap'; }
  const palette = () => { const t = getComputedStyle(document.documentElement); return { clay: t.getPropertyValue('--color-art-first').trim(), paper: t.getPropertyValue('--color-art-second').trim(), body: t.getPropertyValue('--font-body').trim() || 'sans-serif' }; };
  let colors = palette();
  function draw() {
    context.clearRect(0, 0, width, height);
    const anchors = scene.draw(context, width, height, time, colors) || [];
    anchors.forEach((a, i) => {
      const label = labels[i]; if (!label || !a) return;
      const lw = label.offsetWidth, lh = label.offsetHeight;
      const x = Math.min(Math.max(a.label[0], 4), width - lw - 4), y = Math.min(Math.max(a.label[1], 4), height - lh - 4);
      label.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
      const fx = Math.min(Math.max(a.at[0], x), x + lw), fy = y + lh + 3;
      context.globalAlpha = .32; context.strokeStyle = colors.paper; context.lineWidth = .6;
      context.beginPath(); context.moveTo(fx, fy); context.lineTo(a.at[0], a.at[1]); context.stroke();
    });
  }
  function resize() {
    const rect = canvas.getBoundingClientRect(); width = rect.width; height = rect.height;
    const ratio = Math.min(window.devicePixelRatio || 1, 1.75); canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0); colors = palette(); draw();
  }
  function tick(now) {
    frame = 0; if (paused || document.hidden) return;
    if (!lastTime || now - lastTime >= 1000 / 40) { time += Math.min((now - (lastTime || now)) / 1000, .06); lastTime = now; draw(); }
    frame = requestAnimationFrame(tick);
  }
  function updateMotion() {
    cancelAnimationFrame(frame); frame = 0; lastTime = 0;
    motionToggle.setAttribute('aria-pressed', String(paused));
    motionToggle.setAttribute('aria-label', paused ? 'Resume background animation' : 'Pause background animation');
    motionToggle.querySelector('span').textContent = paused ? 'Resume motion' : 'Pause motion';
    if (!paused && !document.hidden) frame = requestAnimationFrame(tick); else draw();
  }
  motionToggle.addEventListener('click', () => { paused = !paused; updateMotion(); });
  reducedMotion.addEventListener('change', event => { paused = event.matches; updateMotion(); });
  document.addEventListener('visibilitychange', updateMotion);
  if ('ResizeObserver' in window) new ResizeObserver(resize).observe(canvas); else window.addEventListener('resize', resize);
  time = scene.still || 0;
  resize(); updateMotion();
} else { motionToggle.hidden = true; }

// Motion layer: replays the workflow steps and counts the comparison figure.
// Every element already holds its final content; nothing here is needed to read the page.
if (!reducedMotion.matches && 'IntersectionObserver' in window && 'MutationObserver' in window) {
  document.documentElement.classList.add('js');
  const replay = () => { panel.classList.remove('replay'); void panel.offsetWidth; panel.classList.add('replay'); };
  new MutationObserver(replay).observe(document.querySelector('#workflow-title'), { childList: true, characterData: true, subtree: true });
  const figure = document.querySelector('.experiment-number');
  const once = new IntersectionObserver(entries => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      once.unobserve(entry.target);
      if (entry.target === panel) replay();
      if (entry.target === figure) {
        const final = figure.textContent, end = Number(final.replace(/[^0-9]/g, '')), start = performance.now();
        const step = now => { const t = Math.min((now - start) / 1400, 1); figure.textContent = t < 1 ? '$' + Math.round(end * (1 - Math.pow(1 - t, 3))) : final; if (t < 1) requestAnimationFrame(step); };
        requestAnimationFrame(step);
      }
    }
  }, { threshold: 0.4 });
  once.observe(panel); once.observe(figure);
}

// Authority map: route each line from the rows' real positions, so a line
// follows its provider row when a row above it opens or closes.
const authorityMap = document.querySelector('.authority-map');
if (authorityMap) {
  const svg = authorityMap.querySelector('.map-lines'), art = authorityMap.querySelector('.map-art');
  const list = authorityMap.querySelector('.provider-list'), core = authorityMap.querySelector('.map-core');
  const agents = [...authorityMap.querySelectorAll('.map-node')], rows = [...list.querySelectorAll('summary')];
  const ins = [...svg.querySelectorAll('.map-in')], outs = [...svg.querySelectorAll('.map-out')];
  const pulses = [...svg.querySelectorAll('.map-pulse')];
  const curve = (x1, y1, x2, y2) => { const m = Math.round((x1 + x2) / 2); return `M${x1} ${y1} C ${m} ${y1}, ${m} ${y2}, ${x2} ${y2}`; };
  const setPath = (path, d) => { path.setAttribute('d', d); path.style.d = `path("${d}")`; };
  function route() {
    const origin = art.getBoundingClientRect(), box = list.getBoundingClientRect();
    if (!origin.width || getComputedStyle(art).display === 'none') return;
    const width = Math.round(box.right - origin.left), height = Math.round(Math.max(origin.height, box.bottom - origin.top));
    svg.style.width = `${width}px`; svg.style.height = `${height}px`; svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
    const at = (rect, side) => [Math.round((side === 'left' ? rect.left : rect.right) - origin.left), Math.round(rect.top + rect.height / 2 - origin.top)];
    const c = core.getBoundingClientRect(), [inX, coreY] = at(c, 'left'), [outX] = at(c, 'right');
    agents.forEach((agent, i) => { const [x, y] = at(agent.getBoundingClientRect(), 'right'); setPath(ins[i], curve(x, y, inX, coreY)); });
    rows.forEach((row, i) => { const [x, y] = at(row.getBoundingClientRect(), 'left'); setPath(outs[i], curve(outX, coreY, x, y)); });
    ins.forEach((path, i) => setPath(pulses[i], path.getAttribute('d')));
    setPath(pulses[ins.length], outs[0].getAttribute('d'));
  }
  for (const details of list.querySelectorAll('details')) details.addEventListener('toggle', route);
  if ('ResizeObserver' in window) new ResizeObserver(route).observe(authorityMap); else window.addEventListener('resize', route);
  if (document.fonts) document.fonts.ready.then(route);
  route();
}
