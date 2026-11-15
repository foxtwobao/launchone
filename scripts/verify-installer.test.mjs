import test from "node:test";
import assert from "node:assert/strict";
import { validateCandidate, validateDownloadUrl, machMinimumVersions, verifyInstaller } from "./verify-installer.mjs";
import { getAdapter } from "./installer-sources.mjs";

const now = Date.now();
const candidate = {
  requestId: "unique-request-123456", environment: "lab", app: "codex", platform: "macos", arch: "aarch64",
  sourceUrl: "https://persistent.oaistatic.com/codex-app-prod/ChatGPT.dmg", sha256: "1".repeat(64),
  size: 1024, format: "dmg", deadline: now + 3600000,
};

test("verification candidate is bound to request, known platform, hash, size and deadline", () => {
  assert.equal(validateCandidate(candidate, candidate.requestId, now).adapter.teamId, "2DC432GLL2");
  for (const change of [
    { requestId: "other-request-123456" }, { environment: "external" }, { app: "unknown" }, { platform: "linux" },
    { arch: "universal" }, { format: "exe" }, { size: 0 }, { size: 2 ** 40 }, { sha256: "not-a-hash" },
    { deadline: now - 1 }, { sourceUrl: "https://attacker.test/installer.dmg" },
  ]) assert.throws(() => validateCandidate({ ...candidate, ...change }, candidate.requestId, now));
});

test("each installer redirect must remain on an adapter's official HTTPS allowlist", () => {
  const adapter = getAdapter("claude-desktop", "macos", "x86_64");
  assert.equal(validateDownloadUrl("https://downloads.claude.ai/file.dmg", adapter).hostname, "downloads.claude.ai");
  for (const url of ["http://downloads.claude.ai/file.dmg", "https://downloads.claude.ai.attacker.test/file.dmg",
    "https://secret@downloads.claude.ai/file.dmg", "https://downloads.claude.ai:8443/file.dmg", "https://downloads.claude.ai/file.dmg#fragment"]) {
    assert.throws(() => validateDownloadUrl(url, adapter));
  }
});

test("workflow commit mismatch and reruns are rejected before any package download", async () => {
  const workflowSha = "2".repeat(40);
  const env = { INSTALLER_REQUEST_ID: candidate.requestId, INSTALLER_CANDIDATE: JSON.stringify({ ...candidate, workflowSha }),
    GITHUB_SHA: "3".repeat(40), GITHUB_RUN_ID: "42", GITHUB_RUN_ATTEMPT: "1" };
  await assert.rejects(verifyInstaller(env), /workflow execution identity/);
  await assert.rejects(verifyInstaller({ ...env, GITHUB_SHA: workflowSha, GITHUB_RUN_ATTEMPT: "2" }), /workflow execution identity/);
});

test("Mach-O system requirements ignore dylib compatibility and SDK versions", () => {
  const sample = `Load command 1
          cmd LC_BUILD_VERSION
      cmdsize 32
     platform 1
        minos 12.0
          sdk 15.0
Load command 2
          cmd LC_LOAD_DYLIB
      cmdsize 88
         name /usr/lib/libSystem.B.dylib (offset 24)
   time stamp 2 Thu Jan  1 00:00:02 1970
      current version 1345.100.2
compatibility version 1.0.0
Load command 3
          cmd LC_VERSION_MIN_MACOSX
      cmdsize 16
      version 11.0
          sdk 13.3
`;
  assert.deepEqual(machMinimumVersions(sample), ["12.0", "11.0"]);
});
