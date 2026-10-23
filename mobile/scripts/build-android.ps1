param([string]$Architectures = 'arm64-v8a,x86_64')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'android-tools.ps1')
$mobileRoot = Split-Path $PSScriptRoot -Parent
try { Set-CosignaJava }
catch {
  Write-Host 'Installing the compatible Cosigna build JDK 21 (user-local, publisher checksum verified).'
  & (Join-Path $PSScriptRoot 'install-android-jdk.ps1')
  Set-CosignaJava
}
$env:ANDROID_HOME = Get-CosignaAndroidSdk
if (-not (Test-Path -LiteralPath (Join-Path $env:ANDROID_HOME 'platforms\android-36\android.jar'))) {
  if (-not (Test-Path -LiteralPath (Join-Path $env:ANDROID_HOME 'licenses\android-sdk-license'))) {
    throw 'Install Android SDK Platform 36 and review its SDK license in Android Studio > SDK Manager, then run again.'
  }
  Write-Host 'Gradle will download required SDK components using the licenses already accepted in Android Studio.'
}
if ($Architectures -notmatch '^(arm64-v8a|x86_64|armeabi-v7a|x86)(,(arm64-v8a|x86_64|armeabi-v7a|x86))*$') { throw 'Invalid CPU architectures.' }
Push-Location $mobileRoot
$oldAppMode = $env:EXPO_PUBLIC_COSIGNA_NATIVE_APP
$oldNodeEnv = $env:NODE_ENV
try {
  $env:EXPO_PUBLIC_COSIGNA_NATIVE_APP = '1'
  $env:NODE_ENV = 'production'
  & node (Join-Path $PSScriptRoot 'patch-expo-speech.cjs')
  if ($LASTEXITCODE -ne 0) { throw 'Speech engine lifecycle patch needs review.' }
  & npx.cmd expo prebuild --platform android --no-install
  if ($LASTEXITCODE -ne 0) { throw 'Android project generation failed.' }
  Push-Location (Join-Path $mobileRoot 'android')
  try {
    & .\gradlew.bat :app:assembleRelease "-PreactNativeArchitectures=$Architectures" --console=plain --max-workers=4
    if ($LASTEXITCODE -ne 0) { throw 'Android APK build failed. Check the Gradle error above.' }
  } finally { Pop-Location }
  $destination = Join-Path $mobileRoot 'dist\native'
  New-Item -ItemType Directory -Path $destination -Force | Out-Null
  Copy-Item -LiteralPath (Join-Path $mobileRoot 'android\app\build\outputs\apk\release\app-release.apk') -Destination (Join-Path $destination 'cosigna-test.apk') -Force
  Write-Host "Bundled test APK: $destination\cosigna-test.apk"
  Write-Host 'Internal testing only: signed with the generated development key. UI is bundled; Metro and Expo Go are not required.'
} finally {
  $env:EXPO_PUBLIC_COSIGNA_NATIVE_APP = $oldAppMode
  $env:NODE_ENV = $oldNodeEnv
  Pop-Location
}
