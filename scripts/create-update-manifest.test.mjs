import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import packageJson from "../package.json" with { type: "json" };

test("creates a platform manifest from public artifact URLs and signatures", async () => {
  const directory = await mkdtemp(join(tmpdir(), "launchone-manifest-"));
  try {
    const macSignature = join(directory, "mac.sig");
    const windowsSignature = join(directory, "windows.sig");
    const output = join(directory, "latest.json");
    await writeFile(macSignature, "mac-signature\n");
    await writeFile(windowsSignature, "windows-signature\n");
    const result = spawnSync(process.execPath, ["scripts/create-update-manifest.mjs"], {
      cwd: new URL("..", import.meta.url),
      encoding: "utf8",
      env: {
        ...process.env,
        LAUNCHONE_UPDATE_MACOS_URL: "https://cdn.example.test/launchone-macos.tar.gz",
        LAUNCHONE_UPDATE_MACOS_SIGNATURE_FILE: macSignature,
        LAUNCHONE_UPDATE_WINDOWS_URL: "https://cdn.example.test/launchone-setup.exe",
        LAUNCHONE_UPDATE_WINDOWS_SIGNATURE_FILE: windowsSignature,
        LAUNCHONE_UPDATE_MANIFEST_OUTPUT: output,
      },
    });
    assert.equal(result.status, 0, result.stderr);
    const manifest = JSON.parse(await readFile(output, "utf8"));
    assert.equal(manifest.version, packageJson.version);
    assert.equal(manifest.platforms["darwin-aarch64"].signature, "mac-signature");
    assert.equal(manifest.platforms["darwin-x86_64"].signature, "mac-signature");
    assert.equal(manifest.platforms["windows-x86_64-nsis"].signature, "windows-signature");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
