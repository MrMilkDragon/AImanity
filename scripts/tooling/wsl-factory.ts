import {
  mkdirSync,
  existsSync,
  lstatSync,
  readFileSync,
  renameSync,
  writeFileSync,
  mkdtempSync,
  rmSync,
  copyFileSync,
  cpSync,
  createReadStream,
  createWriteStream,
  openSync,
  readSync,
  closeSync,
  statfsSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { randomUUID, createHash } from "node:crypto";
import { pipeline } from "node:stream/promises";
import { createGzip } from "node:zlib";
import { Readable } from "node:stream";
import { digest, run, output } from "./process";
import { json, save, collect, records } from "./inventory";
import { tarEntries, entryBytes } from "./archive";
import { databaseImage, fetchPin, type prepareImages } from "./factory";
import { packageInputs } from "./container-host";
import {
  publishWsl,
  phoneSchema,
  phoneManifest,
  phoneRequired,
  qemuBootstrapContract,
} from "./wsl-capsule";

export function wslPlan(root: string) {
  const policy = json(join(root, "config/wsl-backend.json")),
    engine = json(join(root, "config/container-host-runtime.json")).hosts[
      "linux-x64"
    ];
  if (
    policy.format !== 1 ||
    policy.host !== "windows-x64" ||
    policy.backend_host !== "linux-x64" ||
    !/^ubuntu@sha256:[0-9a-f]{64}$/.test(policy.base_image)
  )
    throw Error("Unsupported WSL backend policy");
  return {
    ...policy,
    engine_image: engine.builder_image,
    redis_image: databaseImage(root, "redis"),
    engine_packages: engine.packages,
    engine_versions: { docker: engine.docker, compose: engine.compose },
  };
}
export async function sourceInput(file: string, root: string) {
  const files: Record<string, any> = {};
  let manifest: any,
    services: any,
    total = 0;
  for await (const entry of tarEntries(file, "xz", root)) {
    if (entry.type === "5") continue;
    if (entry.type !== "0" || Object.hasOwn(files, entry.name))
      throw Error("Invalid source payload member");
    if (entry.name === "HOST-MANIFEST.json") {
      manifest = JSON.parse((await entryBytes(entry)).toString());
      continue;
    }
    const hash = createHash("sha256"),
      chunks: Buffer[] = [],
      metadata = entry.name === "runtime/services/manifest.json";
    for await (const chunk of entry.body) {
      hash.update(chunk);
      if (metadata) chunks.push(chunk);
    }
    files[entry.name] = {
      path: entry.name,
      size: entry.size,
      mode: entry.mode,
      sha256: hash.digest("hex"),
    };
    total += entry.size;
    if (metadata) services = JSON.parse(Buffer.concat(chunks).toString());
  }
  const components = [...manifest.components].sort();
  const [servicesRole, phoneRole, ...otherRoles] = components;
  if (
    servicesRole !== "native-services" ||
    (phoneRole !== undefined && phoneRole !== "phone-apps") ||
    otherRoles.length !== 0 ||
    services?.host !== "linux-x64" ||
    services?.vision !== "linux-tesseract-bundled" ||
    Object.keys(files).length !== manifest.files.length ||
    manifest.files.some((row: any) => {
      const actual = files[row.path];
      return (
        !actual ||
        ["size", "mode", "sha256"].some((key) => actual[key] !== row[key])
      );
    })
  )
    throw Error(
      "WSL requires matched Linux x64 backend components and offline OCR",
    );
  const bootstrap = files["runtime/services/bin/assistant"];
  if (!bootstrap || bootstrap.mode !== 0o755)
    throw Error("WSL bootstrap missing");
  return { sha256: await digest(file), components, files, total, bootstrap };
}
export async function preparePhoneTools(root: string, work: string) {
  const pin = {
      url: "https://github.com/meator/android-tools-static/releases/download/36.0.1/android-tools-static-linux-standardlayout-x86_64.tar.gz",
      sha256: "a49e62b843b4520ad0e10cff3ef0e211b23ecaa27fdcff56f2faafef1e04c317",
      size_bytes: 18_854_175,
      version: "36.0.1",
    },
    [archive, notice] = await Promise.all([
      fetchPin(work, "platform-tools.tar.gz", pin),
      fetchPin(work, "adb-notice.b64", {
        url: "https://android.googlesource.com/platform/packages/modules/adb/+/78f769e0904c21771cccf6e4e7b239b21800e189/NOTICE?format=TEXT",
        sha256: "58698510c6f56f9ba8064e80bb8d449aa47e0a1bcef33bb91752aca4da114783",
        size_bytes: 14_260,
      }),
    ]),
    folder = join(work, "sdk"),
    platformTools = join(folder, "platform-tools"),
    adb = join(platformTools, "adb");
  mkdirSync(platformTools, { recursive: true });
  for await (const entry of tarEntries(archive, "gzip")) {
    if (entry.name !== "android-tools-static/adb" || entry.type !== "0") continue;
    await pipeline(Readable.from(entry.body), createWriteStream(adb, {
      flags: "wx",
      mode: 0o755,
    }));
    break;
  }
  writeFileSync(
    join(platformTools, "NOTICE.txt"),
    Buffer.from(readFileSync(notice, "utf8"), "base64"),
    { flag: "wx" },
  );
  writeFileSync(
    join(platformTools, "source.properties"),
    `Pkg.Desc=android-tools-static\nPkg.Revision=${pin.version}\n`,
    { flag: "wx" },
  );
  const header = Buffer.alloc(20),
    fd = openSync(adb, "r");
  try {
    readSync(fd, header);
  } finally {
    closeSync(fd);
  }
  if (
    !header.subarray(0, 7).equals(Buffer.from([127, 69, 76, 70, 2, 1, 1])) ||
    header.readUInt16LE(18) !== 62
  )
    throw Error("Phone bridge requires Linux x64 ADB");
  const vendor = join(root, "packages/protocol/vendor/android-emulator"),
    schema = json(join(vendor, "provenance.json"));
  if (
    lstatSync(join(vendor, "emulator_controller.proto")).size !==
      schema.bytes ||
    (await digest(join(vendor, "emulator_controller.proto"))) !== schema.sha256
  )
    throw Error("Emulator protocol differs");
  mkdirSync(dirname(join(folder, phoneSchema)), { recursive: true });
  copyFileSync(
    join(vendor, "emulator_controller.proto"),
    join(folder, phoneSchema),
  );
  copyFileSync(
    join(vendor, "provenance.json"),
    join(folder, "provenance.json"),
  );
  copyFileSync(join(root, "LICENSE"), join(folder, "LICENSE"));
  const files = records(collect(folder, "")).map((row) => ({
      ...row,
      path: row.path.replace(/^\//, ""),
    })),
    inventory = Object.fromEntries(
      files.map((row) => [
        row.path,
        { bytes: row.size, sha256: row.sha256, mode: row.mode },
      ]),
    );
  if (
    phoneRequired
      .filter((name) => name !== phoneManifest)
      .some((name) => !Object.hasOwn(inventory, name))
  )
    throw Error("Incomplete native phone bridge");
  save(join(folder, phoneManifest), {
    format: 1,
    host: "linux-x64",
    platform_tools_sha256: pin.sha256,
    schema_sha256: schema.sha256,
    redistribution: "review-pending",
    files: inventory,
  });
  return folder;
}
async function engineLicenses(folder: string, pins: any[]) {
  const result = [];
  for (const pin of pins) {
    let info: Record<string, string> | undefined;
    for await (const entry of tarEntries(join(folder, pin.file), "gzip"))
      if (entry.name === ".PKGINFO")
        info = Object.fromEntries(
          (await entryBytes(entry, 65536))
            .toString()
            .split("\n")
            .filter((line) => line.includes(" = "))
            .map((line) => {
              const offset = line.indexOf(" = ");
              return [line.slice(0, offset), line.slice(offset + 3)];
            }),
        );
    if (!info?.license || !info.pkgname)
      throw Error("Missing engine package attribution");
    result.push({
      package: info.pkgname,
      version: info.pkgver,
      license: info.license,
      source: info.url || "",
      archive_url: pin.url,
      sha256: pin.sha256,
    });
  }
  return result;
}
function engineScript(program: string) {
  return (
    '#!/bin/sh\nset -eu\nengine=/opt/aiphone/engine\nexport LD_LIBRARY_PATH="$engine/lib:$engine/usr/lib"\nexport XTABLES_LIBDIR="$engine/usr/lib/xtables"\nPATH="$engine/usr/bin:$engine/usr/sbin:$engine/bin:$engine/sbin:$PATH"\nexport PATH\nexec "$engine/usr/bin/' +
    program +
    '" "$@"\n'
  );
}
function redisStage(image: string) {
  return `FROM ${image} AS redis
USER root
RUN mkdir -p /native-redis/lib /native-redis/bin && \\
    cp -L /lib/ld-musl-x86_64.so.1 /native-redis/lib/ && \\
    for command in redis-server redis-cli; do \\
        binary="/usr/local/bin/$command"; \\
        cp -L "$binary" /native-redis/bin/; \\
        libraries="$(ldd "$binary")" || exit 1; \\
        for library in $(printf '%s\\n' "$libraries" | awk '$2 == "=>" && substr($3, 1, 1) == "/" {print $3}'); do \\
            cp -L "$library" /native-redis/lib/ || exit 1; \\
        done; \\
    done
RUN chroot /native-redis /lib/ld-musl-x86_64.so.1 --library-path /lib /bin/redis-server --version && \\
    chroot /native-redis /lib/ld-musl-x86_64.so.1 --library-path /lib /bin/redis-cli --version
`;
}

export function stageWindowsServices(root: string, destination: string) {
  const context = mkdtempSync(join(dirname(destination), ".windows-services-"));
  try {
    writeFileSync(
      join(context, "Dockerfile"),
      redisStage(databaseImage(root, "redis")) + `FROM postgres:17.11-bookworm@sha256:73bedc9eaa0121b92b9b26bb52e5141b3f6f37479c7ef913c0160455763ca80a AS postgres
RUN set -eu; \\
    source=/usr/lib/postgresql/17; \\
    output=/native-postgres; \\
    mkdir -p "$output/lib/postgresql/17/bin" "$output/lib/postgresql/17/lib" "$output/share/postgresql" "$output/runtime"; \\
    for binary in postgres initdb; do \\
        cp -L "$source/bin/$binary" "$output/lib/postgresql/17/bin/"; \\
    done; \\
    find "$source/lib" -maxdepth 1 -name '*.so' ! -name llvmjit.so -exec cp -L -t "$output/lib/postgresql/17/lib" {} +; \\
    cp -aL /usr/share/postgresql/17 "$output/share/postgresql/"; \\
    cp --parents -L /usr/share/doc/*/copyright "$output/"; \\
    for object in "$output"/lib/postgresql/17/bin/* "$output"/lib/postgresql/17/lib/*.so; do \\
        libraries=$(ldd "$object"); \\
        if printf '%s\\n' "$libraries" | grep -q 'not found'; then exit 1; fi; \\
        for library in $(printf '%s\\n' "$libraries" | awk '$2 == "=>" && substr($3, 1, 1) == "/" {print $3}'); do \\
            case "$library" in \\
                */libc.so.*|*/libpthread.so.*|*/libm.so.*|*/libdl.so.*|*/librt.so.*|*/libresolv.so.*|*/libutil.so.*) ;; \\
                *) cp -L "$library" "$output/runtime/" ;; \\
            esac; \\
        done; \\
    done
FROM ${wslPlan(root).base_image} AS postgres-check
COPY --from=postgres /native-postgres/ /opt/aiphone/postgres/
RUN set -eu; \\
    export LD_LIBRARY_PATH=/opt/aiphone/postgres/runtime; \\
    bin=/opt/aiphone/postgres/lib/postgresql/17/bin; \\
    mkdir /tmp/native-postgres; \\
    chown 65534:65534 /tmp/native-postgres; \\
    setpriv --reuid=65534 --regid=65534 --clear-groups "$bin/initdb" -D /tmp/native-postgres -U ai_phone --locale-provider=builtin --locale=C.UTF-8; \\
    "$bin/postgres" --version
FROM scratch
COPY --from=redis /native-redis/ /redis/
COPY --from=postgres-check /opt/aiphone/postgres/ /postgres/
`,
    );
    const output = join(context, "output");
    run(
      [
        "docker", "buildx", "build", "--platform", "linux/amd64",
        "--network", "none", "--provenance=false",
        "--output", "type=local,dest=" + output, context,
      ],
      { cwd: root },
    );
    for (const service of ["redis", "postgres"])
      renameSync(join(output, service), join(destination, service));
  } finally {
    rmSync(context, { recursive: true });
  }
}

function dockerfile(plan: any) {
  return `FROM ${plan.engine_image} AS engine
COPY packages/ /packages/
RUN apk --no-network add /packages/*.apk && rm -rf /packages /var/cache/apk/*
${redisStage(plan.redis_image)}FROM ${plan.base_image}
COPY --from=engine / /opt/aiphone/engine/
COPY --from=redis /native-redis/ /opt/aiphone/redis/
RUN find /opt/aiphone/engine/bin /opt/aiphone/engine/sbin /opt/aiphone/engine/usr /opt/aiphone/engine/lib /opt/aiphone/engine/etc /opt/aiphone/engine/var -type l -exec sh -c 'for path do target=$(readlink "$path"); case "$target" in /*) if [ -e "/opt/aiphone/engine$target" ] || [ -L "/opt/aiphone/engine$target" ]; then ln -snf "/opt/aiphone/engine$target" "$path"; fi;; esac; done' sh {} +
COPY phone-tools/ /opt/aiphone/native-phone-bridge/
COPY source.payload images.payload source.sha256 /opt/aiphone/bundle/
COPY bootstrap.program /usr/local/libexec/aiphone-bootstrap
COPY docker dockerd /usr/local/bin/
COPY wsl.conf /etc/wsl.conf
COPY engine-sources.json /usr/share/doc/aiphone-engine/source-records.json
RUN cp -L /usr/lib/os-release /tmp/aiphone-os-release && mv /tmp/aiphone-os-release /etc/os-release && chmod 644 /etc/os-release && chmod 755 /usr/local/bin/docker /usr/local/bin/dockerd && ln -s /opt/aiphone/engine/lib/ld-musl-x86_64.so.1 /lib/ld-musl-x86_64.so.1 && mkdir -p /usr/local/lib/docker/cli-plugins && ln -s /opt/aiphone/engine/usr/libexec/docker/cli-plugins/docker-compose /usr/local/lib/docker/cli-plugins/docker-compose
RUN printf '%s\\n' '/opt/aiphone/engine/lib:/opt/aiphone/engine/usr/lib' > /etc/ld-musl-x86_64.path
RUN /usr/bin/env -i PATH=/usr/bin:/bin /opt/aiphone/engine/usr/bin/runc --version
RUN /usr/local/bin/docker --version && /usr/local/bin/docker compose version && /usr/local/bin/dockerd --version
RUN /opt/aiphone/native-phone-bridge/platform-tools/adb version
RUN /usr/bin/setpriv --reuid=65534 --regid=65534 --clear-groups -- /opt/aiphone/redis/lib/ld-musl-x86_64.so.1 --library-path /opt/aiphone/redis/lib /opt/aiphone/redis/bin/redis-server --version && \\
    /opt/aiphone/redis/lib/ld-musl-x86_64.so.1 --library-path /opt/aiphone/redis/lib /opt/aiphone/redis/bin/redis-cli --version
RUN rm -rf /usr/share/gcc/python && rm -f /usr/share/gdb/auto-load/usr/lib/x86_64-linux-gnu/libstdc++.so.*-gdb.py /usr/share/apport/package-hooks/source_shadow.py && \
    ! command -v python && ! command -v python3 && ! /usr/bin/test -x /opt/aiphone/engine/usr/bin/python3 && \
    test -z "$(find /usr /opt/aiphone/engine -type f -name '*.py' -print -quit)"
RUN test "$(/usr/bin/timeout 10 /usr/local/libexec/aiphone-bootstrap --source-staging-contract)" = ai-phone-source-staging-v1 && test "$(/usr/bin/timeout 10 /usr/local/libexec/aiphone-bootstrap --wsl-backend-initializer-contract)" = ai-phone-wsl-initializer-v1
RUN test "$(/usr/bin/timeout 10 /usr/local/libexec/aiphone-bootstrap --qemu-guest-control-contract)" = ${qemuBootstrapContract}
ENTRYPOINT ["/usr/local/libexec/aiphone-bootstrap", "--initialize-wsl-backend"]
`;
}

export async function stageBackendSource(
  root: string,
  context: string,
  source: string,
  sourceIdentity: string,
) {
  copyFileSync(source, join(context, "source.payload"));
  await Bun.write(join(context, "source.sha256"), sourceIdentity + "\n");
  for await (const entry of tarEntries(source, "xz", root))
    if (entry.name === "runtime/services/bin/assistant") {
      const target = createWriteStream(join(context, "bootstrap.program"), {
        flags: "wx",
        mode: 0o755,
      });
      await pipeline(Readable.from(entry.body), target);
      break;
    }
}

export async function buildWsl(
  root: string,
  work: string,
  source: string,
  input: Awaited<ReturnType<typeof sourceInput>>,
  images: Awaited<ReturnType<typeof prepareImages>>,
  packages: string,
  phoneTools: string,
) {
  if (existsSync(work)) throw Error("Use a fresh WSL factory workspace");
  const plan = wslPlan(root);
  if (images.receipt.architecture !== "amd64") throw Error("WSL requires amd64 images");
  await packageInputs(packages, plan.engine_packages);
  let contextBytes =
    lstatSync(source).size + lstatSync(images.file).size + input.bootstrap.size + 65;
  for (const folder of [packages, phoneTools])
    for (const file of collect(folder, ""))
      contextBytes += file.data.length;
  const rootfsBudget = contextBytes + 1536 * 1024 ** 2,
    disk = statfsSync(dirname(work));
  if (
    disk.bavail * disk.bsize <
    4 * 1024 ** 3 + contextBytes + 4 * rootfsBudget
  )
    throw Error("Insufficient WSL factory capacity");
  const context = join(work, "context");
  mkdirSync(context, { recursive: true });
  await stageBackendSource(root, context, source, input.sha256);
  copyFileSync(images.file, join(context, "images.payload"));
  cpSync(packages, join(context, "packages"), { recursive: true });
  cpSync(phoneTools, join(context, "phone-tools"), { recursive: true });
  for (const program of ["docker", "dockerd"])
    await Bun.write(join(context, program), engineScript(program));
  await Bun.write(
    join(context, "wsl.conf"),
    "[interop]\nenabled=false\nappendWindowsPath=false\n[automount]\nenabled=false\n",
  );
  save(
    join(context, "engine-sources.json"),
    await engineLicenses(packages, plan.engine_packages),
  );
  await Bun.write(join(context, "Dockerfile"), dockerfile(plan));
  const provenance = {
    policy: plan,
    source_sha256: input.sha256,
    source_commit: output(["git", "rev-parse", "HEAD"], { cwd: root }),
    source_manifest_sha256: await digest(join(root, "SOURCE-MANIFEST.json")),
    source_components: input.components,
    images_sha256: images.receipt.archive_sha256,
    phone_tools_manifest_sha256: await digest(join(phoneTools, phoneManifest)),
    qemu_bootstrap: qemuBootstrapContract,
  };
  const nonce = randomUUID().replaceAll("-", ""),
    reference = "ai-phone-wsl-factory:" + nonce,
    name = "ai-phone-wsl-factory-" + nonce;
  run(
    [
      "docker",
      "buildx",
      "build",
      "--platform",
      "linux/amd64",
      "--network",
      "none",
      "--load",
      "--provenance=false",
      "-t",
      reference,
      context,
    ],
    { cwd: root },
  );
  const container = output([
      "docker",
      "create",
      "--name",
      name,
      "--network",
      "none",
      "--label",
      "dev.aiphone.factory=wsl-backend-factory",
      reference,
    ]),
    archive = join(work, "rootfs.tar"),
    fd = openSync(archive, "wx");
  try {
    run(["docker", "export", container], { stdio: ["ignore", fd, "inherit"] });
  } finally {
    closeSync(fd);
    run(["docker", "rm", container]);
  }
  const compressed = join(work, "rootfs.tar.gz");
  await pipeline(
    createReadStream(archive),
    createGzip({ level: 9 }),
    createWriteStream(compressed, { flags: "wx" }),
  );
  const result = await publishWsl(
    compressed,
    join(work, "wsl-backend.payload"),
    provenance,
  );
  save(join(work, "receipt.json"), result);
  return result;
}
