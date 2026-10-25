// Page behaviour shared by the landing mocks: motion (GSAP + ScrollTrigger when loaded),
// navigation, the mobile menu and the book-a-demo dialog. Everything degrades to a working,
// fully visible page without GSAP or with reduced motion.
const doc = document;
export const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
export const gsap = window.gsap, ScrollTrigger = window.ScrollTrigger;
const motion = !!gsap && !reduced;
if (motion && ScrollTrigger) gsap.registerPlugin(ScrollTrigger);

/** Splits [data-split] text into words wrapped for a rise-in. Keeps the text for screen readers. */
function split(el) {
  if (el.dataset.splitDone) return [];
  el.dataset.splitDone = '1';
  el.setAttribute('aria-label', el.textContent.replace(/\s+/g, ' ').trim());
  const words = [];
  const walk = node => {
    for (const child of [...node.childNodes]) {
      if (child.nodeType === 3) {
        const frag = doc.createDocumentFragment();
        child.textContent.split(/(\s+)/).forEach(part => {
          if (!part) return;
          if (/^\s+$/.test(part)) { frag.append(part); return; }
          const outer = doc.createElement('span'); outer.className = 'sw'; outer.setAttribute('aria-hidden', 'true');
          const inner = doc.createElement('span'); inner.textContent = part; outer.append(inner); frag.append(outer); words.push(inner);
        });
        child.replaceWith(frag);
      } else if (child.nodeType === 1 && child.tagName !== 'BR') walk(child);
    }
  };
  walk(el);
  return words;
}

/** Counts a number up from 0, keeping its prefix, suffix, commas and decimals. */
function count(el) {
  const text = el.textContent.trim(), m = text.match(/^([^\d]*)([\d,]+(?:\.\d+)?)(.*)$/);
  if (!m) return;
  const [, pre, num, post] = m, target = parseFloat(num.replace(/,/g, '')), decimals = (num.split('.')[1] || '').length, commas = num.includes(',');
  const fmt = v => pre + (commas ? v.toLocaleString('en-US', { minimumFractionDigits: decimals, maximumFractionDigits: decimals }) : v.toFixed(decimals)) + post;
  const state = { v: 0 };
  el.style.fontVariantNumeric = 'tabular-nums';
  gsap.to(state, { v: target, duration: 1.4, ease: 'power3.out', onUpdate: () => { el.textContent = fmt(state.v); },
    scrollTrigger: ScrollTrigger ? { trigger: el, start: 'top 90%', once: true } : undefined });
  el.textContent = fmt(0);
}

export function motionLayer() {
  if (!motion) return;
  doc.querySelectorAll('[data-split]').forEach(el => {
    const words = split(el);
    gsap.from(words, { yPercent: 110, opacity: 0, duration: 0.9, ease: 'power4.out', stagger: 0.035, delay: +(el.dataset.delay || 0),
      scrollTrigger: el.hasAttribute('data-now') || !ScrollTrigger ? undefined : { trigger: el, start: 'top 88%', once: true } });
  });
  if (ScrollTrigger) {
    doc.querySelectorAll('[data-reveal]').forEach(el => {
      const kids = el.hasAttribute('data-stagger') ? [...el.children] : [el];
      gsap.from(kids, { y: +(el.dataset.reveal || 28), opacity: 0, duration: 0.8, ease: 'power3.out', stagger: 0.08,
        scrollTrigger: { trigger: el, start: 'top 86%', once: true } });
    });
    doc.querySelectorAll('[data-parallax]').forEach(el => {
      gsap.to(el, { yPercent: +el.dataset.parallax, ease: 'none', scrollTrigger: { trigger: el.parentElement, start: 'top bottom', end: 'bottom top', scrub: true } });
    });
  }
  doc.querySelectorAll('[data-count]').forEach(count);
}

/** Sticky nav that hides on the way down, returns on the way up and marks the section in view. */
export function navigation() {
  const nav = doc.querySelector('[data-nav]');
  if (!nav) return;
  let last = scrollY;
  addEventListener('scroll', () => {
    const y = scrollY, down = y > last && y > 240;
    nav.classList.toggle('nav-hidden', down && !nav.classList.contains('menu-open'));
    nav.classList.toggle('nav-scrolled', y > 8);
    last = y;
  }, { passive: true });
  const links = [...nav.querySelectorAll('a[href^="#"]')].filter(a => a.getAttribute('href').length > 1);
  const sections = links.map(a => doc.querySelector(a.getAttribute('href'))).filter(Boolean);
  const io = new IntersectionObserver(entries => entries.forEach(e => {
    if (!e.isIntersecting) return;
    links.forEach(a => a.classList.toggle('active', a.getAttribute('href') === '#' + e.target.id));
  }), { rootMargin: '-45% 0px -50% 0px' });
  sections.forEach(s => io.observe(s));
  // Mobile menu.
  const toggle = nav.querySelector('[data-menu-toggle]'), menu = nav.querySelector('[data-menu]');
  if (toggle && menu) {
    const set = open => { nav.classList.toggle('menu-open', open); toggle.setAttribute('aria-expanded', String(open)); toggle.setAttribute('aria-label', open ? 'Close menu' : 'Open menu'); };
    toggle.addEventListener('click', () => set(!nav.classList.contains('menu-open')));
    menu.addEventListener('click', e => { if (e.target.closest('a')) set(false); });
    addEventListener('keydown', e => { if (e.key === 'Escape') set(false); });
  }
}

/** Book-a-demo: every [data-demo-open] opens one dialog; sending composes an email (no backend yet). */
export function demoDialog(to = 'hello@cosigna.ai') {
  const dlg = doc.createElement('dialog');
  dlg.className = 'cd';
  dlg.setAttribute('aria-labelledby', 'cd-title');
  dlg.innerHTML = `
    <form method="dialog" novalidate>
      <h2 id="cd-title">Book a demo</h2>
      <p>Tell us what your agents do with money. We’ll replay some of their real requests through Cosigna for you.</p>
      <label>Name<input name="name" autocomplete="name" required></label>
      <label>Work email<input name="email" type="email" autocomplete="email" required></label>
      <label>Company<input name="company" autocomplete="organization"></label>
      <label>What do your agents pay or refund?
        <select name="use"><option>Customer refunds</option><option>Payouts to contractors or sellers</option><option>Supplier payments</option><option>Something else</option></select></label>
      <p class="err" aria-live="polite"></p>
      <div class="actions"><button type="button" value="cancel" data-close>Cancel</button><button type="submit">Send</button></div>
    </form>
    <div class="done"><h2>Thanks, your email is ready to send.</h2><p>Your email app opened with the details filled in. If it didn’t, write to <a href="mailto:${to}">${to}</a>.</p>
      <div class="actions"><button type="button" data-close>Close</button></div></div>`;
  doc.body.append(dlg);
  const form = dlg.querySelector('form'), err = dlg.querySelector('.err');
  dlg.querySelectorAll('[data-close]').forEach(b => b.addEventListener('click', () => dlg.close()));
  dlg.addEventListener('click', e => { if (e.target === dlg) dlg.close(); });
  dlg.addEventListener('close', () => { dlg.classList.remove('sent'); form.reset(); err.textContent = ''; });
  form.addEventListener('submit', e => {
    e.preventDefault();
    if (!form.checkValidity()) { err.textContent = 'Please add your name and a valid work email.'; form.querySelector(':invalid').focus(); return; }
    const d = Object.fromEntries(new FormData(form));
    const body = `Name: ${d.name}\nEmail: ${d.email}\nCompany: ${d.company || '-'}\nAgents handle: ${d.use}\n`;
    location.href = `mailto:${to}?subject=${encodeURIComponent('Cosigna demo' + (d.company ? ' · ' + d.company : ''))}&body=${encodeURIComponent(body)}`;
    dlg.classList.add('sent');
  });
  doc.addEventListener('click', e => {
    const a = e.target.closest('[data-demo-open]');
    if (!a) return;
    e.preventDefault(); dlg.showModal(); dlg.querySelector('input').focus();
  });
}

/** Copy buttons: [data-copy] copies the text of the element it names (or its own value). */
export function copyButtons() {
  doc.querySelectorAll('[data-copy]').forEach(btn => btn.addEventListener('click', async () => {
    const src = doc.querySelector(btn.dataset.copy);
    try { await navigator.clipboard.writeText(src ? src.innerText : btn.dataset.copy); const t = btn.textContent; btn.textContent = 'Copied'; setTimeout(() => { btn.textContent = t; }, 1400); } catch { /* clipboard blocked */ }
  }));
}

export function boot({ to } = {}) {
  doc.documentElement.classList.add('js');
  navigation(); demoDialog(to); copyButtons(); motionLayer();
}
