$ErrorActionPreference = 'Stop'
$root = $env:AIMANITY_QA_ROOT
$env:APPDATA = Join-Path $root 'profile'
$env:LOCALAPPDATA = $env:APPDATA
$env:AIMANITY_DATA_DIR = Join-Path $env:APPDATA 'dev.aiphone.client'
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $root 'webview'
$hostHome = Join-Path $env:AIMANITY_DATA_DIR 'host'
$dev = Join-Path $root 'dev'
$install = Join-Path $root 'installed'
$installer = Join-Path $PWD 'dist/qa-calcium-input/AImanity-0.0.3-windows-x64-setup.exe'
$probe = Join-Path $PWD 'target/debug/calcium-state-qa.exe'
$native = Join-Path $dev 'aimanity-native.exe'

function Stop-QAClient($app) {
  if (!$app.HasExited) {
    & "$PSScriptRoot/window.ps1" -AppId $app.Id -Action close
    if (!$app.WaitForExit(120000)) { throw 'Client did not exit cleanly in 120 seconds.' }
  }
  if ($app.ExitCode -ne 0) { throw "Client shutdown exit code $($app.ExitCode)." }
}

function Wait-PortsClosed {
  $ports = @(15037,17860,17861,17862,17863,17864,17865)
  $deadline = [DateTime]::UtcNow.AddSeconds(60)
  do {
    $active = Get-NetTCPConnection -LocalPort $ports -State Listen -ErrorAction SilentlyContinue
    if (!$active) { return }
    Start-Sleep -Milliseconds 500
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Guest processes still listen after shutdown: $($active.LocalPort -join ',')"
}

if ((Test-Path $hostHome) -or (Test-Path $install)) { throw 'Fresh isolated Windows profile required.' }
$prior = Start-Process $native -WorkingDirectory $dev -ArgumentList '--prepare-windows-host' -PassThru -Wait
if ($prior.ExitCode -ne 0) { throw "WHPX preparation exit code $($prior.ExitCode)." }
$installArgs = @('/VERYSILENT','/SUPPRESSMSGBOXES','/SP-','/NORESTART','/RESTARTEXITCODE=3010','/NORESTARTAPPLICATIONS',"/DIR=$install")
$setup = Start-Process $installer -ArgumentList $installArgs -PassThru -Wait
if ($setup.ExitCode -ne 0) { throw "Signed Calcium installer exit code $($setup.ExitCode)." }
$app = Start-Process (Join-Path $install 'ai-phone-client.exe') -WorkingDirectory $install -PassThru
try {
  $env:AIMANITY_QA_PID = [string]$app.Id
  powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PSScriptRoot/calcium-first-run.ps1"
  if ($LASTEXITCODE -ne 0) { throw 'Signed Calcium did not complete genuine workbench onboarding.' }
  & $probe seed
  if ($LASTEXITCODE -ne 0) { throw 'Calcium workbench did not persist the state through authenticated requests.' }
} finally {
  Stop-QAClient $app
}
Wait-PortsClosed

$log = Join-Path $root 'native.log'
$checkpoint = Join-Path $hostHome 'deployment.json'
$priorIdentity = (Get-Content $checkpoint -Raw | ConvertFrom-Json).installed_payload
$nextIdentity = (Get-Content (Join-Path $dev 'host/integrity.json') -Raw | ConvertFrom-Json).'source.payload'
if (!$priorIdentity -or !$nextIdentity -or $priorIdentity -eq $nextIdentity) { throw 'Expected distinct Calcium and native source payloads.' }
$app = Start-Process $native -WorkingDirectory $dev -PassThru -RedirectStandardError $log
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(600)
  do {
    if ($app.HasExited) { throw 'Native development client exited before device recovery.' }
    $status = Get-Content $checkpoint -Raw | ConvertFrom-Json
    if ($status.installed_payload -eq $nextIdentity -and $status.progress.state -eq 'ready') { break }
    if ($status.progress.state -in @('failed','blocked')) { throw "Native deployment: $($status.progress.error)" }
    Start-Sleep -Milliseconds 500
  } while ([DateTime]::UtcNow -lt $deadline)
  if ($status.installed_payload -ne $nextIdentity) { throw 'The same Windows profile did not resume with the native payload.' }
  & "$PSScriptRoot/window.ps1" -AppId $app.Id -Action capture
  & $probe read
  if ($LASTEXITCODE -ne 0) { throw 'Private Calcium state did not survive in the native development client.' }
  Get-Content $log | Where-Object { $_ -match 'native_metric first_frame device=[01]' }
} finally {
  if (Test-Path $log) { Get-Content $log -Tail 35 }
  Stop-QAClient $app
}
Wait-PortsClosed
