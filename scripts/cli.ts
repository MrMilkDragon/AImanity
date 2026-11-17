import {
  readFileSync,
  existsSync,
  mkdirSync,
  writeFileSync,
  statfsSync,
  mkdtempSync,
  rmSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { homedir, totalmem } from "node:os";
import { ROOT, run, nativeProgram } from "./tooling/process";
import {
  environment,
  profile,
  startExecutionServices,
  stopServices,
} from "./tooling/native";
import { stageSource } from "./tooling/client-payload";

const [operation, ...args] = process.argv.slice(2),
  option = (name: string) =>
    args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
const profileFile = join(ROOT, "data/deployment/profile.json");
function configure() {
  const supplied = option("--profile"),
    env = environment();
  const value = supplied
    ? JSON.parse(readFileSync(supplied, "utf8"))
    : existsSync(profileFile)
      ? JSON.parse(readFileSync(profileFile, "utf8"))
      : {
          schema_version: 1,
          local_entry: true,
          locale: "en",
          launch_path: env.PATH,
          sdk_root: env.AI_PHONE_SDK_ROOT,
          java_home: env.JAVA_HOME,
          host_proxy: env.AI_PHONE_HOST_PROXY,
          container_proxy: env.AI_PHONE_CONTAINER_PROXY,
          phone: {
            template:
              env.AI_PHONE_ANDROID_ABI === "x86_64"
                ? "android35-x86_64"
                : "android35-arm64",
            cpus: 2,
            memory_mb: 2048,
            storage_gb: 16,
          },
          desktop: { template: "debian13-openbox", cpus: 2, memory_mb: 2560 },
        };
  if (!supplied) {
    value.locale = prompt(
      "Language (en, zh-CN, zh-TW, ja, ko, de, fr, es, ru, ar)",
      value.locale,
    );
    for (const key of [
      "sdk_root",
      "java_home",
      "host_proxy",
      "container_proxy",
    ])
      value[key] = prompt(key, value[key]);
    for (const section of ["phone", "desktop"])
      for (const key of section === "phone"
        ? ["cpus", "memory_mb", "storage_gb"]
        : ["cpus", "memory_mb"])
        value[section][key] = Number(
          prompt(section + " " + key, String(value[section][key])),
        );
  }
  profile(ROOT, "write", [], value);
  return value;
}
function openClient() {
  if (process.platform !== "darwin") throw Error("Open the native client from Windows Start");
  run(["open", "-a", "AImanity"]);
}
switch (operation) {
  case "doctor": {
    const disk = statfsSync(ROOT),
      checks = [
        {
          name: "host",
          passed:
            (process.platform === "darwin" && process.arch === "arm64") ||
            (process.platform === "linux" && process.arch === "x64"),
        },
        { name: "native-services", passed: existsSync(nativeProgram(ROOT)) },
        ...(["docker", "cargo"] as const).map((name) => ({
          name,
          passed: !!Bun.which(name),
        })),
        { name: "memory", passed: totalmem() >= 12 * 1024 ** 3 },
        { name: "disk", passed: disk.bavail * disk.bsize >= 35 * 1024 ** 3 },
      ];
    console.log(
      args.includes("--json")
        ? JSON.stringify(checks)
        : checks.map((c) => (c.passed ? "OK " : "FAIL ") + c.name).join("\n"),
    );
    if (checks.some((c) => !c.passed)) process.exitCode = 1;
    break;
  }
  case "configure":
    console.log(JSON.stringify(configure()));
    break;
  case "install": {
    if (process.platform !== "darwin" && process.platform !== "linux")
      throw Error("Use the Windows installer");
    const value = existsSync(profileFile)
      ? JSON.parse(readFileSync(profileFile, "utf8"))
      : configure();
    const resource =
        option("--resources") || join(ROOT, "apps/native/gen/host"),
      integrity = JSON.parse(
        readFileSync(join(resource, "integrity.json"), "utf8"),
      );
    const home =
      option("--destination") ||
      join(
        process.platform === "darwin"
          ? join(homedir(), "Library/Application Support")
          : process.env.XDG_DATA_HOME || join(homedir(), ".local/share"),
        "dev.aimanity.native/host",
      );
    mkdirSync(dirname(profileFile), { recursive: true, mode: 0o700 });
    const temporary = mkdtempSync(join(dirname(profileFile), ".request-")),
      request = join(temporary, "request.json");
    try {
      writeFileSync(
        request,
        JSON.stringify({
          locale: value.locale,
          phone: {
            cpus: value.phone.cpus,
            memory_mb: value.phone.memory_mb,
            storage_gb: value.phone.storage_gb,
          },
          desktop: {
            cpus: value.desktop.cpus,
            memory_mb: value.desktop.memory_mb,
          },
        }),
        { mode: 0o600, flag: "wx" },
      );
      run([
        nativeProgram(ROOT),
        "--initialize-unix-host",
        resource,
        home,
        request,
        integrity["source.payload"],
        integrity["android.payload"],
        integrity["images.payload"],
      ]);
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
    break;
  }
  case "start":
    startExecutionServices();
    break;
  case "stop":
    stopServices();
    break;
  case "open":
    openClient();
    break;
  case "env":
    for (const [key, value] of Object.entries(environment()))
      console.log(
        "export " + key + "='" + value.replaceAll("'", "'\\''") + "'",
      );
    break;
  case "stage":
    console.log(
      await stageSource(option("--archive")!, option("--destination")!),
    );
    break;
  case "backup":
    run([
      nativeProgram(ROOT),
      "--state-backup",
      "create",
      "--root",
      ROOT,
      ...args,
    ]);
    break;
  case "restore":
    run([
      nativeProgram(ROOT),
      "--state-backup",
      "restore",
      "--root",
      ROOT,
      ...args,
    ]);
    break;
  case "recover-desktop":
    run([
      nativeProgram(ROOT),
      "--state-backup",
      "recover-desktop",
      "--root",
      ROOT,
      ...args,
    ]);
    break;
  default:
    throw Error(
      "Expected doctor|configure|install|start|stop|open|env|stage|backup|restore|recover-desktop",
    );
}
