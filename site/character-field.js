// Original geometry for Cosigna's character field. No copied imagery or model assets.
// A frame depends only on dimensions and phase, so it can be scrubbed and reproduced.
function authorityCharacterFrame(width, height, phase = 0) {
  if (!(width > 0 && height > 0)) return [];
  const cell = width < 600 ? 7 : 8;
  const columns = Math.min(200, Math.ceil(width / cell));
  const rows = Math.max(1, Math.ceil(height / 10));
  const scaleX = width / 11.5, scaleY = Math.min(width / 7, height / 4.9);
  const buffer = new Map(), ramp = '.,:;+=x*#@';
  for (let ring = 0; ring < 3; ring++) {
    const angle = phase * .22 + ring * .87;
    for (let u = 0; u < 108; u++) for (let v = 0; v < 24; v++) {
      const a = u / 108 * Math.PI * 2, b = v / 24 * Math.PI * 2;
      const radius = 1.52 + .46 * Math.cos(b);
      let x = radius * Math.cos(a), y = radius * Math.sin(a), z = .46 * Math.sin(b);
      [y, z] = [y * Math.cos(angle) - z * Math.sin(angle), y * Math.sin(angle) + z * Math.cos(angle)];
      [x, z] = [x * .76 + z * .65, -x * .65 + z * .76];
      x += (ring - 1) * 3.15;
      const col = Math.round((width / 2 + x * scaleX) / cell);
      const row = Math.round((height / 2 + y * scaleY) / 10);
      if (col < 0 || col >= columns || row < 0 || row >= rows) continue;
      const key = row * columns + col, existing = buffer.get(key);
      if (existing && existing.depth > z) continue;
      const light = Math.max(0, Math.min(1, .48 + z * .19 + Math.sin(a + b) * .16));
      buffer.set(key, { x: col * cell, y: row * 10, depth: z,
        char: ramp[Math.floor(light * (ramp.length - 1))], alpha: .18 + light * .68,
        accent: Math.sin(a * 2 - phase * .7 + ring) > .97 && z > 0 });
    }
  }
  return [...buffer.values()];
}

function drawAuthorityCharacters(context, width, height, phase, dark) {
  context.clearRect(0, 0, width, height);
  context.font = '9px ui-monospace, Consolas, monospace';
  context.textBaseline = 'middle';
  for (const glyph of authorityCharacterFrame(width, height, phase)) {
    context.globalAlpha = glyph.alpha;
    context.fillStyle = glyph.accent ? (dark ? '#f3a175' : '#8f4529') : (dark ? '#e5dfd2' : '#514b42');
    context.fillText(glyph.char, glyph.x, glyph.y);
  }
  context.globalAlpha = 1;
}
