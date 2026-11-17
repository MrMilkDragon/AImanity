import { mkdirSync, copyFileSync, cpSync, existsSync, mkdtempSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { run } from "./process";
import { json } from "./inventory";
import { sealDependency, verifyDependency } from "./linux-dependencies";
import { fetchPin } from "./factory";

async function dependencyContext(
  root: string,
  folder: string,
  role: string,
  destination: string,
) {
  mkdirSync(folder, { recursive: true });
  copyFileSync(
    join(root, "scripts", role, "Dockerfile"),
    join(folder, "Dockerfile"),
  );
  copyFileSync(
    join(root, "scripts", role, "collect.ts"),
    join(folder, "collect.ts"),
  );
  const policy = json(join(root, "config/linux-factory.json"));
  await fetchPin(folder, "bootstrap-ca.deb", policy.ca_certificates);
  await Bun.write(
    join(folder, "ubuntu.sources"),
    "Types: deb\nURIs: https://snapshot.ubuntu.com/ubuntu/" +
      policy.snapshot +
      "\nSuites: noble noble-updates noble-security\nComponents: main universe\nArchitectures: amd64\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\nSnapshot: no\n",
  );
  for (const name of ["tooling", "linux-ocr"]) {
    cpSync(join(root, "scripts", name), join(folder, "scripts", name), {
      recursive: true,
    });
  }
  run(
    [
      "docker",
      "buildx",
      "build",
      "--platform",
      "linux/amd64",
      "--target",
      "export",
      "--output",
      "type=local,dest=" + destination,
      folder,
    ],
    { cwd: root },
  );
}

export async function buildFactoryDependency(
  root: string,
  work: string,
  destination: string,
  role: string,
) {
  if (existsSync(destination)) return verifyDependency(root, destination, role);
  const parent = dirname(resolve(destination));
  mkdirSync(parent, { recursive: true });
  const output = mkdtempSync(join(parent, ".dependency-"));
  try {
    await dependencyContext(root, work, role, output);
    const result = sealDependency(root, output, role);
    renameSync(output, destination);
    return result;
  } finally {
    rmSync(output, { recursive: true, force: true });
  }
}
