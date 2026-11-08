import test from "node:test";
import assert from "node:assert/strict";
import { buildEnvironment, selectedEnvironment, applicationConfig } from "./build-environment.mjs";

test("packaging requires explicit environment and lab ignores ambient endpoint overrides", () => {
  assert.throws(() => buildEnvironment(undefined), /明确选择/);
  assert.throws(() => buildEnvironment("staging"), /明确选择/);
  const lab = buildEnvironment("lab", { LAUNCHONE_SERVICE_URL: "https://production.test", EXTRA: "kept" });
  assert.equal(lab.LAUNCHONE_SERVICE_URL, "https://launchone.lab.home.arpa");
  assert.equal(lab.VITE_BUILD_ENV, "lab");
  assert.equal(lab.EXTRA, "kept");
  assert.equal(selectedEnvironment(["--environment", "lab"]), "lab");
  assert.equal(applicationConfig("lab").identifier, "com.tokenone.launchone.lab");
  assert.deepEqual(applicationConfig("production"), {});
});

test("production rejects missing settings, lab addresses and credentials in URLs", () => {
  assert.throws(() => buildEnvironment("production", {}, "/nonexistent"), /配置缺失/);
  const lab = buildEnvironment("lab", {});
  assert.throws(() => buildEnvironment("production", lab, "/nonexistent"), /正式包不能连接 lab/);
  const production = Object.fromEntries(Object.entries(lab).map(([k, v]) => [k, v.replaceAll("lab.home.arpa", "example.com")]));
  assert.equal(buildEnvironment("production", production, "/nonexistent").VITE_BUILD_ENV, "production");
  for (const url of ["http://example.com", "https://user:password@example.com", "https://example.com?key=test"]) {
    assert.throws(() => buildEnvironment("production", { ...production, LAUNCHONE_SERVICE_URL: url }, "/nonexistent"), /HTTPS/);
  }
});
