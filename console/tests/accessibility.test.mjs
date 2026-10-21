import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const css = readFileSync(new URL('../src/app/globals.css', import.meta.url), 'utf8');
function luminance(hex) {
  let digits = hex.replace('#', '');
  if (digits.length === 3) digits = [...digits].map(c => c + c).join('');
  const channel = offset => {
    const value = parseInt(digits.slice(offset, offset + 2), 16) / 255;
    return value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4;
  };
  return .2126 * channel(0) + .7152 * channel(2) + .0722 * channel(4);
}
function contrast(a, b) { const l = [luminance(a), luminance(b)].sort((x, y) => y - x); return (l[0] + .05) / (l[1] + .05); }
let inherited = {};
for (const match of css.matchAll(/:root(?:\[data-theme=light\])?\{([^}]+)\}/g)) {
  const theme = match[0].includes('data-theme=light') ? 'light' : 'dark';
  const colors = { ...inherited, ...Object.fromEntries([...match[1].matchAll(/--([\w-]+):(#[a-fA-F0-9]+)/g)].map(m => [m[1], m[2]])) };
  if (theme === 'dark') inherited = colors;
  test(`${theme} text and semantic badges meet WCAG AA text contrast`, () => {
    for (const foreground of ['fg', 'muted', 'subtle']) for (const background of ['bg', 'panel', 'raised'])
      assert.ok(contrast(colors[foreground], colors[background]) >= 4.5, `${theme}: ${foreground} on ${background}`);
    for (const color of ['green', 'amber', 'red']) assert.ok(contrast(colors[color], colors[`${color}-bg`]) >= 4.5, `${theme}: ${color} badge`);
    assert.ok(contrast('#ffffff', colors.accent) >= 4.5, `${theme}: primary button`);
  });
}
