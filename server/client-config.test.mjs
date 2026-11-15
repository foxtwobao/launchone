import test from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { createApp, readConfig } from "./app.mjs";

const serviceEnv = {
  NODE_ENV: "production",
  LAUNCHONE_PUBLIC_ORIGIN: "https://launchone.test",
  IDONE_ISSUER: "https://idone.test/",
  RESELLER_ORIGIN: "https://reseller.test",
  RESELLER_GATEWAY_ORIGIN: "https://gateway.test",
  RESELLER_APP_CREDENTIAL: "fixture-server-credential-do-not-expose",
};
const clientEnv = {
  LAUNCHONE_CLIENT_ID: "launchone-device-client",
  LAUNCHONE_TOKENONE_URL: "https://tokenone.test",
  LAUNCHONE_CODEX_MODEL: "codex-request-model",
  LAUNCHONE_CLAUDE_MODEL: "claude-request-model",
  LAUNCHONE_TOKENONE_PORTAL_URL: "https://tokenone.test/account",
};

async function fixture(t, env = {}) {
  const app = createApp(readConfig({ ...serviceEnv, ...env }), {
    fetch: async () => { throw new Error("Public client config must not call an authenticated upstream"); },
  });
  const server = app.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(async () => {
    app.dispose();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  });
  return `http://127.0.0.1:${server.address().port}`;
}

test("public config is available before login and exposes only the six public settings", async t => {
  const origin = await fixture(t, {
    ...clientEnv,
    TAURI_SIGNING_PRIVATE_KEY: "fixture-private-signing-key",
    LAUNCHONE_UPDATE_PUBLIC_KEY: "unrelated-update-key",
  });
  const response = await fetch(`${origin}/v1/client-config`);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.equal(response.headers.get("access-control-allow-origin"), null);
  assert.deepEqual(await response.json(), { LAUNCHONE_IDONE_ISSUER: serviceEnv.IDONE_ISSUER, ...clientEnv });

  const invalidSession = await fetch(`${origin}/v1/client-config`, { headers: { Authorization: "Bearer invalid" } });
  assert.equal(invalidSession.status, 200);
  assert.deepEqual(await invalidSession.json(), { LAUNCHONE_IDONE_ISSUER: serviceEnv.IDONE_ISSUER, ...clientEnv });
});

test("recreating the service with changed env publishes new defaults without a client build", async t => {
  const env = { ...clientEnv };
  const original = await fixture(t, env);
  env.LAUNCHONE_CODEX_MODEL = "new-codex-model";
  env.LAUNCHONE_CLAUDE_MODEL = "new-claude-model";
  env.LAUNCHONE_TOKENONE_URL = "https://new-tokenone.test/";
  const updated = await fixture(t, env);
  assert.equal((await (await fetch(`${original}/v1/client-config`)).json()).LAUNCHONE_CODEX_MODEL, clientEnv.LAUNCHONE_CODEX_MODEL);
  assert.deepEqual(await (await fetch(`${updated}/v1/client-config`)).json(), {
    LAUNCHONE_IDONE_ISSUER: serviceEnv.IDONE_ISSUER,
    ...env,
  });
});

test("an unconfigured public endpoint returns a safe retryable error while other routes stay available", async t => {
  const origin = await fixture(t);
  const response = await fetch(`${origin}/v1/client-config`);
  assert.equal(response.status, 503);
  assert.deepEqual(await response.json(), { error: "APP_SERVICE_UNAVAILABLE", retryable: true });
  assert.equal((await fetch(`${origin}/health`)).status, 200);
  assert.equal((await fetch(`${origin}/v1/credentials`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: "{}",
  })).status, 401);
});

test("public env settings must be complete, with bounded text and HTTPS model roots", () => {
  for (const missing of ["LAUNCHONE_CLIENT_ID", "LAUNCHONE_TOKENONE_URL", "LAUNCHONE_CODEX_MODEL", "LAUNCHONE_CLAUDE_MODEL"]) {
    assert.throws(() => readConfig({ ...serviceEnv, ...clientEnv, [missing]: "" }), /configuration|Configure/);
  }
  assert.throws(() => readConfig({ ...serviceEnv, LAUNCHONE_TOKENONE_PORTAL_URL: clientEnv.LAUNCHONE_TOKENONE_PORTAL_URL }), /Configure/);
  for (const root of [
    "http://tokenone.test", "https://user:secret@tokenone.test", "https://tokenone.test/v1",
    "https://tokenone.test/v1/", "https://tokenone.test/?key=secret", "https://tokenone.test/#fragment",
    "https://tokenone.test/?", "https://tokenone.test/#",
  ]) {
    assert.throws(() => readConfig({ ...serviceEnv, ...clientEnv, LAUNCHONE_TOKENONE_URL: root }), /configuration/);
  }
  for (const portal of ["http://tokenone.test/account", "https://user:secret@tokenone.test", "https://tokenone.test/?key=secret", "https://tokenone.test/#fragment"]) {
    assert.throws(() => readConfig({ ...serviceEnv, ...clientEnv, LAUNCHONE_TOKENONE_PORTAL_URL: portal }), /configuration/);
  }
  for (const [key, size] of [["LAUNCHONE_CLIENT_ID", 1025], ["LAUNCHONE_CODEX_MODEL", 257], ["LAUNCHONE_CLAUDE_MODEL", 257]]) {
    assert.throws(() => readConfig({ ...serviceEnv, ...clientEnv, [key]: "x".repeat(size) }), /configuration/);
  }
  for (const key of ["LAUNCHONE_CLIENT_ID", "LAUNCHONE_CODEX_MODEL", "LAUNCHONE_CLAUDE_MODEL"]) {
    for (const control of ["\n", "\t", "\u0000", "\u007f", "\u0085"]) {
      assert.throws(() => readConfig({ ...serviceEnv, ...clientEnv, [key]: `prefix${control}suffix` }), /configuration/);
    }
  }
});

test("public settings trim whitespace, preserve the exact issuer and allow an omitted portal", () => {
  const env = { ...serviceEnv, ...Object.fromEntries(Object.entries(clientEnv).map(([key, value]) => [key, ` ${value} `])) };
  const config = readConfig({ ...env, LAUNCHONE_TOKENONE_PORTAL_URL: " " });
  assert.deepEqual(config.clientConfig, { LAUNCHONE_IDONE_ISSUER: serviceEnv.IDONE_ISSUER, ...clientEnv, LAUNCHONE_TOKENONE_PORTAL_URL: "" });
  assert.equal(readConfig(serviceEnv).clientConfig, undefined);
  assert.equal(readConfig({ ...serviceEnv, ...Object.fromEntries(Object.keys(clientEnv).map(key => [key, " "])) }).clientConfig, undefined);
});
