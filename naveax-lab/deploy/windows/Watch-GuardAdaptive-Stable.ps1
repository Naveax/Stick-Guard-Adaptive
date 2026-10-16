# GuardAdaptive Remote Lab stable backend watchdog. User-logon only, no elevation.
# Starts ONLY the authorized localhost Node backend, never mutates Funnel/VPN,
# Windows drivers, GitHub, or the installed GuardAdaptive application.
[CmdletBinding()]
param([switch]$Once)
$ErrorActionPreference='Stop'
$mutexName='Local\GuardAdaptive-RemoteLab-StableWatch-v1'
$mutex=[Threading.Mutex]::new($false,$mutexName)
$acquired=$false
try {
 $acquired=$mutex.WaitOne(0)
 if(-not $acquired){
  Write-Output 'REMOTE_LAB_WATCH=EXISTING_INSTANCE'
  exit 0
 }
 $root=Join-Path $env:LOCALAPPDATA 'GuardAdaptiveRemoteLab\logs'
 New-Item -ItemType Directory -Path $root -Force|Out-Null
 $log=Join-Path $root 'stable-watch.log'
 $startScript=Join-Path $PSScriptRoot 'Start-GuardAdaptive-Stable.ps1'
 $lastMessage=''
 while($true){
  $message=''
  try{
   $run=(& (Get-Command pwsh.exe).Source -NoProfile -NonInteractive -File $startScript 2>&1 |Out-String).Trim()
   if($LASTEXITCODE -ne 0){throw "Backend start/health exit code: $LASTEXITCODE"}
   $message=($run -split "[\r\n]+"|Select-Object -Last 1)
  }catch{
   $message='FAILED '+($_.Exception.Message -replace '[\r\n]+',' ')
  }
  if($message -ne $lastMessage -or $Once){
   $line=(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss')+' '+$message
   if((Test-Path $log) -and (Get-Item $log).Length -gt 524288){
    Move-Item -LiteralPath $log -Destination ($log+'.old') -Force
   }
   Add-Content -LiteralPath $log -Value $line -Encoding utf8
   Write-Output $line
   $lastMessage=$message
  }
  if($Once){if($message -like 'FAILED *'){exit 1};exit 0}
  Start-Sleep -Seconds 60
 }
}finally{
 if($acquired){$mutex.ReleaseMutex()}
 $mutex.Dispose()
}
