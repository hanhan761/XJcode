param([string]$JobFile,[string]$Root,[int]$WaitPid=0,[switch]$Gui)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
# Native invocation must work even when the parent application supplies a restricted PATHEXT.
$env:PATHEXT='.COM;.EXE;.BAT;.CMD'
$env:ComSpec=Join-Path ([Environment]::GetFolderPath('System')) 'cmd.exe'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
function Full($p){[IO.Path]::GetFullPath($p).TrimEnd([char]92)}
function NoLinks($p){
 $p=Full $p
 while($p){
  if(Test-Path -LiteralPath $p){if((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing a reparse point.'}}
  $parent=Split-Path -Parent $p
  if($parent -eq $p){break};$p=$parent
 }
}
function SafeRoot($p){
 $p=Full $p
 if($p -eq (Full ([IO.Path]::GetPathRoot($p))) -or $p -eq (Full $env:USERPROFILE) -or $p -eq (Full ([Environment]::GetFolderPath('LocalApplicationData'))) -or ($p -split '[\\/]') -contains '.codex'){throw 'Unsafe installation root.'}
 NoLinks $p;return $p
}
function OwnedPath($root,$relative){
 if([IO.Path]::IsPathRooted($relative) -or ($relative -split '[\\/]') -contains '..' -or $relative.Contains(':')){throw 'Invalid ownership entry.'}
 $p=Full (Join-Path $root $relative)
 if(-not $p.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Ownership entry escapes root.'}
 NoLinks $p;return $p
}
function ReadState($root){
 $state=Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $root 'installation.json') | ConvertFrom-Json
 if($state.Schema -ne 1 -or $state.Product -ne 'XJCode' -or (Full $state.Root) -ne $root -or $state.RegistryKey -notmatch '^XJCode-[a-f0-9]{12}$'){throw 'Invalid installation ownership record.'}
 foreach($file in @($state.Files)+@($state.Directories)){if($file){$null=OwnedPath $root $file}}
 return $state
}
function CheckTree($root){
 $queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($root)
 while($queue.Count){$dir=$queue.Dequeue();NoLinks $dir;foreach($item in Get-ChildItem -LiteralPath $dir -Force){if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing data directory containing links.'};if($item.PSIsContainer){$queue.Enqueue($item.FullName)}}}
}
$lock=$null;$log=$null;$ok=$false
try {
 if($JobFile){
  $job=Get-Content -Raw -Encoding UTF8 -LiteralPath $JobFile | ConvertFrom-Json
  $Root=$job.Root;$Gui=[bool]$job.Gui
 } else {
  $job=[pscustomobject]@{Action='Uninstall';Kind='managed';Root=$Root;WaitPids=@($WaitPid);Purge=$false;Home=(Join-Path $env:USERPROFILE '.xjcode');Gui=[bool]$Gui}
 }
 $Root=SafeRoot $Root
 $log=Join-Path ([IO.Path]::GetTempPath()) ('xjcode-maintenance-'+[guid]::NewGuid().ToString('N')+'.log')
 Start-Transcript -Path $log | Out-Null
 foreach($id in $job.WaitPids){if($id -gt 0 -and $id -ne $PID){$p=Get-Process -Id $id -ErrorAction SilentlyContinue;if($p -and -not $p.WaitForExit(60000)){throw 'Close the launching XJCode process and retry.'}}}
 if($job.Kind -eq 'npm'){
  $package=Join-Path $Root 'node_modules\xjcode'
  NoLinks $package
  $meta=Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $package 'package.json') | ConvertFrom-Json
  if($meta.name -ne 'xjcode'){throw 'Not an XJCode npm installation.'}
 }else{$state=ReadState $Root}
 if($Gui){
  Add-Type -AssemblyName System.Windows.Forms
  $form=[Windows.Forms.Form]::new();$form.Text='XJCode · 卸载';$form.Width=520;$form.Height=240;$form.StartPosition='CenterScreen'
  $label=[Windows.Forms.Label]::new();$label.Text='确认卸载 XJCode？默认保留个人登录、配置和历史会话。';$label.SetBounds(20,20,460,55)
  $purge=[Windows.Forms.CheckBox]::new();$purge.Text='同时清除 XJCode 个人数据（登录、配置和历史会话）';$purge.SetBounds(20,85,460,30)
  $button=[Windows.Forms.Button]::new();$button.Text='卸载';$button.SetBounds(340,135,130,35);$button.DialogResult='OK';$form.AcceptButton=$button
  $form.Controls.AddRange(@($label,$purge,$button));if($form.ShowDialog() -ne 'OK'){exit 0};$job.Purge=$purge.Checked
 }
 if($job.Action -eq 'UpdateManaged'){
  if((Get-FileHash -LiteralPath $job.Bootstrap -Algorithm SHA256).Hash -ne $job.BootstrapHash){throw 'Installer checksum mismatch.'}
  $options=@{BundleRoot=$job.BundleRoot;InstallRoot=$Root;BaseUrl=('https://xjcode.wenshanzhike.com/releases/'+$job.Version+'/')}
  if($job.NoPath){$options.NoPath=$true};if($job.PrivateNode){$options.UsePrivateNode=$true}
  & $job.Bootstrap @options
  $ok=$true
 }else{
 $lockPath=Join-Path $Root '.maintenance.lock'
 $lock=[IO.File]::Open($lockPath,'OpenOrCreate','ReadWrite','None')
 if($job.Action -eq 'Uninstall'){
  $processRoot=if($job.Kind -eq 'npm'){Join-Path $Root 'node_modules\xjcode'}else{$Root}
  $running=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and ($_.ExecutablePath.StartsWith($processRoot+'\',[StringComparison]::OrdinalIgnoreCase) -or $_.ExecutablePath.StartsWith((Join-Path $Root '.xjcode-rollback')+'\',[StringComparison]::OrdinalIgnoreCase))})
  if($running.Count){throw 'Other processes are using this XJCode installation. Close them and retry.'}
  if($job.Purge){
   $dataHome=Full $job.Home
   if($dataHome -eq $Root -or $dataHome -eq (Full $env:USERPROFILE) -or $dataHome -eq (Full ([IO.Path]::GetPathRoot($dataHome))) -or ($dataHome -split '[\\/]') -contains '.codex'){throw 'Unsafe personal-data directory.'}
   $default=Full (Join-Path $env:USERPROFILE '.xjcode')
   if($dataHome -ne $default -and (-not (Test-Path -LiteralPath (Join-Path $dataHome '.xjcode-data-owner')) -or (Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $dataHome '.xjcode-data-owner')) -ne 'XJCode data v1')){throw 'Custom data directory ownership is unknown; data was preserved.'}
   if(Test-Path -LiteralPath $dataHome){CheckTree $dataHome}
  }
  if($job.Kind -eq 'npm'){
   & $job.Node $job.Npm uninstall --global --prefix $Root --no-audit --no-fund xjcode
   if($LASTEXITCODE -ne 0){throw 'npm uninstall failed.'}
   $backup=Join-Path $Root '.xjcode-rollback'
   if(Test-Path -LiteralPath $backup){NoLinks $backup;if((Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $backup '.xjcode-owned-rollback')) -eq 'XJCode rollback v1'){CheckTree $backup;Remove-Item -LiteralPath $backup -Recurse -Force}}

  }else{
   $dirs=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
   foreach($dir in $state.Directories){if($dir){$null=$dirs.Add((OwnedPath $Root $dir))}}
   foreach($file in $state.Files){
    if($file -eq 'installation.json'){continue}
    $target=OwnedPath $Root $file
    if(Test-Path -LiteralPath $target -PathType Leaf){Remove-Item -LiteralPath $target -Force}
    $parent=Split-Path -Parent $target
    while($parent -and $parent -ne $Root -and $parent.StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase)){$null=$dirs.Add($parent);$parent=Split-Path -Parent $parent}
   }
   if($state.PathAdded){
    $commands=Full (Join-Path $Root 'commands')
    $paths=@([Environment]::GetEnvironmentVariable('Path','User') -split ';' | Where-Object {$_ -and $_.Trim().Trim([char]34).TrimEnd([char]92) -ne $commands})
    [Environment]::SetEnvironmentVariable('Path',($paths -join ';'),'User')
   }
   foreach($name in @('xjcode.cmd','xjcode.ps1')){
    $shim=Join-Path ([Environment]::GetFolderPath('ApplicationData')) ('npm\'+$name)
    if(Test-Path -LiteralPath $shim -PathType Leaf){
     NoLinks $shim;$text=Get-Content -Raw -Encoding UTF8 -LiteralPath $shim
     if($text.Contains('XJCode managed command forwarder') -and $text.IndexOf((Join-Path $Root 'commands\xjcode.cmd'),[StringComparison]::OrdinalIgnoreCase) -ge 0){Remove-Item -LiteralPath $shim -Force}
    }
   }
   $key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$state.RegistryKey
   if(Test-Path -LiteralPath $key){Remove-Item -LiteralPath $key -Recurse}
   Remove-Item -LiteralPath (Join-Path $Root 'installation.json') -Force
   foreach($dir in @($dirs) | Sort-Object Length -Descending){if((Test-Path -LiteralPath $dir) -and -not (Get-ChildItem -LiteralPath $dir -Force | Select-Object -First 1)){Remove-Item -LiteralPath $dir -Force}}
  }
  if($job.Purge -and (Test-Path -LiteralPath $dataHome)){Remove-Item -LiteralPath $dataHome -Recurse -Force}
  Write-Output 'XJCode uninstall complete.'
 }elseif($job.Action -eq 'UpdateNpm'){
  if((Get-FileHash -LiteralPath $job.Package -Algorithm SHA256).Hash -ne $job.PackageHash){throw 'Update package checksum mismatch.'}
  $running=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and ($_.ExecutablePath.StartsWith((Join-Path $Root 'node_modules\xjcode')+'\',[StringComparison]::OrdinalIgnoreCase) -or $_.ExecutablePath.StartsWith((Join-Path $Root '.xjcode-rollback')+'\',[StringComparison]::OrdinalIgnoreCase))})
  if($running.Count){throw 'Close other XJCode instances before updating this npm installation.'}
  $stage=Join-Path $Root ('.xjcode-staging-'+[guid]::NewGuid().ToString('N'))
  $backup=Join-Path $Root '.xjcode-rollback'
  NoLinks $backup
  if(Test-Path -LiteralPath $backup){
   if(-not (Test-Path -LiteralPath (Join-Path $backup '.xjcode-owned-rollback')) -or (Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $backup '.xjcode-owned-rollback')) -ne 'XJCode rollback v1'){throw 'Unknown rollback directory; left unchanged.'}
   CheckTree $backup;Remove-Item -LiteralPath $backup -Recurse -Force
  }
  try {
   & $job.Node $job.Npm install --global --prefix $stage --offline --no-audit --no-fund --ignore-scripts $job.Package
   if($LASTEXITCODE -ne 0){throw 'npm staging installation failed.'}
   $candidate=Join-Path $stage 'node_modules\xjcode'
   $candidateVersion=& $job.Node (Join-Path $candidate 'bin\xjcode.cjs') --version
   if($LASTEXITCODE -ne 0 -or $candidateVersion -notmatch ('^xjcode '+[regex]::Escape($job.Version)+'$')){throw 'Staged version verification failed.'}
   CheckTree $package
   Move-Item -LiteralPath $package -Destination $backup
   try {Move-Item -LiteralPath $candidate -Destination $package}catch{Move-Item -LiteralPath $backup -Destination $package;throw}
   [IO.File]::WriteAllText((Join-Path $backup '.xjcode-owned-rollback'),'XJCode rollback v1')
   $actual=& $job.Node (Join-Path $Root 'node_modules\xjcode\bin\xjcode.cjs') --version
   if($LASTEXITCODE -ne 0 -or $actual -notmatch ('^xjcode '+[regex]::Escape($job.Version)+'$')){
    $failed=Join-Path $Root ('.xjcode-failed-'+[guid]::NewGuid().ToString('N'))
    Move-Item -LiteralPath $package -Destination $failed
    Move-Item -LiteralPath $backup -Destination $package
    Remove-Item -LiteralPath (Join-Path $package '.xjcode-owned-rollback') -Force
    CheckTree $failed;Remove-Item -LiteralPath $failed -Recurse -Force
    throw 'Updated version check failed; original package restored.'
   }
  }finally{
   if((Test-Path -LiteralPath $stage) -and (Full $stage).StartsWith($Root+'\',[StringComparison]::OrdinalIgnoreCase)){NoLinks $stage;Remove-Item -LiteralPath $stage -Recurse -Force}
  }
  Write-Output 'XJCode update complete. Restart XJCode.'
 }else{throw 'Unsupported maintenance operation.'}
 $ok=$true
 }
}catch{Write-Output ('XJCode maintenance failed at line '+$_.InvocationInfo.ScriptLineNumber+': '+$_.Exception.Message);Write-Output $_.ScriptStackTrace}finally{
 if($lock){$lock.Dispose();Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue}
 if($ok -and $job.Cleanup){
  $cleanup=Full $job.Cleanup;$temp=(Full ([IO.Path]::GetTempPath()))+'\'
  if($cleanup.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $cleanup).StartsWith('xjcode-maintenance-') -and (Full (Split-Path -Parent $job.Package)) -eq $cleanup){
   NoLinks $cleanup
   foreach($file in @($job.Package,(Join-Path $cleanup 'release-manifest.json'))){if(Test-Path -LiteralPath $file -PathType Leaf){NoLinks $file;Remove-Item -LiteralPath $file -Force}}
   if(-not (Get-ChildItem -LiteralPath $cleanup -Force | Select-Object -First 1)){Remove-Item -LiteralPath $cleanup -Force}
  }
 }
 if($log){Stop-Transcript | Out-Null}
}
if($Gui){[Windows.Forms.MessageBox]::Show($(if($ok){'XJCode uninstall complete.'}else{'Operation failed. See '+$log}),'XJCode') | Out-Null}
if(-not $ok){exit 1}
exit 0
