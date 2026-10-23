const fs = require('node:fs');
const path = require('node:path');

// expo-speech 57.0.3 caches one lazy TextToSpeech instance for the module's
// lifetime. Shutting it down on Activity destruction leaves READY pointing at
// an unbound engine when React Native retains its runtime across Activities.
// Release it with the owning module instead. Recheck/remove on Expo upgrades.
const before = '    OnActivityDestroys {\n      textToSpeech.shutdown()\n    }';
const after = '    OnDestroy {\n      _textToSpeech?.shutdown()\n    }';

function patchSpeech(source, version) {
  if (version !== '57.0.3') throw new Error(`Review the Cosigna speech lifecycle patch for expo-speech ${version}.`);
  const normalized = source.replace(/\r\n/g, '\n');
  if (normalized.includes(after) && !normalized.includes(before)) return source;
  if (normalized.split(before).length !== 2 || normalized.includes(after)) {
    throw new Error('expo-speech source changed; review the speech lifecycle patch before building.');
  }
  const patched = normalized.replace(before, after);
  return source.includes('\r\n') ? patched.replace(/\n/g, '\r\n') : patched;
}

if (require.main === module) {
  const root = path.resolve(__dirname, '..', 'node_modules', 'expo-speech');
  const { version } = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const target = path.join(root, 'android/src/main/java/expo/modules/speech/SpeechModule.kt');
  const source = fs.readFileSync(target, 'utf8');
  const patched = patchSpeech(source, version);
  if (source !== patched) fs.writeFileSync(target, patched);
  console.log('Cosigna: expo-speech Android engine lifetime checked.');
}

module.exports = { patchSpeech };
