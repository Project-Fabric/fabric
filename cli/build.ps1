$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$goTool = Get-Command go -ErrorAction SilentlyContinue
if ($goTool) { $goBinary = $goTool.Source }
elseif (Test-Path -LiteralPath 'C:\Program Files\Go\bin\go.exe') { $goBinary = 'C:\Program Files\Go\bin\go.exe' }
else { throw 'Go 1.24 or newer is required. Install from https://go.dev/doc/install, then reopen PowerShell.' }
$env:GOCACHE = Join-Path $PSScriptRoot '.cache\build'
$env:GOMODCACHE = Join-Path $PSScriptRoot '.cache\modules'
$env:GOTELEMETRY = 'off'
& $goBinary mod tidy
if ($LASTEXITCODE -ne 0) { throw 'Could not download Go dependencies. Run this script in your own terminal with network access.' }
& $goBinary fmt ./...
if ($LASTEXITCODE -ne 0) { throw 'Go formatting failed.' }
& $goBinary vet ./...
if ($LASTEXITCODE -ne 0) { throw 'Go vet failed.' }
& $goBinary test ./...
if ($LASTEXITCODE -ne 0) { throw 'Go tests failed.' }
New-Item -ItemType Directory -Force bin | Out-Null
& $goBinary build -trimpath -o bin\cosigna-console.exe ./cmd/cosigna-console
if ($LASTEXITCODE -ne 0) { throw 'Go build failed.' }
Write-Host 'Built bin\cosigna-console.exe. Start with: .\bin\cosigna-console.exe --fixtures'
