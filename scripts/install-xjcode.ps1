param(
    [string]$Destination = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'XJCode\bin'),
    [switch]$NoPath
)
$ErrorActionPreference = 'Stop'
$source = $PSScriptRoot
$manifest = Join-Path $source 'SHA256SUMS.txt'
if (-not (Test-Path -LiteralPath $manifest)) { throw 'Missing package checksums. Extract the full XJCode archive first.' }
$files = @()
foreach ($line in Get-Content -LiteralPath $manifest) {
    if ($line -notmatch '^([0-9a-fA-F]{64})  ([^\\/:]+)$') { throw 'Invalid checksum manifest.' }
    $expected = $Matches[1]
    $name = $Matches[2]
    $file = Join-Path $source $name
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $expected) { throw "Checksum mismatch: $name" }
    $files += $file
}
foreach ($required in @('xjcode.exe', 'xjcode-engine.exe', 'xjcode-login.exe', 'codex-command-runner.exe', 'codex-windows-sandbox-setup.exe')) {
    if ($required -notin ($files | ForEach-Object { Split-Path -Leaf $_ })) { throw "Incomplete package: $required" }
}
$Destination = [IO.Path]::GetFullPath($Destination)
if ($Destination -eq [IO.Path]::GetFullPath($source)) { throw 'Install to a directory different from the extracted package.' }
if (($Destination -split '[\\/]') -contains '.codex' -or (Test-Path -LiteralPath (Join-Path $Destination 'codex.exe'))) {
    throw 'Choose a separate XJCode directory. Installing into a Codex directory is not allowed.'
}
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
foreach ($file in $files) { Copy-Item -LiteralPath $file -Destination $Destination -Force }
Copy-Item -LiteralPath $manifest -Destination $Destination -Force
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @($userPath -split ';' | Where-Object { $_ })
    if ($Destination -notin $entries) { [Environment]::SetEnvironmentVariable('Path', (($entries + $Destination) -join ';'), 'User') }
    if ($Destination -notin ($env:Path -split ';')) { $env:Path = "$Destination;$env:Path" }
}
& (Join-Path $Destination 'xjcode.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Installed executable failed its version check.' }
Write-Host "Installed: $Destination"
Write-Host 'Run xjcode login. Other terminals may need to be reopened for PATH changes.'
