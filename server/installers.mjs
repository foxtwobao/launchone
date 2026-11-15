import { lstat, readFile, realpath } from "node:fs/promises";
import { join, relative, sep } from "node:path";
import { z } from "zod";

export const apps = ["codex", "claude-desktop"];
export const platforms = ["macos", "windows"];
export const architectures = ["aarch64", "x86_64"];
const segment = /^[A-Za-z0-9][A-Za-z0-9._+-]{0,95}$/;
export const installerSchema = z.object({
  app: z.enum(apps), platform: z.enum(platforms), arch: z.enum(architectures),
  supportedArchs: z.array(z.enum(architectures)).min(1).max(2),
  minOsVersion: z.string().regex(/^\d+(?:\.\d+){1,3}$/),
  version: z.string().regex(segment), build: z.string().regex(segment),
  format: z.enum(["dmg", "msix"]), size: z.number().int().positive().max(4 * 1024 ** 3),
  sha256: z.string().regex(/^[a-f0-9]{64}$/), file: z.string(),
}).refine(m => m.supportedArchs.includes(m.arch))
  .refine(m => (m.platform === "macos" && m.format === "dmg") || (m.platform === "windows" && m.format === "msix"))
  .refine(m => validFilePath(m));

export function installerFile(m) {
  const name = m.app === "codex" ? "Codex" : "Claude";
  return `${m.app}/${m.platform}/${m.arch}/${m.version}-${m.build}/${m.sha256}/${name}.${m.format}`;
}
function validFilePath(m) { return m.file === installerFile(m); }
export function manifestPath(root, app, platform, arch) { return join(root, "manifests", app, platform, `${arch}.json`); }
export class InstallerError extends Error {
  constructor(status, code) { super(code); this.status = status; this.code = code; }
}
export async function regularFile(root, file) {
  const rootPath = await realpath(root);
  const components = file.split("/");
  if (components.some(c => !c || c === "." || c === ".." || c.includes("\\"))) throw new InstallerError(404, "NOT_FOUND");
  let current = rootPath;
  for (const [index, component] of components.entries()) {
    current = join(current, component);
    const entry = await lstat(current);
    if (entry.isSymbolicLink() || (index < components.length - 1 ? !entry.isDirectory() : !entry.isFile())) {
      throw new InstallerError(404, "NOT_FOUND");
    }
  }
  const resolved = await realpath(current);
  const rel = relative(rootPath, resolved);
  if (rel.startsWith(`..${sep}`) || rel === "..") throw new InstallerError(404, "NOT_FOUND");
  return resolved;
}
export function registerInstallers(app, config) {
  app.get("/v1/installers/:app/:platform/:arch", async (req, res, next) => {
    try {
      const p = req.params;
      if (!apps.includes(p.app) || !platforms.includes(p.platform) || !architectures.includes(p.arch) || (p.app === "codex" && p.platform === "windows")) {
        throw new InstallerError(404, "INSTALLER_UNSUPPORTED");
      }
      if (!config.installerDirectory || !config.downloadBaseUrl) throw new InstallerError(503, "INSTALLER_UNAVAILABLE");
      const file = await regularFile(join(config.installerDirectory, "manifests"), `${p.app}/${p.platform}/${p.arch}.json`);
      const manifest = installerSchema.parse(JSON.parse(await readFile(file, "utf8")));
      if (manifest.app !== p.app || manifest.platform !== p.platform || manifest.arch !== p.arch) throw new Error("Manifest identity mismatch");
      const artifact = await regularFile(join(config.installerDirectory, "files"), manifest.file);
      if ((await lstat(artifact)).size !== manifest.size) throw new Error("Missing installer");
      const { file: path, ...value } = manifest;
      res.set("Cache-Control", "public, max-age=60").json({ ...value, url: `${config.downloadBaseUrl}/downloads/files/${path}` });
    } catch (error) {
      if (error instanceof InstallerError && error.code === "INSTALLER_UNSUPPORTED") return res.status(error.status).json({ error: error.code, retryable: false });
      res.status(503).json({ error: "INSTALLER_UNAVAILABLE", retryable: true });
    }
  });
  app.all("/downloads/files/:app/:platform/:arch/:version/:sha256/:filename", async (req, res, next) => {
    if (!["GET", "HEAD"].includes(req.method)) return res.status(405).end();
    try {
      if (!config.installerDirectory) throw new InstallerError(404, "NOT_FOUND");
      const p = req.params;
      if (!apps.includes(p.app) || !platforms.includes(p.platform) || !architectures.includes(p.arch)
        || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,192}$/.test(p.version) || !/^[a-f0-9]{64}$/.test(p.sha256) || !/^(Codex|Claude)\.(dmg|msix)$/.test(p.filename)) {
        throw new InstallerError(404, "NOT_FOUND");
      }
      const path = `${p.app}/${p.platform}/${p.arch}/${p.version}/${p.sha256}/${p.filename}`;
      const file = await regularFile(join(config.installerDirectory, "files"), path);
      const metadataFile = await regularFile(join(config.installerDirectory, "files"), `${path.slice(0, path.lastIndexOf("/"))}/published.json`);
      const metadata = installerSchema.parse(JSON.parse(await readFile(metadataFile, "utf8")));
      if (metadata.file !== path || (await lstat(file)).size !== metadata.size) throw new InstallerError(404, "NOT_FOUND");
      res.set({ ETag: `"${metadata.sha256}"`, "Cache-Control": "public, max-age=31536000, immutable", "Content-Type": "application/octet-stream" });
      res.sendFile(file, { etag: false, lastModified: false, cacheControl: false, acceptRanges: true }, error => {
        if (error && !res.headersSent) res.status(error.status === 416 ? 416 : 404).end();
      });
    } catch {
      res.status(404).json({ error: "NOT_FOUND", retryable: false });
    }
  });
}
