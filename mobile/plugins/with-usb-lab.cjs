const { withAndroidManifest, withDangerousMod } = require('expo/config-plugins');
const { mkdir, writeFile } = require('node:fs/promises');
const { join } = require('node:path');

// The USB test backend remains loopback-only. Never allow cleartext to arbitrary hosts.
module.exports = function withUsbLab(config) {
  config = withAndroidManifest(config, config => {
    const app = config.modResults.manifest.application[0];
    app.$['android:networkSecurityConfig'] = '@xml/cosigna_usb_network_security';
    app.$['android:usesCleartextTraffic'] = 'false';
    return config;
  });
  return withDangerousMod(config, ['android', async config => {
    const directory = join(config.modRequest.platformProjectRoot, 'app', 'src', 'main', 'res', 'xml');
    await mkdir(directory, { recursive: true });
    await writeFile(join(directory, 'cosigna_usb_network_security.xml'), `<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
  <base-config cleartextTrafficPermitted="false" />
  <domain-config cleartextTrafficPermitted="true">
    <domain includeSubdomains="false">127.0.0.1</domain>
    <domain includeSubdomains="false">localhost</domain>
  </domain-config>
</network-security-config>
`);
    return config;
  }]);
};
