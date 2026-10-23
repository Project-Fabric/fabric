param([string]$AdbPath, [string]$DeviceSerial)
$ErrorActionPreference = 'Stop'

if (-not $AdbPath) {
  $adbCommand = Get-Command adb -ErrorAction SilentlyContinue
  if ($adbCommand) { $AdbPath = $adbCommand.Source }
  else { $AdbPath = Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\adb.exe' }
}
if (-not (Test-Path -LiteralPath $AdbPath -PathType Leaf)) {
  throw 'ADB not found. In Android Studio > SDK Manager > SDK Tools, install Android SDK Platform-Tools. You can also pass -AdbPath with the full path to adb.exe.'
}
$devices = @(& $AdbPath devices)
if ($LASTEXITCODE -ne 0) { throw 'ADB could not list devices.' }
$ready = @($devices | Where-Object { $_ -match '^\S+\s+device$' } | ForEach-Object { ($_ -split '\s+')[0] })
if ($DeviceSerial) {
  if ($ready -notcontains $DeviceSerial) { throw 'That device is not authorized. Unlock the phone and accept its USB debugging prompt.' }
} elseif ($ready.Count -eq 1) { $DeviceSerial = $ready[0] }
else { throw 'Connect one phone by USB, enable USB debugging and accept the authorization prompt. If multiple devices are connected, pass -DeviceSerial.' }

& $AdbPath -s $DeviceSerial reverse tcp:3097 tcp:3097
if ($LASTEXITCODE -ne 0) { throw 'Could not forward the Cosigna USB test port.' }
& $AdbPath -s $DeviceSerial reverse tcp:8081 tcp:8081
if ($LASTEXITCODE -ne 0) { throw 'Could not forward the Expo development port.' }
Write-Host 'USB forwarding ready. The phone can reach the lab at http://127.0.0.1:3097.'
Write-Host 'Start npm run start:usb in mobile, open Expo Go on the phone, then open exp://127.0.0.1:8081.'
Write-Host 'In Cosigna, choose Open connected order lab and enter a one-use pairing code from the computer browser.'
