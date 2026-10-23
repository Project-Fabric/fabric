const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const tabs = [...document.querySelectorAll('[data-workflow]')];
const panel = document.querySelector('#workflow-panel');
const workflows = {
  refunds: { role: 'REFUNDS AGENT', title: 'A refund that needs a second look.', description: 'A $450 request reaches an agent with a $100 per-action boundary.',
    steps: [['Read the transaction', 'Find the original payment and the customer’s request.'], ['Check the authority', 'The amount is above the agent’s delegated limit.'], ['Ask a person to decide', 'Review the request; an approval does not silently expand the agent’s limit.']],
    outcome: 'NEEDS REVIEW', note: 'This example does not execute a refund.' },
  collections: { role: 'COLLECTIONS ASSISTANT', title: 'A thoughtful follow-up, ready for review.', description: 'Two overdue invoices become two proposed follow-up drafts.',
    steps: [['Read the invoice context', 'Gather the balance, due date and recorded customer context.'], ['Prepare the follow-up', 'Draft a reminder for your team to review.'], ['Keep the person in the loop', 'The draft waits for a person; the agent sends no messages.']],
    outcome: 'DRAFT ONLY', note: 'This example sends no emails or customer messages.' },
  reconciliation: { role: 'RECONCILIATION AGENT', title: 'Find the difference before it becomes a distraction.', description: 'A sample payout differs from the expected transaction total by $25.',
    steps: [['Compare the records', 'Read the transaction and payout data.'], ['Flag the difference', 'Surface the $25 discrepancy with the relevant context.'], ['Prepare a review', 'A person investigates; no balancing entry is posted.']],
    outcome: 'FLAGGED FOR REVIEW', note: 'This example changes no accounting or payment records.' },
};
function selectWorkflow(id, focus = false) {
  const workflow = workflows[id]; if (!workflow) return;
  for (const tab of tabs) { const selected = tab.dataset.workflow === id; tab.setAttribute('aria-selected', String(selected)); tab.tabIndex = selected ? 0 : -1; if (focus && selected) tab.focus(); }
  panel.setAttribute('aria-labelledby', `tab-${id}`);
  for (const [key, value] of [['role', workflow.role], ['title', workflow.title], ['description', workflow.description], ['outcome', workflow.outcome], ['note', workflow.note]]) document.querySelector(`#workflow-${key}`).textContent = value;
  for (const [i, name] of ['one', 'two', 'three'].entries()) { document.querySelector(`#step-${name}-title`).textContent = workflow.steps[i][0]; document.querySelector(`#step-${name}-body`).textContent = workflow.steps[i][1]; }
  if (!reducedMotion.matches) panel.animate?.([{ opacity: .35, transform: 'translateY(5px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 260, easing: 'ease-out' });
}
for (const [index, tab] of tabs.entries()) {
  tab.addEventListener('click', () => selectWorkflow(tab.dataset.workflow));
  tab.addEventListener('keydown', event => {
    const offsets = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    if (event.key in offsets) { event.preventDefault(); selectWorkflow(tabs[(index + offsets[event.key] + tabs.length) % tabs.length].dataset.workflow, true); }
    if (event.key === 'Home' || event.key === 'End') { event.preventDefault(); selectWorkflow(tabs[event.key === 'Home' ? 0 : tabs.length - 1].dataset.workflow, true); }
  });
}
for (const button of document.querySelectorAll('[data-show-workflow]')) button.addEventListener('click', () => { selectWorkflow(button.dataset.showWorkflow); document.querySelector('#workspace').scrollIntoView({ behavior: reducedMotion.matches ? 'instant' : 'smooth' }); });

const dialog = document.querySelector('#dashboard-dialog');
if (dialog) {
for (const button of document.querySelectorAll('[data-open-dashboard]')) button.addEventListener('click', () => dialog.showModal());
dialog.querySelector('.dialog-close').addEventListener('click', () => dialog.close());
dialog.addEventListener('click', event => { if (event.target === dialog) { const rect = dialog.getBoundingClientRect(); if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close(); } });
document.querySelector('#copy-command').addEventListener('click', async () => {
  try { await navigator.clipboard.writeText(document.querySelector('#dashboard-command').textContent); document.querySelector('#copy-status').textContent = 'Commands copied. Run them in your terminal.'; }
  catch { document.querySelector('#copy-status').textContent = 'Select and copy the commands above.'; }
});
}

const menuToggle = document.querySelector('.menu-toggle');
const navigation = document.querySelector('#primary-navigation');
if (menuToggle && navigation) {
  function closeMenu(restoreFocus = false) {
    navigation.classList.remove('is-open'); menuToggle.setAttribute('aria-expanded', 'false');
    menuToggle.setAttribute('aria-label', 'Open navigation'); if (restoreFocus) menuToggle.focus();
  }
  menuToggle.addEventListener('click', () => {
    const open = menuToggle.getAttribute('aria-expanded') !== 'true';
    menuToggle.setAttribute('aria-expanded', String(open)); menuToggle.setAttribute('aria-label', open ? 'Close navigation' : 'Open navigation');
    navigation.classList.toggle('is-open', open);
  });
  navigation.addEventListener('click', event => { if (event.target.closest('a')) closeMenu(); });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && menuToggle.getAttribute('aria-expanded') === 'true') closeMenu(true); });
  document.addEventListener('click', event => { if (!document.querySelector('.site-header').contains(event.target)) closeMenu(); });
}

// Deliberately browser-only illustration: no fetch, credentials or payment call.
const purchaseDemo = document.querySelector('#purchase-demo');
if (purchaseDemo) {
  const approval = document.querySelector('#demo-approve');
  const scenarioButtons = [...document.querySelectorAll('[data-scenario]')];
  let scenario = 'within', completion, sceneRevision = 0;
  const set = (id, value) => { document.querySelector(`#${id}`).textContent = value; };
  function buttonLabel(text) {
    approval.textContent = `${text} `;
    const arrow = document.createElement('span'); arrow.setAttribute('aria-hidden', 'true'); arrow.textContent = '↗'; approval.append(arrow);
  }
  function selectScenario(id) {
    if (!['within', 'over', 'replay'].includes(id)) return;
    clearTimeout(completion); sceneRevision++; scenario = id; purchaseDemo.dataset.scenario = id;
    for (const button of scenarioButtons) button.setAttribute('aria-pressed', String(button.dataset.scenario === id));
    set('demo-item', id === 'over' ? 'Four oat lattes' : 'One oat latte');
    set('demo-amount', id === 'over' ? '$28.00' : '$7.00');
    set('demo-budget-result', id === 'over' ? '$18.00 over limit' : '$3.00 headroom');
    set('demo-budget-label', id === 'over' ? 'Amount exceeds limit' : 'Amount inside limit');
    set('demo-budget-icon', id === 'over' ? '×' : '✓');
    document.querySelector('#demo-budget-icon').className = `check-circle${id === 'over' ? ' blocked' : ''}`;
    document.querySelector('#demo-approval-icon').className = `check-circle${id === 'replay' ? '' : ' pending'}`;
    set('demo-approval-icon', id === 'replay' ? '✓' : '·');
    set('demo-approval-label', 'Exact owner approval');
    set('demo-approval-result', id === 'replay' ? 'Previously recorded' : id === 'over' ? 'Cannot submit' : 'Waiting');
    purchaseDemo.dataset.state = id === 'within' ? 'waiting' : id === 'over' ? 'blocked' : 'replay';
    set('demo-status', id === 'within' ? 'Your approval is required.' : id === 'over' ? 'Stopped at the budget boundary.' : 'The existing result is recovered.');
    set('demo-detail', id === 'within' ? 'A budget alone does not authorize this purchase.' : id === 'over' ? 'Approval cannot silently expand a $10 per-order limit.' : 'This example assumes a prior approved purchase. No second write is made.');
    approval.disabled = id !== 'within';
    buttonLabel(id === 'within' ? 'Simulate exact approval' : id === 'over' ? 'Outside the permitted limit' : 'Existing example receipt');
    set('demo-footnote', 'Illustration only · No money moves');
  }
  for (const button of scenarioButtons) button.addEventListener('click', () => selectScenario(button.dataset.scenario));
  approval.addEventListener('click', () => {
    if (scenario !== 'within' || purchaseDemo.dataset.state !== 'waiting') return;
    const submittedRevision = ++sceneRevision;
    purchaseDemo.dataset.state = 'pending'; approval.disabled = true;
    set('demo-status', 'Recording the example attempt…'); set('demo-detail', 'The execution path records a stable claim before a write.');
    buttonLabel('Illustrating the transaction');
    function finish() {
      if (submittedRevision !== sceneRevision || scenario !== 'within' || purchaseDemo.dataset.state !== 'pending') return;
      purchaseDemo.dataset.state = 'confirmed';
      document.querySelector('#demo-approval-icon').className = 'check-circle';
      set('demo-approval-icon', '✓'); set('demo-approval-result', 'Example approved');
      set('demo-status', 'The example is confirmed.'); set('demo-detail', 'A retry would recover the same attempt and receipt.');
      set('demo-footnote', 'Illustration only · No order was submitted'); buttonLabel('Illustration complete');
    }
    if (reducedMotion.matches) finish(); else completion = setTimeout(finish, 650);
  });
  selectScenario('within');
}

if (!reducedMotion.matches && 'IntersectionObserver' in window) {
  document.body.classList.add('has-motion');
  const observer = new IntersectionObserver(entries => { for (const entry of entries) if (entry.isIntersecting) { entry.target.classList.add('is-visible'); observer.unobserve(entry.target); } }, { threshold: 0.07 });
  for (const element of document.querySelectorAll('.reveal')) observer.observe(element);
}

// Original ambient wireframe, behind the locally authored gateway illustration.
const canvas = document.querySelector('#authority-field');
const context = canvas?.getContext('2d');
const motionToggle = document.querySelector('#motion-toggle');
if (context && motionToggle) {
  let width = 0, height = 0, phase = 0, lastTime = 0, frame = 0;
  let paused = reducedMotion.matches;
  let artVisible = true;
  const ringPoints = [];
  for (let ring = 0; ring < 2; ring++) {
    const paths = [];
    for (let meridian = 0; meridian < 19; meridian++) {
      const path = []; const v = meridian / 19 * Math.PI * 2;
      for (let step = 0; step <= 96; step++) path.push(surface(step / 96 * Math.PI * 2, v, ring));
      paths.push(path);
    }
    for (let step = 0; step < 50; step++) {
      const path = []; const u = step / 50 * Math.PI * 2;
      for (let meridian = 0; meridian <= 28; meridian++) path.push(surface(u, meridian / 28 * Math.PI * 2, ring));
      paths.push(path);
    }
    ringPoints.push(paths);
  }
  function surface(u, v, ring) {
    const radius = 1.72 + .36 * Math.cos(v);
    let x = radius * Math.cos(u), y = radius * Math.sin(u) * 1.13, z = .36 * Math.sin(v);
    const tilt = ring === 0 ? -.7 : .7;
    [x, z] = [x * Math.cos(tilt) + z * Math.sin(tilt), -x * Math.sin(tilt) + z * Math.cos(tilt)];
    x += ring === 0 ? -.72 : .72;
    return [x, y, z];
  }
  function project(point) {
    let [x, y, z] = point;
    const a = .68 + Math.sin(phase * .18) * .12, b = -.46 + Math.sin(phase * .12) * .1;
    [y, z] = [y * Math.cos(a) - z * Math.sin(a), y * Math.sin(a) + z * Math.cos(a)];
    [x, y] = [x * Math.cos(b) - y * Math.sin(b), x * Math.sin(b) + y * Math.cos(b)];
    const perspective = 7 / (7 + z), scale = Math.min(width * .16, height * .26);
    return [width * .53 + x * scale * perspective, height * .51 + y * scale * perspective, z];
  }
  function draw() {
    context.clearRect(0, 0, width, height); context.lineWidth = .65;
    for (const [ring, paths] of ringPoints.entries()) for (const path of paths) {
      const projected = path.map(project), depth = projected.reduce((sum, p) => sum + p[2], 0) / projected.length;
      context.strokeStyle = `rgba(${ring === 0 ? '83,116,207' : '113,146,227'},${Math.max(.07, .17 - depth * .028)})`;
      context.beginPath(); for (const [index, point] of projected.entries()) { if (index === 0) context.moveTo(point[0], point[1]); else context.lineTo(point[0], point[1]); } context.stroke();
    }
    for (let ring = 0; ring < 2; ring++) {
      const p = project(surface(phase * .13 + ring * Math.PI, .35, ring));
      context.beginPath(); context.arc(p[0], p[1], 2.6, 0, Math.PI * 2); context.fillStyle = '#dfcfb3'; context.fill();
    }
  }
  function resize() {
    const rect = canvas.getBoundingClientRect(); width = rect.width; height = rect.height;
    const ratio = Math.min(window.devicePixelRatio || 1, 1.75); canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio); context.setTransform(ratio, 0, 0, ratio, 0, 0); draw();
  }
  function tick(time) {
    frame = 0; if (paused || document.hidden || !artVisible) return;
    if (!lastTime || time - lastTime >= 1000 / 30) { phase += Math.min((time - (lastTime || time)) / 1000, .06); lastTime = time; draw(); }
    frame = requestAnimationFrame(tick);
  }
  function updateMotion() {
    cancelAnimationFrame(frame); frame = 0; lastTime = 0;
    motionToggle.setAttribute('aria-pressed', String(paused));
    motionToggle.setAttribute('aria-label', paused ? 'Resume background animation' : 'Pause background animation');
    motionToggle.querySelector('span').textContent = paused ? 'Resume motion' : 'Pause motion';
    document.body.classList.toggle('motion-paused', paused || document.hidden);
    document.body.classList.toggle('hero-out-of-view', !artVisible);
    if (!paused && !document.hidden && artVisible) frame = requestAnimationFrame(tick); else draw();
  }
  motionToggle.addEventListener('click', () => { paused = !paused; updateMotion(); });
  reducedMotion.addEventListener('change', event => { paused = event.matches; updateMotion(); });
  document.addEventListener('visibilitychange', updateMotion);
  if ('ResizeObserver' in window) new ResizeObserver(resize).observe(canvas); else window.addEventListener('resize', resize);
  if ('IntersectionObserver' in window) new IntersectionObserver(entries => { artVisible = entries.some(entry => entry.isIntersecting); updateMotion(); }).observe(canvas);
  resize(); updateMotion();
} else if (motionToggle) {
  let paused = reducedMotion.matches;
  const update = () => { document.body.classList.toggle('motion-paused', paused); motionToggle.setAttribute('aria-pressed', String(paused)); motionToggle.querySelector('span').textContent = paused ? 'Resume motion' : 'Pause motion'; motionToggle.setAttribute('aria-label', paused ? 'Resume background animation' : 'Pause background animation'); };
  motionToggle.addEventListener('click', () => { paused = !paused; update(); });
  reducedMotion.addEventListener('change', event => { paused = event.matches; update(); }); update();
}
