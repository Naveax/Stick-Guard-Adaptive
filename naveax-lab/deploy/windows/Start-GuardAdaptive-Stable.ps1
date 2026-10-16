# GuardAdaptive Remote Lab / stable Tailscale Funnel localhost backend.
# Read-only controller research. Does not change Tailscale, VPN, driver or game input.
# Secrets are read by the Node backend via a FILE PATH, not passed as plaintext.
[CmdletBinding()]
param([switch]$HealthCheckOnly)
$ErrorActionPreference='Stop'
$hostName='naveax.tail10f02d.ts.net'
$origin="https://$hostName"
$port=48928
$repoRoot=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$server=Join-Path $repoRoot 'server.cjs'
$nodeCmd=Get-Command node.exe -ErrorAction Stop
$node=$nodeCmd.Source
$privateRoot=Join-Path $env:LOCALAPPDATA 'GuardAdaptiveRemoteLabStable'
$keyFile=Join-Path $privateRoot 'secrets\admin_token.txt'
$dataDir=Join-Path $privateRoot 'submissions'
$tailscale='C:\Program Files\Tailscale\tailscale.exe'
if(-not (Test-Path $server -PathType Leaf) -or -not (Test-Path $tailscale -PathType Leaf)){
 throw 'Missing Remote Lab source or expected Tailscale binary.'
}
if(-not(Test-Path $keyFile -PathType Leaf)){throw 'Private admin token file is missing.'}
if((Get-Content -LiteralPath $keyFile -Raw).Trim() -cnotmatch '^[0-9a-fA-F]{64}$'){
 throw 'Private admin token format is invalid.'
}
$ts=(& $tailscale status --json | ConvertFrom-Json)
if($LASTEXITCODE -ne 0 -or $ts.BackendState -ne 'Running' -or
  $ts.Self.DNSName.TrimEnd('.') -cne $hostName){
 throw 'This PC does not have the authorized running Tailscale node identity.'
}
$raw=(& $tailscale funnel status | Out-String)
if($LASTEXITCODE -ne 0 -or $raw -notmatch [regex]::Escape($origin) -or
  $raw -notmatch '127\.0\.0\.1:48928'){
 throw 'Expected pinned Funnel 443 -> localhost 48928 route is not active.'
}
$connections=@(Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue)
if($connections.Count -gt 1){throw 'Ambiguous local listener ownership; refuse.'}
if($connections.Count -eq 1){
 if($connections[0].LocalAddress -ne '127.0.0.1'){throw 'Remote Lab port is not loopback-only.'}
 $pidOnPort=$connections[0].OwningProcess
 $proc=Get-CimInstance Win32_Process -Filter "ProcessId=$pidOnPort"
 if($proc.Name -ne 'node.exe' -or $proc.CommandLine -notlike "*server.cjs*"){
  throw 'Port is owned by a different application.'
 }
 $response=Invoke-RestMethod "http://127.0.0.1:$port/api/status" -TimeoutSec 5
 if($response.upload_enabled -ne $true -or $response.schema -ne 1){
  throw 'Existing listener is not configured for stable consent-gated uploads. Refusing to replace automatically.'
 }
 Write-Output "REMOTE_LAB_STABLE=EXISTING_VALID PID=$pidOnPort HOST=$origin"
 exit 0
}
if($HealthCheckOnly){
 Write-Output 'REMOTE_LAB_STABLE=STOPPED; health-only mode will not start backend'
 exit 2
}
New-Item -ItemType Directory -Path $dataDir -Force|Out-Null
$settings=@{
 REMOTE_LAB_HOST='127.0.0.1'
 REMOTE_LAB_PORT="$port"
 REMOTE_LAB_UPLOADS='enabled'
 REMOTE_LAB_SIMPLE_UI='enabled'
 REMOTE_LAB_PUBLIC_ORIGIN=$origin
 REMOTE_LAB_DATA_DIR=$dataDir
 REMOTE_LAB_ADMIN_TOKEN_FILE=$keyFile
 REMOTE_LAB_TRUST_CF_TUNNEL='disabled'
 REMOTE_LAB_TRUST_LOOPBACK_PROXY='disabled'
}
$process=Start-Process -FilePath $node -ArgumentList @($server) -WorkingDirectory $repoRoot -WindowStyle Hidden -PassThru -Environment $settings
Start-Sleep -Seconds 2
$connection=@(Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue)
if($connection.Count -ne 1 -or $connection[0].OwningProcess -ne $process.Id -or $connection[0].LocalAddress -ne '127.0.0.1'){
 throw 'The new Node backend did not start under the expected PID and loopback socket.'
}
$status=Invoke-RestMethod "http://127.0.0.1:$port/api/status" -TimeoutSec 5
if($status.schema -ne 1 -or $status.upload_enabled -ne $true){throw 'New local server health check failed.'}
Write-Output "REMOTE_LAB_STABLE=STARTED PID=$($process.Id) HOST=$origin UPLOAD_CONSENT_REQUIRED=TRUE"
