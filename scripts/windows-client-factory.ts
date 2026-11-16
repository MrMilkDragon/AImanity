import { ROOT } from "./tooling/process";
import { windowsFactory } from "./tooling/windows-factory";

const [command, ...args] = process.argv.slice(2);
if (command !== "build-qemu") throw Error("Expected build-qemu");

const option = (name: string) => {
  const offset = args.indexOf(name), value = args[offset + 1];
  if (offset === -1 || !value || value.startsWith("--"))
    throw Error(name + " is required");
  return value;
};

const root = args.includes("--root") ? option("--root") : ROOT;
const result = await windowsFactory(root, {
  qemu: option("--qemu"),
  qemuIdentity: option("--qemu-identity"),
  guest: option("--guest"),
  guestIdentity: option("--guest-identity"),
}, args.includes("--sign-release"));
console.log(JSON.stringify(result));
