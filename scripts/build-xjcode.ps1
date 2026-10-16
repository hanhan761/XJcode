param(
    [ValidateSet('dev', 'release')][string]$Profile = 'release',
    [int]$Jobs = 4,
    [switch]$SkipTests
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) { throw 'Install Rust (MSVC) and Rust toolchain 1.95.0 first.' }
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Run from Developer PowerShell for VS 2022 with the C++ workload installed.' }
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) { throw 'Visual Studio C++ tools were not found.' }
    Import-Module (Join-Path $vs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}
function Invoke-Cargo([string[]]$CargoArgs) {
    & cargo.exe +1.95.0 @CargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo failed ($LASTEXITCODE)" }
}
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_DEV_INCREMENTAL = 'false'
$engineTarget = Join-Path $repo '.build\engine'
$nativeTarget = Join-Path $repo '.build\target'
$buildArgs = @('build', '--locked', '-j', "$Jobs")
if ($Profile -eq 'release') { $buildArgs += '--release' }
$env:CARGO_TARGET_DIR = $engineTarget
Push-Location (Join-Path $repo 'codex-rs')
try {
    Invoke-Cargo ($buildArgs + @('-p', 'codex-cli', '--bin', 'codex'))
    Invoke-Cargo ($buildArgs + @('-p', 'codex-windows-sandbox', '--bins'))
    if (-not $SkipTests) { Invoke-Cargo @('test', '--locked', '-p', 'codex-core', '--lib', 'xjtu::tests', '-j', "$Jobs") }
} finally { Pop-Location }
$env:CARGO_TARGET_DIR = $nativeTarget
Push-Location (Join-Path $repo 'xjcode-native')
try {
    Invoke-Cargo ($buildArgs + @('--features', 'login', '--bins'))
    if (-not $SkipTests) {
        Invoke-Cargo @('test', '--locked', '--lib')
        Invoke-Cargo @('test', '--locked', '--features', 'login', '--bin', 'xjcode-login')
        & node.exe --test tests/login.test.cjs
        if ($LASTEXITCODE -ne 0) { throw 'Browser login protocol tests failed.' }
    }
} finally { Pop-Location }
$artifactProfile = if ($Profile -eq 'dev') { 'debug' } else { 'release' }
$package = Join-Path $repo "dist\xjcode-0.1.0-windows-x64-$Profile"
New-Item -ItemType Directory -Force -Path $package | Out-Null
Copy-Item -LiteralPath (Join-Path $engineTarget "$artifactProfile\codex.exe") -Destination (Join-Path $package 'xjcode-engine.exe') -Force
foreach ($binary in @('codex-windows-sandbox-setup.exe', 'codex-command-runner.exe', 'codex-windows-managed-deny-probe.exe')) {
    Copy-Item -LiteralPath (Join-Path $engineTarget "$artifactProfile\$binary") -Destination $package -Force
}
foreach ($binary in @('xjcode.exe', 'xjcode-login.exe')) {
    Copy-Item -LiteralPath (Join-Path $nativeTarget "$artifactProfile\$binary") -Destination $package -Force
}
foreach ($dll in Get-ChildItem -LiteralPath (Join-Path $nativeTarget $artifactProfile) -Filter '*.dll') { Copy-Item -LiteralPath $dll.FullName -Destination $package -Force }
foreach ($doc in @('LICENSE', 'NOTICE', 'README-XJCODE.md')) { Copy-Item -LiteralPath (Join-Path $repo $doc) -Destination $package -Force }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-xjcode.ps1') -Destination (Join-Path $package 'install.ps1') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Install-XJCode.cmd') -Destination $package -Force
$hashes = Get-ChildItem -LiteralPath $package -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name }
[IO.File]::WriteAllLines((Join-Path $package 'SHA256SUMS.txt'), [string[]]$hashes)
for ($attempt = 1; $attempt -le 3; $attempt++) {
    try {
        Compress-Archive -Path (Join-Path $package '*') -DestinationPath "$package.zip" -Force -ErrorAction Stop
        break
    } catch {
        if ($attempt -eq 3) { throw }
        Start-Sleep -Seconds 2
    }
}
Write-Host "Package: $package.zip"
