import test from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";
import { windowsSigningConfig } from "./package-windows.mjs";

test("Windows lab packages do not enable native signing", () => {
  assert.deepEqual(windowsSigningConfig("lab", {}), {});
});

test("production requires complete Botone-compatible Windows signing configuration", () => {
  assert.throws(() => windowsSigningConfig("production", {}), /DESKTOP_WIN_AZURE_CLIENT_ID.*DESKTOP_WIN_PUBLISHER/);
  const env = Object.fromEntries([
    "DESKTOP_WIN_AZURE_CLIENT_ID", "DESKTOP_WIN_AZURE_TENANT_ID", "DESKTOP_WIN_SIGNING_ENDPOINT",
    "DESKTOP_WIN_SIGNING_ACCOUNT", "DESKTOP_WIN_SIGNING_PROFILE", "DESKTOP_WIN_PUBLISHER"
  ].map(name => [name, "configured"]));
  const directory = "directory with spaces";
  const { signCommand } = windowsSigningConfig("production", env, directory);
  assert.equal(signCommand.cmd, "pwsh");
  assert.deepEqual(signCommand.args.slice(-4), ["-File", join(directory, "scripts", "windows-signing.ps1"), "-Path", "%1"]);
  assert.throws(() => windowsSigningConfig("production", { ...env, DESKTOP_WIN_PUBLISHER: " " }), /DESKTOP_WIN_PUBLISHER/);
  assert.throws(() => windowsSigningConfig("other", env), /明确选择/);
});
