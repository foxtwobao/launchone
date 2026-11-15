import { createHash, randomUUID } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, copyFile, link, mkdir, open, readFile, readdir, rename, rm, stat } from "node:fs/promises";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { installerFile, installerSchema, manifestPath } from "../server/installers.mjs";
import { allAdapters, sourceOverride } from "./installer-sources.mjs";
import { downloadOfficial } from "./installer-download.mjs";
import { githubConfig, verifyOnGithub } from "./installer-github.mjs";

const day = 86400_000;
export async function atomicJson(file, value) {
  await mkdir(dirname(file), { recursive: true, mode: 0o750 });
  const temp = `${file}.${randomUUID()}.tmp`;
  const output = await open(temp, "wx", 0o640);
  try { await output.writeFile(`${JSON.stringify(value, null, 2)}\n`); await output.sync(); }
  finally { await output.close(); }
  await rename(temp, file);
}
export async function digest(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}
async function readJson(file) {
  try { return JSON.parse(await readFile(file, "utf8")); }
  catch (error) { if (error.code === "ENOENT") return null; throw error; }
}
export function compareVersion(a, b) {
  // App CFBundle versions/MSIX identities must be numeric. Unknown order fails
  // closed rather than accidentally treating a pre-release as newer.
  if (!/^\d+(?:\.\d+)*$/.test(a) || !/^\d+(?:\.\d+)*$/.test(b)) throw new Error("Cannot safely compare installer versions");
  const x = a.split(".").map(Number), y = b.split(".").map(Number);
  if (![...x, ...y].every(Number.isSafeInteger)) throw new Error("Installer version component is too large");
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0) ? 1 : -1;
  }
  return 0;
}
export async function publishInstaller(root, candidate, report, candidateFile, now = Date.now()) {
  const manifest = installerSchema.parse({ app: candidate.app, platform: candidate.platform, arch: candidate.arch,
    format: candidate.format, sha256: candidate.sha256, size: candidate.size,
    supportedArchs: report.supportedArchs, minOsVersion: report.minOsVersion, version: report.version, build: report.build,
    file: installerFile({ ...candidate, version: report.version, build: report.build }),
  });
  compareVersion(manifest.version, manifest.version);
  compareVersion(manifest.build, manifest.build);
  const currentPath = manifestPath(root, manifest.app, manifest.platform, manifest.arch);
  const previous = await readJson(currentPath);
  if (previous) {
    const ordered = compareVersion(manifest.version, previous.version) || compareVersion(manifest.build, previous.build);
    if (ordered < 0) throw new Error("Older installer cannot replace current version");
    if (manifest.sha256 === previous.sha256 && await digest(join(root, "files", previous.file)).then(hash => hash === manifest.sha256).catch(() => false)) return previous;
  }
  if ((await stat(candidateFile)).size !== manifest.size || await digest(candidateFile) !== manifest.sha256) throw new Error("Candidate changed after native verification");
  const destination = join(root, "files", manifest.file);
  await mkdir(dirname(destination), { recursive: true, mode: 0o750 });
  const staging = `${destination}.${randomUUID()}.tmp`;
  try {
    await copyFile(candidateFile, staging, 1);
    await chmod(staging, 0o640);
    const output = await open(staging, "r+");
    try { await output.sync(); } finally { await output.close(); }
    try {
      // Atomic, no-replace publication on the same filesystem. The final path
      // is never visible while bytes are still being copied.
      await link(staging, destination);
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (await digest(destination) !== manifest.sha256) {
        const published = await readJson(join(dirname(destination), "published.json"));
        if (published) throw new Error("Published installer bytes changed");
        await rm(destination);
        await link(staging, destination);
      }
    }
  } finally { await rm(staging, { force: true }); }
  const metadata = { ...manifest, publishedAt: now, verification: { requestId: candidate.requestId, runId: candidate.runId, workflowSha: candidate.workflowSha } };
  await atomicJson(join(dirname(destination), "published.json"), metadata);
  await atomicJson(join(dirname(destination), "verification.json"), report);
  // Retention tracks retirement, not download time. Current+previous are pinned.
  const historyPath = join(root, "history", manifest.app, manifest.platform, `${manifest.arch}.json`);
  const history = (await readJson(historyPath)) || [];
  const nextHistory = history.filter(item => item.file !== manifest.file && item.file !== previous?.file);
  if (previous) nextHistory.unshift({ file: previous.file, retiredAt: now });
  await atomicJson(currentPath, metadata);
  await atomicJson(historyPath, nextHistory);
  return metadata;
}
export async function cleanupInstallers(root, now = Date.now()) {
  for (const adapter of allAdapters()) {
    const current = await readJson(manifestPath(root, adapter.app, adapter.platform, adapter.arch));
    const historyPath = join(root, "history", adapter.app, adapter.platform, `${adapter.arch}.json`);
    const history = (await readJson(historyPath)) || [];
    const keep = [];
    for (const [index, item] of history.entries()) {
      if (item.file === current?.file || index === 0 || now - item.retiredAt < 7 * day) { keep.push(item); continue; }
      // Validate every path before using persisted metadata as a deletion target.
      const file = item.file;
      if (typeof file !== "string" || !file.startsWith(`${adapter.app}/${adapter.platform}/${adapter.arch}/`)
        || file.split("/").some(part => !part || part === "." || part === ".." || part.includes("\\")) || file.split("/").length !== 6) {
        throw new Error("Invalid retention history path");
      }
      await rm(dirname(join(root, "files", file)), { recursive: true, force: true });
    }
    await atomicJson(historyPath, keep);
  }
  const pendingRoot = join(root, "pending");
  for (const entry of await readdir(pendingRoot, { withFileTypes: true }).catch(error => error.code === "ENOENT" ? [] : Promise.reject(error))) {
    if (!entry.isDirectory() || !/^[a-f0-9-]{36}$/.test(entry.name)) continue;
    const path = join(pendingRoot, entry.name);
    if (now - (await stat(path)).mtimeMs > 7 * day) await rm(path, { recursive: true, force: true });
  }
}
export async function synchronize(root, env = process.env, { download = downloadOfficial, verify = verifyOnGithub, now = Date.now, log = console.log } = {}) {
  if (!isAbsolute(root || "")) throw new Error("Configure an absolute LAUNCHONE_INSTALLER_DIRECTORY");
  if (!["lab", "production"].includes(env.LAUNCHONE_INSTALLER_ENVIRONMENT)) throw new Error("Configure LAUNCHONE_INSTALLER_ENVIRONMENT as lab or production");
  const github = githubConfig(env);
  let failed = false;
  for (const adapter of allAdapters()) {
    if (!adapter.dependenciesVerified) { log(JSON.stringify({ app: adapter.app, platform: adapter.platform, arch: adapter.arch, status: "unavailable", reason: "Native publisher/dependencies not yet verified" })); continue; }
    const jobPath = join(root, "jobs", `${adapter.app}-${adapter.platform}-${adapter.arch}.json`);
    try {
      const sourceUrl = sourceOverride(adapter, env);
      let candidate = await readJson(jobPath);
      let pending;
      if (candidate && candidate.deadline > now() && candidate.environment === env.LAUNCHONE_INSTALLER_ENVIRONMENT && candidate.sourceUrl === sourceUrl) {
        pending = join(root, "pending", candidate.requestId);
        if (await digest(join(pending, "package")) !== candidate.sha256) throw new Error("Persisted candidate changed");
      } else {
        const requestId = randomUUID();
        pending = join(root, "pending", requestId);
        await mkdir(pending, { recursive: true, mode: 0o750 });
        const current = await readJson(manifestPath(root, adapter.app, adapter.platform, adapter.arch));
        const sourceState = await readJson(`${jobPath}.source`);
        const currentUsable = current && await digest(join(root, "files", installerSchema.parse(current).file)).then(hash => hash === current.sha256).catch(() => false);
        const downloaded = await download(adapter, sourceUrl, join(pending, "package"), {
          previous: currentUsable && sourceState?.sourceUrl === sourceUrl && current.sha256 === sourceState.sha256 ? sourceState : null,
        });
        if (downloaded.unchanged || (currentUsable && current.sha256 === downloaded.sha256)) {
          if (!downloaded.unchanged) await atomicJson(`${jobPath}.source`, { sourceUrl, sha256: downloaded.sha256, etag: downloaded.etag, lastModified: downloaded.lastModified });
          await rm(pending, { recursive: true, force: true });
          await rm(jobPath, { force: true });
          log(JSON.stringify({ app: adapter.app, platform: adapter.platform, arch: adapter.arch, status: "unchanged" }));
          continue;
        }
        candidate = { requestId, app: adapter.app, platform: adapter.platform, arch: adapter.arch, format: adapter.format, sourceUrl,
          sha256: downloaded.sha256, size: downloaded.size, environment: env.LAUNCHONE_INSTALLER_ENVIRONMENT,
          deadline: now() + 30 * 60_000, etag: downloaded.etag, lastModified: downloaded.lastModified };
        await atomicJson(jobPath, candidate);
      }
      const report = await verify(candidate, pending, github, { now, save: value => atomicJson(jobPath, value) });
      const published = await publishInstaller(root, candidate, report, join(pending, "package"), now());
      await atomicJson(`${jobPath}.source`, { sourceUrl, sha256: candidate.sha256, etag: candidate.etag, lastModified: candidate.lastModified });
      await rm(jobPath, { force: true });
      await rm(pending, { recursive: true, force: true });
      log(JSON.stringify({ app: adapter.app, platform: adapter.platform, arch: adapter.arch, status: "published", version: published.version, sha256: published.sha256, runId: candidate.runId }));
    } catch (error) {
      failed = true;
      // Only our own fixed errors reach logs; request URLs, credentials and
      // upstream response bodies are never included.
      log(JSON.stringify({ app: adapter.app, platform: adapter.platform, arch: adapter.arch, status: "failed", reason: error instanceof Error ? error.message.replace(/https?:\/\/\S+/g, "[url]") : "Synchronization failed" }));
    }
  }
  await cleanupInstallers(root, now());
  return !failed;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = process.env.LAUNCHONE_INSTALLER_DIRECTORY;
  if (!isAbsolute(root || "")) throw new Error("Configure absolute installer directory");
  await mkdir(root, { recursive: true, mode: 0o750 });
  if (!process.argv.includes("--locked")) {
    // Kernel-owned advisory lock is released even after SIGKILL/reboot. All
    // synchronization and cleanup share it, including manual CLI invocations.
    const child = spawn("flock", ["--nonblock", "--conflict-exit-code", "75", join(root, ".sync.lock"), process.execPath, fileURLToPath(import.meta.url), "--locked"], { stdio: "inherit" });
    child.once("error", () => { console.error("Installer synchronization requires util-linux flock"); process.exitCode = 1; });
    child.once("exit", code => { process.exitCode = code === 75 ? 0 : code ?? 1; });
  } else process.exitCode = await synchronize(root) ? 0 : 1;
}
