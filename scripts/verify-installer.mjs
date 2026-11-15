import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createReadStream } from "node:fs";
import { mkdtemp, open, readdir, realpath, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { getAdapter } from "./installer-sources.mjs";

const CHECKS = ["signature", "publisher", "identity", "architecture", "system", "dependencies"];
const MAX_SIZE = 4 * 1024 ** 3;

export function validateCandidate(value, requestId, now = Date.now()) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || !/^[A-Za-z0-9_-]{16,128}$/.test(requestId || "") || value.requestId !== requestId
    || !["lab", "production"].includes(value.environment)
    || !["macos", "windows"].includes(value.platform)
    || !["x86_64", "aarch64"].includes(value.arch)
    || !/^[a-f0-9]{64}$/.test(value.sha256 || "")
    || !Number.isSafeInteger(value.size) || value.size < 1024 || value.size > MAX_SIZE) {
    throw new Error("Invalid installer verification candidate");
  }
  const deadline = typeof value.deadline === "number" ? value.deadline : Date.parse(value.deadline);
  if (!Number.isFinite(deadline) || deadline <= now || deadline > now + 24 * 60 * 60 * 1000) {
    throw new Error("Installer verification request expired or deadline is invalid");
  }
  const adapter = getAdapter(value.app, value.platform, value.arch);
  if (!adapter || value.format !== adapter.format) throw new Error("Unsupported installer format or platform");
  validateDownloadUrl(value.sourceUrl, adapter);
  return { candidate: value, adapter, deadline };
}

export function validateDownloadUrl(value, adapter) {
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.hash
    || (url.port && url.port !== "443") || !adapter.allowedHosts.includes(url.hostname)) {
    throw new Error("Installer source is outside the official HTTPS allowlist");
  }
  return url;
}

export function numericVersion(value) {
  if (typeof value !== "string" || !/^\d+(?:\.\d+){0,3}$/.test(value)) throw new Error("Invalid numeric system version");
  const parts = value.split(".").map(Number);
  if (!parts.every(Number.isSafeInteger)) throw new Error("Invalid numeric system version");
  return parts;
}
function compareNumeric(a, b) {
  const x = numericVersion(a), y = numericVersion(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) - (y[i] || 0);
  }
  return 0;
}

export function machMinimumVersions(loadCommands) {
  return loadCommands.split(/^Load command \d+\s*$/m).flatMap(block => {
    if (/^\s*cmd LC_BUILD_VERSION\s*$/m.test(block)) return [block.match(/^\s*minos\s+(\d+(?:\.\d+){1,3})\s*$/m)?.[1]].filter(Boolean);
    if (/^\s*cmd LC_VERSION_MIN_MACOSX\s*$/m.test(block)) return [block.match(/^\s*version\s+(\d+(?:\.\d+){1,3})\s*$/m)?.[1]].filter(Boolean);
    return [];
  });
}

async function download(candidate, adapter, target, deadline) {
  let url = validateDownloadUrl(candidate.sourceUrl, adapter);
  const signal = AbortSignal.timeout(Math.max(1, Math.min(deadline - Date.now(), 20 * 60 * 1000)));
  for (let redirect = 0; redirect <= 5; redirect++) {
    const response = await fetch(url, { redirect: "manual", signal, headers: { "Accept-Encoding": "identity" } });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location || redirect === 5) throw new Error("Invalid installer redirect");
      url = validateDownloadUrl(new URL(location, url).href, adapter);
      continue;
    }
    if (response.status !== 200 || !response.body || /text\/html/i.test(response.headers.get("content-type") || "")) {
      await response.body?.cancel();
      throw new Error("Official source did not return a complete installer");
    }
    const declared = response.headers.get("content-length");
    if (declared && Number(declared) !== candidate.size) {
      await response.body.cancel();
      throw new Error("Official installer changed since discovery");
    }
    const file = await open(target, "wx", 0o600);
    const hash = createHash("sha256");
    let size = 0;
    try {
      for await (const chunk of response.body) {
        size += chunk.length;
        if (size > candidate.size) throw new Error("Official installer exceeds candidate size");
        hash.update(chunk);
        let offset = 0;
        while (offset < chunk.length) {
          const written = (await file.write(chunk, offset)).bytesWritten;
          if (!written) throw new Error("Installer download could not be written to disk");
          offset += written;
        }
      }
      await file.sync();
    } finally { await file.close(); }
    if (size !== candidate.size || hash.digest("hex") !== candidate.sha256) {
      throw new Error("Official installer hash does not match candidate");
    }
    return;
  }
  throw new Error("Installer download failed");
}

function command(name, args, options = {}) {
  const result = spawnSync(name, args, { encoding: "utf8", timeout: 120_000, maxBuffer: 4 * 1024 ** 2, ...options });
  if (result.error || result.status !== 0) throw new Error(`Installer verification command failed: ${basename(name)}`);
  return result;
}
function text(value, label) {
  if (typeof value !== "string" || !value.trim() || value.length > 256 || /[\r\n\0]/.test(value)) {
    throw new Error(`Invalid installer ${label}`);
  }
  return value;
}

async function inspectMac(file, adapter, candidate, directory) {
  if (process.platform !== "darwin") throw new Error("macOS verification requires a macOS runner");
  if (!/^[A-Z0-9]{10}$/.test(adapter.teamId) || !Array.isArray(adapter.identities) || !adapter.identities.length) {
    throw new Error("macOS adapter has no trusted signing identity");
  }
  const mount = join(directory, "mounted");
  command("/bin/mkdir", [mount]);
  let attached = false;
  try {
    command("/usr/bin/hdiutil", ["attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint", mount, file]);
    attached = true;
    const candidates = (await readdir(mount, { withFileTypes: true })).filter(entry => entry.isDirectory() && entry.name.endsWith(".app"));
    const apps = [];
    for (const entry of candidates) {
      const app = join(mount, entry.name);
      const info = JSON.parse(command("/usr/bin/plutil", ["-convert", "json", "-o", "-", join(app, "Contents/Info.plist")]).stdout);
      if (adapter.identities.includes(info.CFBundleIdentifier)) apps.push({ app, info });
    }
    if (apps.length !== 1) throw new Error("Installer has no unique expected application");
    const { app, info } = apps[0];
    command("/usr/bin/codesign", ["--verify", "--deep", "--strict", "-R", `anchor apple generic and certificate leaf[subject.OU] = "${adapter.teamId}"`, app]);
    command("/usr/sbin/spctl", ["--assess", "--type", "execute", app]);
    const signature = command("/usr/bin/codesign", ["-d", "--verbose=4", app]);
    const signatureText = signature.stdout + signature.stderr;
    const teamId = signatureText.match(/^TeamIdentifier=(.+)$/m)?.[1];
    const signingIdentity = signatureText.match(/^Identifier=(.+)$/m)?.[1];
    const publisher = signatureText.match(/^Authority=(Developer ID Application: .+)$/m)?.[1];
    if (teamId !== adapter.teamId || signingIdentity !== info.CFBundleIdentifier || !publisher) {
      throw new Error("Unexpected macOS publisher or signing identifier");
    }
    const executableName = text(info.CFBundleExecutable, "executable");
    if (basename(executableName) !== executableName || [".", ".."].includes(executableName)) throw new Error("Invalid app executable path");
    const executable = await realpath(join(app, "Contents/MacOS", executableName));
    const inside = relative(await realpath(app), executable);
    if (inside.startsWith("..") || resolve(app, inside) !== executable || !(await stat(executable)).isFile()) throw new Error("App executable escapes the bundle");
    const rawArchs = command("/usr/bin/lipo", ["-archs", executable]).stdout.trim().split(/\s+/);
    const supportedArchs = [...new Set(rawArchs.map(arch => arch === "arm64" ? "aarch64" : arch).filter(arch => ["aarch64", "x86_64"].includes(arch)))];
    if (!supportedArchs.includes(candidate.arch)) throw new Error("Installer does not contain the requested architecture");
    const nativeArch = candidate.arch === "aarch64" ? "arm64" : "x86_64";
    const minimums = [info.LSMinimumSystemVersion, info.LSMinimumSystemVersionByArchitecture?.[nativeArch]].filter(Boolean);
    const loadCommands = command("/usr/bin/otool", ["-arch", nativeArch, "-l", executable]).stdout;
    minimums.push(...machMinimumVersions(loadCommands));
    if (!minimums.length) throw new Error("Installer minimum system version is missing");
    minimums.forEach(numericVersion);
    const minOsVersion = minimums.sort(compareNumeric).at(-1);
    // The adapter asserts that its official DMG contains the full application;
    // this covers installation prerequisites, not every online application feature.
    if (adapter.dependenciesVerified !== true) throw new Error("Installer dependencies have not been verified");
    return {
      version: text(info.CFBundleShortVersionString, "version"), build: text(info.CFBundleVersion, "build"),
      minOsVersion, supportedArchs, publisher, bundleId: info.CFBundleIdentifier, identity: info.CFBundleIdentifier, teamId,
      checks: Object.fromEntries(CHECKS.map(check => [check, true])),
    };
  } finally {
    if (attached) command("/usr/bin/hdiutil", ["detach", mount]);
  }
}

async function inspectWindows(file, adapter, candidate) {
  if (process.platform !== "win32") throw new Error("Windows verification requires a Windows runner");
  if (!adapter.publisher || !adapter.identities?.length || adapter.dependenciesVerified !== true) {
    throw new Error("Windows publisher and complete installation dependencies must be verified before publishing");
  }
  const result = command("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
    resolve(import.meta.dirname, "verify-installer-windows.ps1")], {
    env: { ...process.env, INSTALLER_PATH: file, INSTALLER_PUBLISHER: adapter.publisher, INSTALLER_IDENTITIES: JSON.stringify(adapter.identities), INSTALLER_ARCH: candidate.arch },
  });
  const metadata = JSON.parse(result.stdout.replace(/^\uFEFF/, "").trim());
  if (metadata.publisher !== adapter.publisher || !adapter.identities.includes(metadata.identity)
    || !metadata.supportedArchs?.includes(candidate.arch) || !CHECKS.every(check => metadata.checks?.[check] === true)) {
    throw new Error("Native Windows verification report is incomplete");
  }
  numericVersion(metadata.minOsVersion);
  return metadata;
}

export async function verifyInstaller(env = process.env) {
  const { candidate, adapter, deadline } = validateCandidate(JSON.parse(env.INSTALLER_CANDIDATE || "null"), env.INSTALLER_REQUEST_ID);
  const workflowSha = env.GITHUB_SHA;
  const runId = Number(env.GITHUB_RUN_ID), runAttempt = Number(env.GITHUB_RUN_ATTEMPT);
  if (!/^[a-f0-9]{40}$/.test(workflowSha || "") || candidate.workflowSha !== workflowSha
    || !Number.isSafeInteger(runId) || runId <= 0 || runAttempt !== 1) {
    throw new Error("Invalid workflow execution identity");
  }
  const directory = await mkdtemp(join(tmpdir(), "launchone-verification-"));
  try {
    const file = join(directory, `candidate.${candidate.format}`);
    await download(candidate, adapter, file, deadline);
    const metadata = candidate.platform === "macos"
      ? await inspectMac(file, adapter, candidate, directory) : await inspectWindows(file, adapter, candidate);
    if (Date.now() >= deadline) throw new Error("Installer verification request expired");
    const hash = createHash("sha256");
    for await (const bytes of createReadStream(file)) hash.update(bytes);
    if ((await stat(file)).size !== candidate.size || hash.digest("hex") !== candidate.sha256) throw new Error("Candidate file changed during verification");
    const report = {
      requestId: candidate.requestId, environment: candidate.environment, runId, runAttempt, workflowSha, commit: workflowSha,
      app: candidate.app, platform: candidate.platform, arch: candidate.arch, sha256: candidate.sha256, size: candidate.size, format: candidate.format,
      ...metadata,
    };
    await writeFile("report.json", `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });
    return report;
  } finally { await rm(directory, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  verifyInstaller().catch(error => { console.error(error.message); process.exitCode = 1; });
}
