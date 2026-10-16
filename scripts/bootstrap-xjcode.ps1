param(
 [string]$BundleRoot=$PSScriptRoot,
 [string]$BaseUrl='https://xjcode.wenshanzhike.com/releases/0.1.3/',
 [string]$InstallRoot=(Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'XJCode'),
 [switch]$NoPath,
 [switch]$UsePrivateNode
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
if(-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64'){throw 'XJCode requires Windows x64.'}
$InstallRoot=[IO.Path]::GetFullPath($InstallRoot)
if(($InstallRoot -split '[\\/]') -contains '.codex'){throw 'Cannot install into a Codex directory.'}
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
$stage=Join-Path $InstallRoot ('staging-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
if([string]::IsNullOrWhiteSpace($BundleRoot)){$BundleRoot=$stage}
$oldArchive=$env:XJCODE_ARCHIVE_PATH
function Get-Artifact($Name,$Hash) {
 if($Name -notmatch '^[a-zA-Z0-9._-]+$' -or $Name -in @('.','..')){throw 'Invalid artifact name.'}
 $local=Join-Path $BundleRoot $Name
 if(Test-Path -LiteralPath $local){$file=$local}
 else {
  $file=Join-Path $stage $Name
  $uri=[uri]::new([uri]$BaseUrl,$Name)
  if($uri.Scheme -ne 'https'){throw 'HTTPS is required.'}
  Invoke-WebRequest -UseBasicParsing -Uri $uri.AbsoluteUri -OutFile $file -TimeoutSec 300
 }
 if((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $Hash){throw "Checksum mismatch: $Name"}
 return $file
}
function Expand-VerifiedZip($Archive,$To) {
 Add-Type -AssemblyName System.IO.Compression.FileSystem
 $root=[IO.Path]::GetFullPath($To).TrimEnd('\')+'\'
 $zip=[IO.Compression.ZipFile]::OpenRead($Archive)
 try {
  foreach($entry in $zip.Entries){
   $target=[IO.Path]::GetFullPath((Join-Path $root $entry.FullName))
   if(-not $target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'Unsafe archive path'}
  }
 } finally {$zip.Dispose()}
 [IO.Compression.ZipFile]::ExtractToDirectory($Archive,$To)
}
try {
 Write-Output '[1/4] Checking installation files...'
 $manifestPath=Join-Path $BundleRoot 'release-manifest.json'
 if(-not (Test-Path -LiteralPath $manifestPath)){
  $manifestPath=Join-Path $stage 'release-manifest.json'
  if(([uri]$BaseUrl).Scheme -ne 'https'){throw 'HTTPS is required.'}
  Invoke-WebRequest -UseBasicParsing -Uri ([uri]::new([uri]$BaseUrl,'release-manifest.json')).AbsoluteUri -OutFile $manifestPath -TimeoutSec 60
 }
 $manifest=Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
 if($manifest.version -notmatch '^\d+\.\d+\.\d+$' -or $manifest.node.version -notmatch '^v\d+\.\d+\.\d+$'){throw 'Invalid release manifest'}
 $package=Get-Artifact $manifest.npm.file $manifest.npm.sha256
 $archive=$null
 if(-not $manifest.npm.bundledNative){$archive=Get-Artifact $manifest.native.file $manifest.native.sha256}
 Write-Output '[2/4] Preparing Node.js...'
 $node=$null;$npm=$null
 if(-not $UsePrivateNode){
  $candidate=Get-Command node.exe -ErrorAction SilentlyContinue
  if($candidate){
   $version=& $candidate.Source --version
   $npmCandidate=Join-Path (Split-Path $candidate.Source) 'node_modules\npm\bin\npm-cli.js'
   if($LASTEXITCODE -eq 0 -and $version -match '^v(\d+)\.' -and [int]$Matches[1] -ge 22 -and (Test-Path -LiteralPath $npmCandidate)){
    $node=$candidate.Source;$npm=$npmCandidate
   }
  }
 }
 $hadNode=[bool](Get-Command node.exe -ErrorAction SilentlyContinue)
 if(-not $node){
  $nodeDir=Join-Path $InstallRoot ('runtime\node-'+$manifest.node.version+'-win-x64')
  $node=Join-Path $nodeDir 'node.exe'
  $npm=Join-Path $nodeDir 'node_modules\npm\bin\npm-cli.js'
  if(-not (Test-Path -LiteralPath $node) -or -not (Test-Path -LiteralPath $npm)){
   $nodeZip=Get-Artifact $manifest.node.file $manifest.node.sha256
   $runtimeStage=Join-Path $stage 'runtime'
   Expand-VerifiedZip $nodeZip $runtimeStage
   New-Item -ItemType Directory -Force -Path (Split-Path $nodeDir) | Out-Null
   if(Test-Path -LiteralPath $nodeDir){throw 'Existing private Node runtime is incomplete. No files were overwritten.'}
   Move-Item -LiteralPath (Join-Path $runtimeStage ('node-'+$manifest.node.version+'-win-x64')) -Destination $nodeDir
  }
  Write-Output 'Using the private XJCode Node.js runtime.'
 } else {Write-Output 'Using compatible existing Node.js; its installation and npm configuration stay unchanged.'}
 $env:Path=(Split-Path $node)+';'+$env:Path
 Write-Output '[3/4] Installing the XJCode npm package...'
 $releaseId=$manifest.version+'-'+$manifest.npm.sha256.Substring(0,12)
 $prefix=Join-Path $InstallRoot ('apps\'+$releaseId)
 $entry=Join-Path $prefix 'node_modules\xjcode\bin\xjcode.cjs'
 if(-not (Test-Path -LiteralPath $entry)){
  $prefixStage=Join-Path $stage 'app'
  $env:XJCODE_ARCHIVE_PATH=$archive
  & $node $npm install --global --prefix $prefixStage --offline --no-audit --no-fund --ignore-scripts=false $package
  if($LASTEXITCODE -ne 0){throw 'npm installation failed.'}
  & $node (Join-Path $prefixStage 'node_modules\xjcode\bin\xjcode.cjs') --version
  if($LASTEXITCODE -ne 0){throw 'XJCode version check failed.'}
  New-Item -ItemType Directory -Force -Path (Split-Path $prefix) | Out-Null
  Move-Item -LiteralPath $prefixStage -Destination $prefix
 }
 & $node $entry --version
 if($LASTEXITCODE -ne 0){throw 'Installed XJCode verification failed.'}
 Write-Output '[4/4] Registering the xjcode command...'
 $commands=Join-Path $InstallRoot 'commands'
 New-Item -ItemType Directory -Force -Path $commands | Out-Null
 $newline=[Environment]::NewLine
 $command='@echo off'+$newline+'"'+$node+'" "'+$entry+'" %*'+$newline
 $candidate=Join-Path $commands 'xjcode.cmd.new'
 [IO.File]::WriteAllText($candidate,$command,[Text.Encoding]::Default)
 Move-Item -LiteralPath $candidate -Destination (Join-Path $commands 'xjcode.cmd') -Force
 if(-not $hadNode){
  [IO.File]::WriteAllText((Join-Path $commands 'node.cmd'),'@echo off'+$newline+'"'+$node+'" %*'+$newline,[Text.Encoding]::Default)
  [IO.File]::WriteAllText((Join-Path $commands 'npm.cmd'),'@echo off'+$newline+'"'+$node+'" "'+$npm+'" %*'+$newline,[Text.Encoding]::Default)
 }
 if(-not $NoPath){
  $entries=@([Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object {$_ -and $_ -ne $commands})
  [Environment]::SetEnvironmentVariable('Path',((@($commands)+$entries)-join ';'),'User')
  $env:Path=$commands+';'+$env:Path
 }
 Write-Output 'Installation complete. Open a new PowerShell and run: xjcode login'
 Write-Output 'Your Codex installation, credentials and npm prefix are unchanged.'
} finally {
 $env:XJCODE_ARCHIVE_PATH=$oldArchive
 $resolved=[IO.Path]::GetFullPath($stage)
 if($resolved.StartsWith($InstallRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){
  Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
 }
}
