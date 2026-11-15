import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { downloadOfficial } from "./installer-download.mjs";
import { getAdapter, allowedSource } from "./installer-sources.mjs";
import { atomicJson, cleanupInstallers, compareVersion, publishInstaller } from "./sync-installers.mjs";
import { manifestPath } from "../server/installers.mjs";
import { validateReport, verifyOnGithub } from "./installer-github.mjs";

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "installer-sync-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}
function candidate(data, suffix = "1") {
  return { requestId: `request-${suffix}`, environment: "lab", app: "codex", platform: "macos", arch: "aarch64", format: "dmg", size: data.length,
    sha256: createHash("sha256").update(data).digest("hex"), workflowSha: "a".repeat(40), runId: 123, deadline: Date.now() + 60_000 };
}
function report(c, version = "1.0") {
  return { ...c, runAttempt: 1, version, build: "1", minOsVersion: "12.0", supportedArchs: ["aarch64"],
    bundleId: "com.openai.codex", teamId: "2DC432GLL2", publisher: "Developer ID Application: OpenAI", checks: Object.fromEntries(["signature", "publisher", "identity", "architecture", "system", "dependencies"].map(k => [k, true])) };
}
test("official download follows only allowlisted redirects and never sends service credentials", async t => {
  const root = await fixture(t);
  const calls = [];
  const adapter = getAdapter("codex", "macos", "aarch64");
  const downloaded = await downloadOfficial(adapter, adapter.sourceUrl, join(root, "package"), { request: async (url, options) => {
    calls.push({ url: String(url), options });
    if (calls.length === 1) return new Response(null, { status: 302, headers: { location: "https://persistent.oaistatic.com/another.dmg" } });
    return new Response("complete-package", { headers: { "content-length": "16", "content-type": "application/octet-stream" } });
  } });
  assert.equal(downloaded.size, 16);
  assert.ok(calls.every(c => !c.options.headers.Authorization && c.options.redirect === "manual"));
  await assert.rejects(downloadOfficial(adapter, adapter.sourceUrl, join(root, "bad"), { request: async () => new Response(null, { status: 302, headers: { location: "https://attacker.test/pkg.dmg" } }) }), /allowed official/);
  await assert.rejects(downloadOfficial(adapter, adapter.sourceUrl, join(root, "html"), { request: async () => new Response("<html>error", { headers: { "content-type": "application/octet-stream" } }) }), /not a package/);
  assert.throws(() => allowedSource("https://user:pass@persistent.oaistatic.com/a", adapter));
});
test("publication is immutable, rejects corruption/downgrade, retains current+previous and retirement grace", async t => {
  const root = await fixture(t);
  const file = join(root, "candidate");
  const now = 2_000_000_000_000;
  const records = [];
  for (const [i, version] of ["1.0", "2.0", "3.0"].entries()) {
    const data = Buffer.from(`package-${version}`);
    await writeFile(file, data);
    const c = candidate(data, String(i));
    records.push(await publishInstaller(root, c, report(c, version), file, now + i));
  }
  const currentPath = manifestPath(root, "codex", "macos", "aarch64");
  const current = JSON.parse(await readFile(currentPath, "utf8"));
  await writeFile(file, "tampered");
  await assert.rejects(publishInstaller(root, candidate(Buffer.from("expected")), report(candidate(Buffer.from("expected")), "4.0"), file, now + 3), /Candidate changed/);
  assert.equal(JSON.parse(await readFile(currentPath, "utf8")).sha256, current.sha256);
  await writeFile(file, "old package");
  const old = candidate(Buffer.from("old package"));
  await assert.rejects(publishInstaller(root, old, report(old, "1.0"), file), /Older installer/);
  await cleanupInstallers(root, now + 6 * 86400_000);
  await readFile(join(root, "files", records[0].file));
  await cleanupInstallers(root, now + 8 * 86400_000);
  await assert.rejects(readFile(join(root, "files", records[0].file)), { code: "ENOENT" });
  await readFile(join(root, "files", records[1].file));
  await readFile(join(root, "files", records[2].file));
  if (process.platform !== "win32") {
    assert.equal((await stat(join(root, "files", records[2].file))).mode & 0o777, 0o640);
    assert.equal((await stat(dirname(join(root, "files", records[2].file)))).mode & 0o777, 0o750);
  }
  assert.equal(compareVersion("12.10", "12.9"), 1);
});
test("native reports bind content, task, publisher and all checks", () => {
  const c = candidate(Buffer.from("package"));
  const good = report(c);
  assert.equal(validateReport(c, { id: c.runId }, good), good);
  for (const delta of [{ sha256: "b".repeat(64) }, { requestId: "other" }, { environment: "production" }, { teamId: "attacker" }, { runAttempt: 2 }, { checks: { signature: true } }]) {
    assert.throws(() => validateReport(c, { id: c.runId }, { ...good, ...delta }));
  }
});
test("GitHub verification rejects wrong ref, workflow commit, run and rerun before reading artifacts", async t => {
  const root = await fixture(t);
  const config = { repo: "owner/project", ref: "main", token: "secret-test", workflow: "verify-installers.yml" };
  for (const delta of [{ run_attempt: 2 }, { head_sha: "b".repeat(40) }, { event: "pull_request" }, { workflow_id: 999 }, { head_repository: { full_name: "attacker/project" } }]) {
    const c = { ...candidate(Buffer.from("package")), repository: config.repo, ref: "main", workflowId: 7 };
    const run = { id: c.runId, workflow_id: 7, head_sha: c.workflowSha, head_branch: "main", event: "workflow_dispatch", run_attempt: 1,
      head_repository: { full_name: config.repo }, display_title: `installer-verify-${c.requestId}`, path: ".github/workflows/verify-installers.yml", status: "completed", conclusion: "success", ...delta };
    let requests = 0;
    await assert.rejects(verifyOnGithub(c, root, config, { request: async (url, opts) => {
      requests++; assert.equal(opts.headers.Authorization, "Bearer secret-test"); return Response.json(run);
    } }), /Untrusted verifier run/);
    assert.equal(requests, 1);
  }
});

function reportZip(text) {
  const name = Buffer.from("report.json"), data = Buffer.from(text);
  let crc = 0xffffffff;
  for (const b of data) {
    crc ^= b;
    for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  crc = (crc ^ 0xffffffff) >>> 0;
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(name.length, 26);
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
  central.writeUInt32LE(crc, 16); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24); central.writeUInt16LE(name.length, 28);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(1, 8); end.writeUInt16LE(1, 10);
  end.writeUInt32LE(central.length + name.length, 12); end.writeUInt32LE(local.length + name.length + data.length, 16);
  return Buffer.concat([local, name, data, central, name, end]);
}
test("automatic dispatch authenticates run and downloads its sole report without leaking GitHub token", async t => {
  const root = await fixture(t);
  const config = { repo: "owner/project", ref: "main", token: "secret-test", workflow: "verify-installers.yml" };
  const c = candidate(Buffer.from("package"));
  delete c.workflowSha; delete c.runId;
  const saved = [];
  const run = () => ({ id: 123, workflow_id: 7, head_sha: "a".repeat(40), head_branch: "main", event: "workflow_dispatch", run_attempt: 1,
    head_repository: { full_name: config.repo }, display_title: `installer-verify-${c.requestId}`, path: ".github/workflows/verify-installers.yml", status: "completed", conclusion: "success" });
  const verified = await verifyOnGithub(c, root, config, { save: async value => saved.push(structuredClone(value)), request: async (value, options) => {
    const url = String(value);
    if (url === "https://reports.blob.core.windows.net/signed") {
      assert.equal(options.headers, undefined);
      assert.equal(options.redirect, "error");
      return new Response(reportZip(JSON.stringify(report(c))));
    }
    assert.equal(options.headers.Authorization, "Bearer secret-test");
    if (url.endsWith("/branches/main")) return Response.json({ protected: true, commit: { sha: "a".repeat(40) } });
    if (url.endsWith("/actions/workflows/verify-installers.yml")) return Response.json({ id: 7, path: ".github/workflows/verify-installers.yml", state: "active" });
    if (url.endsWith("/dispatches")) {
      assert.equal(JSON.parse(options.body).inputs.request_id, c.requestId);
      return new Response(null, { status: 204 });
    }
    if (url.includes("/actions/workflows/7/runs?")) return Response.json({ workflow_runs: [run()] });
    if (url.endsWith("/actions/runs/123")) return Response.json(run());
    if (url.includes("/actions/runs/123/artifacts?")) return Response.json({ artifacts: [{ id: 9, name: `installer-report-${c.requestId}`, size_in_bytes: 2000, expired: false, workflow_run: { id: 123 } }] });
    if (url.endsWith("/actions/artifacts/9/zip")) return new Response(null, { status: 302, headers: { location: "https://reports.blob.core.windows.net/signed" } });
    throw new Error("Unexpected API request");
  } });
  assert.equal(verified.sha256, c.sha256);
  assert.equal(saved.length, 2);
  assert.equal(saved[1].runId, 123);
});
