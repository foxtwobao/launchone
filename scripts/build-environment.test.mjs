import test from "node:test";
import assert from "node:assert/strict";
import { buildEnvironment, selectedEnvironment, applicationConfig } from "./build-environment.mjs";

const production = {
  LAUNCHONE_SERVICE_URL: "https://launchone.example.com",
  LAUNCHONE_UPDATE_PUBLIC_KEY: "fixture-update-public-key",
};
const staleBusinessSettings = {
  LAUNCHONE_IDONE_ISSUER: "http://stale.test",
  LAUNCHONE_CLIENT_ID: "stale-client",
  LAUNCHONE_TOKENONE_URL: "http://stale.test/v1",
  LAUNCHONE_CODEX_BASE_URL: "http://stale.test/v1",
  LAUNCHONE_CLAUDE_BASE_URL: "http://stale.test",
  LAUNCHONE_CODEX_MODEL: "stale-model",
  LAUNCHONE_CLAUDE_MODEL: "stale-model",
  LAUNCHONE_TOKENONE_PORTAL_URL: "http://stale.test/account",
};

test("packaging requires explicit environment and lab ignores ambient bootstrap overrides", () => {
  assert.throws(() => buildEnvironment(undefined), /明确选择/);
  assert.throws(() => buildEnvironment("staging"), /明确选择/);
  const expected = buildEnvironment("lab", {});
  const lab = buildEnvironment("lab", {
    ...production,
    ...staleBusinessSettings,
    LAUNCHONE_BUILD_ENV: "production",
    VITE_BUILD_ENV: "production",
    EXTRA: "kept",
  });
  assert.equal(lab.LAUNCHONE_SERVICE_URL, "https://launchone.lab.home.arpa");
  assert.equal(lab.LAUNCHONE_UPDATE_PUBLIC_KEY, expected.LAUNCHONE_UPDATE_PUBLIC_KEY);
  assert.equal(lab.LAUNCHONE_BUILD_ENV, "lab");
  assert.equal(lab.VITE_BUILD_ENV, "lab");
  assert.equal(lab.EXTRA, "kept");
  for (const key of Object.keys(staleBusinessSettings)) assert.equal(Object.hasOwn(lab, key), false, key);
  assert.equal(selectedEnvironment(["--environment", "lab"]), "lab");
  assert.equal(applicationConfig("lab").identifier, "com.tokenone.launchone.lab");
  assert.deepEqual(applicationConfig("production"), {});
  assert.equal(applicationConfig("lab", lab.LAUNCHONE_UPDATE_PUBLIC_KEY).plugins.updater.pubkey,
    lab.LAUNCHONE_UPDATE_PUBLIC_KEY);
});

test("production needs only the service URL and updater public key", () => {
  const env = buildEnvironment("production", production, "/nonexistent");
  assert.deepEqual(env, { ...production, LAUNCHONE_BUILD_ENV: "production", VITE_BUILD_ENV: "production" });
  assert.throws(() => buildEnvironment("production", {}, "/nonexistent"), /配置缺失/);
  for (const key of Object.keys(production)) {
    assert.throws(() => buildEnvironment("production", { ...production, [key]: " " }, "/nonexistent"), new RegExp(`配置缺失：${key}`));
  }
});

test("stale business settings never reach desktop build subprocesses", () => {
  const env = buildEnvironment("production", {
    ...production,
    ...staleBusinessSettings,
    LAUNCHONE_UNKNOWN_SETTING: "must-not-inherit",
    EXTRA: "kept",
    TAURI_SIGNING_PRIVATE_KEY: "fixture-signing-key",
  }, "/nonexistent");
  assert.deepEqual(Object.keys(env).filter(key => key.startsWith("LAUNCHONE_")).sort(),
    ["LAUNCHONE_BUILD_ENV", "LAUNCHONE_SERVICE_URL", "LAUNCHONE_UPDATE_PUBLIC_KEY"]);
  assert.equal(env.EXTRA, "kept");
  assert.equal(env.TAURI_SIGNING_PRIVATE_KEY, "fixture-signing-key");
});

test("service URL still requires HTTPS without credentials, query or fragment and production rejects lab", () => {
  for (const url of ["http://example.com", "https://user:password@example.com", "https://example.com?key=test", "https://example.com/#fragment"]) {
    assert.throws(() => buildEnvironment("production", { ...production, LAUNCHONE_SERVICE_URL: url }, "/nonexistent"), /HTTPS/);
  }
  for (const url of ["https://launchone.lab.home.arpa", "https://localhost"]) {
    assert.throws(() => buildEnvironment("production", { ...production, LAUNCHONE_SERVICE_URL: url }, "/nonexistent"), /正式包不能连接 lab/);
  }
});
