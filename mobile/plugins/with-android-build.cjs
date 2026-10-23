const { withAppBuildGradle, withGradleProperties, withDangerousMod } = require('expo/config-plugins');
const fs = require('node:fs/promises');
const path = require('node:path');

function configureDaemonJvm(contents = '') {
  // Studio can generate Java 25 daemon criteria which override JAVA_HOME.
  // Drop URLs for that version and use the same Java 21 as the build launcher.
  return contents.split(/\r?\n/).filter(line => line.trim() && !/^toolchain(?:Version|Vendor|Implementation|Url\.)/.test(line)).join('\n')
    + '\ntoolchainVersion=21\n';
}

const block = `// @cosigna-native-build begin
// Keep generated C++ object paths within Windows Ninja's filename limit.
if (System.getProperty("os.name").toLowerCase().contains("windows")) {
    def cosignaBuildId = java.security.MessageDigest.getInstance("SHA-256")
        .digest(rootProject.projectDir.canonicalPath.getBytes("UTF-8")).encodeHex().toString().take(10)
    android.externalNativeBuild.cmake.buildStagingDirectory = new File(System.getProperty("user.home"), ".cxx/" + cosignaBuildId)
    android.defaultConfig.externalNativeBuild.cmake.arguments.add("-DCMAKE_OBJECT_PATH_MAX=250")
}
// @cosigna-native-build end`;
function configureAppGradle(contents) {
  return contents.replace(/\n?\/\/ @cosigna-native-build begin[\s\S]*?\/\/ @cosigna-native-build end\n?/g, '').trimEnd() + '\n\n' + block + '\n';
}
module.exports = function withAndroidBuild(config) {
  config = withDangerousMod(config, ['android', async config => {
    const target = path.join(config.modRequest.platformProjectRoot, 'gradle', 'gradle-daemon-jvm.properties');
    const contents = await fs.readFile(target, 'utf8').catch(error => { if (error.code !== 'ENOENT') throw error; return ''; });
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.writeFile(target, configureDaemonJvm(contents));
    return config;
  }]);
  config = withGradleProperties(config, config => {
    for (const [key, value] of Object.entries({ 'org.gradle.jvmargs': '-Xmx3072m -XX:MaxMetaspaceSize=1024m', 'org.gradle.workers.max': '4' })) {
      config.modResults = config.modResults.filter(value => value.type !== 'property' || value.key !== key);
      config.modResults.push({ type: 'property', key, value });
    }
    return config;
  });
  return withAppBuildGradle(config, config => {
    if (config.modResults.language !== 'groovy') throw new Error('Cosigna native build expects the Expo Groovy Android template.');
    config.modResults.contents = configureAppGradle(config.modResults.contents); return config;
  });
};
module.exports.configureAppGradle = configureAppGradle;
module.exports.configureDaemonJvm = configureDaemonJvm;
