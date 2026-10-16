# Dedicated per-user scheduled task, starts at interactive Windows login.
# No administrator privileges, no SYSTEM account, no changes to other tasks.
[CmdletBinding()]
param([switch]$PreviewOnly)
$ErrorActionPreference='Stop'
$name='GuardAdaptive-RemoteLab-StableWatch'
$watch=Join-Path $PSScriptRoot 'Watch-GuardAdaptive-Stable.ps1'
$pwsh=(Get-Command pwsh.exe -ErrorAction Stop).Source
if(-not(Test-Path $watch -PathType Leaf)){throw 'Watch script absent'}
$existing=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
if($existing){
 $actions=@($existing.Actions)
 if($actions.Count -ne 1 -or $actions[0].Execute -ine $pwsh -or
  $actions[0].Arguments -notlike "*Watch-GuardAdaptive-Stable.ps1*"){
  throw 'Task name already used by another or modified action: refusing overwrite.'
 }
 Write-Output "REMOTE_LAB_TASK_ALREADY_CONFIGURED=$name STATE=$($existing.State)"
 exit 0
}
$user="$env:USERDOMAIN\$env:USERNAME"
$arguments='-NoLogo -NoProfile -NonInteractive -File "'+$watch+'"'
$action=New-ScheduledTaskAction -Execute $pwsh -Argument $arguments
$trigger=New-ScheduledTaskTrigger -AtLogOn -User $user
$principal=New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable
$task=New-ScheduledTask -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'GuardAdaptive Remote Lab stable localhost backend watchdog, user-logon only'
if($PreviewOnly){
 Write-Output "REMOTE_LAB_TASK_PREVIEW=PASS NAME=$name USER=$user SCRIPT=$watch"
 exit 0
}
Register-ScheduledTask -TaskName $name -InputObject $task -ErrorAction Stop|Out-Null
$got=Get-ScheduledTask -TaskName $name -ErrorAction Stop
if($got.Actions.Count -ne 1 -or $got.Actions[0].Execute -ine $pwsh -or
  $got.Actions[0].Arguments -notlike "*Watch-GuardAdaptive-Stable.ps1*"){
 throw 'Registered task did not match expected dedicated script'
}
Write-Output "REMOTE_LAB_TASK_REGISTERED=PASS NAME=$name USER_LOGON_ONLY=TRUE"
