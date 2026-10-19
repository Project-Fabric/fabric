# Installs the latest `fabric` release for Windows into %LOCALAPPDATA%\Programs\fabric
# (or $env:FABRIC_INSTALL_DIR) and adds it to your user PATH.
#
#   irm https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.ps1 | iex
#
# Set $env:FABRIC_VERSION = "v0.1.0" to pick a version. The archive's SHA-256 checksum is
# verified before anything is installed.
$ErrorActionPreference = "Stop"

$repo = "Project-Fabric/fabric"
$target = "x86_64-pc-windows-msvc"
$dir = if ($env:FABRIC_INSTALL_DIR) { $env:FABRIC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\fabric" }

$version = $env:FABRIC_VERSION
if (-not $version) {
    $version = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
}

$name = "fabric-$version-$target"
$url = "https://github.com/$repo/releases/download/$version/$name.zip"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
    Write-Host "Downloading fabric $version for $target"
    Invoke-WebRequest $url -OutFile "$tmp\$name.zip" -UseBasicParsing
    Invoke-WebRequest "$url.sha256" -OutFile "$tmp\$name.zip.sha256" -UseBasicParsing
    $expected = ((Get-Content "$tmp\$name.zip.sha256" -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash "$tmp\$name.zip" -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) { throw "checksum verification failed; nothing was installed" }

    Expand-Archive "$tmp\$name.zip" -DestinationPath $tmp
    New-Item -ItemType Directory -Force $dir | Out-Null
    Copy-Item "$tmp\$name\fabric.exe" $dir -Force
    Write-Host "Installed $dir\fabric.exe"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($userPath -split ';') -notcontains $dir) {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$dir", "User")
        Write-Host "Added $dir to your PATH; open a new terminal to use 'fabric'"
    }
    Write-Host "Next: cd into a project and run 'fabric init'"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
