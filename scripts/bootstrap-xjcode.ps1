param(
 [string]$BundleRoot=$PSScriptRoot,
 [string]$BaseUrl='https://xjcode.wenshanzhike.com/releases/0.1.5/',
 [string]$InstallRoot=(Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'XJCode'),
 [switch]$NoPath,
 [switch]$UsePrivateNode
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
if(-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64'){throw 'XJCode requires Windows x64.'}
$InstallRoot=[IO.Path]::GetFullPath($InstallRoot).TrimEnd([char]92)
if($InstallRoot -eq ([IO.Path]::GetPathRoot($InstallRoot)).TrimEnd([char]92) -or $InstallRoot -eq $env:USERPROFILE -or $InstallRoot -eq $env:LOCALAPPDATA){throw 'Unsafe installation root.'}
function NoLinks($p){
 while($p){
  if((Test-Path -LiteralPath $p) -and ((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Installation paths cannot contain links.'}
  $next=Split-Path -Parent $p;if($next -eq $p){break};$p=$next
 }
}
NoLinks $InstallRoot
if(($InstallRoot -split '[\\/]') -contains '.codex'){throw 'Cannot install into a Codex directory.'}
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
$operationLock=[IO.File]::Open((Join-Path $InstallRoot '.maintenance.lock'),'OpenOrCreate','ReadWrite','None')
$stateFile=Join-Path $InstallRoot 'installation.json'
$oldState=$null
if(Test-Path -LiteralPath $stateFile){$oldState=Get-Content -Raw -Encoding UTF8 -LiteralPath $stateFile | ConvertFrom-Json;if($oldState.Product -ne 'XJCode' -or $oldState.Schema -ne 1 -or [IO.Path]::GetFullPath($oldState.Root) -ne $InstallRoot){$operationLock.Dispose();throw 'Invalid installation ownership record.'}}
foreach($name in @('apps','runtime','commands','maintenance')){NoLinks (Join-Path $InstallRoot $name)}
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
 $manifest=Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
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
 $maintenance=Join-Path $InstallRoot 'maintenance'
 New-Item -ItemType Directory -Force -Path $maintenance | Out-Null
 Copy-Item -LiteralPath (Join-Path $prefix 'node_modules\xjcode\native\manage-xjcode.ps1') -Destination $maintenance -Force
 Copy-Item -LiteralPath (Join-Path $prefix 'node_modules\xjcode\native\XJCode-Uninstall.exe') -Destination (Join-Path $InstallRoot 'XJCode-Uninstall.exe') -Force
 $pathBefore=@([Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object {$_})
 $pathAdded=((-not $NoPath) -and $commands -notin $pathBefore) -or ($oldState -and $oldState.PathAdded)
 $owned=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
 $ownedDirs=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
 if($oldState){foreach($name in $oldState.Directories){if($name){$null=$ownedDirs.Add([string]$name)}}}
 if($oldState){foreach($name in $oldState.Files){$null=$owned.Add([string]$name)}}
 function AddOwnedTree($dir){
  $queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($dir)
  while($queue.Count){$current=$queue.Dequeue();NoLinks $current;$null=$ownedDirs.Add($current.Substring($InstallRoot.Length+1));foreach($file in Get-ChildItem -LiteralPath $current -Force){
   if($file.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Owned files cannot contain links.'}
   if($file.PSIsContainer){$queue.Enqueue($file.FullName)}else{$null=$owned.Add($file.FullName.Substring($InstallRoot.Length+1))}
  }}
 }
 foreach($legacy in Get-ChildItem -LiteralPath (Join-Path $InstallRoot 'apps') -Directory){
  if($legacy.Name -notmatch '^\d+\.\d+\.\d+-[a-f0-9]{12}$'){continue}
  NoLinks $legacy.FullName
  $metaFile=Join-Path $legacy.FullName 'node_modules\xjcode\package.json'
  if(Test-Path -LiteralPath $metaFile){$meta=Get-Content -Raw -Encoding UTF8 -LiteralPath $metaFile | ConvertFrom-Json;if($meta.name -eq 'xjcode'){AddOwnedTree $legacy.FullName}}
 }
 AddOwnedTree $maintenance
 if($node.StartsWith($InstallRoot+'\',[StringComparison]::OrdinalIgnoreCase)){AddOwnedTree (Split-Path $node)}
 foreach($name in @('commands\xjcode.cmd','XJCode-Uninstall.exe','installation.json')){$null=$owned.Add($name)}
 foreach($name in @('commands\node.cmd','commands\npm.cmd')){
  $shim=Join-Path $InstallRoot $name
  if((-not $hadNode) -or ((Test-Path -LiteralPath $shim) -and (Get-Content -Raw -Encoding UTF8 -LiteralPath $shim).Contains('@echo off'))){$null=$owned.Add($name)}
 }
 $sha=[Security.Cryptography.SHA256]::Create()
 try{$keyHash=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($InstallRoot.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,12)}finally{$sha.Dispose()}
 $state=@{Schema=1;Product='XJCode';Root=$InstallRoot;Version=$manifest.version;Files=@($owned);Directories=@($ownedDirs);PathAdded=[bool]$pathAdded;NoPath=[bool]$NoPath;PrivateNode=$node.StartsWith($InstallRoot+'\',[StringComparison]::OrdinalIgnoreCase);Node=$node;Npm=$npm;RegistryKey=('XJCode-'+$keyHash)}
 $state | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath ($stateFile+'.new') -Encoding UTF8
 Move-Item -LiteralPath ($stateFile+'.new') -Destination $stateFile -Force
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
 $key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$state.RegistryKey
 New-Item -Path $key -Force | Out-Null
 foreach($item in @{DisplayName='XJCode';DisplayVersion=$manifest.version;Publisher='XJCode';InstallLocation=$InstallRoot;UninstallString=('"'+(Join-Path $InstallRoot 'XJCode-Uninstall.exe')+'"');DisplayIcon=(Join-Path $InstallRoot 'XJCode-Uninstall.exe')}.GetEnumerator()){New-ItemProperty -LiteralPath $key -Name $item.Key -Value $item.Value -PropertyType String -Force | Out-Null}
 Write-Output 'Installation complete. Open a new PowerShell and run: xjcode login'
 Write-Output 'Your Codex installation, credentials and npm prefix are unchanged.'
} finally {
 $env:XJCODE_ARCHIVE_PATH=$oldArchive
 if($operationLock){$operationLock.Dispose();Remove-Item -LiteralPath (Join-Path $InstallRoot '.maintenance.lock') -Force -ErrorAction SilentlyContinue}
 $resolved=[IO.Path]::GetFullPath($stage)
 if($resolved.StartsWith($InstallRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){
  Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
 }
}
