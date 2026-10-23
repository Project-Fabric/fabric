$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Publisher metadata verified via Adoptium's public API. No system Java/IDE change.
$toolchainRoot = Join-Path $env:LOCALAPPDATA 'Cosigna\toolchains'
$jdkDirectory = Join-Path $toolchainRoot 'jdk-21.0.12.1+1'
if (Test-Path -LiteralPath (Join-Path $jdkDirectory 'bin\java.exe')) { Write-Host "JDK already installed: $jdkDirectory"; return }
New-Item -ItemType Directory -Path $toolchainRoot -Force | Out-Null
$archive = Join-Path $toolchainRoot 'temurin-21.0.12.1-windows-x64.zip'
$checksum = 'f9d6e191ab098c0d416e7d588a24420a8621cd2f4720dab2459b8b7b2d2d8b4e'
if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $checksum) {
  Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip' -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $checksum) { throw 'JDK checksum does not match publisher metadata; installation stopped.' }
Expand-Archive -LiteralPath $archive -DestinationPath $toolchainRoot
if (-not (Test-Path -LiteralPath (Join-Path $jdkDirectory 'bin\java.exe'))) { throw 'JDK extraction did not produce the expected compiler.' }
Write-Host "Installed build JDK: $jdkDirectory"
