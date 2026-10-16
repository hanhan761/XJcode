$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root=[IO.Path]::GetFullPath($env:XJCODE_EXTRACT_TO).TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar
$zip=[IO.Compression.ZipFile]::OpenRead($env:XJCODE_EXTRACT_ZIP)
try {
 foreach($entry in $zip.Entries){
  $target=[IO.Path]::GetFullPath((Join-Path $root $entry.FullName))
  if(-not $target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'Unsafe archive path'}
 }
} finally {$zip.Dispose()}
[IO.Compression.ZipFile]::ExtractToDirectory($env:XJCODE_EXTRACT_ZIP,$env:XJCODE_EXTRACT_TO)
