import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { deepEqual, equal, match, ok } from "node:assert/strict";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { RegistryRequestSchema, RegistryStateSchema } from "../packages/protocol/src/gen/ai_phone/v1/registry_pb";
import { DesktopActionRequestSchema, DesktopObservationSchema, DesktopRequestSchema } from "../packages/protocol/src/gen/ai_phone/v1/desktop_pb";

const root = process.env.AIMANITY_QA_ROOT;
if (!root || process.platform !== "win32") throw Error("Run on the disposable Windows acceptance host");
import {FileListingSchema, FileLocationSchema, FileManageRequestSchema, FileManageResultSchema, FileOperation} from "../packages/protocol/src/gen/ai_phone/v1/workbench_pb";
const baseline = process.env.AIMANITY_QA_BASELINE ?? "";
const candidate = "0.0.3";
ok(baseline === "" || baseline === "0.0.1" || baseline === "0.0.2", "known baseline");
const started = performance.now();
const install = join(root, "installed");
const appData = join(root, "profile");
const home = join(appData, "dev.aiphone.client", "host");
const installer = join(root, "AImanity-" + (baseline || candidate) + "-windows-x64-setup.exe");
const installation = Bun.spawn([installer, "/VERYSILENT", "/SUPPRESSMSGBOXES", "/SP-",
  "/NORESTART", "/RESTARTEXITCODE=3010", `/DIR=${install}`,
], { stdout: "ignore", stderr: "inherit" });
const exit = await installation.exited;
if (exit === 3010) throw Error("Windows requires a reboot after enabling the Hypervisor Platform");
equal(exit, 0, "Inno Setup exit code");
console.log(JSON.stringify({ installerExit: exit, ms: Math.round(performance.now() - started) }));

const env = { ...process.env, APPDATA: appData, LOCALAPPDATA: appData,
  WEBVIEW2_USER_DATA_FOLDER: join(root, "webview"),
};
const program = join(install, "ai-phone-client.exe");
const app = Bun.spawn([program], { cwd: install, env, stdout: "ignore", stderr: "inherit" });
console.log("Started installed AImanity GUI process " + app.pid);
const origin = "http://127.0.0.1:17860";
let webReady = false;
const watchWeb = setInterval(async () => {
  if (webReady) return;
  const response = await fetch(`${origin}/phone`, { method: "HEAD", redirect: "manual", signal: AbortSignal.timeout(2400) }).catch(() => undefined);
  if (!response?.ok || !response.headers.get("content-type")?.startsWith("text/html")) return;
  webReady = true;
  console.log(`Installed Windows workbench HTTP: ${response.status} ${response.headers.get("content-type")} at ${Math.round(performance.now() - started)} ms`);
}, 4000);
const checkpoint = join(home, "deployment.json");
const androidRoot = join(home, "user", "data", "android");
const logPaths = ["user/qemu/owner.log", "user/qemu/boot.log", "user/qemu/qemu.log",
  "user/data/android/native/sdk.log", "user/data/android/whpx-avd/cuttlefish/kernel.log", "deployment.json"];
let androidStarted = 0;
let reportedAndroid = false;
let reportedListeners = false;
const watchAndroid = setInterval(() => {
  if (!existsSync(checkpoint) || !existsSync(join(androidRoot, "native", "sdk.log"))) return;
  if (!androidStarted) androidStarted = Date.now();
  const reportBoot = !reportedAndroid && Date.now() - androidStarted >= 85000;
  const reportListeners = reportedAndroid && !reportedListeners && Date.now() - androidStarted >= 114000;
  if (!reportBoot && !reportListeners) return;
  const components = join(home, "components", "android");
  if (!existsSync(components)) return;
  const [identity] = readdirSync(components);
  const adb = join(components, identity!, "sdk", "platform-tools", "adb.exe");
  const adbInfo = (...args: string[]) => Bun.spawnSync([adb, "-P", "15037", ...args], { env, timeout: 4000 }).stdout.toString().trim();
  if (reportBoot) {
    reportedAndroid = true;
    console.log(`Android boot transport: ${adbInfo("devices", "-l")}\n${adbInfo("-s", "127.0.0.1:5565", "shell", "getprop sys.boot_completed; id -u")}`);
  }
  if (reportListeners) {
    reportedListeners = true;
    const shell = (command: string) => adbInfo("-s", "127.0.0.1:5565", "shell", command);
    console.log(`Android first-run user state: ${shell("am get-started-user-state 0; date +%s")}; host clock ${Date.now()}`);
    for (const name of ["notifications", "attention"]) {
      const raw = shell(`cat /data/user/0/dev.aiphone.input/files/${name}.json 2>/dev/null`);
      if (!raw) { console.log(`Android ${name} listener has not written a receipt`); continue; }
      const receipt = JSON.parse(raw);
      console.log(`Android ${name} listener: connected=${receipt.connected}, checked_at_ms=${receipt.checked_at_ms}`);
    }
  }
}, 10000);

const automation = String.raw`
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
`;

try {
  const script = Buffer.from(automation, "utf16le").toString("base64");
  const desktopSetup = Bun.spawn(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", script], {
    env: { ...env, AIMANITY_QA_PID: String(app.pid) }, stdout: "inherit", stderr: "inherit",
  });
  const desktopSetupExit = await desktopSetup.exited;
  if (desktopSetupExit !== 0) {
    ok(existsSync(checkpoint), "Windows setup stopped before creating its checkpoint");
    const state = JSON.parse(readFileSync(checkpoint, "utf8")).progress.state;
    equal(state, "ready", "Windows setup must reach actual dual-device readiness before checking the workbench");
    console.error("Workbench UI failed. Checking the independently served workbench and real devices.");
  }

  const protocol = "application/x-protobuf";
  const login = await fetch(`${origin}/api/auth/enter`, { method: "POST",
    headers: { "content-type": protocol, origin }, body: new Uint8Array(), signal: AbortSignal.timeout(10000) });
  equal(login.status, 200, "workbench sign-in");
  const setCookie = login.headers.get("set-cookie");
  ok(setCookie, "workbench issued a session");
  const cookie = setCookie.split(";", 1)[0]!;
  const registry = await fetch(`${origin}/api/registry/state`, { method: "POST",
    headers: { cookie, "content-type": protocol, origin },
    body: toBinary(RegistryRequestSchema, create(RegistryRequestSchema)), signal: AbortSignal.timeout(10000) });
  equal(registry.status, 200, "installed service registry");
  const devices = fromBinary(RegistryStateSchema, new Uint8Array(await registry.arrayBuffer())).devices;
  const desktop = devices.find((device) => device.kind === "desktop" && device.runtimeState === "managed");
  const phone = devices.find((device) => device.kind === "phone" && device.runtimeState === "managed");
  ok(desktop, "installed desktop is registered");
  ok(phone, "installed Android is registered");
  async function verifyFrame(endpoint: string, device: string, session = cookie) {
    const screen = await fetch(`${origin}${endpoint}`, { headers: { cookie: session }, signal: AbortSignal.timeout(12000) });
    equal(screen.status, 200, `${device} media HTTP status`);
    match(screen.headers.get("content-type") ?? "", /^multipart\/x-mixed-replace/i);
    const reader = screen.body!.getReader();
    let frames = Buffer.alloc(0);
    const start = Buffer.from([0xff, 0xd8, 0xff]), end = Buffer.from([0xff, 0xd9]);
    try {
      while (frames.length < 2_000_000) {
        const { value, done } = await reader.read();
        if (done) break;
        frames = Buffer.concat([frames, Buffer.from(value)]);
        const offset = frames.indexOf(start);
        if (offset >= 0 && frames.indexOf(end, offset + 3) > offset) {
          console.log(`${device} sent a complete JPEG through the workbench`);
          return;
        }
      }
    } finally { await reader.cancel(); }
    throw Error(`${device} did not send a complete JPEG`);
  }
  async function verifyLinuxControl(session = cookie) {
    const headers = { cookie: session, origin, "content-type": protocol };
    const controlResponse = await fetch(origin + "/api/desktop/control", { method: "POST", headers,
      body: toBinary(DesktopRequestSchema, create(DesktopRequestSchema, {deviceId: desktop!.id})), signal: AbortSignal.timeout(20000) });
    equal(controlResponse.status, 200, "Linux live control");
    const control = fromBinary(DesktopObservationSchema, new Uint8Array(await controlResponse.arrayBuffer()));
    const result = await fetch(origin + "/api/desktop/action", { method: "POST", headers,
      body: toBinary(DesktopActionRequestSchema, create(DesktopActionRequestSchema, {
        deviceId: desktop!.id, actionId: crypto.randomUUID(), observationId: control.observationId, epoch: control.epoch,
        kind: "click", x: 650, y: 550, button: 1, clicks: 1, waitForPaint: true,
      })), signal: AbortSignal.timeout(20000) });
    equal(result.status, 200, "Linux real click");
    const frame = fromBinary(DesktopObservationSchema, new Uint8Array(await result.arrayBuffer()));
    equal(frame.deviceId, desktop!.id);
    ok(frame.observationId && frame.observationId !== control.observationId, "Linux click returned a new observation receipt");
    const png = Buffer.from(frame.png);
    equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    deepEqual([png.readUInt32BE(16), png.readUInt32BE(20)], [1280, 800]);
    console.log("Linux workbench click returned a fresh 1280×800 PNG");
  }
  const components = join(home, "components", "android");
  const [identity] = readdirSync(components);
  ok(identity, "installed Android capsule");
  const adb = join(components, identity, "sdk", "platform-tools", "adb.exe");
  function android(...args: string[]) {
    const result = Bun.spawnSync([adb, "-P", "15037", "-s", "127.0.0.1:5565", ...args], { env });
    equal(result.exitCode, 0, "installed Android ADB command");
    return result.stdout;
  }
  equal(android("shell", "id", "-u").toString().trim(), "0");
  equal(desktopSetupExit, 0, "old signed version loaded the workbench");

  if (baseline) {
    const expectedName = `update-ownership-${crypto.randomUUID()}.txt`;
    const guestFolder = "/home/ai/Files";
    const updateStart = performance.now();
    const diskIdentity = join(home, "user", "qemu", "disk", "base.sha256");
    const legacyIdentity = readFileSync(diskIdentity, "utf8").trim();
    match(legacyIdentity, /^[a-f0-9]{64}$/);
    const staged = new FormData();
    staged.append("location", new Blob([toBinary(FileLocationSchema, create(FileLocationSchema, {path: "/"}))], {type: protocol}));
    staged.append("file", new File([expectedName], expectedName, {type: "text/plain"}));
    const uploaded = await fetch(`${origin}/api/files/upload`, {method: "POST", headers: {cookie, origin}, body: staged, signal: AbortSignal.timeout(10000)});
    equal(uploaded.status, 200, "saved prior workspace file");
    const location = fromBinary(FileLocationSchema, new Uint8Array(await uploaded.arrayBuffer()));
    const copy = await fetch(`${origin}/api/files/manage`, {method: "POST", headers: {cookie, origin, "content-type": protocol},
      body: toBinary(FileManageRequestSchema, create(FileManageRequestSchema, {operation: FileOperation.COPY, sources: [location], target: {path: guestFolder, deviceId: desktop.id}})),
      signal: AbortSignal.timeout(15000)});
    equal(copy.status, 200);
    const copied = fromBinary(FileManageResultSchema, new Uint8Array(await copy.arrayBuffer())).results[0];
    equal(copied?.error, "", "wrote prior Linux overlay file through workbench");
    ok(copied?.target?.path, "Linux copy returned its stored path");
    android("shell", `printf %s '${expectedName}' > /sdcard/Download/${expectedName}`);
    const updatePattern = "AImanity-" + candidate + "-windows-x64-setup.exe";
    const updateFolder = process.env.AIMANITY_QA_UPDATE_CACHE!;
    mkdirSync(updateFolder, {recursive: true});
    const helperFile = join(updateFolder, "ai-phone-update-helper.exe");
    copyFileSync(program, helperFile);
    copyFileSync(join(root, updatePattern), join(updateFolder, updatePattern));
    const helper = Bun.spawn([helperFile, "--install-client-update", String(app.pid), candidate], {
      env, stdin: "ignore", stdout: "pipe", stderr: "inherit",
    });
    const ready = helper.stdout.getReader();
    equal((await ready.read()).value?.[0], 1, "the installed client's signed-updater helper acquired the official installation and EXE");
    ready.releaseLock();
    const close = Bun.spawnSync(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", `(Get-Process -Id ${app.pid}).CloseMainWindow()`], {env});
    equal(close.stdout.toString().trim(), "True", "close the previous signed client so the updater installs and relaunches");
    equal(await helper.exited, 0, "the previous signed client's update helper installer and relaunch exit code");
    equal(readFileSync(diskIdentity, "utf8").trim(), legacyIdentity, "previous Linux QCOW backing identity survives");
    const release = JSON.parse(readFileSync(join(install, "host", "qemu", "manifest.json"), "utf8"));
    equal(release.source_commit, process.env.CALCIUM_SOURCE, "new source release layer");
    const deadline = performance.now() + 600_000;
    let upgradedCookie = "";
    do {
      const deployed = JSON.parse(readFileSync(checkpoint, "utf8"));
      if (deployed.payload !== deployed.installed_payload || deployed.progress.state !== "ready") {
        await Bun.sleep(750);
        continue;
      }
      const response = await fetch(`${origin}/api/auth/enter`, {method: "POST", headers: {origin, "content-type": protocol}, body: new Uint8Array(), signal: AbortSignal.timeout(2000)}).catch(() => undefined);
      if (response?.ok) upgradedCookie = response.headers.get("set-cookie")?.split(";", 1)[0] ?? "";
      if (upgradedCookie) break;
      await Bun.sleep(750);
    } while (performance.now() < deadline);
    ok(upgradedCookie, "native updater reopened the newly deployed workbench");
    async function checkRetained(id: string, path: string, file: string) {
      const listing = await fetch(`${origin}/api/files/list`, {method: "POST", headers: {cookie: upgradedCookie, origin, "content-type": protocol},
        body: toBinary(FileLocationSchema, create(FileLocationSchema, {deviceId:id,path})), signal: AbortSignal.timeout(20000)});
      equal(listing.status, 200, `new workbench lists prior ${path}`);
      const {entries} = fromBinary(FileListingSchema, new Uint8Array(await listing.arrayBuffer()));
      ok(entries.some(entry => entry.path === file && entry.sizeBytes === BigInt(expectedName.length)), `prior ${path} retained exact file length and name`);
    }
    await checkRetained("", "/", location.path);
    await checkRetained(desktop.id, guestFolder, copied.target.path);
    equal(android("shell", "cat", `/sdcard/Download/${expectedName}`).toString(), expectedName, "prior virtual phone retained exact content");
    console.log(JSON.stringify({ retainedAfterUpgrade: baseline, workspace: true, linux: true, android: true }));
    await verifyLinuxControl(upgradedCookie);
    await Promise.all([
      verifyFrame("/api/desktop/media?device_id=" + encodeURIComponent(desktop.id), "Linux after update", upgradedCookie),
      verifyFrame("/api/phone/media/screen?device=" + encodeURIComponent(phone.id), "Android after update", upgradedCookie),
    ]);
    console.log(JSON.stringify({upgrade: baseline + " → " + candidate, method: "native updater helper with QA-staged signed EXE", release: release.release_sha256,
      workspace: true, linux: true, android: true, ms: Math.round(performance.now() - updateStart)}));
  }

} catch (error) {
  const network = Bun.spawnSync(["powershell.exe", "-NoProfile", "-Command", "Get-NetTCPConnection -LocalPort 17860 -ErrorAction SilentlyContinue | Select-Object State,OwningProcess | Format-Table -HideTableHeaders; Get-Process ai-phone-client,qemu-system-x86_64 -ErrorAction SilentlyContinue | Select-Object ProcessName,Id,StartTime | Format-Table -HideTableHeaders"], { env, timeout: 12000 });
  console.error("Loopback and process state at failure:\n" + network.stdout.toString());
  for (const log of logPaths) {
    const file = join(home, log);
    if (existsSync(file)) {
      const output = readFileSync(file, "utf8");
      const milestones = output.split("\n").filter(line => /adbd|boot_completed|keyguard|SurfaceFlinger|panic|WHPX|SystemServer|FATAL EXCEPTION/i.test(line)).slice(-32);
      console.error(log, milestones.join("\n"), output.slice(-3600));
    }
  }
  throw error;
} finally {
  clearInterval(watchWeb);
  clearInterval(watchAndroid);
  if (app.exitCode === null) app.kill();
}
