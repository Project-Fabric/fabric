const { withAndroidManifest, withDangerousMod } = require('expo/config-plugins');
const { mkdir, writeFile } = require('node:fs/promises');
const { join } = require('node:path');

// Assistant provides request text only. No credential, budget or approval is accepted.
function artifacts(packageName) {
  if (!/^[a-zA-Z]\w*(?:\.[a-zA-Z]\w*)+$/.test(packageName)) throw new Error('Invalid Android package.');
  return {
    shortcuts: `<?xml version="1.0" encoding="utf-8"?>
<shortcuts xmlns:android="http://schemas.android.com/apk/res/android">
  <capability android:name="actions.intent.GET_THING">
    <intent android:action="android.intent.action.VIEW"
      android:targetPackage="${packageName}"
      android:targetClass="${packageName}.CosignaAssistantActivity">
      <parameter android:name="thing.name" android:key="request" />
    </intent>
  </capability>
</shortcuts>
`,
    activity: `package ${packageName}

import android.app.Activity
import android.os.Bundle
import android.content.Intent
import android.net.Uri
import java.util.UUID

// Exported entry point is untrusted. It can only open an editable search request.
class CosignaAssistantActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val request = intent.getStringExtra("request")?.trim()
    val uri = Uri.Builder().scheme("cosigna-preview").authority("order-lab")
      .appendQueryParameter("assistantInvocation", UUID.randomUUID().toString())
    if (request != null && request.isNotEmpty() && request.length <= 4096 &&
        request.none { it.code < 32 || it.code == 127 }) {
      uri.appendQueryParameter("assistantRequest", request)
    }
    startActivity(Intent(this, MainActivity::class.java)
      .setAction(Intent.ACTION_VIEW).setData(uri.build())
      .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP))
    finish()
  }
}
`,
  };
}

function configureManifest(manifest, packageName) {
  const app = manifest.application[0];
  const main = app.activity.find(value => value.$['android:name'] === '.MainActivity');
  if (!main) throw new Error('Cosigna MainActivity not found.');
  main['meta-data'] = (main['meta-data'] ?? []).filter(value => value.$['android:name'] !== 'android.app.shortcuts');
  main['meta-data'].push({ $: { 'android:name': 'android.app.shortcuts', 'android:resource': '@xml/cosigna_shortcuts' } });
  const name = `${packageName}.CosignaAssistantActivity`;
  app.activity = app.activity.filter(value => value.$['android:name'] !== name);
  app.activity.push({ $: { 'android:name': name, 'android:exported': 'true', 'android:excludeFromRecents': 'true' } });
  // No stored bearer tokens or task text in OS backups for this single-owner test app.
  app.$['android:allowBackup'] = 'false';
  return manifest;
}

module.exports = function withCosignaAssistant(config) {
  const packageName = config.android.package;
  config = withAndroidManifest(config, config => {
    configureManifest(config.modResults.manifest, packageName); return config;
  });
  return withDangerousMod(config, ['android', async config => {
    const root = join(config.modRequest.platformProjectRoot, 'app', 'src', 'main');
    const source = join(root, 'java', ...packageName.split('.'));
    const xml = join(root, 'res', 'xml');
    await mkdir(source, { recursive: true }); await mkdir(xml, { recursive: true });
    const generated = artifacts(packageName);
    await writeFile(join(source, 'CosignaAssistantActivity.kt'), generated.activity);
    await writeFile(join(xml, 'cosigna_shortcuts.xml'), generated.shortcuts);
    return config;
  }]);
};
module.exports.artifacts = artifacts;
module.exports.configureManifest = configureManifest;
