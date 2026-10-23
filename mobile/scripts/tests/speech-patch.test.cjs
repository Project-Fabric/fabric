const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const { patchSpeech } = require('../patch-expo-speech.cjs');

test('the installed speech engine belongs to the module, not a destroyed Activity', () => {
  const root = path.resolve(__dirname, '../../node_modules/expo-speech');
  const { version } = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const source = fs.readFileSync(path.join(root, 'android/src/main/java/expo/modules/speech/SpeechModule.kt'), 'utf8');
  assert.equal(patchSpeech(source, version), source, 'Run npm install to apply the required native patch');
  assert.match(source, /OnDestroy\s*\{\s*_textToSpeech\?\.shutdown\(\)/);
  assert.doesNotMatch(source, /OnActivityDestroys/);
});

test('the pinned lifecycle patch is repeatable and refuses changed dependency code', () => {
  for (const newline of ['\n', '\r\n']) {
    const original = ['    OnActivityDestroys {', '      textToSpeech.shutdown()', '    }'].join(newline);
    const patched = patchSpeech(original, '57.0.3');
    assert.equal(patchSpeech(patched, '57.0.3'), patched);
    assert.ok(patched.includes(`OnDestroy {${newline}`));
  }
  assert.throws(() => patchSpeech('changed upstream code', '57.0.3'), /source changed/);
  assert.throws(() => patchSpeech('changed upstream code', '57.0.4'), /Review/);
});
