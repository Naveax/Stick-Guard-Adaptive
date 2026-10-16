# Provision a dedicated per-user storage identity for the stable Funnel pilot.
# Never reads or alters the Cloudflare pilot's existing secret or records.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$base=Join-Path $env:LOCALAPPDATA 'GuardAdaptiveRemoteLabStable'
$secrets=Join-Path $base 'secrets'
$records=Join-Path $base 'submissions'
$key=Join-Path $secrets 'admin_token.txt'
foreach($dir in @($base,$secrets,$records)){
 New-Item -ItemType Directory -Path $dir -Force | Out-Null
}
$me=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
foreach($dir in @($base,$secrets,$records)){
 & icacls.exe $dir /inheritance:r /grant:r ('*'+$me+':(OI)(CI)F') '*S-1-5-18:(OI)(CI)F' | Out-Null
 if($LASTEXITCODE -ne 0){throw 'Failed to enforce per-user directory ACL.'}
}
if(-not (Test-Path -LiteralPath $key -PathType Leaf)){
 $entropy=[byte[]]::new(32)
 [Security.Cryptography.RandomNumberGenerator]::Fill($entropy)
 try {
  $hex=[Convert]::ToHexString($entropy).ToLowerInvariant()
  $bytes=[Text.Encoding]::ASCII.GetBytes($hex)
  $stream=[IO.File]::Open($key,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
  try {$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}
  finally {$stream.Dispose();[Array]::Clear($bytes,0,$bytes.Length)}
 }finally {[Array]::Clear($entropy,0,$entropy.Length)}
}
& icacls.exe $key /inheritance:r /grant:r ('*'+$me+':F') '*S-1-5-18:F' | Out-Null
if($LASTEXITCODE -ne 0){throw 'Failed to enforce secret-file ACL.'}
$format=(Get-Content -LiteralPath $key -Raw).Trim()
if($format -cnotmatch '^[a-f0-9]{64}$'){throw 'Stable admin secret invalid; no replacement attempted.'}
Write-Output 'REMOTE_LAB_STABLE_STORAGE=READY SEPARATE_TOKEN=TRUE SEPARATE_RECORDS=TRUE'
