const { test } = require('node:test');
const assert = require('node:assert/strict');
const { artifacts, configureManifest } = require('../../plugins/with-cosigna-assistant.cjs');
const { configureAppGradle, configureDaemonJvm } = require('../../plugins/with-android-build.cjs');

test('Studio Java 25 daemon criteria cannot override the compatible build JDK', () => {
  const original = '# generated\ntoolchainVersion=25\ntoolchainVendor=JETBRAINS\ntoolchainUrl.WINDOWS.X86_64=https://example.test/java25\n';
  const patched = configureDaemonJvm(original);
  assert.match(patched, /toolchainVersion=21/);
  assert.doesNotMatch(patched, /java25|=25|JETBRAINS/);
  assert.equal(configureDaemonJvm(patched), patched);
});
test('native build regeneration retains existing Gradle settings and installs one Windows path fix', () => {
  const original = 'android { namespace "com.cosigna.preview" }\n';
  const once = configureAppGradle(original), twice = configureAppGradle(once);
  assert.equal(twice, once);
  assert(twice.startsWith(original.trim()));
  assert.equal((twice.match(/@cosigna-native-build begin/g) ?? []).length, 1);
  assert.match(twice, /CMAKE_OBJECT_PATH_MAX=250/);
});
test('assistant capability targets a draft-only native activity, not a payment endpoint', () => {
  const generated = artifacts('com.cosigna.preview');
  assert.match(generated.shortcuts, /actions.intent.GET_THING/);
  assert.match(generated.shortcuts, /android:key="request"/);
  assert.match(generated.activity, /appendQueryParameter\("assistantRequest", request\)/);
  assert(!generated.activity.includes('/approve'));
  assert(!generated.activity.includes('Authorization'));
  assert.throws(() => artifacts('bad"package'), /Invalid Android package/);
});
test('prebuild is idempotent and disables backing up the scoped device session', () => {
  const manifest = { application: [{ $: {}, activity: [{ $: { 'android:name': '.MainActivity' } }] }] };
  configureManifest(manifest, 'com.cosigna.preview'); configureManifest(manifest, 'com.cosigna.preview');
  assert.equal(manifest.application[0].activity.length, 2);
  assert.equal(manifest.application[0].activity[0]['meta-data'].length, 1);
  assert.equal(manifest.application[0].$['android:allowBackup'], 'false');
});
