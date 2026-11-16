import { buildWslBackend } from "./tooling/wsl-qualification";
const result = await buildWslBackend(undefined, true);
console.log(JSON.stringify(result.report));
