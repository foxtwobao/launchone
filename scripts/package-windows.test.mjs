import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import * as packaging from "./package-windows.mjs";

test("exports the installer and its updater signature with matching short names", () => {
  const bundle = mkdtempSync(join(tmpdir(), "launchone-nsis-"));
  const output = join(bundle, "output");
  try {
    const installer = "LaunchOne_0.2.5_x64-setup.exe";
    writeFileSync(join(bundle, installer), "installer bytes");
    writeFileSync(join(bundle, `${installer}.sig`), "signature bytes");

    assert.equal(typeof packaging.exportWindowsInstaller, "function");
    packaging.exportWindowsInstaller(bundle, output, "0.2.5");

    const filename = "LaunchOne_0.2.5.exe";
    const files = [[filename, "installer bytes"], [`${filename}.sig`, "signature bytes"]];
    for (const [name, contents] of files) {
      assert.equal(readFileSync(join(output, name), "utf8"), contents);
    }
    assert.equal(readFileSync(join(output, "SHA256SUMS.txt"), "utf8"), files.map(([name, contents]) =>
      `${createHash("sha256").update(contents).digest("hex")}  ${name}\n`).join(""));
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});
