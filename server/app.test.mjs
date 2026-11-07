import test from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { createApp, readConfig } from "./app.mjs";

async function fixture(t, changes = {}) {
  let time = Date.now();
  const config = { origin: "https://launchone.test", issuer: "https://idone.test", resellerOrigin: "https://reseller.test",
    gatewayOrigin: "https://gateway.test", credential: "fixture-app-credential" };
  const state = { authorized: true, emailVerified: true, userIssuer: config.issuer, userSub: "fixture-sub", ...changes };
  const calls = [];
  const json = (value, status = 200) => Response.json(value, { status });
  const mock = async (target, options = {}) => {
    const url = String(target);
    if (url === `${config.issuer}/.well-known/openid-configuration`) {
      return json({ issuer: state.discoveryIssuer ?? config.issuer, userinfo_endpoint: state.userinfoEndpoint ?? `${config.issuer}/userinfo` });
    }
    if (url === `${config.issuer}/userinfo`) {
      if (options.headers.Authorization === "Bearer invalid") return json({ status: "error" });
      if (state.rejectToken) return json({ error: "unauthorized" }, 401);
      return json({ sub: state.userSub, iss: state.userIssuer, email_verified: state.emailVerified });
    }
    assert.ok(url.startsWith(`${config.resellerOrigin}/api/apps/`));
    assert.equal(options.redirect, "error");
    assert.equal(options.headers.Authorization, "Bearer fixture-app-credential");
    const body = JSON.parse(options.body);
    calls.push({ path: new URL(url).pathname, body });
    if (url.endsWith("/keys/ensure")) {
      assert.deepEqual(body, { identity: { issuer: config.issuer, subject: state.userSub } });
      if (state.ensureError) return json({ error: state.ensureError }, state.ensureStatus ?? 403);
      if (!state.authorized) return json({ error: "APP_USER_AUTHORIZATION_REQUIRED" }, 403);
      return json({ status: "ready", tokenone_user_id: "42", api_key: { id: "701", key: "sk-fixture-personal", group_id: "8", status: "active" } });
    }
    if (url.endsWith("/authorizations")) {
      assert.deepEqual(Object.keys(body).sort(), ["code_challenge", "identity", "redirect_uri", "state"]);
      assert.equal(body.redirect_uri, `${config.origin}/auth/reseller/callback`);
      assert.equal(body.code_challenge.length, 43);
      state.authorization = body;
      return json({ authorization_url: state.authorizationUrl ?? `${config.gatewayOrigin}/_channel/start?flow=fixture-flow`, expires_in: 300 });
    }
    if (url.endsWith("/authorizations/complete")) {
      assert.equal(body.code, "fixture-code");
      const { createHash } = await import("node:crypto");
      assert.equal(createHash("sha256").update(body.code_verifier).digest("base64url"), state.authorization.code_challenge);
      if (state.completeError) return json({ error: state.completeError }, 400);
      state.authorized = true;
      if (state.loseExchangeResponse) throw new Error("fixture lost response");
      return json({ status: "authorized", app_id: "launchone" });
    }
    throw new Error("Unexpected fixture URL");
  };
  const app = createApp(config, { fetch: mock, now: () => time });
  const server = app.listen(0, "127.0.0.1");
  await once(server, "listening");
  const origin = `http://127.0.0.1:${server.address().port}`;
  t.after(async () => { app.dispose(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  const request = (path, { body, token = "fixture-user-token", headers, ...options } = {}) => fetch(`${origin}${path}`, {
    redirect: "manual", ...options, headers: { Authorization: `Bearer ${token}`, ...(body !== undefined ? { "Content-Type": "application/json" } : {}), ...headers },
    ...(body !== undefined ? { method: "POST", body: JSON.stringify(body) } : {}),
  });
  const start = () => request("/v1/credentials", { body: {} });
  const complete = (flow, query = `code=fixture-code&state=${state.authorization.state}`) => request(`/auth/reseller/callback?${query}`);
  return { config, state, calls, request, start, complete, advance: ms => { time += ms; } };
}

test("returns an existing personal Key only to the authenticated native request", async t => {
  const f = await fixture(t);
  const response = await f.start();
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.equal(response.headers.get("access-control-allow-origin"), null);
  assert.deepEqual(await response.json(), { status: "ready", key: "sk-fixture-personal" });
  assert.equal(f.calls.length, 1);
});

test("rejects invalid tokens, forged identities and browser calls", async t => {
  const f = await fixture(t);
  assert.equal((await f.request("/v1/credentials", { body: {}, token: "invalid" })).status, 401);
  assert.equal((await f.request("/v1/credentials", { body: { identity: { subject: "victim" } } })).status, 400);
  assert.equal((await f.request("/v1/credentials?subject=victim", { body: {} })).status, 400);
  assert.equal((await f.request("/v1/credentials", { body: {}, headers: { Origin: "https://browser.test" } })).status, 403);
  assert.equal(f.calls.length, 0);
});

test("requires the configured issuer; signup email policy remains with IDONE", async t => {
  const f = await fixture(t, { userIssuer: "https://other.test" });
  assert.equal((await f.start()).status, 401);
  f.state.userIssuer = f.config.issuer;
  f.state.emailVerified = false;
  const response = await f.start();
  assert.equal(response.status, 200);
  assert.equal((await response.json()).status, "ready");
  assert.equal(f.calls.length, 1);
});

test("rejects discovery issuer or UserInfo origin mismatches before forwarding a user token", async t => {
  const f = await fixture(t, { discoveryIssuer: "https://other.test" });
  assert.equal((await f.start()).status, 503);
  f.state.discoveryIssuer = f.config.issuer;
  f.state.userinfoEndpoint = "https://other.test/userinfo";
  assert.equal((await f.start()).status, 503);
  assert.equal(f.calls.length, 0);
});

test("first authorization opens a browser without disclosing the Key and completes once", async t => {
  const f = await fixture(t, { authorized: false });
  const flow = await (await f.start()).json();
  assert.equal(flow.status, "authorization_required");
  assert.equal(flow.key, undefined);
  assert.equal(flow.authorization_url, `${f.config.origin}/authorize/${flow.id}`);
  const browser = await f.request(`/authorize/${flow.id}`);
  assert.equal(browser.status, 302);
  assert.equal(browser.headers.get("location"), "https://gateway.test/_channel/start?flow=fixture-flow");
  assert.deepEqual(await (await f.request(`/v1/authorizations/${flow.id}`)).json(), { status: "pending" });
  assert.equal((await f.request(`/v1/authorizations/${flow.id}`, { token: "another-session" })).status, 400);
  const callback = await f.complete(flow);
  assert.equal(callback.status, 303);
  assert.equal(callback.headers.get("location"), "/authorization-finished");
  assert.equal((await f.complete(flow)).status, 400);
  const result = await f.request(`/v1/authorizations/${flow.id}`);
  assert.deepEqual(await result.json(), { status: "ready", key: "sk-fixture-personal" });
  assert.equal(f.calls.filter(call => call.path.endsWith("/complete")).length, 1);
  const page = await (await f.request("/authorization-finished")).text();
  assert.ok(!page.includes("sk-fixture"));
});

test("rejects wrong state, duplicate query parameters and expired flows without exchanging", async t => {
  const f = await fixture(t, { authorized: false });
  const flow = await (await f.start()).json();
  assert.equal((await f.complete(flow, `code=fixture-code&state=${"x".repeat(43)}`)).status, 400);
  assert.equal((await f.complete(flow, `code=fixture-code&state=${f.state.authorization.state}&state=${f.state.authorization.state}`)).status, 400);
  f.advance(301_000);
  assert.equal((await f.complete(flow)).status, 400);
  assert.equal((await f.request(`/v1/authorizations/${flow.id}`)).status, 400);
  assert.equal(f.calls.filter(call => call.path.endsWith("/complete")).length, 0);
});

test("revalidates the initiating login before consuming a callback", async t => {
  const f = await fixture(t, { authorized: false });
  const flow = await (await f.start()).json();
  f.state.rejectToken = true;
  await f.complete(flow);
  assert.equal(f.calls.filter(call => call.path.endsWith("/complete")).length, 0);
  f.state.rejectToken = false;
  assert.equal((await f.request(`/v1/authorizations/${flow.id}`)).status, 401);
});

test("a lost exchange response recovers through ensure and never replays the code", async t => {
  const f = await fixture(t, { authorized: false, loseExchangeResponse: true });
  const flow = await (await f.start()).json();
  await f.complete(flow);
  assert.deepEqual(await (await f.request(`/v1/authorizations/${flow.id}`)).json(), { status: "ready", key: "sk-fixture-personal" });
  assert.equal((await f.complete(flow)).status, 400);
  assert.equal(f.calls.filter(call => call.path.endsWith("/complete")).length, 1);
});

test("explicit exchange rejection remains an error, not a successful UI result", async t => {
  const f = await fixture(t, { authorized: false, completeError: "AUTHORIZATION_FLOW_INVALID" });
  const flow = await (await f.start()).json();
  await f.complete(flow);
  const response = await f.request(`/v1/authorizations/${flow.id}`);
  assert.equal(response.status, 400);
  assert.equal((await response.json()).error, "AUTHORIZATION_FLOW_INVALID");
});

test("coalesces repeated starts for the same desktop login", async t => {
  const f = await fixture(t, { authorized: false });
  const responses = await Promise.all([f.start(), f.start(), f.start()]);
  const flows = await Promise.all(responses.map(response => response.json()));
  assert.equal(new Set(flows.map(flow => flow.id)).size, 1);
  assert.equal(f.calls.filter(call => call.path.endsWith("/authorizations")).length, 1);
});

test("does not convert quota, disabled-account or service errors into authorization", async t => {
  const f = await fixture(t);
  for (const code of ["TOKENONE_KEY_QUOTA_EXHAUSTED", "TOKENONE_USER_DISABLED", "APP_SERVICE_UNAVAILABLE"]) {
    f.state.ensureError = code;
    const response = await f.start();
    assert.equal((await response.json()).error, code);
  }
  assert.equal(f.calls.filter(call => call.path.endsWith("/authorizations")).length, 0);
});

test("rejects gateway redirects outside the configured lab gateway", async t => {
  const f = await fixture(t, { authorized: false, authorizationUrl: "https://attacker.test/_channel/start?flow=x" });
  assert.equal((await f.start()).status, 503);
});

test("service config keeps publisher credentials server-side and requires exact HTTPS origins", () => {
  const env = { LAUNCHONE_PUBLIC_ORIGIN: "https://launchone.test", IDONE_ISSUER: "https://idone.test",
    RESELLER_ORIGIN: "https://reseller.test", RESELLER_GATEWAY_ORIGIN: "https://gateway.test",
    RESELLER_APP_CREDENTIAL: "fixture-app-credential", NODE_ENV: "production" };
  assert.equal(readConfig(env).port, 55400);
  for (const origin of ["http://launchone.test", "https://user:secret@launchone.test", "https://launchone.test?query=x", "https://launchone.test/path"]) {
    assert.throws(() => readConfig({ ...env, LAUNCHONE_PUBLIC_ORIGIN: origin }));
  }
});
