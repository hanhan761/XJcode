param(
    [string]$InstallRoot = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'XJCode')
)

$ErrorActionPreference = 'Stop'
$InstallRoot = [IO.Path]::GetFullPath($InstallRoot)
$commands = Join-Path $InstallRoot 'commands'
$entry = Join-Path $commands 'xjcode.cmd'
if (-not (Test-Path -LiteralPath $entry -PathType Leaf)) {
    throw 'The managed XJCode command is missing. Install XJCode first.'
}

# Existing terminals can retain a PATH from before XJCode was installed.
# Forward only XJCode's command files in the existing per-user npm bin folder;
# do not change npm settings, other commands, or PowerShell profiles.
$compatibilityRoot = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'npm'))
if ($commands.TrimEnd('\') -eq $compatibilityRoot.TrimEnd('\')) {
    throw 'The command forwarding directory must differ from the managed entry.'
}
New-Item -ItemType Directory -Path $compatibilityRoot -Force | Out-Null
$newline = [Environment]::NewLine
$quotedEntry = $entry.Replace("'", "''")
$forwarders = @{
    'xjcode.cmd' = (@(
        '@echo off'
        'rem XJCode managed command forwarder'
        ('call "' + $entry + '" %*')
        'exit /b %ERRORLEVEL%'
    ) -join $newline) + $newline
    'xjcode.ps1' = (@(
        '# XJCode managed command forwarder'
        ("& '" + $quotedEntry + "' @args")
        'exit $LASTEXITCODE'
    ) -join $newline) + $newline
}

foreach ($name in @('xjcode.cmd', 'xjcode.ps1')) {
    $destination = Join-Path $compatibilityRoot $name
    if (Test-Path -LiteralPath $destination) {
        $existing = [IO.File]::ReadAllText($destination)
        if ($existing -eq $forwarders[$name]) { continue }
        $backupRoot = Join-Path $InstallRoot 'command-backups'
        New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
        $digest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        $backup = Join-Path $backupRoot ($name + '.' + $digest + '.bak')
        if (-not (Test-Path -LiteralPath $backup)) {
            Copy-Item -LiteralPath $destination -Destination $backup
        }
    }
    $encoding = if ($name.EndsWith('.ps1')) {
        [Text.UTF8Encoding]::new($true)
    } else {
        [Text.Encoding]::Default
    }
    $pending = Join-Path $compatibilityRoot ($name + '.xjcode-new')
    [IO.File]::WriteAllText($pending, $forwarders[$name], $encoding)
    Move-Item -LiteralPath $pending -Destination $destination -Force
}

$userEntries = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object {
    $_ -and $_.Trim().Trim('"').TrimEnd('\') -ne $commands.TrimEnd('\')
})
$userPath = (@($commands) + $userEntries) -join ';'
if ($userPath -ne [Environment]::GetEnvironmentVariable('Path', 'User')) {
    [Environment]::SetEnvironmentVariable('Path', $userPath, 'User')
}
$processEntries = @($env:Path -split ';' | Where-Object {
    $_ -and $_.Trim().Trim('"').TrimEnd('\') -ne $commands.TrimEnd('\')
})
$env:Path = (@($commands) + $processEntries) -join ';'

[pscustomobject]@{
    ManagedCommand = $entry
    PowerShellForwarder = Join-Path $compatibilityRoot 'xjcode.ps1'
    CmdForwarder = Join-Path $compatibilityRoot 'xjcode.cmd'
    CommandSource = (Get-Command XJcode -ErrorAction Stop).Source
}
