import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, dirname, resolve, relative, isAbsolute } from "node:path";
import { digest, output } from "./process";
import { json } from "./inventory";

function metadata(root: string, manifest: string, noDependencies = false, target?: string) {
  return JSON.parse(
    output(
      [
        "cargo",
        "metadata",
        "--manifest-path",
        join(root, manifest),
        "--locked",
        "--format-version",
        "1",
        ...(noDependencies ? ["--no-deps"] : []),
        ...(target ? ["--filter-platform", target] : []),
      ],
      { cwd: root },
    ),
  );
}
function notices(folder: string) {
  return readdirSync(folder, { withFileTypes: true })
    .filter(entry => entry.isFile() && /^(LICENSE|LICENCE|COPYING|NOTICE|COPYRIGHT)/i.test(entry.name))
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(entry => join(folder, entry.name));
}
function within(root: string, file: string) {
  const name = relative(root, file);
  if (name === ".." || name.startsWith("../") || isAbsolute(name))
    throw Error("Notice path outside its source tree");
}
async function supplements(root: string, manifest: string, pkg: any, folder: string) {
  const config = join(root, "config/native-notice-supplements.json");
  if (!existsSync(config)) return [];
  const key = pkg.name + "@" + pkg.version,
    match = json(config).packages.find((row: any) => row.package === key);
  if (!match) return [];
  if (pkg.repository !== match.repository)
    throw Error("Supplemental notice repository differs: " + key);
  if (match.commit) {
    if (json(join(folder, ".cargo_vcs_info.json")).git.sha1 !== match.commit)
      throw Error("Supplemental source differs: " + key);
  } else {
    const lock = Bun.TOML.parse(
        readFileSync(join(root, dirname(manifest), "Cargo.lock"), "utf8"),
      ) as any,
      row = lock.package.find(
        (row: any) => row.name === pkg.name && row.version === pkg.version,
      );
    if (!row?.checksum || row.checksum !== match.archive_sha256)
      throw Error("Supplemental archive differs: " + key);
    for (const item of match.grant_files) {
      const path = resolve(folder, item.file);
      within(folder, path);
      if ((await digest(path)) !== item.sha256)
        throw Error("Supplemental grant differs: " + key);
    }
  }
  const result = [];
  for (const item of match.files) {
    const path = resolve(root, item.file);
    within(join(root, "docs/public/licenses/native"), path);
    if ((await digest(path)) !== item.sha256)
      throw Error("Supplemental notice differs: " + key);
    result.push({ path, url: item.url });
  }
  return result;
}
export async function rustNotices(
  root: string,
  manifest: string,
  component: string,
  target?: string,
) {
  const graph = metadata(root, manifest, false, target),
    owned = new Set([
      ...metadata(root, "Cargo.toml", true).workspace_members,
      ...graph.workspace_members,
    ]),
    blocks = [
      "AImanity " +
        component +
        ` dependency notices\n\nLocked build graph${target ? " for " + target : ""}, including build dependencies. Component licences remain independent.\n`,
    ],
    missing: string[] = [];
  for (const pkg of graph.packages.sort(
    (a: any, b: any) =>
      a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
  )) {
    if (owned.has(pkg.id)) continue;
    const folder = dirname(pkg.manifest_path),
      paths = notices(folder);
    if (pkg.license_file) {
      const path = resolve(folder, pkg.license_file);
      if (existsSync(path) && !paths.includes(path)) paths.push(path);
    }
    const extra = await supplements(root, manifest, pkg, folder);
    if (!paths.length && !extra.length)
      missing.push(pkg.name + "@" + pkg.version);
    blocks.push(
      "\n" +
        "=".repeat(72) +
        "\n" +
        pkg.name +
        " " +
        pkg.version +
        "\nSPDX: " +
        (pkg.license || "see licence file") +
        "\nSource: " +
        (pkg.repository || "https://crates.io/crates/" + pkg.name) +
        "\nAttribution: " +
        (pkg.authors?.join(", ") || "See retained upstream grant/source") +
        "\n",
    );
    for (const path of paths)
      blocks.push(
        "\n--- " +
          path.split(/[\\/]/).at(-1) +
          " ---\n" +
          readFileSync(path, "utf8"),
      );
    for (const item of extra)
      blocks.push(
        "\n--- " +
          item.path.split(/[\\/]/).at(-1) +
          " (pinned grant or linked full licence text) ---\n" +
          item.url +
          "\n" +
          readFileSync(item.path, "utf8"),
      );
  }
  const runtime = join(root, "config/rust-runtime-notices.json");
  if (existsSync(runtime))
    for (const pkg of json(runtime).packages) {
      blocks.push(
        "\n" +
          "=".repeat(72) +
          "\n" +
          pkg.package +
          "\nStandard-library copyright and accompanying terms; compiler tools are not bundled.\n",
      );
      for (const item of pkg.files) {
        const file = join(root, item.file);
        if ((await digest(file)) !== item.sha256)
          throw Error("Rust runtime notice differs");
        blocks.push(
          "\n--- " +
            item.file.split("/").at(-1) +
            " ---\n" +
            item.url +
            "\n" +
            readFileSync(file, "utf8"),
        );
      }
    }
  return {
    text: blocks.join("\n").replace(/\r\n/g, "\n").replace(/[\t ]+$/gm, ""),
    missing,
  };
}
