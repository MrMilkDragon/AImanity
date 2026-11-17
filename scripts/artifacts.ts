import { ROOT } from "./tooling/process";
import { buildNative } from "./tooling/native-build";
const [operation, ...args] = process.argv.slice(2);
const root = args.includes("--root") ? args[args.indexOf("--root") + 1]! : ROOT;
if (operation !== "native") throw Error("Expected artifacts native");
console.log(JSON.stringify(await buildNative(root)));
