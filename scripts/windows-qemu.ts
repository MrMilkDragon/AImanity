import { prepareWindowsQemu } from "./tooling/windows-qemu";

const [destination, calcium, ...rest] = process.argv.slice(2);
if (!destination || rest.length)
  throw Error("Usage: scripts/windows-qemu.ts <new-output-dir> [Calcium-installer.exe]");

console.log(JSON.stringify(await prepareWindowsQemu(destination, calcium)));
