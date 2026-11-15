param(
    [Parameter(Mandatory=$true)][string]$AcceptanceRoot
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path $AcceptanceRoot).Path
$Install = Join-Path $Root 'installed'
$Program = Join-Path $Install 'ai-phone-client.exe'
$Uninstaller = Join-Path $Install 'unins000.exe'
$Roaming = Join-Path $Root 'profile\dev.aiphone.client'
$Local = Split-Path -Parent $env:AIMANITY_QA_UPDATE_CACHE
$Launcher = Get-Process -Name 'ai-phone-client' | Where-Object { $_.Path -eq $Program -and $_.MainWindowTitle -eq 'AImanity' }
if (@($Launcher).Count -ne 1 -or -not (Test-Path $Uninstaller) -or -not (Test-Path "$Roaming\host\deployment.json")) {
    throw 'Run uninstall after the restarted installed app has restored both devices.'
}
$Backup = Join-Path $Root 'external-backup'
$LocalProbe = Join-Path $Local 'webview-uninstall-probe'
New-Item -ItemType Directory -Path $Local -Force | Out-Null
Set-Content -Path $Backup -Value 'external backup'
Set-Content -Path $LocalProbe -Value 'private cache'
if (-not $Launcher.CloseMainWindow() -or -not $Launcher.WaitForExit(110000) -or $Launcher.ExitCode -ne 0) {
    throw 'Normal close before uninstall failed.'
}
$Timer = [Diagnostics.Stopwatch]::StartNew()
$Log = Join-Path $Root 'uninstall.log'
$Process = Start-Process -FilePath $Uninstaller -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',"/LOG=$Log" -Wait -PassThru
if ($Process.ExitCode -ne 0) {
    Get-Content $Log -Tail 30
    throw "Uninstall exit=$($Process.ExitCode)"
}
if ((Test-Path $Program) -or (Test-Path "$Install\host") -or (Test-Path $Roaming) -or (Test-Path $Local) -or (Get-Content $Backup -Raw).Trim() -ne 'external backup') {
    Get-Content $Log -Tail 30
    throw 'Uninstall left installed app or private data, or did not preserve the external backup.'
}
Write-Host "Uninstall removed the app, private data and both device disks in $($Timer.ElapsedMilliseconds) ms; external backup retained."
