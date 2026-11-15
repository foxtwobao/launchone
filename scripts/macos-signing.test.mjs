import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { generateKeyPairSync } from "node:crypto";
import { withMacSigning } from "./macos-signing.mjs";

const { privateKey: apiKeyPem } = generateKeyPairSync("ec", {
  namedCurve: "prime256v1",
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
  publicKeyEncoding: { type: "spki", format: "pem" },
});
const secrets = {
  MACOS_DEVELOPER_ID_P12_BASE64: "Zml4dHVyZS1wMTI=",
  MACOS_DEVELOPER_ID_P12_PASSWORD: " fixture password ",
  APP_STORE_CONNECT_KEY_ID: "FIXTUREKEY",
  APP_STORE_CONNECT_ISSUER_ID: "fixture-issuer",
  APP_STORE_CONNECT_API_KEY_BASE64: Buffer.from(apiKeyPem).toString("base64"),
};
const ambient = {
  APPLE_CERTIFICATE: "wrong-certificate",
  APPLE_CERTIFICATE_PASSWORD: "wrong-password",
  APPLE_ID: "wrong-id",
  APPLE_PASSWORD: "wrong-password",
  APPLE_TEAM_ID: "WRONGTEAM",
  APPLE_SIGNING_IDENTITY: "wrong-identity",
  APPLE_API_KEY: "wrong-key",
  APPLE_API_KEY_PATH: "/wrong/key.p8",
  APPLE_API_KEY_P8: "raw legacy PEM",
  DESKTOP_MAC_CSC_LINK: "legacy-certificate",
  DESKTOP_MAC_CSC_KEY_PASSWORD: "legacy-password",
};

test("production maps Botone secrets to Tauri and scopes a private P8 file to the build", async () => {
  let keyPath;
  const input = { ...ambient, ...secrets, TAURI_SIGNING_PRIVATE_KEY: "updater-key" };
  const result = await withMacSigning("production", input, async env => {
    keyPath = env.APPLE_API_KEY_PATH;
    assert.equal(env.APPLE_CERTIFICATE, secrets.MACOS_DEVELOPER_ID_P12_BASE64);
    assert.equal(env.APPLE_CERTIFICATE_PASSWORD, secrets.MACOS_DEVELOPER_ID_P12_PASSWORD);
    assert.equal(env.APPLE_API_KEY, secrets.APP_STORE_CONNECT_KEY_ID);
    assert.equal(env.APPLE_API_ISSUER, secrets.APP_STORE_CONNECT_ISSUER_ID);
    assert.equal(readFileSync(keyPath, "utf8"), apiKeyPem);
    if (process.platform !== "win32") {
      assert.equal(statSync(keyPath).mode & 0o777, 0o600);
      assert.equal(statSync(dirname(keyPath)).mode & 0o777, 0o700);
    }
    for (const key of ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID", "APPLE_SIGNING_IDENTITY", "APPLE_API_KEY_P8", "DESKTOP_MAC_CSC_LINK", "DESKTOP_MAC_CSC_KEY_PASSWORD", ...Object.keys(secrets)]) {
      assert.equal(env[key], undefined, key);
    }
    assert.equal(env.TAURI_SIGNING_PRIVATE_KEY, "updater-key");
    await Promise.resolve();
    return "built";
  });
  assert.equal(result, "built");
  assert.equal(existsSync(dirname(keyPath)), false);
  assert.equal(input.APPLE_API_KEY_PATH, "/wrong/key.p8");
});

test("production removes the private key when a build throws", async () => {
  let keyPath;
  const error = new Error("build failed");
  await assert.rejects(withMacSigning("production", secrets, async env => {
    keyPath = env.APPLE_API_KEY_PATH;
    await Promise.resolve();
    throw error;
  }), candidate => candidate === error);
  assert.equal(existsSync(dirname(keyPath)), false);
});

test("production rejects each missing secret before creating files or starting a build", async () => {
  const temporaryRoot = mkdtempSync(join(tmpdir(), "launchone-signing-test-"));
  try {
    for (const key of Object.keys(secrets)) {
      await assert.rejects(withMacSigning("production", { ...secrets, [key]: " " }, () => {
        assert.fail("build must not start");
      }, temporaryRoot), error => error.message.includes(key) && !error.message.includes("fixture password"));
      assert.deepEqual(readdirSync(temporaryRoot), []);
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("Lab strips ambient signing and notarization credentials without creating a key", async () => {
  const env = await withMacSigning("lab", { ...ambient, ...secrets, PATH: "/fixture/bin", TAURI_SIGNING_PRIVATE_KEY: "lab-updater-key" }, env => env, "/does-not-exist");
  assert.deepEqual(env, { PATH: "/fixture/bin", TAURI_SIGNING_PRIVATE_KEY: "lab-updater-key" });
});

test("production rejects malformed Base64 and encoded non-PEM content before starting a build", async () => {
  for (const value of [apiKeyPem, "invalid!base64", "Zg=", "Zh==", "Zg===", Buffer.from("not a private key").toString("base64")]) {
    await assert.rejects(withMacSigning("production", { ...secrets, APP_STORE_CONNECT_API_KEY_BASE64: value }, () => {
      assert.fail("build must not start");
    }, "/does-not-exist"), /APP_STORE_CONNECT_API_KEY_BASE64/);
  }
});

test("production accepts wrapped Base64 without changing the decoded PEM", async () => {
  const wrapped = secrets.APP_STORE_CONNECT_API_KEY_BASE64.match(/.{1,64}/g).join("\r\n");
  await withMacSigning("production", { ...secrets, APP_STORE_CONNECT_API_KEY_BASE64: ` \n${wrapped}\n` }, env => {
    assert.equal(readFileSync(env.APPLE_API_KEY_PATH, "utf8"), apiKeyPem);
  });
});

test("production does not use legacy raw-P8 credentials as an implicit fallback", async () => {
  await assert.rejects(withMacSigning("production", ambient, () => {
    assert.fail("build must not start");
  }), /MACOS_DEVELOPER_ID_P12_BASE64.*APP_STORE_CONNECT_API_KEY_BASE64/);
});
