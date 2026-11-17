  $ErrorActionPreference = 'Stop'
  Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
  $checkpoint = Join-Path $env:APPDATA 'dev.aiphone.client\host\deployment.json'
  $element = [System.Windows.Automation.AutomationElement]
  $descendants = [System.Windows.Automation.TreeScope]::Descendants
  $buttonType = [System.Windows.Automation.PropertyCondition]::new($element::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
  $enabled = [System.Windows.Automation.PropertyCondition]::new($element::IsEnabledProperty, $true)
  $visible = [System.Windows.Automation.PropertyCondition]::new($element::IsOffscreenProperty, $false)
  $lastPhase = ''

  function Find-Button($name, $seconds) {
    $named = [System.Windows.Automation.PropertyCondition]::new($element::NameProperty, $name)
    $selector = [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]] @($buttonType, $enabled, $visible, $named))
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    do {
      $process = Get-Process -Id $env:AIMANITY_QA_PID
      if ($process.MainWindowHandle) {
        $window = $element::FromHandle($process.MainWindowHandle)
        $button = $window.FindFirst($descendants, $selector)
        if ($button) { return $button }
      }
      if (Test-Path $checkpoint) {
        $progress = (Get-Content $checkpoint -Raw | ConvertFrom-Json).progress
        $phase = "$($progress.phase): $($progress.state)"
        if ($phase -ne $script:lastPhase) {
          Write-Host "Native setup: $phase"
          $script:lastPhase = $phase
        }
        if ($progress.state -in @('failed', 'blocked', 'interrupted')) {
          throw "Native setup $phase ($($progress.error))"
        }
      }
      Start-Sleep -Milliseconds 450
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($window) {
      $window.FindAll($descendants, $buttonType) |
        ForEach-Object { 'Button: {0}; enabled={1}; offscreen={2}' -f $_.Current.Name, $_.Current.IsEnabled, $_.Current.IsOffscreen } |
        Select-Object -First 15 | Write-Host
    }
    throw "Installed window did not expose the enabled '$name' button."
  }

  function Invoke-Button($button) {
    $name = $button.Current.Name
    $action = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $action.Invoke()
    Write-Host "Clicked installed client button: $name"
  }

  function Wait-WorkbenchHealth {
    Add-Type -AssemblyName System.Net.Http
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseProxy = $false
    $handler.AllowAutoRedirect = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(4)
    $deadline = [DateTime]::UtcNow.AddSeconds(35)
    $observed = ''
    try {
      do {
        $probe = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Head, 'http://127.0.0.1:17860/phone')
        $response = $null
        try {
          $response = $client.SendAsync($probe).GetAwaiter().GetResult()
          $media = $response.Content.Headers.ContentType.MediaType
          $observed = "{0} {1}; redirect={2}" -f [int]$response.StatusCode, $media, $response.Headers.Location
          if ($response.IsSuccessStatusCode -and $media -eq 'text/html') {
            Write-Host "Pre-open workbench HEAD /phone: $observed"
            return
          }
        } catch {
          $observed = $_.Exception.GetBaseException().Message
        } finally {
          if ($response) { $response.Dispose() }
          $probe.Dispose()
        }
        Start-Sleep -Milliseconds 350
      } while ([DateTime]::UtcNow -lt $deadline)
      throw "Workbench HEAD /phone did not return HTML: $observed"
    } finally {
      $client.Dispose()
    }
  }

  Invoke-Button (Find-Button 'Continue' 45)
  $start = Find-Button 'Start installation' 20
  $deploy = [System.Diagnostics.Stopwatch]::StartNew()
  Invoke-Button $start
  $enter = Find-Button 'Configure model' 600
  $ready = (Get-Content $checkpoint -Raw | ConvertFrom-Json).progress.state
  if ($ready -ne 'ready') { throw "The native checkpoint was '$ready' after device setup." }
  "First-run devices ready in $($deploy.ElapsedMilliseconds) ms"
  Wait-WorkbenchHealth
  $setupWindow = (Get-Process -Id $env:AIMANITY_QA_PID).MainWindowHandle
  $navigation = & curl.exe --noproxy '*' --silent --show-error --max-time 4 --head 'http://127.0.0.1:17860/phone?language=en' |
    Where-Object { $_ -match '^(HTTP/|location:)' }
  Write-Host "First-open redirect: $($navigation -join ' ')"
  Invoke-Button $enter

  $owned = [System.Windows.Automation.PropertyCondition]::new($element::ProcessIdProperty, [int]$env:AIMANITY_QA_PID)
  $document = [System.Windows.Automation.PropertyCondition]::new($element::ControlTypeProperty, [System.Windows.Automation.ControlType]::Document)
  $desktop = $element::RootElement
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    $windows = $desktop.FindAll([System.Windows.Automation.TreeScope]::Children, $owned)
    foreach ($window in $windows) {
      if ($window.Current.NativeWindowHandle -eq $setupWindow.ToInt32() -or $window.Current.IsOffscreen) { continue }
      $content = $window.FindFirst($descendants, $document)
      if ($content -and $content.Current.Name -like '*AImanity*') {
        "Installed workbench HWND $($window.Current.NativeWindowHandle); window='$($window.Current.Name)'; document='$($content.Current.Name)'"
        exit 0
      }
    }
    Start-Sleep -Milliseconds 450
  } while ([DateTime]::UtcNow -lt $deadline)
  $diagnostics = [System.Windows.Automation.PropertyCondition]::new($element::IsOffscreenProperty, $false)
  foreach ($window in $windows) {
    Write-Host "AImanity window $($window.Current.NativeWindowHandle); title='$($window.Current.Name)'; class='$($window.Current.ClassName)'"
    $window.FindAll($descendants, $diagnostics) |
      Where-Object { $_.Current.Name } | Select-Object -First 35 |
      ForEach-Object { Write-Host "  $($_.Current.ControlType.ProgrammaticName) '$($_.Current.Name)' ($($_.Current.AutomationId))" }
  }
  throw 'The installed workbench window did not appear.'
