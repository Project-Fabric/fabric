param(
  [ValidateSet('Prepare','Run','Verify','Review','Offline','Agent-Smoke')][string]$Stage = 'Offline',
  [string]$RunDir,
  [string]$Config = (Join-Path $PSScriptRoot 'config.example.json'),
  [string]$Model
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$originalStripe = $env:COSIGNA_STRIPE_TEST_SECRET_KEY
$originalCases = $env:COSIGNA_CASES_SECRET
Push-Location $projectRoot
try {
  $env:COSIGNA_STRIPE_TEST_SECRET_KEY = $null
  $env:COSIGNA_CASES_SECRET = $null
  # Build before reading credentials; compiler/dependency processes never receive them.
  & cargo build -p cosigna-agent --locked
  if ($LASTEXITCODE -ne 0) { throw 'Gateway build failed.' }
  $argsList = @('-I', (Join-Path $PSScriptRoot 'run.py'), $Stage.ToLowerInvariant(), '--config', $Config)
  if ($RunDir) { $argsList += @('--run-dir', $RunDir) }
  if ($Model) { $argsList += @('--model', $Model) }
  if ($Stage -in @('Offline','Agent-Smoke')) {
    if ($Stage -eq 'Agent-Smoke') { $argsList += '--execute-agent' }
    & python @argsList
    if ($LASTEXITCODE -ne 0) { throw 'Offline wiring check failed. Read the report.' }
    return
  }
  if ($Stage -ne 'Prepare' -and -not $RunDir) {
    $latest = Join-Path $env:LOCALAPPDATA 'Cosigna\agent-refund-demo\latest.txt'
    if (-not (Test-Path -LiteralPath $latest)) { throw 'Run Prepare first or pass -RunDir.' }
    $RunDir = (Get-Content -LiteralPath $latest -Raw -Encoding UTF8).Trim()
    $argsList += @('--run-dir', $RunDir)
  }
  if ($RunDir) { Write-Host "Using prepared run: $RunDir" }
  try {
    $stripeInput = Read-Host 'Stripe TEST secret key (hidden)' -AsSecureString
    $env:COSIGNA_STRIPE_TEST_SECRET_KEY = [System.Net.NetworkCredential]::new('', $stripeInput).Password
    if ($env:COSIGNA_STRIPE_TEST_SECRET_KEY -notmatch '^sk_test_[A-Za-z0-9_]+$') { throw 'A Stripe test secret key is required; live keys are refused.' }
    if ($Stage -ne 'Prepare') {
      $settings = Get-Content -LiteralPath (Join-Path $RunDir 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
      $subdomain = $settings.config.zendesk_subdomain
      if ($subdomain -notmatch '^[a-z0-9][a-z0-9-]{0,62}$') { throw 'Invalid Zendesk subdomain.' }
      $clientInput = Read-Host 'Zendesk OAuth client secret (hidden; requests tickets:read only)' -AsSecureString
      $clientSecret = [System.Net.NetworkCredential]::new('', $clientInput).Password
      $body = @{ grant_type='client_credentials'; client_id=$settings.config.oauth_client_id; client_secret=$clientSecret; scope='tickets:read' } | ConvertTo-Json
      try {
        $tokenResponse = Invoke-RestMethod -Method Post -Uri "https://$subdomain.zendesk.com/oauth/tokens" -ContentType 'application/json' -Body $body -MaximumRedirection 0
      } catch { throw 'Zendesk refused the read-only token request. Check the OAuth client privately; no response body is printed.' }
      if (-not $tokenResponse.access_token) { throw 'Zendesk returned no OAuth token.' }
      if ($tokenResponse.scope -and $tokenResponse.scope -ne 'tickets:read') { throw 'Zendesk returned an unexpected scope.' }
      $env:COSIGNA_CASES_SECRET = $tokenResponse.access_token
      $clientSecret = $null; $body = $null; $tokenResponse = $null
    }
    if ($Stage -eq 'Run') { $argsList += '--execute-agent' }
    & python @argsList
    if ($LASTEXITCODE -ne 0) { throw 'Demo stopped or a check failed. Use Verify before retrying any agent.' }
  } finally {
    $env:COSIGNA_STRIPE_TEST_SECRET_KEY = $originalStripe
    $env:COSIGNA_CASES_SECRET = $originalCases
    $clientSecret = $null; $body = $null; $tokenResponse = $null
    $stripeInput = $null; $clientInput = $null
  }
} finally {
  $env:COSIGNA_STRIPE_TEST_SECRET_KEY = $originalStripe
  $env:COSIGNA_CASES_SECRET = $originalCases
  Pop-Location
}
