import test from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, writeFile, symlink } from "node:fs/promises";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { createApp, readConfig } from "./app.mjs";
import { installerFile, manifestPath } from "./installers.mjs";

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "installer-service-"));
  const config = { installerDirectory: root, downloadBaseUrl: "https://downloads.test", issuer: "https://id.test", origin: "https://app.test" };
  const data = Buffer.from("official-complete-package-bytes");
  const manifest = { app: "claude-desktop", platform: "macos", arch: "aarch64", supportedArchs: ["aarch64", "x86_64"],
    minOsVersion: "12.0", version: "1.2.3", build: "123", format: "dmg", size: data.length, sha256: createHash("sha256").update(data).digest("hex") };
  manifest.file = installerFile(manifest);
  await mkdir(dirname(join(root, "files", manifest.file)), { recursive: true });
  await writeFile(join(root, "files", manifest.file), data);
  await writeFile(join(dirname(join(root, "files", manifest.file)), "published.json"), JSON.stringify(manifest));
  const path = manifestPath(root, manifest.app, manifest.platform, manifest.arch);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, JSON.stringify(manifest));
  const app = createApp(config);
  const server = app.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(async () => { app.dispose(); server.closeAllConnections(); await new Promise(r => server.close(r)); await rm(root, { recursive: true, force: true }); });
  return { root, manifest, data, config, origin: `http://127.0.0.1:${server.address().port}` };
}
test("anonymous manifest uses download origin and refuses missing/unsupported combinations", async t => {
  const f = await fixture(t);
  const response = await fetch(`${f.origin}/v1/installers/claude-desktop/macos/aarch64`);
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.url, `https://downloads.test/downloads/files/${f.manifest.file}`);
  assert.equal(body.file, undefined);
  assert.equal(body.minOsVersion, "12.0");
  assert.equal((await fetch(`${f.origin}/v1/installers/claude-desktop/macos/x86_64`)).status, 503);
  assert.equal((await fetch(`${f.origin}/v1/installers/codex/windows/x86_64`)).status, 404);
  assert.equal((await fetch(`${f.origin}/v1/installers/unknown/macos/aarch64`)).status, 404);
});
test("immutable stream supports strong ETag, HEAD, valid Range/If-Range and range failures", async t => {
  const f = await fixture(t);
  const url = `${f.origin}/downloads/files/${f.manifest.file}`;
  const response = await fetch(url);
  const etag = `"${f.manifest.sha256}"`;
  assert.equal(response.headers.get("etag"), etag);
  assert.equal(await response.text(), f.data.toString());
  const range = await fetch(url, { headers: { Range: "bytes=4-10", "If-Range": etag } });
  assert.equal(range.status, 206);
  assert.equal(range.headers.get("content-range"), `bytes 4-10/${f.data.length}`);
  assert.equal(await range.text(), f.data.subarray(4, 11).toString());
  const stale = await fetch(url, { headers: { Range: "bytes=4-", "If-Range": '"different"' } });
  assert.equal(stale.status, 200);
  assert.equal(await stale.text(), f.data.toString());
  const invalid = await fetch(url, { headers: { Range: "bytes=99999-" } });
  assert.equal(invalid.status, 416);
  assert.equal(invalid.headers.get("content-range"), `bytes */${f.data.length}`);
  const head = await fetch(url, { method: "HEAD" });
  assert.equal(head.headers.get("content-length"), String(f.data.length));
  assert.equal(await head.text(), "");
});
test("never exposes unpublished, private, symlinked or mismatched files", async t => {
  const f = await fixture(t);
  const url = `${f.origin}/downloads/files/${f.manifest.file}`;
  const record = join(dirname(join(f.root, "files", f.manifest.file)), "published.json");
  assert.equal((await fetch(url.replace("Claude.dmg", "published.json"))).status, 404);
  await rm(record);
  assert.equal((await fetch(url)).status, 404);
  await writeFile(record, JSON.stringify(f.manifest));
  const file = join(f.root, "files", f.manifest.file);
  await rm(file);
  await symlink(join(f.root, "secret"), file);
  await writeFile(join(f.root, "secret"), f.data);
  assert.equal((await fetch(url)).status, 404);
  assert.equal((await fetch(`${f.origin}/v1/installers/claude-desktop/macos/aarch64`)).status, 503);
});
test("mirror config requires complete HTTPS origin and absolute storage, no implicit fallback", () => {
  const base = { LAUNCHONE_PUBLIC_ORIGIN: "https://app.test", IDONE_ISSUER: "https://id.test", RESELLER_ORIGIN: "https://r.test", RESELLER_GATEWAY_ORIGIN: "https://g.test", RESELLER_APP_CREDENTIAL: "long-test-credential", NODE_ENV: "production" };
  assert.equal(readConfig(base).installerDirectory, undefined);
  assert.throws(() => readConfig({ ...base, LAUNCHONE_INSTALLER_DIRECTORY: "/var/installers" }));
  assert.throws(() => readConfig({ ...base, LAUNCHONE_DOWNLOAD_BASE_URL: "http://downloads.test", LAUNCHONE_INSTALLER_DIRECTORY: "/var/installers" }));
  assert.throws(() => readConfig({ ...base, LAUNCHONE_DOWNLOAD_BASE_URL: "https://downloads.test/path", LAUNCHONE_INSTALLER_DIRECTORY: "/var/installers" }));
  assert.equal(readConfig({ ...base, LAUNCHONE_DOWNLOAD_BASE_URL: "https://downloads.test", LAUNCHONE_INSTALLER_DIRECTORY: "/var/installers" }).downloadBaseUrl, "https://downloads.test");
});
