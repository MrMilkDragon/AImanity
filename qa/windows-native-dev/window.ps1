param(
  [Parameter(Mandatory = $true)] [int] $AppId,
  [Parameter(Mandatory = $true)] [ValidateSet("fresh", "setup", "capture", "close")] [string] $Action
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NativeQaInput {
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out Rect r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref Point p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, IntPtr extra);
}
'@
if ($env:RUNNER_ENVIRONMENT -ne "github-hosted" -or $env:AIMANITY_QA_ROOT -notmatch '^[A-Z]:\\Dysprosium-QA-') {
  throw "Requires the disposable Microsoft QA runner and dedicated root."
}
[void][NativeQaInput]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
$deadline = [DateTime]::UtcNow.AddSeconds(45)
do {
  $process = Get-Process -Id $AppId
  $handle = $process.MainWindowHandle
  $ready = $handle -ne [IntPtr]::Zero -and ($Action -ne "capture" -or $process.MainWindowTitle -eq "AImanity")
  if ($ready) { break }
  Start-Sleep -Milliseconds 300
} while ([DateTime]::UtcNow -lt $deadline)
if (!$ready) { throw "The expected native AImanity window did not appear." }
if ($Action -eq "close") {
  if (!$process.CloseMainWindow()) { throw "Could not send the native window close request." }
  return
}
[void][NativeQaInput]::SetForegroundWindow($handle)
Start-Sleep -Milliseconds 300
if ([NativeQaInput]::GetForegroundWindow() -ne $handle) { throw "Native app did not become the foreground target. No input sent." }
$bounds = [NativeQaInput+Rect]::new()
$origin = [NativeQaInput+Point]::new()
if (![NativeQaInput]::GetClientRect($handle, [ref]$bounds) -or ![NativeQaInput]::ClientToScreen($handle, [ref]$origin)) {
  throw "Could not determine the native window bounds."
}
$width = $bounds.Right
$height = $bounds.Bottom
if ($width -lt 650 -or $height -lt 590) { throw "QA requires an unoccluded native window of at least 650×590." }
function Capture([string] $name) {
  $bitmap = [System.Drawing.Bitmap]::new($width, $height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bitmap.Size)
  } finally { $graphics.Dispose() }
  $bitmap.Save((Join-Path $env:AIMANITY_QA_ROOT $name), [System.Drawing.Imaging.ImageFormat]::Png)
  return $bitmap
}
function Click([double] $rx, [double] $ry) {
  if ([NativeQaInput]::GetForegroundWindow() -ne $handle) { throw "Foreground changed. No input sent." }
  if (![NativeQaInput]::SetCursorPos([int]($origin.X + $width*$rx), [int]($origin.Y + $height*$ry))) { throw "Could not position QA cursor." }
  [NativeQaInput]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 70
  [NativeQaInput]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
}
if ($Action -eq "capture") {
  (Capture "native-workbench.png").Dispose()
  Write-Host "Native AImanity captured from window $handle with client bounds ${width}x${height}"
  return
}
(Capture "native-language.png").Dispose()
# English is in the first row of the right column.
# English is selected in place; Continue advances the setup.
Click .70 .34
Click .50 .895
Start-Sleep -Milliseconds 450
$image = Capture "native-devices.png"
try {
  # Detect the solid install button before sending the only advancing click.
  $hits = @()
  for ($y = [int]($height*.87); $y -lt $height*.955; $y+=2) {
    $color = $image.GetPixel([int]($width*.917), $y)
    if ($color.R -le 65 -and $color.G -le 65 -and $color.B -le 65) { $hits += $y }
  }
  if ($hits.Count -lt 5) { throw "Install button did not render after choosing English." }
  if ($Action -eq "setup") {
    Write-Host "Native language selection and device setup rendered from the development executable."
    return
  }
  $center = ($hits[0] + $hits[-1]) / 2
  Click .85 ($center/$height)
  Write-Host "Native GUI selected English and activated the visually verified install button."
} finally { $image.Dispose() }
