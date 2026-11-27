param([Parameter(Mandatory=$true)][string]$Exe)
$ErrorActionPreference = 'Stop'
$signature = Get-AuthenticodeSignature -LiteralPath $Exe
if (-not $signature.SignerCertificate) { throw 'Installer has no signer' }
$install = Join-Path $env:RUNNER_TEMP 'overlay-installed'
$args = @('/VERYSILENT','/SUPPRESSMSGBOXES','/SP-','/NORESTART','/NORESTARTAPPLICATIONS',"/DIR=`"$install`"")
$process = Start-Process -FilePath $Exe -ArgumentList $args -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "In-place installer returned $($process.ExitCode)" }
"AIMANITY_QA_CLIENT=$(Join-Path $install 'ai-phone-client.exe')" >> $env:GITHUB_ENV
