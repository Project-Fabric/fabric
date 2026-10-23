$ErrorActionPreference = 'Stop'
function Get-CosignaAndroidSdk {
  $candidates = @($env:ANDROID_HOME, $env:ANDROID_SDK_ROOT, (Join-Path $env:LOCALAPPDATA 'Android\Sdk'))
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path -LiteralPath (Join-Path $candidate 'platform-tools\adb.exe'))) { return $candidate }
  }
  throw 'Android SDK not found. In Android Studio > SDK Manager, install Platform-Tools, Android SDK Platform 36, Build-Tools and Emulator.'
}
function Set-CosignaJava {
  $localJdks = @(Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Cosigna\toolchains') -Directory -Filter 'jdk-21*' -ErrorAction SilentlyContinue | Sort-Object Name -Descending | ForEach-Object FullName)
  $candidates = @($env:COSIGNA_ANDROID_JAVA_HOME, $env:JAVA_HOME) + $localJdks + @('C:\Program Files\Android\Android Studio\jbr', (Join-Path $env:LOCALAPPDATA 'Programs\Android Studio\jbr'))
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path -LiteralPath (Join-Path $candidate 'bin\java.exe'))) {
      $releaseFile = Join-Path $candidate 'release'
      $versionLine = if (Test-Path -LiteralPath $releaseFile) { Get-Content -LiteralPath $releaseFile | Where-Object { $_ -match '^JAVA_VERSION=' } | Select-Object -First 1 } else { '' }
      if ($versionLine -notmatch '^JAVA_VERSION="(17|21)\.') { continue }
      $env:JAVA_HOME = $candidate
      $env:PATH = (Join-Path $candidate 'bin') + ';' + $env:PATH
      return
    }
  }
  throw 'This Android build needs JDK 17 or 21. Run mobile\scripts\install-android-jdk.ps1 or set COSIGNA_ANDROID_JAVA_HOME. Android Studio Java 25 fails native CMake configuration with this Gradle plugin.'
}
