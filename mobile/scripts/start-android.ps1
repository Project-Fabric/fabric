param([string]$DeviceSerial, [string]$AvdName, [switch]$Rebuild, [switch]$Fixtures, [switch]$EnableHostMicrophone)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'android-tools.ps1')
$mobileRoot = Split-Path $PSScriptRoot -Parent
$projectRoot = Split-Path $mobileRoot -Parent
$sdk = Get-CosignaAndroidSdk
$adb = Join-Path $sdk 'platform-tools\adb.exe'
$apk = Join-Path $mobileRoot 'dist\native\cosigna-test.apk'
if ($Rebuild -or -not (Test-Path -LiteralPath $apk)) {
  & (Join-Path $PSScriptRoot 'build-android.ps1')
  if (-not (Test-Path -LiteralPath $apk)) { throw 'APK not found after build.' }
}
function Get-ReadyCosignaDevices {
  @(& $adb devices | Where-Object { $_ -match '^\S+\s+device$' } | ForEach-Object { ($_ -split '\s+')[0] })
}
$devices = @(Get-ReadyCosignaDevices)
if ($AvdName) {
  if ($AvdName -notmatch '^[A-Za-z0-9_.-]+$') { throw 'Use an AVD name containing letters, digits, dots, hyphens or underscores.' }
  $emulator = Join-Path $sdk 'emulator\emulator.exe'
  if (-not (Test-Path -LiteralPath $emulator)) { throw 'Install Android Emulator in Android Studio > SDK Manager > SDK Tools.' }
  $avds = @(& $emulator -list-avds)
  if ($avds -notcontains $AvdName) { throw "AVD not found. Create it in Android Studio > Device Manager. Available: $($avds -join ', ')" }
  # Reuse that AVD if already booted; do not silently choose another device.
  $matching = @($devices | Where-Object { $_ -match '^emulator-' -and (@(& $adb -s $_ emu avd name) -contains $AvdName) })
  if ($matching.Count -eq 1) { $DeviceSerial = $matching[0] }
  else {
    Start-Process -FilePath $emulator -ArgumentList @('-avd', $AvdName) -WindowStyle Normal | Out-Null
    $bootDeadline = (Get-Date).AddMinutes(4)
    do {
      Start-Sleep -Seconds 2
      $matching = @(Get-ReadyCosignaDevices | Where-Object { $_ -match '^emulator-' -and (@(& $adb -s $_ emu avd name) -contains $AvdName) })
      if ($matching.Count -eq 1) { $DeviceSerial = $matching[0]; break }
    } while ((Get-Date) -lt $bootDeadline)
    if (-not $DeviceSerial) { throw 'Emulator did not connect within four minutes. Check Android Studio Device Manager.' }
  }
}
$devices = @(Get-ReadyCosignaDevices)
if (-not $DeviceSerial -and $devices.Count -eq 1) { $DeviceSerial = $devices[0] }
if (-not $DeviceSerial -or $devices -notcontains $DeviceSerial) { throw 'Start one emulator in Android Studio Device Manager or connect one authorized phone. For multiple devices use -DeviceSerial or -AvdName.' }
$bootDeadline = (Get-Date).AddMinutes(3)
while ((& $adb -s $DeviceSerial shell getprop sys.boot_completed) -ne '1') {
  if ((Get-Date) -ge $bootDeadline) { throw 'Android did not finish booting within three minutes.' }
  Start-Sleep -Seconds 2
}
& $adb -s $DeviceSerial reverse tcp:3097 tcp:3097
if ($LASTEXITCODE -ne 0) { throw 'Backend forwarding failed.' }
if ($EnableHostMicrophone) {
  if ($DeviceSerial -notmatch '^emulator-') { throw '-EnableHostMicrophone is only for Android emulators. Allow microphone access on the physical phone instead.' }
  $microphoneResult = @(& $adb -s $DeviceSerial emu avd hostmicon)
  if ($LASTEXITCODE -ne 0 -or -not ($microphoneResult | Where-Object { $_.Trim() -eq 'OK' })) { throw 'Could not enable host microphone input. Use emulator Extended controls > Microphone instead.' }
  Write-Host 'Emulator host microphone input enabled. Use Windows Sound settings to select the input device; Cosigna still needs recording permission.'
}
& $adb -s $DeviceSerial install -r $apk
if ($LASTEXITCODE -ne 0) { throw 'APK installation failed. The installed app was not removed.' }
if (-not $Fixtures -and -not $env:COSIGNA_GROQ_API_KEY) {
  $labAlreadyRunning = Get-NetTCPConnection -State Listen -LocalPort 3097 -ErrorAction SilentlyContinue
  if (-not $labAlreadyRunning) {
    $env:COSIGNA_GROQ_API_KEY = [System.Net.NetworkCredential]::new('', (Read-Host 'Groq API key (hidden, kept in this terminal)' -AsSecureString)).Password
  }
}
$runtime = Join-Path $projectRoot 'testbeds\voice-merchant\android-runtime.mjs'
$runtimeArgs = @($runtime, '--adb', $adb, '--device', $DeviceSerial)
if ($Fixtures) { $runtimeArgs += '--fixtures' }
& node @runtimeArgs
if ($LASTEXITCODE -ne 0) { throw 'Cosigna test runtime stopped with an error. Read the status above; do not reset its data.' }
