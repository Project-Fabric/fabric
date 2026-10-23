param([string]$WifiAddress)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $WifiAddress) {
  $addresses = @(Get-NetIPConfiguration | Where-Object IPv4DefaultGateway | ForEach-Object { $_.IPv4Address.IPAddress } | Where-Object { $_ -match '^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)' })
  if ($addresses.Count -ne 1) { throw 'Pass -WifiAddress with this computer''s private Wi-Fi IPv4 address.' }
  $WifiAddress = $addresses[0]
}
if ($WifiAddress -notmatch '^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)\d') { throw 'Use a private Wi-Fi IPv4 address.' }
if (Get-NetTCPConnection -State Listen -LocalPort 8081,3099 -ErrorAction SilentlyContinue) { throw 'Expo or a Wi-Fi relay is already running. Stop it in its own terminal before starting a new Wi-Fi session.' }
$oldGroqKey = $env:COSIGNA_GROQ_API_KEY
try {
  if (-not $env:COSIGNA_GROQ_API_KEY -and -not (Get-NetTCPConnection -State Listen -LocalPort 3097 -ErrorAction SilentlyContinue)) {
    Write-Host 'Voice and natural-language planning need the server Groq key. It is not sent to Expo or the phone.'
    $env:COSIGNA_GROQ_API_KEY = [System.Net.NetworkCredential]::new('', (Read-Host 'Groq API key (hidden)' -AsSecureString)).Password
    if (-not $env:COSIGNA_GROQ_API_KEY) { throw 'No Groq key entered. Nothing was started.' }
  }
  & node (Join-Path $projectRoot 'testbeds\voice-merchant\wifi-runtime.mjs') --host $WifiAddress
  if ($LASTEXITCODE -ne 0) { throw 'Wi-Fi test stopped. Read the status above; keep the existing transaction data.' }
} finally { $env:COSIGNA_GROQ_API_KEY = $oldGroqKey }
