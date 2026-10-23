import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const frame = runInNewContext(`${readFileSync(new URL('../character-field.js', import.meta.url), 'utf8')}\nauthorityCharacterFrame;`);
test('character geometry is bounded, deterministic and reversible at desktop and phone widths', () => {
  for (const [width, height] of [[1440, 220], [390, 160]]) {
    const original = JSON.stringify(frame(width, height, 1.1));
    assert.notEqual(JSON.stringify(frame(width, height, 2.8)), original);
    assert.equal(JSON.stringify(frame(width, height, 1.1)), original);
    const glyphs = frame(width, height, 1.1); assert.ok(glyphs.length > 100);
    for (const glyph of glyphs) {
      assert.ok(glyph.x >= 0 && glyph.x < width && glyph.y >= 0 && glyph.y < height);
      assert.ok(Number.isFinite(glyph.depth) && glyph.alpha >= 0 && glyph.alpha <= 1);
      assert.equal(glyph.char.length, 1);
    }
  }
  assert.equal(frame(0, 0).length, 0);
});
