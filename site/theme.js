// Run before styles load so a saved preference never flashes the opposite theme.
(() => {
  const key = 'cosigna.theme';
  const root = document.documentElement;
  const device = window.matchMedia('(prefers-color-scheme: dark)');
  const valid = value => value === 'light' || value === 'dark';
  let preference = 'system';
  try { const saved = localStorage.getItem(key); if (valid(saved)) preference = saved; } catch { /* Storage is optional. */ }

  function apply() {
    const theme = preference === 'system' ? (device.matches ? 'dark' : 'light') : preference;
    root.dataset.theme = theme;
    root.dataset.themePreference = preference;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#0c0c0b' : '#f5f2eb');
    for (const toggle of document.querySelectorAll('[data-theme-toggle]')) {
      const label = `Switch to ${theme === 'dark' ? 'light' : 'dark'} theme`;
      toggle.setAttribute('aria-label', label); toggle.title = label; toggle.hidden = false;
    }
    for (const reset of document.querySelectorAll('[data-theme-system]')) {
      reset.setAttribute('aria-pressed', String(preference === 'system'));
      reset.textContent = preference === 'system' ? 'Device theme active' : 'Use device theme';
      reset.hidden = false;
    }
  }
  function choose(value) {
    preference = value;
    try { if (value === 'system') localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* Keep the choice for this page. */ }
    apply();
  }
  apply();
  device.addEventListener('change', () => { if (preference === 'system') apply(); });
  window.addEventListener('storage', event => {
    if (event.key !== key && event.key !== null) return;
    preference = valid(event.newValue) ? event.newValue : 'system'; apply();
  });
  function connect() {
    for (const toggle of document.querySelectorAll('[data-theme-toggle]')) toggle.addEventListener('click', () => choose(root.dataset.theme === 'dark' ? 'light' : 'dark'));
    for (const reset of document.querySelectorAll('[data-theme-system]')) reset.addEventListener('click', () => choose('system'));
    apply();
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', connect, { once: true }); else connect();
})();
